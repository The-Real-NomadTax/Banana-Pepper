//! The tracer end to end on desktop Linux: a guest root whose /usr is the
//! host's (bound), with guest-only files, run through `banana-tracer`.
#![cfg(target_os = "linux")]
use std::{
    io::Read,
    os::unix::net::UnixListener,
    path::{Path, PathBuf},
    process::{Command, Output},
    time::{Duration, Instant},
};

struct Guest {
    dir: PathBuf,
}

impl Guest {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("banana-tracer-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let root = dir.join("root");
        for d in ["etc", "usr", "proc", "dev", "tmp", "work"] {
            std::fs::create_dir_all(root.join(d)).unwrap();
        }
        std::fs::create_dir_all(dir.join("tmp")).unwrap();
        std::fs::create_dir_all(dir.join("work")).unwrap();
        // Merged /usr, as on the host and in the Android image.
        for alias in ["bin", "sbin", "lib", "lib64"] {
            std::os::unix::fs::symlink(format!("usr/{alias}"), root.join(alias)).unwrap();
        }
        std::fs::write(root.join("etc/marker"), "guest marker\n").unwrap();
        std::fs::write(root.join("etc/passwd"), "root:x:0:0:root:/root:/bin/sh\n").unwrap();
        std::fs::write(root.join("etc/group"), "root:x:0:\n").unwrap();
        std::os::unix::fs::symlink("/etc/marker", root.join("etc/absolute-link")).unwrap();
        Self { dir }
    }

    fn run(&self, script: &str) -> Output {
        let tracer = env!("CARGO_BIN_EXE_banana-tracer");
        let dir = &self.dir;
        let bind = |host: &Path, guest: &str| format!("{}:{guest}", host.display());
        Command::new(tracer)
            .args(["--kill-on-exit", "--link2symlink", "-0", "-r"])
            .arg(dir.join("root"))
            .args(["-b", &bind(Path::new("/usr"), "/usr")])
            .args(["-b", "/proc", "-b", "/dev"])
            .args(["-b", &bind(&dir.join("tmp"), "/tmp")])
            .args(["-b", &bind(&dir.join("work"), "/work")])
            .args(["-w", "/work", "/bin/sh", "-c", script])
            .output()
            .unwrap()
    }
}

impl Drop for Guest {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn stdout(output: &Output) -> String {
    assert!(
        output.status.success(),
        "status {:?}\nstdout {}\nstderr {}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[test]
fn the_guest_sees_its_own_root() {
    let guest = Guest::new("root");
    let out = stdout(&guest.run(
        "cat /etc/marker; cat /etc/absolute-link; pwd; cd /etc && pwd; /bin/readlink /proc/self/exe; \
         ls / | tr '\\n' ' '; echo; test -e /home && echo leaked || echo isolated",
    ));
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines[0], "guest marker");
    assert_eq!(lines[1], "guest marker");
    assert_eq!(lines[2], "/work");
    assert_eq!(lines[3], "/etc");
    assert_eq!(lines[4], "/bin/readlink");
    for name in ["bin", "etc", "usr", "work"] {
        assert!(
            lines[5].split(' ').any(|n| n == name),
            "{name} in {}",
            lines[5]
        );
    }
    assert_eq!(lines[6], "isolated");
}

#[test]
fn fake_root_and_hard_links() {
    let guest = Guest::new("ids");
    let out = stdout(&guest.run(
        "id -u; id -g; stat -c %u:%g /etc/marker; chown 123:456 /etc/marker && echo chowned; \
         echo content > /tmp/a && ln /tmp/a /tmp/b && rm /tmp/a && cat /tmp/b; \
         ln /tmp/b /tmp/b 2>/dev/null || echo exists",
    ));
    assert_eq!(
        out.lines().collect::<Vec<_>>(),
        ["0", "0", "0:0", "chowned", "content", "exists"]
    );
}

#[test]
fn unix_socket_paths_are_translated() {
    let guest = Guest::new("socket");
    let listener = UnixListener::bind(guest.dir.join("tmp/server")).unwrap();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut text = String::new();
        stream.read_to_string(&mut text).unwrap();
        text
    });
    let out = stdout(&guest.run(
        "python3 -c \"import socket,os;s=socket.socket(socket.AF_UNIX);s.connect('/tmp/server');s.sendall(os.getcwd().encode());s.close()\"",
    ));
    assert_eq!(out, "");
    assert_eq!(server.join().unwrap(), "/work");
}

#[test]
fn exit_status_and_kill_on_exit() {
    let guest = Guest::new("exit");
    assert_eq!(guest.run("exit 7").status.code(), Some(7));
    let start = Instant::now();
    let output = guest.run("sleep 60 & echo started");
    assert_eq!(stdout(&output), "started\n");
    assert!(
        start.elapsed() < Duration::from_secs(20),
        "the background sleep outlived the command"
    );
}
