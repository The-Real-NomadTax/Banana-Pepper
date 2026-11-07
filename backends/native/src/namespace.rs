//! Linux session namespaces without setuid helpers (no fusermount, no
//! bubblewrap). `banana-sandbox`, a fresh single-threaded process, creates
//! user and mount namespaces (FUSE mounts are unprivileged there since Linux
//! 4.18), opens `/dev/fuse` inside them (the kernel requires the device to
//! come from the mounting namespace), mounts it and hands the connection back
//! to the host over a socket; the host serves the session view on it. The
//! helper then either builds an isolated root and runs the guest (`run`) or
//! keeps the host root and holds the namespace for other processes to enter
//! (`hold`, used for Wine and its server). When the last process in the
//! namespace exits, the mount and the connection end with it.
use banana_api::SessionView;
use std::{
    ffi::{CString, OsString},
    fs::File,
    io::{self, Read},
    os::{
        fd::{AsRawFd, FromRawFd, OwnedFd, RawFd},
        unix::{ffi::OsStrExt, process::CommandExt},
    },
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    sync::Arc,
};

/// The helper inside a distribution or development build.
pub fn helper(resources: &Path) -> PathBuf {
    resources.join("helpers").join("banana-sandbox")
}

fn check(r: libc::c_int) -> io::Result<libc::c_int> {
    if r < 0 { Err(io::Error::last_os_error()) } else { Ok(r) }
}
/// A copy of `fd` that a spawned child inherits.
fn inheritable(fd: RawFd) -> io::Result<OwnedFd> {
    let copy = check(unsafe { libc::fcntl(fd, libc::F_DUPFD, 3) })?;
    Ok(unsafe { OwnedFd::from_raw_fd(copy) })
}

// ---- host side ---------------------------------------------------------------

/// The socket over which a helper hands back the FUSE connection it mounted.
pub struct ViewConnection {
    host: OwnedFd,
    child: OwnedFd,
}
impl ViewConnection {
    pub fn new() -> io::Result<Self> {
        if !Path::new("/dev/fuse").exists() {
            return Err(io::Error::new(io::ErrorKind::NotFound, "FUSE (/dev/fuse) is unavailable"));
        }
        let mut fds = [0; 2];
        check(unsafe {
            libc::socketpair(libc::AF_UNIX, libc::SOCK_STREAM | libc::SOCK_CLOEXEC, 0, fds.as_mut_ptr())
        })?;
        let (host, end) = unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) };
        Ok(Self { host, child: inheritable(end.as_raw_fd())? })
    }
    /// Helper arguments naming the inherited socket.
    pub fn args(&self) -> [String; 2] {
        ["--channel".into(), self.child.as_raw_fd().to_string()]
    }
    /// After the helper is spawned: receive the connection it mounted, then
    /// serve the view on it until the namespace ends.
    pub fn serve(self, view: Arc<dyn SessionView>) -> Result<(), String> {
        let Self { host, child } = self;
        drop(child);
        let device = receive_fd(host.as_raw_fd())
            .map_err(|e| format!("The Linux session sandbox did not start ({e}); see game.log"))?;
        let device = File::from(device);
        std::thread::Builder::new()
            .name("session-fuse".into())
            .spawn(move || {
                if let Err(e) = view.serve_fuse(device) {
                    eprintln!("Session filesystem ended: {e}");
                }
            })
            .map_err(|e| e.to_string())?;
        Ok(())
    }
}

/// A namespace that keeps the host root with the view mounted at a host
/// path, for processes that must all see it (Wine and its server).
pub struct Holder {
    child: Child,
    _stdin: ChildStdin,
    user: File,
    mount: File,
}
impl Holder {
    pub fn start(
        resources: &Path,
        target: &Path,
        view: Arc<dyn SessionView>,
        log: &Path,
    ) -> Result<Self, String> {
        std::fs::create_dir_all(target).map_err(|e| e.to_string())?;
        let connection = ViewConnection::new().map_err(|e| e.to_string())?;
        let log = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(log)
            .map_err(|e| e.to_string())?;
        let mut child = Command::new(helper(resources))
            .arg("hold")
            .args(connection.args())
            .arg("--view")
            .arg(target)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(log)
            .spawn()
            .map_err(|e| format!("Cannot start the session sandbox: {e}"))?;
        connection.serve(view)?;
        let stdin = child.stdin.take().ok_or("Missing holder input")?;
        let ns = |kind: &str| {
            File::open(format!("/proc/{}/ns/{kind}", child.id())).map_err(|e| e.to_string())
        };
        Ok(Self {
            user: ns("user")?,
            mount: ns("mnt")?,
            _stdin: stdin,
            child,
        })
    }
    /// Run `command` inside the namespace, starting in `cwd` there.
    pub fn enter(&self, command: &mut Command, cwd: Option<&Path>) {
        let (user, mount) = (self.user.as_raw_fd(), self.mount.as_raw_fd());
        let cwd = cwd.map(|p| CString::new(p.as_os_str().as_bytes()).unwrap_or_default());
        unsafe {
            command.pre_exec(move || {
                check(libc::setns(user, libc::CLONE_NEWUSER))?;
                check(libc::setns(mount, libc::CLONE_NEWNS))?;
                if let Some(dir) = &cwd {
                    check(libc::chdir(dir.as_ptr()))?;
                }
                Ok(())
            });
        }
    }
}
impl Drop for Holder {
    fn drop(&mut self) {
        // Closing its input ends the holder; processes still inside keep the
        // namespace (and the view) until they exit.
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

// ---- helper side ---------------------------------------------------------------

enum Op {
    Bind { source: PathBuf, destination: PathBuf, readonly: bool, view: bool },
    Symlink { target: PathBuf, link: PathBuf },
    Tmpfs(PathBuf),
    Dev(PathBuf),
    Proc(PathBuf),
    View(PathBuf),
}
#[derive(Default)]
struct Plan {
    ops: Vec<Op>,
    unshare_all: bool,
    die_with_parent: bool,
    new_session: bool,
    clear_env: bool,
    env: Vec<(OsString, OsString)>,
    chdir: Option<PathBuf>,
    channel: Option<RawFd>,
    /// `/dev/fuse`, opened right after entering the user namespace.
    device: Option<RawFd>,
    command: Vec<OsString>,
}

fn parse(args: &[OsString]) -> Result<Plan, String> {
    let mut plan = Plan::default();
    let mut i = 0;
    let take = |i: &mut usize| -> Result<OsString, String> {
        *i += 1;
        args.get(*i).cloned().ok_or_else(|| "Missing sandbox argument value".to_string())
    };
    while i < args.len() {
        let a = args[i].to_string_lossy().into_owned();
        match a.as_str() {
            "--" => {
                plan.command = args[i + 1..].to_vec();
                break;
            }
            "--unshare-all" => plan.unshare_all = true,
            "--die-with-parent" => plan.die_with_parent = true,
            "--new-session" => plan.new_session = true,
            "--clearenv" => plan.clear_env = true,
            "--ro-bind" | "--bind" | "--dev-bind" | "--view-bind" | "--view-ro-bind" => {
                let source = PathBuf::from(take(&mut i)?);
                let destination = PathBuf::from(take(&mut i)?);
                plan.ops.push(Op::Bind {
                    source,
                    destination,
                    readonly: a == "--ro-bind" || a == "--view-ro-bind",
                    view: a.starts_with("--view"),
                });
            }
            "--symlink" => {
                let target = PathBuf::from(take(&mut i)?);
                let link = PathBuf::from(take(&mut i)?);
                plan.ops.push(Op::Symlink { target, link });
            }
            "--tmpfs" => plan.ops.push(Op::Tmpfs(take(&mut i)?.into())),
            "--dev" => plan.ops.push(Op::Dev(take(&mut i)?.into())),
            "--proc" => plan.ops.push(Op::Proc(take(&mut i)?.into())),
            "--view" => plan.ops.push(Op::View(take(&mut i)?.into())),
            "--setenv" => {
                let k = take(&mut i)?;
                let v = take(&mut i)?;
                plan.env.push((k, v));
            }
            "--chdir" => plan.chdir = Some(take(&mut i)?.into()),
            "--channel" => plan.channel = take(&mut i)?.to_str().and_then(|v| v.parse().ok()),
            _ if a.starts_with("--") => return Err(format!("Unknown sandbox option {a}")),
            _ => {
                plan.command = args[i..].to_vec();
                break;
            }
        }
        i += 1;
    }
    Ok(plan)
}

fn c(p: impl AsRef<Path>) -> CString {
    CString::new(p.as_ref().as_os_str().as_bytes()).unwrap_or_default()
}
fn mount(source: Option<&Path>, target: &Path, fstype: Option<&str>, flags: libc::c_ulong, data: Option<&str>) -> io::Result<()> {
    let source = source.map(c);
    let fstype = fstype.map(|t| CString::new(t).unwrap());
    let data = data.map(|d| CString::new(d).unwrap());
    check(unsafe {
        libc::mount(
            source.as_ref().map_or(std::ptr::null(), |s| s.as_ptr()),
            c(target).as_ptr(),
            fstype.as_ref().map_or(std::ptr::null(), |s| s.as_ptr()),
            flags,
            data.as_ref().map_or(std::ptr::null(), |s| s.as_ptr().cast()),
        )
    })
    .map(|_| ())
    .map_err(|e| io::Error::new(e.kind(), format!("mount {}: {e}", target.display())))
}
fn write_file(path: &str, text: &str) -> io::Result<()> {
    std::fs::write(path, text).map_err(|e| io::Error::new(e.kind(), format!("{path}: {e}")))
}

/// Enter fresh user + mount (and optionally more) namespaces, mapping the
/// caller's own IDs.
fn enter_namespaces(extra: libc::c_int) -> Result<(), String> {
    let (uid, gid) = unsafe { (libc::getuid(), libc::getgid()) };
    if unsafe { libc::unshare(libc::CLONE_NEWUSER | libc::CLONE_NEWNS | extra) } != 0 {
        let e = io::Error::last_os_error();
        return Err(format!(
            "Cannot create a user namespace ({e}). Unprivileged user namespaces are disabled \
             on this system (on Ubuntu, AppArmor restricts them unless this program has a profile)."
        ));
    }
    write_file("/proc/self/setgroups", "deny").map_err(|e| e.to_string())?;
    write_file("/proc/self/uid_map", &format!("{uid} {uid} 1\n")).map_err(|e| e.to_string())?;
    write_file("/proc/self/gid_map", &format!("{gid} {gid} 1\n")).map_err(|e| e.to_string())?;
    mount(None, Path::new("/"), None, libc::MS_REC | libc::MS_PRIVATE, None).map_err(|e| e.to_string())
}
/// Pass `fd` over a Unix socket.
fn send_fd(socket: RawFd, fd: RawFd) -> io::Result<()> {
    let mut byte = [1u8];
    let mut iov = libc::iovec { iov_base: byte.as_mut_ptr().cast(), iov_len: 1 };
    let mut space = [0u8; 64];
    let mut msg: libc::msghdr = unsafe { std::mem::zeroed() };
    msg.msg_iov = &mut iov;
    msg.msg_iovlen = 1;
    msg.msg_control = space.as_mut_ptr().cast();
    msg.msg_controllen = unsafe { libc::CMSG_SPACE(size_of::<RawFd>() as u32) } as _;
    unsafe {
        let cmsg = libc::CMSG_FIRSTHDR(&msg);
        (*cmsg).cmsg_level = libc::SOL_SOCKET;
        (*cmsg).cmsg_type = libc::SCM_RIGHTS;
        (*cmsg).cmsg_len = libc::CMSG_LEN(size_of::<RawFd>() as u32) as _;
        std::ptr::write_unaligned(libc::CMSG_DATA(cmsg).cast::<RawFd>(), fd);
    }
    if unsafe { libc::sendmsg(socket, &msg, 0) } != 1 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}
/// Receive a descriptor sent with [`send_fd`]; an error at end of stream.
fn receive_fd(socket: RawFd) -> io::Result<OwnedFd> {
    let mut byte = [0u8];
    let mut iov = libc::iovec { iov_base: byte.as_mut_ptr().cast(), iov_len: 1 };
    let mut space = [0u8; 64];
    let mut msg: libc::msghdr = unsafe { std::mem::zeroed() };
    msg.msg_iov = &mut iov;
    msg.msg_iovlen = 1;
    msg.msg_control = space.as_mut_ptr().cast();
    msg.msg_controllen = space.len() as _;
    let n = loop {
        let n = unsafe { libc::recvmsg(socket, &mut msg, libc::MSG_CMSG_CLOEXEC) };
        if n >= 0 || io::Error::last_os_error().raw_os_error() != Some(libc::EINTR) {
            break n;
        }
    };
    if n < 0 {
        return Err(io::Error::last_os_error());
    }
    unsafe {
        let cmsg = libc::CMSG_FIRSTHDR(&msg);
        if n == 1 && !cmsg.is_null() && (*cmsg).cmsg_type == libc::SCM_RIGHTS {
            let fd = std::ptr::read_unaligned(libc::CMSG_DATA(cmsg).cast::<RawFd>());
            return Ok(OwnedFd::from_raw_fd(fd));
        }
    }
    Err(io::Error::new(io::ErrorKind::UnexpectedEof, "no connection received"))
}

/// Open `/dev/fuse` in the new user namespace (before the root changes).
fn open_fuse(plan: &mut Plan) -> Result<(), String> {
    if plan.channel.is_some() {
        let fd = check(unsafe { libc::open(c("/dev/fuse").as_ptr(), libc::O_RDWR | libc::O_CLOEXEC) })
            .map_err(|e| format!("/dev/fuse: {e}"))?;
        plan.device = Some(fd);
    }
    Ok(())
}
/// Mount the opened FUSE connection at `target` and hand it to the host.
fn mount_view(plan: &mut Plan, target: &Path) -> Result<(), String> {
    let channel = plan.channel.take().ok_or("--view needs --channel")?;
    let fd = plan.device.take().ok_or("FUSE was not opened")?;
    std::fs::create_dir_all(target).map_err(|e| format!("{}: {e}", target.display()))?;
    let (uid, gid) = unsafe { (libc::getuid(), libc::getgid()) };
    mount(
        Some(Path::new("banana-pepper")),
        target,
        Some("fuse"),
        libc::MS_NOSUID | libc::MS_NODEV,
        Some(&format!("fd={fd},rootmode=40000,user_id={uid},group_id={gid}")),
    )
    .map_err(|e| e.to_string())?;
    send_fd(channel, fd).map_err(|e| format!("Cannot hand the view to the host: {e}"))?;
    unsafe {
        libc::close(fd);
        libc::close(channel);
    }
    // The kernel creates the root inode owned by uid 0, which this namespace
    // does not map; writes there fail (EACCES) until its attributes are
    // fetched. Fetch them now that the host serves the connection.
    std::fs::metadata(target).map_err(|e| format!("Session view did not answer: {e}"))?;
    Ok(())
}

/// Create `destination` like `source` (directory or empty file).
fn mount_point(source: &Path, destination: &Path) -> io::Result<()> {
    if std::fs::metadata(source)?.is_dir() {
        std::fs::create_dir_all(destination)
    } else {
        if let Some(parent) = destination.parent() {
            std::fs::create_dir_all(parent)?;
        }
        if !destination.exists() {
            File::create(destination)?;
        }
        Ok(())
    }
}
fn bind(source: &Path, destination: &Path, readonly: bool) -> io::Result<()> {
    mount_point(source, destination)?;
    mount(Some(source), destination, None, libc::MS_BIND | libc::MS_REC, None)?;
    if readonly {
        // Keep the flags the kernel locks for mounts made in a user namespace.
        let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
        check(unsafe { libc::statvfs(c(destination).as_ptr(), &mut st) })?;
        let mut flags = libc::MS_BIND | libc::MS_REMOUNT | libc::MS_RDONLY;
        for (st_flag, ms_flag) in [
            (libc::ST_NOSUID, libc::MS_NOSUID),
            (libc::ST_NODEV, libc::MS_NODEV),
            (libc::ST_NOEXEC, libc::MS_NOEXEC),
            (libc::ST_NOATIME, libc::MS_NOATIME),
            (libc::ST_NODIRATIME, libc::MS_NODIRATIME),
            (libc::ST_RELATIME, libc::MS_RELATIME),
        ] {
            if st.f_flag & st_flag != 0 {
                flags |= ms_flag;
            }
        }
        mount(None, destination, None, flags, None)?;
    }
    Ok(())
}
fn under(root: &Path, p: &Path) -> PathBuf {
    root.join(p.strip_prefix("/").unwrap_or(p))
}
fn dev(old: &Path, destination: &Path) -> io::Result<()> {
    std::fs::create_dir_all(destination)?;
    mount(Some(Path::new("tmpfs")), destination, Some("tmpfs"), libc::MS_NOSUID, Some("mode=0755"))?;
    for node in ["null", "zero", "full", "random", "urandom", "tty"] {
        let source = old.join("dev").join(node);
        if source.exists() {
            bind(&source, &destination.join(node), false)?;
        }
    }
    for (link, target) in [
        ("fd", "/proc/self/fd"),
        ("stdin", "/proc/self/fd/0"),
        ("stdout", "/proc/self/fd/1"),
        ("stderr", "/proc/self/fd/2"),
    ] {
        std::os::unix::fs::symlink(target, destination.join(link))?;
    }
    let shm = destination.join("shm");
    std::fs::create_dir_all(&shm)?;
    mount(Some(Path::new("tmpfs")), &shm, Some("tmpfs"), libc::MS_NOSUID | libc::MS_NODEV, Some("mode=1777"))?;
    let pts = destination.join("pts");
    std::fs::create_dir_all(&pts)?;
    if mount(
        Some(Path::new("devpts")),
        &pts,
        Some("devpts"),
        libc::MS_NOSUID | libc::MS_NOEXEC,
        Some("newinstance,ptmxmode=0666,mode=620"),
    )
    .is_ok()
    {
        std::os::unix::fs::symlink("pts/ptmx", destination.join("ptmx"))?;
    }
    Ok(())
}

/// Build the isolated root under /newroot, resolving sources under /oldroot.
fn build_root(plan: &mut Plan) -> Result<(), String> {
    let e = |e: io::Error| e.to_string();
    // Resolve host sources (including absolute symlinks such as
    // /etc/localtime) while the host root is still the root.
    for op in &mut plan.ops {
        if let Op::Bind { source, view: false, .. } = op {
            *source = std::fs::canonicalize(&*source)
                .map_err(|err| format!("bind {}: {err}", source.display()))?;
        }
    }
    let base = Path::new("/tmp");
    mount(Some(Path::new("tmpfs")), base, Some("tmpfs"), libc::MS_NOSUID | libc::MS_NODEV, Some("mode=0755")).map_err(e)?;
    std::fs::create_dir_all(base.join("newroot")).map_err(e)?;
    std::fs::create_dir_all(base.join("oldroot")).map_err(e)?;
    mount(Some(&base.join("newroot")), &base.join("newroot"), None, libc::MS_BIND | libc::MS_REC, None).map_err(e)?;
    check(unsafe { libc::chdir(c(base).as_ptr()) }).map_err(e)?;
    check(unsafe { libc::syscall(libc::SYS_pivot_root, c(".").as_ptr(), c("oldroot").as_ptr()) as libc::c_int }).map_err(e)?;
    check(unsafe { libc::chdir(c("/").as_ptr()) }).map_err(e)?;
    let (old, new) = (Path::new("/oldroot"), Path::new("/newroot"));
    let mut view_at: Option<PathBuf> = None;
    for op in std::mem::take(&mut plan.ops) {
        match op {
            Op::Bind { source, destination, readonly, view } => {
                let source = if view {
                    under(view_at.as_ref().ok_or("--view-bind before --view")?, &source)
                } else {
                    under(old, &source)
                };
                bind(&source, &under(new, &destination), readonly)
                    .map_err(|err| format!("bind {}: {err}", source.display()))?;
            }
            Op::Symlink { target, link } => {
                let link = under(new, &link);
                if let Some(parent) = link.parent() {
                    std::fs::create_dir_all(parent).map_err(e)?;
                }
                std::os::unix::fs::symlink(&target, &link).map_err(e)?;
            }
            Op::Tmpfs(d) => {
                let d = under(new, &d);
                std::fs::create_dir_all(&d).map_err(e)?;
                mount(Some(Path::new("tmpfs")), &d, Some("tmpfs"), libc::MS_NOSUID | libc::MS_NODEV, Some("mode=0755")).map_err(e)?;
            }
            Op::Dev(d) => dev(old, &under(new, &d)).map_err(e)?,
            Op::Proc(d) => {
                let d = under(new, &d);
                std::fs::create_dir_all(&d).map_err(e)?;
                mount(Some(Path::new("proc")), &d, Some("proc"), libc::MS_NOSUID | libc::MS_NODEV | libc::MS_NOEXEC, None).map_err(e)?;
            }
            Op::View(d) => {
                let d = under(new, &d);
                mount_view(plan, &d)?;
                view_at = Some(d);
            }
        }
    }
    // Drop the host root, then make /newroot the root.
    check(unsafe { libc::umount2(c(old).as_ptr(), libc::MNT_DETACH) }).map_err(e)?;
    check(unsafe { libc::chdir(c(new).as_ptr()) }).map_err(e)?;
    check(unsafe { libc::syscall(libc::SYS_pivot_root, c(".").as_ptr(), c(".").as_ptr()) as libc::c_int }).map_err(e)?;
    check(unsafe { libc::umount2(c(".").as_ptr(), libc::MNT_DETACH) }).map_err(e)?;
    check(unsafe { libc::chdir(c("/").as_ptr()) }).map_err(e)?;
    Ok(())
}

fn exec(plan: &Plan) -> String {
    if plan.clear_env {
        unsafe { libc::clearenv() };
    }
    for (k, v) in &plan.env {
        unsafe { std::env::set_var(k, v) };
    }
    if let Some(dir) = &plan.chdir {
        if let Err(e) = std::env::set_current_dir(dir) {
            return format!("chdir {}: {e}", dir.display());
        }
    }
    let Some(program) = plan.command.first() else {
        return "No command to run".into();
    };
    let e = Command::new(program).args(&plan.command[1..]).exec();
    format!("Cannot run {}: {e}", program.to_string_lossy())
}

fn wait_status(pid: libc::pid_t) -> i32 {
    let mut status = 0;
    loop {
        let r = unsafe { libc::waitpid(pid, &mut status, 0) };
        if r == pid {
            break;
        }
        if r < 0 && io::Error::last_os_error().raw_os_error() != Some(libc::EINTR) {
            return 125;
        }
    }
    if libc::WIFEXITED(status) {
        libc::WEXITSTATUS(status)
    } else {
        128 + libc::WTERMSIG(status)
    }
}

fn run(mut plan: Plan) -> Result<i32, String> {
    if plan.die_with_parent {
        unsafe { libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) };
    }
    let extra = if plan.unshare_all {
        libc::CLONE_NEWPID | libc::CLONE_NEWIPC | libc::CLONE_NEWUTS | libc::CLONE_NEWNET | libc::CLONE_NEWCGROUP
    } else {
        0
    };
    enter_namespaces(extra)?;
    open_fuse(&mut plan)?;
    if plan.unshare_all {
        // The first child is PID 1 of the new PID namespace.
        match unsafe { libc::fork() } {
            -1 => return Err(io::Error::last_os_error().to_string()),
            0 => {}
            child => {
                // This side only waits; the namespace side owns the connection.
                for fd in [plan.channel.take(), plan.device.take()].into_iter().flatten() {
                    unsafe { libc::close(fd) };
                }
                return Ok(wait_status(child));
            }
        }
        unsafe { libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) };
    }
    build_root(&mut plan)?;
    if plan.new_session {
        unsafe { libc::setsid() };
    }
    if !plan.unshare_all {
        return Err(exec(&plan));
    }
    // PID 1 stays a minimal init: run the guest as PID 2 and reap.
    match unsafe { libc::fork() } {
        -1 => Err(io::Error::last_os_error().to_string()),
        0 => {
            let e = exec(&plan);
            eprintln!("banana-sandbox: {e}");
            unsafe { libc::_exit(127) }
        }
        guest => {
            let mut status = 0;
            loop {
                let pid = unsafe { libc::waitpid(-1, &mut status, 0) };
                if pid == guest {
                    return Ok(if libc::WIFEXITED(status) {
                        libc::WEXITSTATUS(status)
                    } else {
                        128 + libc::WTERMSIG(status)
                    });
                }
                if pid < 0 && io::Error::last_os_error().raw_os_error() != Some(libc::EINTR) {
                    return Ok(125);
                }
            }
        }
    }
}

fn hold(mut plan: Plan) -> Result<i32, String> {
    unsafe { libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) };
    enter_namespaces(0)?;
    open_fuse(&mut plan)?;
    let Some(Op::View(target)) = plan.ops.pop() else {
        return Err("hold needs --view TARGET".into());
    };
    mount_view(&mut plan, &target)?;
    // Hold the namespace open until the host closes our input.
    let mut sink = [0u8; 64];
    while matches!(io::stdin().read(&mut sink), Ok(n) if n > 0) {}
    Ok(0)
}

/// `banana-sandbox run|hold OPTIONS [--] COMMAND...`
pub fn helper_main() -> ! {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let result = match args.first().map(|a| a.to_string_lossy().into_owned()).as_deref() {
        Some("run") => parse(&args[1..]).and_then(run),
        Some("hold") => parse(&args[1..]).and_then(hold),
        _ => Err("Usage: banana-sandbox run|hold OPTIONS [--] COMMAND...".into()),
    };
    match result {
        Ok(code) => std::process::exit(code),
        Err(e) => {
            eprintln!("banana-sandbox: {e}");
            std::process::exit(125)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn options_parse_in_order() {
        let args: Vec<OsString> = [
            "--unshare-all", "--ro-bind", "/usr", "/usr", "--view", "/game", "--view-bind",
            "saves", "/saves", "--setenv", "A", "1", "--chdir", "/game", "--channel", "7",
            "--", "/game/run", "-x",
        ]
        .iter()
        .map(OsString::from)
        .collect();
        let plan = parse(&args).unwrap();
        assert!(plan.unshare_all);
        assert_eq!(plan.ops.len(), 3);
        assert!(matches!(&plan.ops[2], Op::Bind { view: true, readonly: false, .. }));
        assert_eq!(plan.channel, Some(7));
        assert_eq!(plan.command, [OsString::from("/game/run"), OsString::from("-x")]);
        assert!(parse(&[OsString::from("--bogus")]).is_err());
    }
}
