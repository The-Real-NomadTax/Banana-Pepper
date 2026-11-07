//! Unprivileged guest namespace. The process tracer brokers operations into the
//! same View used by FUSE/WinFsp. Sparse memfds are transient page caches; only
//! accessed ranges are populated, and changed blocks are committed to View.
use crate::{FileHandle, View};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    ffi::CString,
    fs::{self, File},
    io::{self, Read, Write},
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::{
            fs::FileExt,
            net::{UnixListener, UnixStream},
        },
    },
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};
const BLOCK: u64 = 65536;
const LIMIT: u64 = 256 * 1024 * 1024;
pub const PROTOCOL: u32 = 1;
struct Object {
    handle: FileHandle,
    path: String,
    linked: bool,
    cache: File,
    blocks: BTreeMap<u64, [u8; 32]>,
    mapped: bool,
    dirty: bool,
}
struct Mapping {
    guest: String,
    source: String,
    readonly: bool,
}
struct State {
    view: Arc<View>,
    objects: BTreeMap<u64, Object>,
    mappings: Vec<Mapping>,
    tag: String,
    cached: u64,
}
pub struct Server {
    pub socket: PathBuf,
    state: Arc<Mutex<State>>,
    stop: Arc<AtomicBool>,
    threads: Vec<thread::JoinHandle<()>>,
}
#[derive(Default, Debug)]
struct Reply {
    error: i32,
    id: u64,
    len: u64,
    mode: u32,
    directory: u32,
    modified: u64,
    accessed: u64,
    created: u64,
    body: Vec<u8>,
}
fn error(e: io::Error) -> i32 {
    e.raw_os_error().unwrap_or(match e.kind() {
        io::ErrorKind::NotFound => libc::ENOENT,
        io::ErrorKind::AlreadyExists => libc::EEXIST,
        io::ErrorKind::PermissionDenied => libc::EACCES,
        io::ErrorKind::NotADirectory => libc::ENOTDIR,
        io::ErrorKind::IsADirectory => libc::EISDIR,
        io::ErrorKind::DirectoryNotEmpty => libc::ENOTEMPTY,
        io::ErrorKind::InvalidInput => libc::EINVAL,
        _ => libc::EIO,
    })
}
fn err(n: i32) -> io::Error {
    io::Error::from_raw_os_error(n)
}
fn absolute(path: &str) -> io::Result<String> {
    if !path.starts_with('/') || path.contains('\0') {
        return Err(err(libc::EINVAL));
    }
    let mut parts = Vec::new();
    for p in path.split('/') {
        match p {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            _ => parts.push(p),
        }
    }
    Ok(format!("/{}", parts.join("/")))
}
impl State {
    fn resolve(&self, p: &str, write: bool) -> io::Result<String> {
        let p = absolute(p)?;
        let m = self
            .mappings
            .iter()
            .find(|m| p == m.guest || p.strip_prefix(&m.guest).is_some_and(|s| s.starts_with('/')))
            .ok_or(err(libc::EXDEV))?;
        if write && m.readonly {
            return Err(err(libc::EROFS));
        }
        let tail = p.strip_prefix(&m.guest).unwrap().trim_start_matches('/');
        let result = [m.source.as_str(), tail]
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("/");
        if !result.is_empty() {
            banana_content::path(&result)?;
        }
        self.view.canonical_path(&result)
    }
    fn sync(&mut self, id: u64) -> io::Result<()> {
        let ids = if id == 0 {
            self.objects.keys().copied().collect()
        } else {
            vec![id]
        };
        for id in ids {
            let o = self.objects.get_mut(&id).ok_or(err(libc::EBADF))?;
            if o.handle.metadata()?.directory || (!o.dirty && !o.mapped) {
                continue;
            }
            let length = o.cache.metadata()?.len();
            let old = o.handle.metadata()?.len;
            if length != old && o.linked {
                self.view.truncate(&o.path, length)?;
            }
            for (&block, hash) in &mut o.blocks {
                if block * BLOCK >= length {
                    continue;
                }
                let mut bytes = vec![0; ((length - block * BLOCK).min(BLOCK)) as usize];
                o.cache.read_exact_at(&mut bytes, block * BLOCK)?;
                let next: [u8; 32] = Sha256::digest(&bytes).into();
                if *hash != next {
                    o.handle.write(block * BLOCK, &bytes)?;
                    *hash = next;
                }
            }
            o.dirty = false;
        }
        self.view.flush()
    }
    fn hydrate(&mut self, id: u64, offset: u64, len: u64, flags: u32) -> io::Result<()> {
        self.sync(id)?;
        let o = self.objects.get_mut(&id).ok_or(err(libc::EBADF))?;
        if o.handle.metadata()?.directory {
            return Err(err(libc::EISDIR));
        }
        let end = offset.checked_add(len).ok_or(err(libc::EINVAL))?;
        let end = if flags & 1 != 0 {
            end
        } else {
            end.min(o.cache.metadata()?.len())
        };
        if end <= offset {
            return Ok(());
        }
        let begin = offset / BLOCK;
        let last = (end - 1) / BLOCK;
        let missing = (begin..=last).filter(|b| !o.blocks.contains_key(b)).count() as u64 * BLOCK;
        if self.cached + missing > LIMIT {
            return Err(err(libc::ENOMEM));
        }
        for b in begin..=last {
            if o.blocks.contains_key(&b) {
                continue;
            }
            let mut data =
                vec![0; ((o.handle.metadata()?.len.saturating_sub(b * BLOCK)).min(BLOCK)) as usize];
            let n = o.handle.read(b * BLOCK, &mut data)?;
            data.truncate(n);
            o.cache.write_all_at(&data, b * BLOCK)?;
            o.blocks.insert(b, Sha256::digest(&data).into());
            self.cached += BLOCK;
        }
        o.mapped |= flags & 2 != 0;
        o.dirty |= flags & 1 != 0;
        Ok(())
    }
    fn metadata(&self, p: &str, id: u64) -> io::Result<Reply> {
        let (m, a, inode) = if id != 0 {
            let o = self.objects.get(&id).ok_or(err(libc::EBADF))?;
            (o.handle.metadata()?, o.handle.attributes()?, id)
        } else {
            (
                self.view.metadata(p)?,
                self.view.attributes(p)?,
                self.view.identity(p)?,
            )
        };
        Ok(Reply {
            id: inode,
            len: m.len,
            mode: a.mode.unwrap_or(if m.directory || m.executable {
                0o755
            } else {
                0o644
            }) as u32,
            directory: m.directory as u32,
            modified: a.modified_ns,
            accessed: a.accessed_ns,
            created: a.created_ns,
            ..Default::default()
        })
    }
    fn request(
        &mut self,
        op: u32,
        flags: u32,
        id: u64,
        offset: u64,
        len: u64,
        p: &str,
        q: &str,
    ) -> io::Result<Reply> {
        if op == 0 {
            return Ok(Reply {
                body: self.resolve(p, false)?.into_bytes(),
                ..Default::default()
            });
        }
        if op == 10 {
            let guest = absolute(p)?;
            let source = if q.is_empty() {
                String::new()
            } else {
                banana_content::path(q)?.to_owned()
            };
            self.mappings.retain(|m| m.guest != guest);
            self.mappings.push(Mapping {
                guest,
                source,
                readonly: flags != 0,
            });
            self.mappings
                .sort_by_key(|m| std::cmp::Reverse(m.guest.len()));
            return Ok(Reply::default());
        }
        if op == 14 {
            self.resolve(p, false)?;
            return Ok(Reply {
                body: absolute(p)?.into_bytes(),
                ..Default::default()
            });
        }
        if op == 11 {
            return Ok(Reply {
                len: PROTOCOL as u64,
                body: self.tag.as_bytes().to_vec(),
                ..Default::default()
            });
        }
        self.sync(0)?;
        let path = if p.is_empty() {
            String::new()
        } else {
            self.resolve(
                p,
                matches!(op, 5..=9) || (op == 2 && flags & (2 | 4 | 16) != 0),
            )?
        };
        match op {
            15 => {
                let object = self.objects.get(&id).ok_or(err(libc::EBADF))?;
                if object.handle.metadata()?.directory {
                    return Err(err(libc::EISDIR));
                }
                let mut body = vec![0; len.min(65536) as usize];
                let count = object.handle.read(offset, &mut body)?;
                body.truncate(count);
                Ok(Reply {
                    len: count as u64,
                    body,
                    ..Default::default()
                })
            }
            1 => self.metadata(&path, id),
            2 => {
                let exists = self.view.metadata(&path).is_ok();
                if exists && flags & 12 == 12 {
                    return Err(err(libc::EEXIST));
                }
                if !exists {
                    if flags & 4 == 0 {
                        return Err(err(libc::ENOENT));
                    }
                    self.view.create(&path, false)?;
                }
                let m = self.view.metadata(&path)?;
                if flags & 32 != 0 && !m.directory {
                    return Err(err(libc::ENOTDIR));
                }
                if m.directory && flags & 2 != 0 {
                    return Err(err(libc::EISDIR));
                }
                let inode = self.view.identity(&path)?;
                if !self.objects.contains_key(&inode) {
                    let name = CString::new(format!("banana-vfs-{}-{inode}", self.tag)).unwrap();
                    let fd = unsafe {
                        libc::syscall(libc::SYS_memfd_create, name.as_ptr(), libc::MFD_CLOEXEC)
                            as i32
                    };
                    if fd < 0 {
                        return Err(io::Error::last_os_error());
                    }
                    let cache = unsafe { File::from_raw_fd(fd) };
                    cache.set_len(m.len)?;
                    {
                        use std::os::unix::fs::PermissionsExt;
                        cache.set_permissions(fs::Permissions::from_mode(if m.executable {
                            0o700
                        } else {
                            0o600
                        }))?;
                    }
                    let handle = self.view.open_file(&path)?;
                    self.objects.insert(
                        inode,
                        Object {
                            handle,
                            path: path.clone(),
                            linked: true,
                            cache,
                            blocks: BTreeMap::new(),
                            mapped: false,
                            dirty: false,
                        },
                    );
                }
                if flags & 16 != 0 {
                    if flags & 2 == 0 {
                        return Err(err(libc::EACCES));
                    }
                    self.truncate(inode, 0)?;
                }
                let mut r = self.metadata(&path, inode)?;
                r.body = format!(
                    "/proc/{}/fd/{}",
                    std::process::id(),
                    self.objects[&inode].cache.as_raw_fd()
                )
                .into_bytes();
                Ok(r)
            }
            3 => {
                self.hydrate(id, offset, len, flags)?;
                Ok(Reply::default())
            }
            4 => {
                self.sync(id)?;
                Ok(Reply::default())
            }
            5 => {
                let id = if id == 0 {
                    self.objects
                        .iter()
                        .find(|(_, o)| o.path == path && o.linked)
                        .map(|(&i, _)| i)
                        .unwrap_or(0)
                } else {
                    id
                };
                if id == 0 {
                    self.view.truncate(&path, len)?
                } else {
                    self.truncate(id, len)?;
                }
                Ok(Reply::default())
            }
            6 => {
                let m = self.view.metadata(&path)?;
                if m.directory != (flags != 0) {
                    return Err(err(if m.directory {
                        libc::EISDIR
                    } else {
                        libc::ENOTDIR
                    }));
                }
                self.view.remove(&path)?;
                for o in self.objects.values_mut() {
                    if o.path == path {
                        o.linked = false;
                    }
                }
                Ok(Reply::default())
            }
            7 => {
                let target = self.resolve(q, true)?;
                self.view.rename(&path, &target, flags == 0)?;
                for o in self.objects.values_mut() {
                    if o.path == target {
                        o.linked = false;
                    }
                    if o.path == path || o.path.starts_with(&format!("{path}/")) {
                        o.path = format!("{target}{}", &o.path[path.len()..]);
                    }
                }
                Ok(Reply::default())
            }
            8 => {
                self.view.create(&path, true)?;
                Ok(Reply::default())
            }
            9 => {
                let path = if id == 0 {
                    path
                } else {
                    self.objects.get(&id).ok_or(err(libc::EBADF))?.path.clone()
                };
                let mut a = self.view.attributes(&path)?;
                if flags & 0x80000000 != 0 {
                    a.mode = Some((flags & 0xfff) as u16)
                }
                if offset != u64::MAX {
                    a.modified_ns = offset
                }
                if len != u64::MAX {
                    a.accessed_ns = len
                }
                self.view.set_attributes(&path, a)?;
                Ok(Reply::default())
            }
            12 => {
                let o = self.objects.get(&id).ok_or(err(libc::EBADF))?;
                if !o.handle.metadata()?.directory {
                    return Err(err(libc::ENOTDIR));
                }
                let entries = self.view.read_dir(&o.path)?;
                let mut all = vec![(".".to_owned(), id, true), ("..".to_owned(), id, true)];
                for e in entries {
                    let path = if o.path.is_empty() {
                        e.name.clone()
                    } else {
                        format!("{}/{}", o.path, e.name)
                    };
                    all.push((e.name, self.view.identity(&path)?, e.metadata.directory));
                }
                let mut body = Vec::new();
                let mut next = offset;
                for (i, (name, inode, dir)) in all.into_iter().enumerate().skip(offset as usize) {
                    let size = (19 + name.len() + 1 + 7) & !7;
                    if body.len() + size > len.min(65536) as usize {
                        if body.is_empty() {
                            return Err(err(libc::EINVAL));
                        }
                        break;
                    }
                    body.extend(inode.to_le_bytes());
                    body.extend(((i + 1) as u64).to_le_bytes());
                    body.extend((size as u16).to_le_bytes());
                    body.push(if dir { 4 } else { 8 });
                    body.extend(name.as_bytes());
                    body.resize(body.len() + size - 19 - name.len(), 0);
                    next = (i + 1) as u64;
                }
                Ok(Reply {
                    len: next,
                    body,
                    ..Default::default()
                })
            }
            13 => {
                let o = self.objects.get(&id).ok_or(err(libc::EBADF))?;
                if !o.linked && flags == 0 {
                    return Err(err(libc::ENOENT));
                }
                let root = self
                    .mappings
                    .iter()
                    .find(|m| m.source.is_empty() && !m.readonly)
                    .ok_or(err(libc::EIO))?;
                Ok(Reply {
                    body: format!(
                        "{}/{}{}",
                        root.guest,
                        o.path,
                        if o.linked { "" } else { " (deleted)" }
                    )
                    .into_bytes(),
                    ..Default::default()
                })
            }
            _ => Err(err(libc::ENOSYS)),
        }
    }
    fn write_bytes(&mut self, id: u64, offset: u64, bytes: &[u8]) -> io::Result<Reply> {
        self.sync(0)?;
        let object = self.objects.get_mut(&id).ok_or(err(libc::EBADF))?;
        if object.handle.metadata()?.directory {
            return Err(err(libc::EISDIR));
        }
        let count = object.handle.write(offset, bytes)?;
        object.cache.set_len(object.handle.metadata()?.len)?;
        let end = offset.checked_add(count as u64).ok_or(err(libc::EINVAL))?;
        for (&block, _) in &object.blocks {
            let begin = offset.max(block * BLOCK);
            let stop = end.min((block + 1) * BLOCK);
            if begin < stop {
                object.cache.write_all_at(
                    &bytes[(begin - offset) as usize..(stop - offset) as usize],
                    begin,
                )?;
            }
        }
        Ok(Reply {
            len: count as u64,
            ..Default::default()
        })
    }
    fn truncate(&mut self, id: u64, len: u64) -> io::Result<()> {
        self.sync(id)?;
        let o = self.objects.get_mut(&id).ok_or(err(libc::EBADF))?;
        if !o.linked {
            return Err(err(libc::ENOTSUP));
        }
        self.view.truncate(&o.path, len)?;
        o.cache.set_len(len)?;
        let old = o.blocks.len();
        o.blocks.retain(|&b, _| b * BLOCK < len);
        self.cached -= ((old - o.blocks.len()) as u64) * BLOCK;
        if len % BLOCK != 0 {
            let b = len / BLOCK;
            if let Some(hash) = o.blocks.get_mut(&b) {
                let mut bytes = vec![0; (len % BLOCK) as usize];
                o.cache.read_exact_at(&mut bytes, b * BLOCK)?;
                *hash = Sha256::digest(bytes).into();
            }
        }
        Ok(())
    }
}
impl Server {
    pub fn start(view: Arc<View>, socket: &Path, root: &str) -> io::Result<Self> {
        let listener = UnixListener::bind(socket)?;
        listener.set_nonblocking(true)?;
        let tag = format!(
            "{:x}{:x}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let state = Arc::new(Mutex::new(State {
            view,
            objects: BTreeMap::new(),
            mappings: vec![Mapping {
                guest: absolute(root)?,
                source: String::new(),
                readonly: false,
            }],
            tag,
            cached: 0,
        }));
        let stop = Arc::new(AtomicBool::new(false));
        let mut threads = Vec::new();
        let s = state.clone();
        let done = stop.clone();
        threads.push(thread::spawn(move || {
            while !done.load(Ordering::Acquire) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        let _ = stream.set_read_timeout(Some(Duration::from_millis(200)));
                        loop {
                            if done.load(Ordering::Acquire) {
                                break;
                            }
                            let mut h = [0u8; 40];
                            if receive(&mut stream, &mut h, &done).is_err() {
                                break;
                            }
                            let u32at = |n| u32::from_le_bytes(h[n..n + 4].try_into().unwrap());
                            let u64at = |n| u64::from_le_bytes(h[n..n + 8].try_into().unwrap());
                            let a = u32at(32) as usize;
                            let b = u32at(36) as usize;
                            if a > (if u32at(0) == 16 { 65536 } else { 4096 }) || b > 4096 {
                                break;
                            }
                            let mut paths = vec![0; a + b];
                            if receive(&mut stream, &mut paths, &done).is_err() {
                                break;
                            }
                            let result = (|| {
                                if u32at(0) == 16 {
                                    return s.lock().unwrap().write_bytes(
                                        u64at(8),
                                        u64at(16),
                                        &paths[..a],
                                    );
                                }

                                let p = std::str::from_utf8(&paths[..a])
                                    .map_err(|_| err(libc::EINVAL))?;
                                let q = std::str::from_utf8(&paths[a..])
                                    .map_err(|_| err(libc::EINVAL))?;
                                s.lock().unwrap().request(
                                    u32at(0),
                                    u32at(4),
                                    u64at(8),
                                    u64at(16),
                                    u64at(24),
                                    p,
                                    q,
                                )
                            })();
                            let r = result.unwrap_or_else(|e| Reply {
                                error: error(e),
                                ..Default::default()
                            });
                            if send(&mut stream, &r).is_err() {
                                break;
                            }
                        }
                    }
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(10))
                    }
                    Err(_) => break,
                }
            }
            let _ = s.lock().unwrap().sync(0);
        }));
        let s = state.clone();
        let done = stop.clone();
        threads.push(thread::spawn(move || {
            while !done.load(Ordering::Acquire) {
                thread::sleep(Duration::from_millis(100));
                let _ = s.lock().unwrap().sync(0);
            }
        }));
        Ok(Self {
            socket: socket.to_path_buf(),
            state,
            stop,
            threads,
        })
    }
    pub fn map(&self, guest: &str, source: &str, readonly: bool) -> io::Result<()> {
        self.state
            .lock()
            .unwrap()
            .request(10, readonly as u32, 0, 0, 0, guest, source)
            .map(|_| ())
    }
    pub fn flush(&self) -> io::Result<()> {
        self.state.lock().unwrap().sync(0)
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        for t in self.threads.drain(..) {
            let _ = t.join();
        }
        let _ = self.flush();
        let _ = fs::remove_file(&self.socket);
    }
}
fn receive(stream: &mut UnixStream, mut bytes: &mut [u8], stop: &AtomicBool) -> io::Result<()> {
    while !bytes.is_empty() {
        if stop.load(Ordering::Acquire) {
            return Err(io::ErrorKind::Interrupted.into());
        }
        match stream.read(bytes) {
            Ok(0) => return Err(io::ErrorKind::UnexpectedEof.into()),
            Ok(n) => {
                bytes = &mut bytes[n..];
            }
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::Interrupted
                        | io::ErrorKind::WouldBlock
                        | io::ErrorKind::TimedOut
                ) =>
            {
                continue;
            }
            Err(e) => return Err(e),
        }
    }
    Ok(())
}
fn send(s: &mut UnixStream, r: &Reply) -> io::Result<()> {
    let mut out = Vec::new();
    out.extend(r.error.to_le_bytes());
    out.extend((r.body.len() as u32).to_le_bytes());
    out.extend(r.id.to_le_bytes());
    out.extend(r.len.to_le_bytes());
    out.extend(r.mode.to_le_bytes());
    out.extend(r.directory.to_le_bytes());
    out.extend(r.modified.to_le_bytes());
    out.extend(r.accessed.to_le_bytes());
    out.extend(r.created.to_le_bytes());
    out.extend(&r.body);
    s.write_all(&out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::MetadataExt;
    #[test]
    fn writable_open_and_streaming_reads_keep_the_mapping_cache_sparse() {
        let temp = tempfile::tempdir().unwrap();
        let source = Arc::new(banana_content::MemorySource {
            files: BTreeMap::from([("data".into(), Arc::from(vec![1; 1024 * 1024]))]),
        });
        let view = Arc::new(
            View::new(
                source.clone(),
                &temp.path().join("overlay"),
                "fixture",
                BTreeMap::new(),
                BTreeMap::new(),
            )
            .unwrap(),
        );
        let server = Server::start(view.clone(), &temp.path().join("vfs.sock"), "/game").unwrap();
        {
            let mut state = server.state.lock().unwrap();
            let opened = state.request(2, 3, 0, 0, 0, "/game/data", "").unwrap();
            assert_eq!(
                state.objects[&opened.id].cache.metadata().unwrap().blocks(),
                0
            );
            assert!(!view.has_content_override("data"));
            assert_eq!(
                state
                    .request(15, 0, opened.id, 0, 65536, "", "")
                    .unwrap()
                    .body,
                vec![1; 65536]
            );
            assert_eq!(state.cached, 0);
            state.write_bytes(opened.id, 123, b"X").unwrap();
            assert_eq!(
                state.objects[&opened.id].cache.metadata().unwrap().blocks(),
                0
            );
            let mut bytes = [0; 1];
            view.read("data", 123, &mut bytes).unwrap();
            assert_eq!(bytes, b"X"[..]);
            state.hydrate(opened.id, 0, 4096, 2).unwrap();
            state.objects[&opened.id]
                .cache
                .write_all_at(b"Y", 124)
                .unwrap();
            state.sync(0).unwrap();
            view.read("data", 124, &mut bytes).unwrap();
            assert_eq!(bytes, b"Y"[..]);
        }
        server.map("/readonly", "", true).unwrap();
        assert_eq!(
            server
                .state
                .lock()
                .unwrap()
                .request(2, 2, 0, 0, 0, "/readonly/data", "")
                .unwrap_err()
                .raw_os_error(),
            Some(libc::EROFS)
        );
        assert_eq!(
            server
                .state
                .lock()
                .unwrap()
                .resolve("/game/../outside", false)
                .unwrap_err()
                .raw_os_error(),
            Some(libc::EXDEV)
        );
        assert_eq!(
            banana_content::read_all(source.as_ref(), "data", 2 * 1024 * 1024).unwrap(),
            vec![1; 1024 * 1024]
        );
        assert_eq!(
            fs::read_dir(temp.path().join("overlay/blocks"))
                .unwrap()
                .count(),
            2
        );
    }
}
