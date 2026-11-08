//! The trace loop: spawns the command, follows every thread and process it
//! creates, and rewrites system calls so the guest sees its own root.
use crate::{
    arch::{self, Regs, Stop, nr},
    exec,
    mem::{self, Scratch},
    path::{self, Fs},
    view::{self, View},
};
use std::{
    cell::RefCell,
    collections::HashMap,
    ffi::CString,
    io,
    path::{Path, PathBuf},
};

/// What the tracer was asked to run (PRoot's options, same spelling).
#[derive(Clone, Debug, Default)]
pub struct Options {
    pub root: PathBuf,
    /// Host path, guest path.
    pub binds: Vec<(PathBuf, String)>,
    /// Initial guest working directory.
    pub cwd: String,
    /// `-0`: the guest runs as root (ids, ownership, chown).
    pub fake_root: bool,
    /// `--link2symlink`: hard links become symbolic links (Android denies link()).
    pub link2symlink: bool,
    /// `--kill-on-exit`: when the command exits, kill everything it started.
    pub kill_on_exit: bool,
    pub verbose: u8,
    pub argv: Vec<String>,
}

/// Fixes to apply when a rewritten system call returns.
#[derive(Default)]
struct Exit {
    /// Argument registers to put back (callers may rely on them).
    restore: Vec<(usize, u64)>,
    /// The call was skipped: its result.
    result: Option<i64>,
    fix: Fix,
    /// View bookkeeping (or a descriptor handoff) for this call.
    view: Option<view::Pending>,
}

#[derive(Default)]
enum Fix {
    #[default]
    None,
    /// getcwd(buf, size): the kernel's host path becomes the guest path.
    Getcwd { buffer: u64, size: u64 },
    /// readlink of a /proc magic link: host target to guest path.
    ProcLink { buffer: u64, size: u64 },
    /// stat-family result: the tracer's ids read as root.
    Owner { buffer: u64, offset: usize },
}

struct Thread {
    tgid: i32,
    /// Created by a traced clone; its first SIGSTOP is the attach stop.
    awaiting_stop: bool,
    exit: Exit,
    /// Handled at a seccomp stop; the syscall-entry stop that follows is not new.
    entered: bool,
}

pub struct Tracer {
    fs: Fs,
    options: Options,
    threads: HashMap<i32, Thread>,
    /// Guest program path per process, for /proc/<pid>/exe.
    programs: HashMap<i32, String>,
    /// Program a pending execve will run, per process.
    executing: HashMap<i32, String>,
    uid: u32,
    gid: u32,
    /// The session's view server (`BANANA_VFS_SOCKET`), when it has one.
    view: RefCell<Option<View>>,
    /// A seccomp filter stops the tracee only for calls the tracer handles.
    seccomp: bool,
}

/// Guest path arguments a view-owned path can appear in: (dirfd, path).
type PathArg = Option<(Option<usize>, usize)>;
fn view_paths(call: i64) -> (PathArg, PathArg) {
    let one = |d, a| (Some((d, a)), None);
    match call {
        c if [
            nr::open,
            nr::creat,
            nr::stat,
            nr::lstat,
            nr::access,
            nr::unlink,
            nr::rmdir,
            nr::mkdir,
            nr::chmod,
            nr::truncate,
            nr::readlink,
            nr::chdir,
            nr::mknod,
            nr::link,
        ]
        .contains(&c) =>
        {
            one(None, 0)
        }
        c if c == nr::symlink => one(None, 1),
        c if c == nr::rename => (Some((None, 0)), Some((None, 1))),
        c if [
            nr::openat,
            nr::openat2,
            nr::newfstatat,
            nr::statx,
            nr::faccessat,
            nr::faccessat2,
            nr::unlinkat,
            nr::mkdirat,
            nr::fchmodat,
            nr::fchmodat2,
            nr::utimensat,
            nr::readlinkat,
            nr::mknodat,
            nr::linkat,
        ]
        .contains(&c) =>
        {
            one(Some(0), 1)
        }
        c if c == nr::renameat || c == nr::renameat2 => (Some((Some(0), 1)), Some((Some(2), 3))),
        c if c == nr::symlinkat => one(Some(1), 2),
        _ => (None, None),
    }
}

/// Calls the tracer handles. A seccomp filter stops the tracee for these
/// only; descriptor calls matter only with a view, ids only with `-0`.
fn traced_calls(view: bool, fake_root: bool) -> Vec<i64> {
    let mut calls = vec![
        nr::open,
        nr::creat,
        nr::stat,
        nr::lstat,
        nr::fstat,
        nr::access,
        nr::truncate,
        nr::chdir,
        nr::rename,
        nr::mkdir,
        nr::rmdir,
        nr::link,
        nr::unlink,
        nr::symlink,
        nr::readlink,
        nr::chmod,
        nr::chown,
        nr::lchown,
        nr::fchown,
        nr::utime,
        nr::utimes,
        nr::mknod,
        nr::statfs,
        nr::chroot,
        nr::setxattr,
        nr::lsetxattr,
        nr::getxattr,
        nr::lgetxattr,
        nr::listxattr,
        nr::llistxattr,
        nr::removexattr,
        nr::lremovexattr,
        nr::inotify_add_watch,
        nr::openat,
        nr::openat2,
        nr::mkdirat,
        nr::mknodat,
        nr::fchownat,
        nr::futimesat,
        nr::newfstatat,
        nr::unlinkat,
        nr::renameat,
        nr::renameat2,
        nr::linkat,
        nr::symlinkat,
        nr::readlinkat,
        nr::fchmodat,
        nr::fchmodat2,
        nr::faccessat,
        nr::faccessat2,
        nr::utimensat,
        nr::statx,
        nr::execve,
        nr::execveat,
        nr::getcwd,
        nr::bind,
        nr::connect,
    ];
    if fake_root {
        calls.extend([
            nr::getuid,
            nr::geteuid,
            nr::getgid,
            nr::getegid,
            nr::getresuid,
            nr::getresgid,
            nr::setuid,
            nr::setgid,
            nr::setreuid,
            nr::setregid,
            nr::setresuid,
            nr::setresgid,
            nr::setfsuid,
            nr::setfsgid,
            nr::setgroups,
        ]);
    }
    if view {
        calls.extend([
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
            nr::dup,
            nr::dup2,
            nr::dup3,
            nr::mremap,
            nr::msync,
            nr::copy_file_range,
            nr::sendfile,
            nr::splice,
        ]);
    }
    calls.retain(|&c| c >= 0);
    calls.sort_unstable();
    calls.dedup();
    calls
}

/// A classic BPF program: trap `calls` (and any foreign architecture) to
/// the tracer, allow everything else.
fn seccomp_program(calls: &[i64]) -> Vec<libc::sock_filter> {
    const LOAD: u16 = 0x20; // BPF_LD | BPF_W | BPF_ABS
    const JEQ: u16 = 0x15; // BPF_JMP | BPF_JEQ | BPF_K
    const RET: u16 = 0x06; // BPF_RET | BPF_K
    const TRACE: u32 = 0x7ff0_0000;
    const ALLOW: u32 = 0x7fff_0000;
    let op = |code, jt, jf, k| libc::sock_filter { code, jt, jf, k };
    let mut program = vec![
        op(LOAD, 0, 0, 4), // seccomp_data.arch
        op(JEQ, 1, 0, arch::AUDIT_ARCH),
        op(RET, 0, 0, TRACE),
        op(LOAD, 0, 0, 0), // seccomp_data.nr
    ];
    let n = calls.len();
    for (i, &call) in calls.iter().enumerate() {
        // On a match, skip the remaining tests and the ALLOW to reach TRACE.
        program.push(op(JEQ, (n - i) as u8, 0, call as u32));
    }
    program.push(op(RET, 0, 0, ALLOW));
    program.push(op(RET, 0, 0, TRACE));
    program
}

/// `Seccomp_filters` of a process (Linux 5.9+).
fn seccomp_filters(pid: &str) -> Option<u32> {
    std::fs::read_to_string(format!("/proc/{pid}/status"))
        .ok()?
        .lines()
        .find_map(|l| l.strip_prefix("Seccomp_filters:"))
        .and_then(|v| v.trim().parse().ok())
}

/// Programs for exec planning: the view first, then the guest root.
struct Programs<'a> {
    tracer: &'a Tracer,
    tgid: i32,
    tid: i32,
}

impl exec::Programs for Programs<'_> {
    fn head(&self, guest: &str, limit: usize) -> Result<Vec<u8>, i32> {
        if let Some(view) = self.tracer.view.borrow_mut().as_mut()
            && view.owns(guest)?
        {
            return view.head(guest, limit);
        }
        use std::io::Read;
        let host = self.tracer.fs.translate(guest, true, self.tgid, self.tid)?;
        let file = std::fs::File::open(&host).map_err(errno)?;
        if file.metadata().is_ok_and(|m| m.is_dir()) {
            return Err(libc::EACCES);
        }
        let mut head = Vec::new();
        file.take(limit as u64)
            .read_to_end(&mut head)
            .map_err(errno)?;
        Ok(head)
    }
    fn host(&self, guest: &str) -> Result<PathBuf, i32> {
        if let Some(view) = self.tracer.view.borrow_mut().as_mut()
            && view.owns(guest)?
        {
            return view.program(guest);
        }
        self.tracer.fs.translate(guest, true, self.tgid, self.tid)
    }
}

fn errno(e: io::Error) -> i32 {
    e.raw_os_error().unwrap_or(libc::EIO)
}

fn thread_group(tid: i32) -> i32 {
    std::fs::read_to_string(format!("/proc/{tid}/status"))
        .ok()
        .and_then(|s| {
            s.lines()
                .find_map(|l| l.strip_prefix("Tgid:"))
                .and_then(|v| v.trim().parse().ok())
        })
        .unwrap_or(tid)
}

fn ptrace(request: libc::c_uint, tid: i32, data: usize) -> io::Result<libc::c_long> {
    // SAFETY: requests used here take an integer or no data argument.
    let r = unsafe { libc::ptrace(request as _, tid, 0usize, data) };
    if r < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(r)
    }
}

fn event_message(tid: i32) -> io::Result<u64> {
    let mut value: libc::c_ulong = 0;
    // SAFETY: GETEVENTMSG writes one unsigned long.
    let r = unsafe { libc::ptrace(libc::PTRACE_GETEVENTMSG as _, tid, 0usize, &mut value) };
    if r < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(value as u64)
    }
}

/// Follow the flags of a path argument.
enum Follow {
    Yes,
    No,
    /// Follows unless the flag in argument `.0` has bit `.1`.
    Unless(usize, u64),
}

impl Tracer {
    pub fn new(options: Options) -> Self {
        let fs = Fs::new(options.root.clone(), options.binds.clone());
        // SAFETY: plain id queries.
        let (uid, gid) = unsafe { (libc::getuid(), libc::getgid()) };
        Self {
            fs,
            options,
            threads: HashMap::new(),
            programs: HashMap::new(),
            executing: HashMap::new(),
            uid,
            gid,
            view: RefCell::new(None),
            seccomp: false,
        }
    }

    fn log(&self, level: u8, message: impl FnOnce() -> String) {
        if self.options.verbose >= level {
            eprintln!("banana-tracer: {}", message());
        }
    }

    /// Run the command to completion; its exit status (128+signal if killed).
    pub fn run(mut self) -> io::Result<i32> {
        let program = self
            .options
            .argv
            .first()
            .cloned()
            .ok_or_else(|| io::Error::other("No command"))?;
        let cwd = path::normalize(&self.options.cwd);
        if let Ok(socket) = std::env::var("BANANA_VFS_SOCKET") {
            let empty = std::env::var_os("BANANA_VFS_EMPTY")
                .map(PathBuf::from)
                .ok_or_else(|| {
                    io::Error::other("BANANA_VFS_EMPTY is required with BANANA_VFS_SOCKET")
                })?;
            *self.view.get_mut() = Some(View::connect(&socket, empty).map_err(io::Error::other)?);
            // The broker endpoint is the tracer's, never the guest's.
            // SAFETY: the tracer is still single-threaded.
            unsafe {
                std::env::remove_var("BANANA_VFS_SOCKET");
                std::env::remove_var("BANANA_VFS_EMPTY");
            }
        }
        // A working directory inside the view: the kernel's cwd is the empty
        // stand-in directory, and the tracer keeps the guest path.
        let in_view = match self.view.get_mut() {
            Some(view) => view.owns(&cwd).map_err(io::Error::from_raw_os_error)?,
            None => false,
        };
        let host_cwd = if in_view {
            self.view.get_mut().as_ref().unwrap().empty.clone()
        } else {
            self.fs
                .translate(&cwd, true, 0, 0)
                .map_err(io::Error::from_raw_os_error)?
        };
        let c = |s: &str| CString::new(s).map_err(|_| io::Error::other("NUL in argument"));
        let c_program = c(&path::join(&cwd, &program))?;
        let c_args: Vec<CString> = self
            .options
            .argv
            .iter()
            .map(|a| c(a))
            .collect::<Result<_, _>>()?;
        let mut pointers: Vec<*const libc::c_char> = c_args.iter().map(|a| a.as_ptr()).collect();
        pointers.push(std::ptr::null());
        let c_cwd = CString::new(host_cwd.as_os_str().as_encoded_bytes())
            .map_err(|_| io::Error::other("NUL in cwd"))?;
        unsafe extern "C" {
            static environ: *const *const libc::c_char;
        }
        let calls = traced_calls(self.view.get_mut().is_some(), self.options.fake_root);
        let mut program = seccomp_program(&calls);
        let filter = libc::sock_fprog {
            len: program.len() as u16,
            filter: program.as_mut_ptr(),
        };
        let use_seccomp = std::env::var_os("BANANA_TRACER_NO_SECCOMP").is_none();
        // SAFETY: the child only calls async-signal-safe functions on data
        // prepared before fork.
        let child = unsafe { libc::fork() };
        if child < 0 {
            return Err(io::Error::last_os_error());
        }
        if child == 0 {
            unsafe {
                libc::ptrace(libc::PTRACE_TRACEME as _, 0, 0usize, 0usize);
                if libc::chdir(c_cwd.as_ptr()) != 0 {
                    libc::_exit(127);
                }
                // Installed last: until the tracer asks for seccomp stops,
                // trapped calls would fail. The tracer checks it took.
                if use_seccomp && libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) == 0 {
                    libc::syscall(libc::SYS_seccomp, 1usize, 0usize, &filter);
                }
                libc::raise(libc::SIGSTOP);
                libc::execve(c_program.as_ptr(), pointers.as_ptr(), environ);
                libc::_exit(127);
            }
        }
        let mut status = 0;
        // SAFETY: waiting for the child just forked.
        unsafe { libc::waitpid(child, &mut status, 0) };
        if in_view && let Some(view) = self.view.get_mut() {
            view.enter_directory(child, &cwd)
                .map_err(io::Error::from_raw_os_error)?;
        }
        let options = libc::PTRACE_O_TRACESYSGOOD
            | libc::PTRACE_O_TRACEFORK
            | libc::PTRACE_O_TRACEVFORK
            | libc::PTRACE_O_TRACECLONE
            | libc::PTRACE_O_TRACEEXEC
            | libc::PTRACE_O_TRACESECCOMP
            | libc::PTRACE_O_EXITKILL;
        ptrace(libc::PTRACE_SETOPTIONS as _, child, options as usize)?;
        self.seccomp = match (seccomp_filters("self"), seccomp_filters(&child.to_string())) {
            (Some(own), Some(theirs)) => theirs > own,
            _ => false,
        };
        self.log(1, || {
            format!(
                "seccomp filter: {} ({} calls)",
                if self.seccomp {
                    "on"
                } else {
                    "off, every call stops"
                },
                calls.len()
            )
        });
        self.threads.insert(
            child,
            Thread {
                tgid: child,
                awaiting_stop: false,
                exit: Exit::default(),
                entered: false,
            },
        );
        self.resume(child, 0)?;
        self.wait(child)
    }

    /// Continue a stopped thread: to its next syscall stop while a call it
    /// is in still needs its exit, otherwise to its next seccomp stop.
    fn resume(&self, tid: i32, signal: i32) -> io::Result<()> {
        let stepping = !self.seccomp
            || self.threads.get(&tid).is_some_and(|t| {
                let e = &t.exit;
                t.entered
                    || !e.restore.is_empty()
                    || e.result.is_some()
                    || !matches!(e.fix, Fix::None)
                    || e.view.is_some()
            });
        let request = if stepping {
            libc::PTRACE_SYSCALL
        } else {
            libc::PTRACE_CONT
        };
        match ptrace(request as _, tid, signal as usize) {
            Err(e) if e.raw_os_error() != Some(libc::ESRCH) => Err(e),
            _ => Ok(()),
        }
    }

    fn wait(&mut self, first: i32) -> io::Result<i32> {
        let mut code = None;
        loop {
            let mut status = 0;
            // SAFETY: waits for any traced child or thread.
            let tid = unsafe { libc::waitpid(-1, &mut status, libc::__WALL) };
            if tid < 0 {
                let e = io::Error::last_os_error();
                if e.raw_os_error() == Some(libc::EINTR) {
                    continue;
                }
                if e.raw_os_error() == Some(libc::ECHILD) {
                    return Ok(code.unwrap_or(0));
                }
                return Err(e);
            }
            if libc::WIFEXITED(status) || libc::WIFSIGNALED(status) {
                if self.threads.remove(&tid).is_some_and(|t| t.tgid == tid)
                    && let Some(view) = self.view.get_mut()
                {
                    view.exit(tid);
                }
                if tid == first {
                    code = Some(if libc::WIFEXITED(status) {
                        libc::WEXITSTATUS(status)
                    } else {
                        128 + libc::WTERMSIG(status)
                    });
                    if self.options.kill_on_exit {
                        for &other in self.threads.keys() {
                            // SAFETY: signals a traced thread.
                            unsafe { libc::kill(other, libc::SIGKILL) };
                        }
                    }
                }
                if self.threads.is_empty() && code.is_some() {
                    return Ok(code.unwrap_or(0));
                }
                continue;
            }
            if !libc::WIFSTOPPED(status) {
                continue;
            }
            let signal = libc::WSTOPSIG(status);
            let event = (status >> 16) & 0xff;
            let deliver = if signal == libc::SIGTRAP | 0x80 {
                self.syscall(tid)?;
                0
            } else if signal == libc::SIGTRAP && event == libc::PTRACE_EVENT_SECCOMP {
                self.syscall(tid)?;
                // Stepping on from here passes the call's syscall-entry stop.
                let stepping = !self.seccomp
                    || self.threads.get(&tid).is_some_and(|t| {
                        let e = &t.exit;
                        !e.restore.is_empty()
                            || e.result.is_some()
                            || !matches!(e.fix, Fix::None)
                            || e.view.is_some()
                    });
                if let Some(thread) = self.threads.get_mut(&tid) {
                    thread.entered = stepping;
                }
                0
            } else if signal == libc::SIGTRAP && event != 0 {
                self.event(tid, event)?;
                0
            } else if signal == libc::SIGSTOP
                && self.threads.get(&tid).is_none_or(|t| t.awaiting_stop)
            {
                // The attach stop of a new thread or process.
                match self.threads.get_mut(&tid) {
                    Some(thread) => thread.awaiting_stop = false,
                    None => {
                        let tgid = thread_group(tid);
                        self.threads.insert(
                            tid,
                            Thread {
                                tgid,
                                awaiting_stop: false,
                                exit: Exit::default(),
                                entered: false,
                            },
                        );
                    }
                }
                0
            } else {
                signal
            };
            self.resume(tid, deliver)?;
        }
    }

    fn event(&mut self, tid: i32, event: i32) -> io::Result<()> {
        match event {
            libc::PTRACE_EVENT_FORK | libc::PTRACE_EVENT_VFORK | libc::PTRACE_EVENT_CLONE => {
                let child = event_message(tid)? as i32;
                let parent = self.threads.get(&tid).map_or(tid, |t| t.tgid);
                let tgid = if event == libc::PTRACE_EVENT_CLONE {
                    thread_group(child)
                } else {
                    child
                };
                if tgid != parent
                    && let Some(program) = self.programs.get(&parent).cloned()
                {
                    self.programs.insert(tgid, program);
                }
                if let Some(view) = self.view.get_mut() {
                    view.fork(parent, tgid);
                }
                let seen = self.threads.contains_key(&child);
                let thread = self.threads.entry(child).or_insert(Thread {
                    tgid,
                    awaiting_stop: true,
                    exit: Exit::default(),
                    entered: false,
                });
                thread.tgid = tgid;
                if seen {
                    thread.awaiting_stop = false;
                }
            }
            libc::PTRACE_EVENT_EXEC => {
                let former = event_message(tid).unwrap_or(tid as u64) as i32;
                let tgid = self.threads.get(&former).map_or(tid, |t| t.tgid);
                // Other threads of the process are gone; the execing one
                // now carries the process id.
                self.threads
                    .retain(|&t, thread| thread.tgid != tgid || t == former);
                if let Some(mut thread) = self.threads.remove(&former) {
                    thread.exit = Exit::default();
                    thread.tgid = tid;
                    self.threads.insert(tid, thread);
                }
                if let Some(view) = self.view.get_mut() {
                    view.exec(tgid);
                }
                if let Some(program) = self.executing.remove(&tgid) {
                    self.log(1, || format!("{tid} runs {program}"));
                    self.programs.insert(tid, program);
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn syscall(&mut self, tid: i32) -> io::Result<()> {
        match arch::syscall_stop(tid)? {
            Stop::Entry { nr, args } => {
                // The entry stop after a seccomp stop already handled.
                if let Some(thread) = self.threads.get_mut(&tid)
                    && thread.entered
                {
                    thread.entered = false;
                    return Ok(());
                }
                // The entry of a call injected by a descriptor handoff.
                if let Some(thread) = self.threads.get(&tid)
                    && let Some(view::Pending::Transfer(t)) = &thread.exit.view
                    && t.injecting
                {
                    return Ok(());
                }
                let exit = match self.enter(tid, nr, args) {
                    Ok(exit) => exit,
                    Err(error) => {
                        arch::set_syscall(tid, -1)?;
                        Exit {
                            result: Some(-(error as i64)),
                            ..Exit::default()
                        }
                    }
                };
                if let Some(thread) = self.threads.get_mut(&tid) {
                    thread.exit = exit;
                }
                Ok(())
            }
            Stop::Exit { result } => {
                let mut exit = match self.threads.get_mut(&tid) {
                    Some(thread) => std::mem::take(&mut thread.exit),
                    None => return Ok(()),
                };
                if let Some(mut pending) = exit.view.take() {
                    let tgid = self.tgid(tid);
                    let view = self
                        .view
                        .get_mut()
                        .as_mut()
                        .ok_or_else(|| io::Error::other("no view"))?;
                    let transfer = matches!(pending, view::Pending::Transfer(_));
                    let running = view
                        .leave(tid, tgid, result, &mut pending)
                        .map_err(io::Error::from_raw_os_error)?;
                    if running {
                        exit.view = Some(pending);
                        if let Some(thread) = self.threads.get_mut(&tid) {
                            thread.exit = exit;
                        }
                        return Ok(());
                    }
                    if transfer {
                        return Ok(()); // the handoff restored the registers
                    }
                }
                self.leave(tid, result, exit)
            }
            Stop::Other => Ok(()),
        }
    }

    fn tgid(&self, tid: i32) -> i32 {
        self.threads.get(&tid).map_or(tid, |t| t.tgid)
    }

    /// The guest directory a relative path in `dirfd` resolves against.
    fn base(&self, tid: i32, dirfd: Option<i64>) -> Result<String, i32> {
        if let Some(view) = self.view.borrow_mut().as_mut() {
            let tgid = self.tgid(tid);
            match dirfd {
                Some(fd) if fd as i32 != libc::AT_FDCWD => {
                    if let Some(directory) = view.directory_of(tid, tgid, fd) {
                        return directory;
                    }
                }
                _ => {
                    if let Some(cwd) = view.cwd(tgid) {
                        return Ok(cwd.clone());
                    }
                }
            }
        }
        let link = match dirfd {
            Some(fd) if fd as i32 != libc::AT_FDCWD => format!("/proc/{tid}/fd/{}", fd as i32),
            _ => format!("/proc/{tid}/cwd"),
        };
        let host = std::fs::read_link(&link).map_err(|e| {
            if dirfd.is_some() {
                libc::EBADF
            } else {
                errno(e)
            }
        })?;
        Ok(self
            .fs
            .guest_of(&host)
            .unwrap_or_else(|| host.to_string_lossy().into_owned()))
    }

    fn guest_path(
        &self,
        tid: i32,
        dirfd: Option<i64>,
        address: u64,
    ) -> Result<Option<String>, i32> {
        if address == 0 {
            return Ok(None);
        }
        let raw = mem::string(tid, address, libc::PATH_MAX as usize).map_err(errno)?;
        if raw.is_empty() {
            return Ok(None);
        }
        let raw = String::from_utf8(raw).map_err(|_| libc::EILSEQ)?;
        if raw.starts_with('/') {
            Ok(Some(raw))
        } else {
            Ok(Some(path::join(&self.base(tid, dirfd)?, &raw)))
        }
    }

    fn enter(&mut self, tid: i32, call: i64, args: [u64; 6]) -> Result<Exit, i32> {
        let tgid = self.tgid(tid);
        let mut regs = Regs::read(tid).map_err(errno)?;
        let mut scratch = Scratch::new(tid, regs.sp());
        let mut exit = Exit::default();
        let flag = |arg: usize, bit: u64| args[arg] & bit != 0;
        let nofollow = libc::AT_SYMLINK_NOFOLLOW as u64;

        if self.view.borrow().is_some() {
            let (first, second) = view_paths(call);
            let guest = |p: PathArg| -> Result<Option<String>, i32> {
                let Some((dirfd, arg)) = p else {
                    return Ok(None);
                };
                Ok(self
                    .guest_path(tid, dirfd.map(|d| args[d] as i64), args[arg])?
                    .map(|g| path::normalize(&g)))
            };
            let paths = view::Paths {
                first: guest(first)?,
                second: guest(second)?,
            };
            let outcome = self
                .view
                .borrow_mut()
                .as_mut()
                .unwrap()
                .enter(tid, tgid, call, args, &paths)?;
            match outcome {
                view::Outcome::Skip(result) => return self.skip(tid, result),
                view::Outcome::Rewritten(pending) => {
                    return Ok(Exit {
                        view: Some(pending),
                        ..Exit::default()
                    });
                }
                view::Outcome::Continue(pending) => exit.view = pending,
            }
        }

        // Paths to translate: (dirfd argument, path argument, follow).
        let mut paths: Vec<(Option<usize>, usize, Follow)> = Vec::new();
        match call {
            c if c == nr::open => paths.push((None, 0, Follow::Unless(1, libc::O_NOFOLLOW as u64))),
            c if c == nr::creat
                || c == nr::access
                || c == nr::truncate
                || c == nr::chdir
                || c == nr::chmod
                || c == nr::utime
                || c == nr::utimes
                || c == nr::statfs
                || c == nr::setxattr
                || c == nr::getxattr
                || c == nr::listxattr
                || c == nr::removexattr =>
            {
                paths.push((None, 0, Follow::Yes))
            }
            c if c == nr::stat => {
                paths.push((None, 0, Follow::Yes));
                exit.fix = Fix::Owner {
                    buffer: args[1],
                    offset: arch::STAT_UID,
                };
            }
            c if c == nr::lstat => {
                paths.push((None, 0, Follow::No));
                exit.fix = Fix::Owner {
                    buffer: args[1],
                    offset: arch::STAT_UID,
                };
            }
            c if c == nr::mkdir
                || c == nr::rmdir
                || c == nr::unlink
                || c == nr::mknod
                || c == nr::lsetxattr
                || c == nr::lgetxattr
                || c == nr::llistxattr
                || c == nr::lremovexattr =>
            {
                paths.push((None, 0, Follow::No))
            }
            c if c == nr::rename => {
                paths.push((None, 0, Follow::No));
                paths.push((None, 1, Follow::No));
            }
            c if c == nr::symlink => paths.push((None, 1, Follow::No)),
            c if c == nr::readlink || c == nr::readlinkat => {
                let (dirfd, arg) = if c == nr::readlink {
                    (None, 0)
                } else {
                    (Some(0), 1)
                };
                let guest = self
                    .guest_path(tid, dirfd.map(|d| args[d] as i64), args[arg])?
                    .ok_or(libc::ENOENT)?;
                let guest = path::normalize(&guest);
                let (buffer, size) = (args[arg + 1], args[arg + 2]);
                if let Some(program) = self.proc_exe(tgid, &guest) {
                    let bytes = &program.as_bytes()[..program.len().min(size as usize)];
                    mem::write(tid, buffer, bytes).map_err(errno)?;
                    return self.skip(tid, bytes.len() as i64);
                }
                if guest.starts_with("/proc/") {
                    exit.fix = Fix::ProcLink { buffer, size };
                }
                paths.push((dirfd, arg, Follow::No));
            }
            c if c == nr::openat || c == nr::openat2 => {
                let follow = if c == nr::openat {
                    Follow::Unless(2, libc::O_NOFOLLOW as u64)
                } else {
                    Follow::Yes
                };
                paths.push((Some(0), 1, follow));
            }
            c if c == nr::mkdirat || c == nr::mknodat || c == nr::unlinkat => {
                paths.push((Some(0), 1, Follow::No))
            }
            c if c == nr::newfstatat => {
                paths.push((Some(0), 1, Follow::Unless(3, nofollow)));
                exit.fix = Fix::Owner {
                    buffer: args[2],
                    offset: arch::STAT_UID,
                };
            }
            c if c == nr::statx => {
                paths.push((Some(0), 1, Follow::Unless(2, nofollow)));
                exit.fix = Fix::Owner {
                    buffer: args[4],
                    offset: arch::STATX_UID,
                };
            }
            c if c == nr::fstat => {
                exit.fix = Fix::Owner {
                    buffer: args[1],
                    offset: arch::STAT_UID,
                }
            }
            c if c == nr::futimesat
                || c == nr::fchmodat
                || c == nr::faccessat
                || c == nr::inotify_add_watch =>
            {
                paths.push((Some(0), 1, Follow::Yes))
            }
            c if c == nr::faccessat2 || c == nr::fchmodat2 || c == nr::utimensat => {
                paths.push((Some(0), 1, Follow::Unless(3, nofollow)))
            }
            c if c == nr::renameat || c == nr::renameat2 => {
                paths.push((Some(0), 1, Follow::No));
                paths.push((Some(2), 3, Follow::No));
            }
            c if c == nr::symlinkat => paths.push((Some(1), 2, Follow::No)),
            c if c == nr::link || c == nr::linkat => {
                let (old, new, follow) = if c == nr::link {
                    ((None, 0), (None, 1), false)
                } else {
                    (
                        (Some(0), 1),
                        (Some(2), 3),
                        flag(4, libc::AT_SYMLINK_FOLLOW as u64),
                    )
                };
                if self.options.link2symlink {
                    let source =
                        self.resolve(tid, old.0.map(|d| args[d] as i64), args[old.1], follow)?;
                    let target =
                        self.resolve(tid, new.0.map(|d| args[d] as i64), args[new.1], false)?;
                    let result = self
                        .link2symlink(&source, &target)
                        .map_or_else(|e| -(e as i64), |()| 0);
                    return self.skip(tid, result);
                }
                paths.push((old.0, old.1, if follow { Follow::Yes } else { Follow::No }));
                paths.push((new.0, new.1, Follow::No));
            }
            c if c == nr::chown || c == nr::lchown || c == nr::fchownat || c == nr::fchown => {
                if self.options.fake_root {
                    if c != nr::fchown {
                        let (dirfd, arg, follow) = match c {
                            c if c == nr::chown => (None, 0, true),
                            c if c == nr::lchown => (None, 0, false),
                            _ => (Some(0), 1, !flag(4, nofollow)),
                        };
                        if args[arg] != 0
                            && mem::string(tid, args[arg], 2).is_ok_and(|s| !s.is_empty())
                        {
                            let host = self.resolve(
                                tid,
                                dirfd.map(|d| args[d] as i64),
                                args[arg],
                                follow,
                            )?;
                            std::fs::symlink_metadata(&host).map_err(errno)?;
                        }
                    }
                    return self.skip(tid, 0);
                }
                match c {
                    c if c == nr::chown => paths.push((None, 0, Follow::Yes)),
                    c if c == nr::lchown => paths.push((None, 0, Follow::No)),
                    c if c == nr::fchownat => paths.push((Some(0), 1, Follow::Unless(4, nofollow))),
                    _ => {}
                }
            }
            c if c == nr::execve || c == nr::execveat => {
                return self.execve(tid, tgid, call, args, regs, scratch);
            }
            c if c == nr::chroot => {
                let host = self.resolve(tid, None, args[0], true)?;
                return self.skip(
                    tid,
                    if host == self.fs.host_of("/") {
                        0
                    } else {
                        -(libc::EPERM as i64)
                    },
                );
            }
            c if c == nr::getcwd => {
                exit.fix = Fix::Getcwd {
                    buffer: args[0],
                    size: args[1],
                }
            }
            c if c == nr::bind || c == nr::connect => {
                return self.socket_address(tid, call == nr::connect, args, regs, scratch);
            }
            c if self.options.fake_root
                && [nr::getuid, nr::geteuid, nr::getgid, nr::getegid].contains(&c) =>
            {
                return self.skip(tid, 0);
            }
            c if self.options.fake_root && (c == nr::getresuid || c == nr::getresgid) => {
                for &pointer in &args[..3] {
                    if pointer != 0 {
                        mem::write(tid, pointer, &0u32.to_ne_bytes()).map_err(errno)?;
                    }
                }
                return self.skip(tid, 0);
            }
            c if self.options.fake_root
                && [
                    nr::setuid,
                    nr::setgid,
                    nr::setreuid,
                    nr::setregid,
                    nr::setresuid,
                    nr::setresgid,
                    nr::setfsuid,
                    nr::setfsgid,
                    nr::setgroups,
                ]
                .contains(&c) =>
            {
                return self.skip(tid, 0);
            }
            _ => {}
        }
        if !self.options.fake_root {
            exit.fix = match exit.fix {
                Fix::Owner { .. } => Fix::None,
                other => other,
            };
        }
        let mut changed = false;
        for (dirfd, arg, follow) in paths {
            let Some(guest) = self.guest_path(tid, dirfd.map(|d| args[d] as i64), args[arg])?
            else {
                continue; // NULL or empty (AT_EMPTY_PATH): the descriptor is the target
            };
            let follow = match follow {
                Follow::Yes => true,
                Follow::No => false,
                Follow::Unless(a, bit) => !flag(a, bit),
            };
            let host = self.fs.translate(&guest, follow, tgid, tid)?;
            self.log(2, || format!("{tid} {call}: {guest} -> {}", host.display()));
            let address = scratch.push_str(host.as_os_str().as_encoded_bytes());
            exit.restore.push((arg, args[arg]));
            regs.set_arg(arg, address);
            if let Some(d) = dirfd {
                // Translated paths are absolute: dirfd no longer matters.
                exit.restore.push((d, args[d]));
                regs.set_arg(d, libc::AT_FDCWD as i64 as u64);
            }
            changed = true;
        }
        if changed {
            scratch.commit().map_err(errno)?;
            regs.write(tid).map_err(errno)?;
        }
        Ok(exit)
    }

    /// Skip the call; it returns `result`.
    fn skip(&self, tid: i32, result: i64) -> Result<Exit, i32> {
        arch::set_syscall(tid, -1).map_err(errno)?;
        Ok(Exit {
            result: Some(result),
            ..Exit::default()
        })
    }

    fn resolve(
        &self,
        tid: i32,
        dirfd: Option<i64>,
        address: u64,
        follow: bool,
    ) -> Result<PathBuf, i32> {
        let guest = self.guest_path(tid, dirfd, address)?.ok_or(libc::ENOENT)?;
        self.fs.translate(&guest, follow, self.tgid(tid), tid)
    }

    /// The guest program behind `/proc/<pid>/exe`.
    fn proc_exe(&self, tgid: i32, guest: &str) -> Option<String> {
        let pid = guest.strip_prefix("/proc/")?.strip_suffix("/exe")?;
        let pid = if pid == "self" {
            tgid
        } else {
            self.tgid(pid.parse().ok()?)
        };
        self.programs.get(&pid).cloned()
    }

    fn execve(
        &mut self,
        tid: i32,
        tgid: i32,
        call: i64,
        args: [u64; 6],
        mut regs: Regs,
        mut scratch: Scratch,
    ) -> Result<Exit, i32> {
        let (dirfd, path_arg, argv_arg, env_arg) = if call == nr::execve {
            (None, 0, 1, 2)
        } else {
            (Some(args[0] as i64), 1, 2, 3)
        };
        let guest = self
            .guest_path(tid, dirfd, args[path_arg])?
            .ok_or(libc::ENOENT)?;
        let guest = path::normalize(&guest);
        let guest = self.proc_exe(tgid, &guest).unwrap_or(guest);
        let argv = mem::strings(tid, args[argv_arg]).map_err(errno)?;
        let plan = exec::plan(
            &Programs {
                tracer: self,
                tgid,
                tid,
            },
            &guest,
            argv,
        )?;
        self.log(1, || {
            format!("{tid} execve {guest} via {}", plan.host.display())
        });
        let pointers: Vec<u64> = plan.argv.iter().map(|a| scratch.push_str(a)).collect();
        let mut table: Vec<u8> = pointers.iter().flat_map(|p| p.to_ne_bytes()).collect();
        table.extend_from_slice(&0u64.to_ne_bytes());
        let argv_address = scratch.push(&table);
        let path_address = scratch.push_str(plan.host.as_os_str().as_encoded_bytes());
        scratch.commit().map_err(errno)?;
        let mut exit = Exit::default();
        for arg in 0..4 {
            exit.restore.push((arg, args[arg]));
        }
        let envp = args[env_arg];
        if call != nr::execve {
            arch::set_syscall(tid, nr::execve).map_err(errno)?;
            regs = Regs::read(tid).map_err(errno)?;
        }
        regs.set_arg(0, path_address);
        regs.set_arg(1, argv_address);
        regs.set_arg(2, envp);
        regs.write(tid).map_err(errno)?;
        self.executing.insert(tgid, plan.program);
        Ok(exit)
    }

    /// AF_UNIX path addresses for bind and connect.
    fn socket_address(
        &self,
        tid: i32,
        connect: bool,
        args: [u64; 6],
        mut regs: Regs,
        mut scratch: Scratch,
    ) -> Result<Exit, i32> {
        let length = (args[2] as usize).min(std::mem::size_of::<libc::sockaddr_un>());
        if args[1] == 0 || length < 3 {
            return Ok(Exit::default());
        }
        let mut address = vec![0u8; length];
        mem::read(tid, args[1], &mut address).map_err(errno)?;
        if u16::from_ne_bytes([address[0], address[1]]) != libc::AF_UNIX as u16 || address[2] == 0 {
            return Ok(Exit::default()); // not a path (abstract or another family)
        }
        let name = address[2..].split(|&b| b == 0).next().unwrap_or_default();
        let raw = String::from_utf8(name.to_vec()).map_err(|_| libc::EILSEQ)?;
        let guest = if raw.starts_with('/') {
            raw
        } else {
            path::join(&self.base(tid, None)?, &raw)
        };
        let host = self.fs.translate(&guest, connect, self.tgid(tid), tid)?;
        let host = host.as_os_str().as_encoded_bytes();
        if host.len() >= 108 {
            return self.skip(tid, -(libc::ENAMETOOLONG as i64));
        }
        let mut rewritten = address[..2].to_vec();
        rewritten.extend_from_slice(host);
        rewritten.push(0);
        let pointer = scratch.push(&rewritten);
        scratch.commit().map_err(errno)?;
        let exit = Exit {
            restore: vec![(1, args[1]), (2, args[2])],
            ..Exit::default()
        };
        regs.set_arg(1, pointer);
        regs.set_arg(2, rewritten.len() as u64);
        regs.write(tid).map_err(errno)?;
        Ok(exit)
    }

    /// Emulate `link(source, target)`: the file moves to a hidden name in its
    /// directory and both names become symbolic links to it, so either can be
    /// removed without losing the content.
    fn link2symlink(&self, source: &Path, target: &Path) -> Result<(), i32> {
        if std::fs::symlink_metadata(target).is_ok() {
            return Err(libc::EEXIST);
        }
        let meta = std::fs::symlink_metadata(source).map_err(errno)?;
        if meta.is_dir() {
            return Err(libc::EPERM);
        }
        let stored = match std::fs::read_link(source) {
            Ok(link)
                if link
                    .file_name()
                    .is_some_and(|n| n.to_string_lossy().starts_with(".l2s.")) =>
            {
                link.to_string_lossy().into_owned()
            }
            _ => {
                let directory = source.parent().ok_or(libc::EINVAL)?;
                let name = source.file_name().ok_or(libc::EINVAL)?.to_string_lossy();
                let mut random = [0u8; 8];
                // Through syscall(): bionic exports getrandom only from API 28.
                // SAFETY: fills the buffer.
                unsafe { libc::syscall(libc::SYS_getrandom, random.as_mut_ptr(), random.len(), 0) };
                let hidden =
                    directory.join(format!(".l2s.{name}.{:016x}", u64::from_ne_bytes(random)));
                std::fs::rename(source, &hidden).map_err(errno)?;
                let guest = self.fs.guest_of(&hidden).ok_or(libc::EXDEV)?;
                std::os::unix::fs::symlink(&guest, source).map_err(errno)?;
                guest
            }
        };
        std::os::unix::fs::symlink(&stored, target).map_err(errno)
    }

    fn leave(&mut self, tid: i32, result: i64, exit: Exit) -> io::Result<()> {
        if exit.restore.is_empty() && exit.result.is_none() && matches!(exit.fix, Fix::None) {
            return Ok(());
        }
        let mut regs = Regs::read(tid)?;
        for &(arg, value) in &exit.restore {
            // AArch64 returns in x0, its first argument register.
            if !(cfg!(target_arch = "aarch64") && arg == 0) {
                regs.set_arg(arg, value);
            }
        }
        let mut result = exit.result.unwrap_or(result);
        if exit.result.is_none() && result >= 0 {
            result = self
                .fix(tid, result, &exit.fix)
                .unwrap_or_else(|e| -(e as i64));
        }
        regs.set_result(result);
        regs.write(tid)
    }

    fn fix(&self, tid: i32, result: i64, fix: &Fix) -> Result<i64, i32> {
        match *fix {
            Fix::None => Ok(result),
            Fix::Getcwd { buffer, size } => {
                let host = mem::string(tid, buffer, size as usize).map_err(errno)?;
                let host = PathBuf::from(String::from_utf8_lossy(&host).into_owned());
                let guest = self
                    .fs
                    .guest_of(&host)
                    .unwrap_or_else(|| host.to_string_lossy().into_owned());
                if guest.len() as u64 + 1 > size {
                    return Err(libc::ERANGE);
                }
                let mut bytes = guest.into_bytes();
                bytes.push(0);
                mem::write(tid, buffer, &bytes).map_err(errno)?;
                Ok(bytes.len() as i64)
            }
            Fix::ProcLink { buffer, size } => {
                let mut host = vec![0u8; result as usize];
                mem::read(tid, buffer, &mut host).map_err(errno)?;
                let host = String::from_utf8_lossy(&host).into_owned();
                let viewed = self
                    .view
                    .borrow_mut()
                    .as_mut()
                    .and_then(|v| v.link_target(&host));
                let Some(guest) = viewed.or_else(|| self.fs.guest_of(Path::new(&host))) else {
                    return Ok(result);
                };
                let bytes = &guest.as_bytes()[..guest.len().min(size as usize)];
                mem::write(tid, buffer, bytes).map_err(errno)?;
                Ok(bytes.len() as i64)
            }
            Fix::Owner { buffer, offset } => {
                let mut ids = [0u8; 8];
                mem::read(tid, buffer + offset as u64, &mut ids).map_err(errno)?;
                let uid = u32::from_ne_bytes(ids[..4].try_into().unwrap());
                let gid = u32::from_ne_bytes(ids[4..].try_into().unwrap());
                let uid = if uid == self.uid { 0 } else { uid };
                let gid = if gid == self.gid { 0 } else { gid };
                ids[..4].copy_from_slice(&uid.to_ne_bytes());
                ids[4..].copy_from_slice(&gid.to_ne_bytes());
                mem::write(tid, buffer + offset as u64, &ids).map_err(errno)?;
                Ok(result)
            }
        }
    }
}
