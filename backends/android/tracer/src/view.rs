//! View protocol 2: guest paths the session's view server owns are served
//! by it (`banana-vfs::rootless`) instead of the guest root. The server keeps
//! content, sparse memfd caches and persistence; the tracer keeps what the
//! kernel would: open descriptions (offset, mode, close-on-exec) per
//! descriptor table, and the virtual working directory.
//!
//! An open view file is a real descriptor in the tracee: the server's cache
//! memfd, handed over by injected `socket`/`connect`/`recvmsg` calls on a
//! private abstract socket whose peer credentials the tracer checks (Android
//! denies reopening memfds through /proc). Since every descriptor shares the
//! memfd's single kernel offset, reads and writes go through the server.
use crate::{
    arch::{self, Regs, nr},
    mem::{self, Scratch},
};
use std::{
    cell::RefCell,
    collections::HashMap,
    io::{Read, Write},
    os::{
        fd::{AsRawFd, FromRawFd, OwnedFd},
        unix::net::UnixStream,
    },
    path::PathBuf,
    rc::Rc,
};

pub const PROTOCOL: u64 = 2;
/// Largest read or write moved per call, as the server allows.
const CHUNK: usize = 65536;

#[derive(Default, Debug)]
pub struct Reply {
    pub id: u64,
    pub length: u64,
    pub mode: u32,
    pub directory: bool,
    pub modified: u64,
    pub accessed: u64,
    pub created: u64,
    pub body: Vec<u8>,
}

/// The connection to the view server (one per session, used serially).
pub struct Broker {
    stream: UnixStream,
    pub tag: String,
}

fn errno(e: std::io::Error) -> i32 {
    e.raw_os_error().unwrap_or(libc::EIO)
}

impl Broker {
    pub fn connect(socket: &str) -> Result<Self, String> {
        let stream = UnixStream::connect(socket)
            .map_err(|e| format!("Cannot reach the view server {socket}: {e}"))?;
        let mut broker = Self {
            stream,
            tag: String::new(),
        };
        let hello = broker
            .rpc(11, 0, 0, 0, 0, b"", b"")
            .map_err(|e| format!("View server handshake failed: errno {e}"))?;
        if hello.length != PROTOCOL {
            return Err(format!(
                "The view server speaks protocol {}, not {PROTOCOL}",
                hello.length
            ));
        }
        broker.tag = String::from_utf8_lossy(&hello.body).into_owned();
        Ok(broker)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn rpc(
        &mut self,
        op: u32,
        flags: u32,
        id: u64,
        offset: u64,
        length: u64,
        p: &[u8],
        q: &[u8],
    ) -> Result<Reply, i32> {
        let mut out = Vec::with_capacity(40 + p.len() + q.len());
        out.extend(op.to_le_bytes());
        out.extend(flags.to_le_bytes());
        out.extend(id.to_le_bytes());
        out.extend(offset.to_le_bytes());
        out.extend(length.to_le_bytes());
        out.extend((p.len() as u32).to_le_bytes());
        out.extend((q.len() as u32).to_le_bytes());
        out.extend(p);
        out.extend(q);
        self.stream.write_all(&out).map_err(|_| libc::EIO)?;
        let mut head = [0u8; 56];
        self.stream.read_exact(&mut head).map_err(|_| libc::EIO)?;
        let u32at = |o: usize| u32::from_le_bytes(head[o..o + 4].try_into().unwrap());
        let u64at = |o: usize| u64::from_le_bytes(head[o..o + 8].try_into().unwrap());
        let error = u32at(0) as i32;
        let mut body = vec![0u8; u32at(4) as usize];
        self.stream.read_exact(&mut body).map_err(|_| libc::EIO)?;
        if error != 0 {
            return Err(error);
        }
        Ok(Reply {
            id: u64at(8),
            length: u64at(16),
            mode: u32at(24),
            directory: u32at(28) != 0,
            modified: u64at(32),
            accessed: u64at(40),
            created: u64at(48),
            body,
        })
    }

    /// Receive the descriptor sent after an op-17 reply.
    fn receive_fd(&mut self) -> Result<OwnedFd, i32> {
        let mut byte = 0u8;
        let mut vector = libc::iovec {
            iov_base: (&mut byte as *mut u8).cast(),
            iov_len: 1,
        };
        let mut control = [0u64; 8];
        // SAFETY: a zeroed msghdr pointing at local buffers.
        let mut message: libc::msghdr = unsafe { std::mem::zeroed() };
        message.msg_iov = &mut vector;
        message.msg_iovlen = 1;
        message.msg_control = control.as_mut_ptr().cast();
        message.msg_controllen = std::mem::size_of_val(&control) as _;
        loop {
            // SAFETY: receives into the buffers above.
            let n = unsafe {
                libc::recvmsg(
                    self.stream.as_raw_fd(),
                    &mut message,
                    libc::MSG_CMSG_CLOEXEC,
                )
            };
            if n == 1 {
                break;
            }
            if n < 0 && std::io::Error::last_os_error().raw_os_error() == Some(libc::EINTR) {
                continue;
            }
            return Err(libc::EPROTO);
        }
        // SAFETY: the kernel filled `control`; the header is validated.
        unsafe {
            let header = libc::CMSG_FIRSTHDR(&message);
            if header.is_null()
                || (*header).cmsg_level != libc::SOL_SOCKET
                || (*header).cmsg_type != libc::SCM_RIGHTS
            {
                return Err(libc::EPROTO);
            }
            Ok(OwnedFd::from_raw_fd(
                libc::CMSG_DATA(header).cast::<i32>().read_unaligned(),
            ))
        }
    }
}

/// One open description of a view file, shared by dup'd and inherited
/// descriptors.
struct OpenFile {
    id: u64,
    offset: u64,
    mode: u32,
}

#[derive(Clone)]
struct Entry {
    file: Rc<RefCell<OpenFile>>,
    cloexec: bool,
}

type Table = Rc<RefCell<HashMap<i32, Entry>>>;

/// Work left for a call's exit stop.
pub enum Pending {
    /// dup/dup2/dup3/F_DUPFD: the new descriptor shares `source`'s entry.
    Dup {
        source: i32,
        cloexec: bool,
    },
    Close(i32),
    SetCloexec {
        fd: i32,
        cloexec: bool,
    },
    /// A successful real chdir/fchdir leaves the view.
    LeaveCwd,
    /// Descriptor handoff in progress (injected calls).
    Transfer(Box<Transfer>),
}

pub struct Transfer {
    stage: Stage,
    /// Registers at the original call's entry: restored at the end.
    base: Regs,
    listener: OwnedFd,
    file: OwnedFd,
    address: u64,
    address_length: u64,
    message: u64,
    control: u64,
    socket: i64,
    id: u64,
    mode: u32,
    /// Fd received by the tracee, once known.
    received: i64,
    /// The next stop is the entry of an injected call.
    pub injecting: bool,
}

#[derive(PartialEq)]
enum Stage {
    Socket,
    Connect,
    Receive,
    Close,
}

pub enum Outcome {
    /// Not a view call: translate normally, then do `Option<Pending>` at exit.
    Continue(Option<Pending>),
    /// Skipped; return this result.
    Skip(i64),
    /// Rewritten (descriptor handoff); finish at exit.
    Rewritten(Pending),
}

pub struct View {
    broker: Broker,
    /// The empty host directory standing for view directories as cwd.
    pub empty: PathBuf,
    tables: HashMap<i32, Table>,
    /// Guest working directory per process, while it is inside the view.
    cwd: HashMap<i32, String>,
}

/// Paths for one call, already made absolute (lexically) by the tracer.
pub struct Paths {
    pub first: Option<String>,
    pub second: Option<String>,
}

fn time(ns: u64) -> (i64, u64) {
    ((ns / 1_000_000_000) as i64, ns % 1_000_000_000)
}

/// `struct stat` for a view reply.
fn stat_bytes(r: &Reply) -> Vec<u8> {
    let mode = (if r.directory {
        libc::S_IFDIR
    } else {
        libc::S_IFREG
    }) as u32
        | r.mode;
    let nlink: u64 = if r.directory { 2 } else { 1 };
    let blocks = r.length.div_ceil(512);
    let (a, an) = time(r.accessed);
    let (m, mn) = time(r.modified);
    let (c, cn) = time(r.created);
    let mut v = Vec::new();
    v.extend(0u64.to_ne_bytes()); // st_dev
    v.extend(r.id.to_ne_bytes()); // st_ino
    if cfg!(target_arch = "x86_64") {
        v.extend(nlink.to_ne_bytes());
        v.extend(mode.to_ne_bytes());
        v.extend([0u8; 8]); // uid, gid
        v.extend([0u8; 4]);
        v.extend(0u64.to_ne_bytes()); // st_rdev
        v.extend(r.length.to_ne_bytes());
        v.extend(65536u64.to_ne_bytes());
        v.extend(blocks.to_ne_bytes());
    } else {
        v.extend(mode.to_ne_bytes());
        v.extend((nlink as u32).to_ne_bytes());
        v.extend([0u8; 8]); // uid, gid
        v.extend(0u64.to_ne_bytes()); // st_rdev
        v.extend(0u64.to_ne_bytes());
        v.extend(r.length.to_ne_bytes());
        v.extend(65536u32.to_ne_bytes());
        v.extend(0u32.to_ne_bytes());
        v.extend(blocks.to_ne_bytes());
    }
    for (s, n) in [(a, an), (m, mn), (c, cn)] {
        v.extend(s.to_ne_bytes());
        v.extend(n.to_ne_bytes());
    }
    v.resize(
        if cfg!(target_arch = "x86_64") {
            144
        } else {
            128
        },
        0,
    );
    v
}

/// `struct statx` for a view reply.
fn statx_bytes(r: &Reply) -> Vec<u8> {
    let mut v = vec![0u8; 256];
    let put = |v: &mut Vec<u8>, o: usize, b: &[u8]| v[o..o + b.len()].copy_from_slice(b);
    let mode = ((if r.directory {
        libc::S_IFDIR
    } else {
        libc::S_IFREG
    }) as u32
        | r.mode) as u16;
    put(
        &mut v,
        0,
        &(libc::STATX_BASIC_STATS | libc::STATX_BTIME).to_ne_bytes(),
    );
    put(&mut v, 4, &65536u32.to_ne_bytes());
    put(
        &mut v,
        16,
        &(if r.directory { 2u32 } else { 1 }).to_ne_bytes(),
    );
    put(&mut v, 28, &mode.to_ne_bytes());
    put(&mut v, 32, &r.id.to_ne_bytes());
    put(&mut v, 40, &r.length.to_ne_bytes());
    put(&mut v, 48, &r.length.div_ceil(512).to_ne_bytes());
    for (offset, ns) in [
        (64, r.accessed),
        (80, r.created),
        (96, r.modified),
        (112, r.modified),
    ] {
        let (s, n) = time(ns);
        put(&mut v, offset, &s.to_ne_bytes());
        put(&mut v, offset + 8, &(n as u32).to_ne_bytes());
    }
    v
}

/// Open flags to the server's: 1 read, 2 write, 4 create, 8 exclusive,
/// 16 truncate, 32 directory.
fn open_flags(mode: u32) -> u32 {
    let mode = mode as i32;
    let directory = if mode & libc::O_DIRECTORY != 0 { 32 } else { 0 };
    if mode & libc::O_PATH != 0 {
        return 1 | directory;
    }
    let access = mode & libc::O_ACCMODE;
    (if access != libc::O_WRONLY { 1 } else { 0 })
        | (if access != libc::O_RDONLY { 2 } else { 0 })
        | (if mode & libc::O_CREAT != 0 { 4 } else { 0 })
        | (if mode & libc::O_EXCL != 0 { 8 } else { 0 })
        | (if mode & libc::O_TRUNC != 0 { 16 } else { 0 })
        | directory
}

impl View {
    pub fn connect(socket: &str, empty: PathBuf) -> Result<Self, String> {
        Ok(Self {
            broker: Broker::connect(socket)?,
            empty,
            tables: HashMap::new(),
            cwd: HashMap::new(),
        })
    }

    /// Whether the server owns a (lexically absolute) guest path.
    pub fn owns(&mut self, guest: &str) -> Result<bool, i32> {
        match self.broker.rpc(0, 0, 0, 0, 0, guest.as_bytes(), b"") {
            Ok(_) => Ok(true),
            Err(libc::EXDEV) => Ok(false),
            Err(e) => Err(e),
        }
    }

    pub fn cwd(&self, tgid: i32) -> Option<&String> {
        self.cwd.get(&tgid)
    }

    /// Start a process inside the view (the initial `-w`).
    pub fn enter_directory(&mut self, tgid: i32, guest: &str) -> Result<(), i32> {
        let r = self.broker.rpc(1, 0, 0, 0, 0, guest.as_bytes(), b"")?;
        if !r.directory {
            return Err(libc::ENOTDIR);
        }
        self.cwd.insert(tgid, guest.to_owned());
        Ok(())
    }

    fn table(&mut self, tgid: i32) -> Table {
        self.tables.entry(tgid).or_default().clone()
    }

    /// A new process or thread: threads share their table, processes copy it.
    pub fn fork(&mut self, parent: i32, child: i32) {
        if parent == child {
            return;
        }
        let copy: HashMap<i32, Entry> = self.table(parent).borrow().clone();
        self.tables.insert(child, Rc::new(RefCell::new(copy)));
        if let Some(cwd) = self.cwd.get(&parent).cloned() {
            self.cwd.insert(child, cwd);
        }
    }

    /// exec closes close-on-exec descriptors.
    pub fn exec(&mut self, tgid: i32) {
        let kept: HashMap<i32, Entry> = self
            .table(tgid)
            .borrow()
            .iter()
            .filter(|(_, e)| !e.cloexec)
            .map(|(&f, e)| (f, e.clone()))
            .collect();
        self.tables.insert(tgid, Rc::new(RefCell::new(kept)));
    }

    pub fn exit(&mut self, tgid: i32) {
        self.tables.remove(&tgid);
        self.cwd.remove(&tgid);
    }

    /// The view object behind tracee descriptor `fd`, checked against the
    /// kernel (stale entries after close_range or exec are dropped).
    fn descriptor(&mut self, tid: i32, tgid: i32, fd: i64) -> Option<(u64, Rc<RefCell<OpenFile>>)> {
        let fd = fd as i32;
        let table = self.table(tgid);
        let entry = table.borrow().get(&fd).cloned()?;
        let link = std::fs::read_link(format!("/proc/{tid}/fd/{fd}")).ok();
        let prefix = format!("/memfd:banana-vfs-{}-", self.broker.tag);
        let id = link.as_ref().and_then(|l| {
            l.to_str()?
                .strip_prefix(&prefix)?
                .split(' ')
                .next()?
                .parse::<u64>()
                .ok()
        });
        match id {
            Some(id) if entry.file.borrow().id == id => Some((id, entry.file)),
            _ => {
                table.borrow_mut().remove(&fd);
                None
            }
        }
    }

    /// Guest path of a view object (op 13), for /proc links and fchdir.
    pub fn guest_path(&mut self, id: u64, deleted_ok: bool) -> Result<String, i32> {
        let r = self.broker.rpc(13, deleted_ok as u32, id, 0, 0, b"", b"")?;
        Ok(String::from_utf8_lossy(&r.body).into_owned())
    }

    /// The guest path a /proc descriptor link of a view file stands for.
    pub fn link_target(&mut self, link: &str) -> Option<String> {
        let prefix = format!("/memfd:banana-vfs-{}-", self.broker.tag);
        let id = link
            .strip_prefix(&prefix)?
            .split(' ')
            .next()?
            .parse()
            .ok()?;
        self.guest_path(id, true).ok()
    }

    /// Guest directory of a view descriptor (for `*at` calls).
    pub fn directory_of(&mut self, tid: i32, tgid: i32, fd: i64) -> Option<Result<String, i32>> {
        let (id, _) = self.descriptor(tid, tgid, fd)?;
        Some(self.guest_path(id, false))
    }

    /// The first bytes of a view file (for exec planning).
    pub fn head(&mut self, guest: &str, limit: usize) -> Result<Vec<u8>, i32> {
        let r = self.broker.rpc(2, 1, 0, 0, 0, guest.as_bytes(), b"")?;
        if r.directory {
            return Err(libc::EACCES);
        }
        Ok(self
            .broker
            .rpc(15, 0, r.id, 0, limit.min(CHUNK) as u64, b"", b"")?
            .body)
    }

    /// Host path a view program executes from: its hydrated cache.
    pub fn program(&mut self, guest: &str) -> Result<PathBuf, i32> {
        let r = self.broker.rpc(2, 1, 0, 0, 0, guest.as_bytes(), b"")?;
        if r.directory {
            return Err(libc::EACCES);
        }
        self.broker.rpc(3, 0, r.id, 0, r.length, b"", b"")?;
        Ok(PathBuf::from(String::from_utf8_lossy(&r.body).into_owned()))
    }

    /// Handle a call at entry. `paths` are the call's guest paths.
    #[allow(clippy::too_many_arguments)]
    pub fn enter(
        &mut self,
        tid: i32,
        tgid: i32,
        call: i64,
        args: [u64; 6],
        paths: &Paths,
    ) -> Result<Outcome, i32> {
        if let Some(outcome) = self.descriptor_call(tid, tgid, call, args)? {
            return Ok(outcome);
        }
        if call == nr::getcwd {
            if let Some(cwd) = self.cwd.get(&tgid) {
                let mut bytes = cwd.clone().into_bytes();
                bytes.push(0);
                if bytes.len() as u64 > args[1] {
                    return Ok(Outcome::Skip(-(libc::ERANGE as i64)));
                }
                mem::write(tid, args[0], &bytes).map_err(errno)?;
                return Ok(Outcome::Skip(bytes.len() as i64));
            }
            return Ok(Outcome::Continue(None));
        }
        let Some(path) = paths.first.as_deref() else {
            // fstatat(fd, "", AT_EMPTY_PATH) / statx on a view descriptor.
            if (call == nr::newfstatat || call == nr::statx)
                && let Some((id, _)) = self.descriptor(tid, tgid, args[0] as i64)
            {
                let statx = call == nr::statx;
                let buffer = if statx { args[4] } else { args[2] };
                let r = self.metadata(tid, None, id, buffer, statx);
                return Ok(Outcome::Skip(r.unwrap_or_else(|e| -(e as i64))));
            }
            return Ok(Outcome::Continue(None));
        };
        if !self.owns(path)? {
            let leave =
                (call == nr::chdir && self.cwd.contains_key(&tgid)).then_some(Pending::LeaveCwd);
            return Ok(Outcome::Continue(leave));
        }
        let p = path.as_bytes();
        let rpc = |s: &mut Self, op: u32, flags: u32, offset: u64, length: u64, q: &[u8]| {
            s.broker
                .rpc(op, flags, 0, offset, length, p, q)
                .map(|_| 0i64)
        };
        let skip = |r: Result<i64, i32>| Ok(Outcome::Skip(r.unwrap_or_else(|e| -(e as i64))));
        let n = call;
        if n == nr::open || n == nr::openat || n == nr::creat {
            let mode = if n == nr::creat {
                (libc::O_WRONLY | libc::O_CREAT | libc::O_TRUNC) as u32
            } else {
                args[if n == nr::open { 1 } else { 2 }] as u32
            };
            let r = match self.broker.rpc(2, open_flags(mode), 0, 0, 0, p, b"") {
                Ok(r) => r,
                Err(e) => return skip(Err(e)),
            };
            return match self.begin_transfer(tid, r.id, mode) {
                Ok(pending) => Ok(Outcome::Rewritten(pending)),
                Err(e) => skip(Err(e)),
            };
        }
        if n == nr::openat2 {
            return skip(Err(libc::ENOTSUP));
        }
        if n == nr::stat || n == nr::lstat || n == nr::newfstatat || n == nr::statx {
            let buffer = if n == nr::statx {
                args[4]
            } else if n == nr::newfstatat {
                args[2]
            } else {
                args[1]
            };
            return skip(self.metadata(tid, Some(p), 0, buffer, n == nr::statx));
        }
        if n == nr::access || n == nr::faccessat || n == nr::faccessat2 {
            return skip(rpc(self, 1, 0, 0, 0, b""));
        }
        if n == nr::unlink || n == nr::rmdir || n == nr::unlinkat {
            let directory =
                n == nr::rmdir || (n == nr::unlinkat && args[2] & libc::AT_REMOVEDIR as u64 != 0);
            return skip(rpc(self, 6, directory as u32, 0, 0, b""));
        }
        if n == nr::mkdir || n == nr::mkdirat {
            return skip(rpc(self, 8, 0, 0, 0, b""));
        }
        if n == nr::rename || n == nr::renameat || n == nr::renameat2 {
            let Some(target) = paths.second.as_deref() else {
                return skip(Err(libc::EFAULT));
            };
            let flags = if n == nr::renameat2 { args[4] } else { 0 };
            if flags & !1 != 0 {
                return skip(Err(libc::ENOTSUP));
            }
            return skip(rpc(self, 7, flags as u32, 0, 0, target.as_bytes()));
        }
        if n == nr::chmod || n == nr::fchmodat || n == nr::fchmodat2 {
            let mode = args[if n == nr::chmod { 1 } else { 2 }] as u32;
            return skip(rpc(
                self,
                9,
                0x8000_0000 | (mode & 0o7777),
                u64::MAX,
                u64::MAX,
                b"",
            ));
        }
        if n == nr::utimensat {
            let (access, modified) = self.times(tid, args[2])?;
            return skip(rpc(self, 9, 0, modified, access, b""));
        }
        if n == nr::truncate {
            return skip(rpc(self, 5, 0, 0, args[1], b""));
        }
        if n == nr::chdir {
            let r = self.broker.rpc(1, 0, 0, 0, 0, p, b"");
            return skip(match r {
                Ok(r) if r.directory => {
                    self.cwd.insert(tgid, path.to_owned());
                    Ok(0)
                }
                Ok(_) => Err(libc::ENOTDIR),
                Err(e) => Err(e),
            });
        }
        if n == nr::readlink || n == nr::readlinkat {
            return skip(Err(libc::EINVAL));
        }
        // Anything else on a view path (links, nodes, xattrs, chown...).
        skip(Err(libc::ENOTSUP))
    }

    fn times(&self, tid: i32, address: u64) -> Result<(u64, u64), i32> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos() as u64);
        if address == 0 {
            return Ok((now, now));
        }
        let mut raw = [0u8; 32];
        mem::read(tid, address, &mut raw).map_err(errno)?;
        let pick = |o: usize| {
            let s = i64::from_ne_bytes(raw[o..o + 8].try_into().unwrap());
            let n = i64::from_ne_bytes(raw[o + 8..o + 16].try_into().unwrap());
            match n {
                libc::UTIME_OMIT => u64::MAX,
                libc::UTIME_NOW => now,
                _ => s as u64 * 1_000_000_000 + n as u64,
            }
        };
        Ok((pick(0), pick(16)))
    }

    fn metadata(
        &mut self,
        tid: i32,
        path: Option<&[u8]>,
        id: u64,
        buffer: u64,
        statx: bool,
    ) -> Result<i64, i32> {
        let r = self
            .broker
            .rpc(1, 0, id, 0, 0, path.unwrap_or_default(), b"")?;
        let bytes = if statx {
            statx_bytes(&r)
        } else {
            stat_bytes(&r)
        };
        mem::write(tid, buffer, &bytes).map_err(errno)?;
        Ok(0)
    }

    /// Calls on descriptors (and the descriptor bookkeeping for all fds).
    fn descriptor_call(
        &mut self,
        tid: i32,
        tgid: i32,
        n: i64,
        args: [u64; 6],
    ) -> Result<Option<Outcome>, i32> {
        let a = args[0] as i64;
        let skip = |r: Result<i64, i32>| Ok(Some(Outcome::Skip(r.unwrap_or_else(|e| -(e as i64)))));
        let fcntl = n == nr::fcntl;
        if n == nr::dup
            || n == nr::dup2
            || n == nr::dup3
            || (fcntl
                && (args[1] == libc::F_DUPFD as u64 || args[1] == libc::F_DUPFD_CLOEXEC as u64))
        {
            self.descriptor(tid, tgid, a);
            let cloexec = (n == nr::dup3 && args[2] & libc::O_CLOEXEC as u64 != 0)
                || (fcntl && args[1] == libc::F_DUPFD_CLOEXEC as u64);
            return Ok(Some(Outcome::Continue(Some(Pending::Dup {
                source: a as i32,
                cloexec,
            }))));
        }
        if n == nr::mremap {
            let maps =
                std::fs::read_to_string(format!("/proc/{tid}/maps")).map_err(|_| libc::EACCES)?;
            let prefix = format!("memfd:banana-vfs-{}-", self.broker.tag);
            let virtual_mapping = maps.lines().any(|line| {
                let Some((range, _)) = line.split_once(' ') else {
                    return false;
                };
                let Some((s, e)) = range.split_once('-') else {
                    return false;
                };
                let (s, e) = (
                    u64::from_str_radix(s, 16).unwrap_or(0),
                    u64::from_str_radix(e, 16).unwrap_or(0),
                );
                args[0] >= s && args[0] < e && line.contains(&prefix)
            });
            return if virtual_mapping {
                skip(Err(libc::ENOTSUP))
            } else {
                Ok(None)
            };
        }
        if n == nr::msync {
            self.broker.rpc(4, 0, 0, 0, 0, b"", b"")?;
            return Ok(None);
        }
        if n == nr::copy_file_range || n == nr::sendfile || n == nr::splice {
            let (x, y) = if n == nr::sendfile {
                (args[1], args[0])
            } else {
                (args[0], args[2])
            };
            if self.descriptor(tid, tgid, x as i64).is_some()
                || self.descriptor(tid, tgid, y as i64).is_some()
            {
                // Shared cache descriptors have no per-open kernel offset:
                // libc falls back to its read/write loop.
                return skip(Err(if n == nr::copy_file_range {
                    libc::EXDEV
                } else {
                    libc::EOPNOTSUPP
                }));
            }
            return Ok(None);
        }
        let fd = if n == nr::mmap { args[4] as i64 } else { a };
        let handled = [
            nr::read,
            nr::write,
            nr::pread64,
            nr::pwrite64,
            nr::readv,
            nr::writev,
            nr::preadv,
            nr::pwritev,
            nr::preadv2,
            nr::pwritev2,
            nr::fstat,
            nr::ftruncate,
            nr::fsync,
            nr::fdatasync,
            nr::fchmod,
            nr::fchdir,
            nr::getdents64,
            nr::fallocate,
            nr::close,
            nr::lseek,
            nr::mmap,
            nr::fcntl,
        ];
        if !handled.contains(&n) {
            return Ok(None);
        }
        let Some((id, file)) = self.descriptor(tid, tgid, fd) else {
            if n == nr::fchdir && self.cwd.contains_key(&tgid) {
                return Ok(Some(Outcome::Continue(Some(Pending::LeaveCwd))));
            }
            return Ok(None);
        };
        let mode = file.borrow().mode;
        let path_only = mode & libc::O_PATH as u32 != 0;
        let access = mode as i32 & libc::O_ACCMODE;
        if fcntl {
            return match args[1] as i32 {
                libc::F_GETFL => skip(Ok(mode as i64)),
                libc::F_SETFL => {
                    let mask = (libc::O_APPEND
                        | libc::O_NONBLOCK
                        | libc::O_ASYNC
                        | libc::O_DIRECT
                        | libc::O_NOATIME) as u32;
                    let mut f = file.borrow_mut();
                    f.mode = (f.mode & !mask) | (args[2] as u32 & mask);
                    skip(Ok(0))
                }
                libc::F_SETFD => Ok(Some(Outcome::Continue(Some(Pending::SetCloexec {
                    fd: a as i32,
                    cloexec: args[2] & libc::FD_CLOEXEC as u64 != 0,
                })))),
                _ => Ok(None),
            };
        }
        if n == nr::preadv
            || n == nr::pwritev
            || n == nr::preadv2
            || n == nr::pwritev2
            || n == nr::fallocate
        {
            return skip(Err(libc::EOPNOTSUPP));
        }
        if n == nr::fstat {
            return skip(self.metadata(tid, None, id, args[1], false));
        }
        if n == nr::ftruncate {
            if path_only || access == libc::O_RDONLY {
                return skip(Err(libc::EBADF));
            }
            return skip(self.broker.rpc(5, 0, id, 0, args[1], b"", b"").map(|_| 0));
        }
        if n == nr::fchmod {
            if access == libc::O_RDONLY {
                return skip(Err(libc::EROFS));
            }
            let flags = 0x8000_0000 | (args[1] as u32 & 0o7777);
            return skip(
                self.broker
                    .rpc(9, flags, id, u64::MAX, u64::MAX, b"", b"")
                    .map(|_| 0),
            );
        }
        if n == nr::fchdir {
            let guest = self.guest_path(id, false);
            return skip(guest.and_then(|g| {
                let r = self.broker.rpc(1, 0, id, 0, 0, b"", b"")?;
                if !r.directory {
                    return Err(libc::ENOTDIR);
                }
                self.cwd.insert(tgid, g);
                Ok(0)
            }));
        }
        if n == nr::getdents64 {
            let offset = file.borrow().offset;
            let r = match self.broker.rpc(12, 0, id, offset, args[2], b"", b"") {
                Ok(r) => r,
                Err(e) => return skip(Err(e)),
            };
            mem::write(tid, args[1], &r.body).map_err(errno)?;
            file.borrow_mut().offset = r.length;
            return skip(Ok(r.body.len() as i64));
        }
        if n == nr::fsync || n == nr::fdatasync {
            return skip(self.broker.rpc(4, 0, id, 0, 0, b"", b"").map(|_| 0));
        }
        if n == nr::close {
            if let Err(e) = self.broker.rpc(4, 0, id, 0, 0, b"", b"") {
                return skip(Err(e));
            }
            return Ok(Some(Outcome::Continue(Some(Pending::Close(a as i32)))));
        }
        if n == nr::lseek {
            if path_only {
                return skip(Err(libc::EBADF));
            }
            let base = match args[2] as i32 {
                libc::SEEK_SET => 0i64,
                libc::SEEK_CUR => file.borrow().offset as i64,
                libc::SEEK_END => match self.broker.rpc(1, 0, id, 0, 0, b"", b"") {
                    Ok(r) => r.length as i64,
                    Err(e) => return skip(Err(e)),
                },
                _ => return skip(Err(libc::EINVAL)),
            };
            let Some(next) = base.checked_add(args[1] as i64).filter(|&o| o >= 0) else {
                return skip(Err(libc::EINVAL));
            };
            file.borrow_mut().offset = next as u64;
            return skip(Ok(next));
        }
        if n == nr::mmap {
            let (protection, flags) = (args[2] as i32, args[3] as i32);
            let shared = flags & libc::MAP_SHARED != 0;
            if path_only
                || access == libc::O_WRONLY
                || (shared && protection & libc::PROT_WRITE != 0 && access == libc::O_RDONLY)
            {
                return skip(Err(libc::EACCES));
            }
            // Populate the mapped range of the cache, then let the kernel map it.
            if let Err(e) = self.broker.rpc(
                3,
                if shared { 2 } else { 0 },
                id,
                args[5],
                args[1],
                b"",
                b"",
            ) {
                return skip(Err(e));
            }
            return Ok(Some(Outcome::Continue(None)));
        }
        // read, write, pread64, pwrite64, readv, writev.
        let writing = n == nr::write || n == nr::pwrite64 || n == nr::writev;
        if path_only
            || if writing {
                access == libc::O_RDONLY
            } else {
                access == libc::O_WRONLY
            }
        {
            return skip(Err(libc::EBADF));
        }
        let positioned = n == nr::pread64 || n == nr::pwrite64;
        let mut offset = if positioned {
            args[3]
        } else {
            file.borrow().offset
        };
        if writing && !positioned && mode & libc::O_APPEND as u32 != 0 {
            match self.broker.rpc(1, 0, id, 0, 0, b"", b"") {
                Ok(r) => offset = r.length,
                Err(e) => return skip(Err(e)),
            }
        }
        let vectors: Vec<(u64, u64)> = if n == nr::readv || n == nr::writev {
            if args[2] > 1024 {
                return skip(Err(libc::EINVAL));
            }
            let mut raw = vec![0u8; args[2] as usize * 16];
            mem::read(tid, args[1], &mut raw).map_err(|_| libc::EFAULT)?;
            raw.chunks_exact(16)
                .map(|c| {
                    (
                        u64::from_ne_bytes(c[..8].try_into().unwrap()),
                        u64::from_ne_bytes(c[8..].try_into().unwrap()),
                    )
                })
                .collect()
        } else {
            vec![(args[1], args[2])]
        };
        let total = vectors
            .iter()
            .map(|v| v.1 as usize)
            .sum::<usize>()
            .min(CHUNK);
        let count = if writing {
            let mut data = Vec::with_capacity(total);
            for &(base, length) in &vectors {
                let take = (length as usize).min(total - data.len());
                let start = data.len();
                data.resize(start + take, 0);
                mem::read(tid, base, &mut data[start..]).map_err(|_| libc::EFAULT)?;
                if data.len() == total {
                    break;
                }
            }
            match self
                .broker
                .rpc(16, 0, id, offset, data.len() as u64, &data, b"")
            {
                Ok(r) => r.length,
                Err(e) => return skip(Err(e)),
            }
        } else {
            let r = match self.broker.rpc(15, 0, id, offset, total as u64, b"", b"") {
                Ok(r) => r,
                Err(e) => return skip(Err(e)),
            };
            let mut done = 0usize;
            for &(base, length) in &vectors {
                if done >= r.body.len() {
                    break;
                }
                let take = (length as usize).min(r.body.len() - done);
                if mem::write(tid, base, &r.body[done..done + take]).is_err() {
                    if done == 0 {
                        return skip(Err(libc::EFAULT));
                    }
                    break;
                }
                done += take;
            }
            done as u64
        };
        if !positioned {
            file.borrow_mut().offset = offset + count;
        }
        skip(Ok(count as i64))
    }

    /// Exit-stop work for a pending call. Returns `true` while a handoff
    /// is still running injected calls.
    pub fn leave(
        &mut self,
        tid: i32,
        tgid: i32,
        result: i64,
        pending: &mut Pending,
    ) -> Result<bool, i32> {
        match pending {
            Pending::Dup { source, cloexec } => {
                if result >= 0 {
                    let table = self.table(tgid);
                    let mut table = table.borrow_mut();
                    match table.get(source).cloned() {
                        Some(entry) if result as i32 != *source => {
                            table.insert(
                                result as i32,
                                Entry {
                                    file: entry.file,
                                    cloexec: *cloexec,
                                },
                            );
                        }
                        Some(_) => {}
                        None => {
                            table.remove(&(result as i32));
                        }
                    }
                }
                Ok(false)
            }
            Pending::Close(fd) => {
                if result >= 0 {
                    self.table(tgid).borrow_mut().remove(fd);
                }
                Ok(false)
            }
            Pending::SetCloexec { fd, cloexec } => {
                if result >= 0
                    && let Some(e) = self.table(tgid).borrow_mut().get_mut(fd)
                {
                    e.cloexec = *cloexec;
                }
                Ok(false)
            }
            Pending::LeaveCwd => {
                if result >= 0 {
                    self.cwd.remove(&tgid);
                }
                Ok(false)
            }
            Pending::Transfer(transfer) => self.step(tid, tgid, result, transfer),
        }
    }

    /// Turn the current call into `socket()` and prepare the handoff.
    fn begin_transfer(&mut self, tid: i32, id: u64, mode: u32) -> Result<Pending, i32> {
        self.broker.rpc(17, 0, id, 0, 0, b"", b"")?;
        let file = self.broker.receive_fd()?;
        // SAFETY: a new socket owned below.
        let listener = unsafe {
            libc::socket(
                libc::AF_UNIX,
                libc::SOCK_SEQPACKET | libc::SOCK_CLOEXEC | libc::SOCK_NONBLOCK,
                0,
            )
        };
        if listener < 0 {
            return Err(libc::EMFILE);
        }
        // SAFETY: `listener` is a fresh descriptor.
        let listener = unsafe { OwnedFd::from_raw_fd(listener) };
        let mut name = [0u8; 24];
        // SAFETY: fills `name` (syscall(): bionic exports getrandom from API 28).
        if unsafe { libc::syscall(libc::SYS_getrandom, name.as_mut_ptr(), name.len(), 0) } != 24 {
            return Err(libc::EIO);
        }
        let mut address = (libc::AF_UNIX as u16).to_ne_bytes().to_vec();
        address.push(0); // abstract namespace
        address.extend(name);
        // SAFETY: binds and listens on the socket created above.
        unsafe {
            let mut raw: libc::sockaddr_un = std::mem::zeroed();
            std::ptr::copy_nonoverlapping(
                address.as_ptr(),
                (&mut raw as *mut libc::sockaddr_un).cast::<u8>(),
                address.len(),
            );
            if libc::bind(
                listener.as_raw_fd(),
                (&raw as *const libc::sockaddr_un).cast(),
                address.len() as u32,
            ) != 0
                || libc::listen(listener.as_raw_fd(), 1) != 0
            {
                return Err(libc::EADDRINUSE);
            }
        }
        let base = Regs::read(tid).map_err(errno)?;
        let mut scratch = Scratch::new(tid, base.sp());
        let address_pointer = scratch.push(&address);
        let byte = scratch.push(&[0]);
        let mut vector = byte.to_ne_bytes().to_vec();
        vector.extend(1u64.to_ne_bytes());
        let vector = scratch.push(&vector);
        let control = scratch.push(&[0u8; 24]);
        let mut message = vec![0u8; 56];
        message[16..24].copy_from_slice(&vector.to_ne_bytes());
        message[24..32].copy_from_slice(&1u64.to_ne_bytes());
        message[32..40].copy_from_slice(&control.to_ne_bytes());
        message[40..48].copy_from_slice(&24u64.to_ne_bytes());
        let message = scratch.push(&message);
        scratch.commit().map_err(errno)?;
        let mut regs = base;
        regs.set_arg(0, libc::AF_UNIX as u64);
        regs.set_arg(1, (libc::SOCK_SEQPACKET | libc::SOCK_CLOEXEC) as u64);
        regs.set_arg(2, 0);
        regs.write(tid).map_err(errno)?;
        arch::set_syscall(tid, nr::socket).map_err(errno)?;
        Ok(Pending::Transfer(Box::new(Transfer {
            stage: Stage::Socket,
            base,
            listener,
            file,
            address: address_pointer,
            address_length: address.len() as u64,
            message,
            control,
            socket: -1,
            id,
            mode,
            received: -1,
            injecting: false,
        })))
    }

    fn inject(tid: i32, transfer: &mut Transfer, call: i64, args: &[u64]) -> Result<bool, i32> {
        let mut regs = Regs::read(tid).map_err(errno)?;
        regs.inject(call, args);
        regs.write(tid).map_err(errno)?;
        transfer.injecting = true;
        Ok(true)
    }

    /// Finish: close the tracee's socket if any, then restore the original
    /// registers with `result`.
    fn finish(tid: i32, transfer: &mut Transfer, result: i64) -> Result<bool, i32> {
        if transfer.socket >= 0 && transfer.stage != Stage::Close {
            transfer.stage = Stage::Close;
            transfer.received = result;
            let socket = transfer.socket as u64;
            return Self::inject(tid, transfer, nr::close, &[socket]);
        }
        let mut regs = transfer.base;
        regs.set_result(result);
        regs.write(tid).map_err(errno)?;
        Ok(false)
    }

    fn step(&mut self, tid: i32, tgid: i32, result: i64, t: &mut Transfer) -> Result<bool, i32> {
        t.injecting = false;
        if t.stage == Stage::Close {
            return Self::finish(tid, t, t.received);
        }
        if result < 0 {
            return Self::finish(tid, t, result);
        }
        match t.stage {
            Stage::Socket => {
                t.socket = result;
                t.stage = Stage::Connect;
                let args = [result as u64, t.address, t.address_length];
                Self::inject(tid, t, nr::connect, &args)
            }
            Stage::Connect => {
                // SAFETY: accepts on the tracer's own listener.
                let peer = unsafe {
                    libc::accept4(
                        t.listener.as_raw_fd(),
                        std::ptr::null_mut(),
                        std::ptr::null_mut(),
                        libc::SOCK_CLOEXEC | libc::SOCK_NONBLOCK,
                    )
                };
                if peer < 0 {
                    return Self::finish(tid, t, -(libc::EPROTO as i64));
                }
                // SAFETY: `peer` is a fresh descriptor.
                let peer = unsafe { OwnedFd::from_raw_fd(peer) };
                let mut credentials: libc::ucred = unsafe { std::mem::zeroed() };
                let mut length = std::mem::size_of::<libc::ucred>() as u32;
                // SAFETY: reads the peer credentials into `credentials`.
                let ok = unsafe {
                    libc::getsockopt(
                        peer.as_raw_fd(),
                        libc::SOL_SOCKET,
                        libc::SO_PEERCRED,
                        (&mut credentials as *mut libc::ucred).cast(),
                        &mut length,
                    )
                } == 0;
                // SO_PEERCRED names the thread group, not the connecting thread.
                if !ok || credentials.pid != tgid || credentials.uid != unsafe { libc::getuid() } {
                    return Self::finish(tid, t, -(libc::EACCES as i64));
                }
                if send_fd(peer.as_raw_fd(), t.file.as_raw_fd()).is_err() {
                    return Self::finish(tid, t, -(libc::EIO as i64));
                }
                t.stage = Stage::Receive;
                let flags = if t.mode & libc::O_CLOEXEC as u32 != 0 {
                    libc::MSG_CMSG_CLOEXEC as u64
                } else {
                    0
                };
                let args = [t.socket as u64, t.message, flags];
                Self::inject(tid, t, nr::recvmsg, &args)
            }
            Stage::Receive => {
                let mut control = [0u8; 24];
                let mut message = [0u8; 56];
                if result != 1
                    || mem::read(tid, t.control, &mut control).is_err()
                    || mem::read(tid, t.message, &mut message).is_err()
                {
                    return Self::finish(tid, t, -(libc::EIO as i64));
                }
                let flags = i32::from_ne_bytes(message[48..52].try_into().unwrap());
                let level = i32::from_ne_bytes(control[8..12].try_into().unwrap());
                let kind = i32::from_ne_bytes(control[12..16].try_into().unwrap());
                if flags & libc::MSG_CTRUNC != 0
                    || level != libc::SOL_SOCKET
                    || kind != libc::SCM_RIGHTS
                {
                    return Self::finish(tid, t, -(libc::EPROTO as i64));
                }
                let fd = i32::from_ne_bytes(control[16..20].try_into().unwrap());
                let file = Rc::new(RefCell::new(OpenFile {
                    id: t.id,
                    offset: 0,
                    mode: t.mode & !(libc::O_CLOEXEC as u32),
                }));
                self.table(tgid).borrow_mut().insert(
                    fd,
                    Entry {
                        file,
                        cloexec: t.mode & libc::O_CLOEXEC as u32 != 0,
                    },
                );
                Self::finish(tid, t, fd as i64)
            }
            Stage::Close => unreachable!(),
        }
    }
}

fn send_fd(socket: i32, fd: i32) -> std::io::Result<()> {
    let mut byte = 0u8;
    let mut vector = libc::iovec {
        iov_base: (&mut byte as *mut u8).cast(),
        iov_len: 1,
    };
    let mut control = [0u64; 4];
    // SAFETY: builds an SCM_RIGHTS message over local buffers.
    unsafe {
        let mut message: libc::msghdr = std::mem::zeroed();
        message.msg_iov = &mut vector;
        message.msg_iovlen = 1;
        message.msg_control = control.as_mut_ptr().cast();
        message.msg_controllen = libc::CMSG_SPACE(4) as _;
        let header = libc::CMSG_FIRSTHDR(&message);
        (*header).cmsg_level = libc::SOL_SOCKET;
        (*header).cmsg_type = libc::SCM_RIGHTS;
        (*header).cmsg_len = libc::CMSG_LEN(4) as _;
        libc::CMSG_DATA(header).cast::<i32>().write_unaligned(fd);
        loop {
            match libc::sendmsg(socket, &message, libc::MSG_NOSIGNAL) {
                1 => return Ok(()),
                -1 if std::io::Error::last_os_error().raw_os_error() == Some(libc::EINTR) => {
                    continue;
                }
                _ => return Err(std::io::Error::last_os_error()),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_flags_match_the_server_bits() {
        assert_eq!(open_flags(libc::O_RDONLY as u32), 1);
        assert_eq!(
            open_flags((libc::O_RDWR | libc::O_CREAT | libc::O_TRUNC) as u32),
            1 | 2 | 4 | 16
        );
        assert_eq!(
            open_flags((libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL) as u32),
            2 | 4 | 8
        );
        assert_eq!(
            open_flags((libc::O_PATH | libc::O_DIRECTORY) as u32),
            1 | 32
        );
    }

    #[test]
    fn stat_layouts_have_the_kernel_sizes() {
        let r = Reply {
            id: 7,
            length: 1000,
            mode: 0o644,
            ..Reply::default()
        };
        let s = stat_bytes(&r);
        assert_eq!(
            s.len(),
            if cfg!(target_arch = "x86_64") {
                144
            } else {
                128
            }
        );
        assert_eq!(u64::from_ne_bytes(s[8..16].try_into().unwrap()), 7);
        assert_eq!(u64::from_ne_bytes(s[48..56].try_into().unwrap()), 1000);
        let x = statx_bytes(&r);
        assert_eq!(
            u16::from_ne_bytes(x[28..30].try_into().unwrap()) as u32,
            libc::S_IFREG as u32 | 0o644
        );
    }
}
