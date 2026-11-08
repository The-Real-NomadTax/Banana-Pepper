//! View protocol 2 end to end on desktop Linux: a real `banana-vfs` view
//! server owns /game; guest programs read, write, list, map and execute its
//! files through the tracer, and the changes land in the view's overlay.
#![cfg(target_os = "linux")]
use banana_vfs::{View, rootless::Server};
use std::{collections::BTreeMap, path::Path, process::Command, sync::Arc};

#[test]
fn guest_programs_use_the_view() {
    let dir = std::env::temp_dir().join(format!("banana-tracer-view-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let root = dir.join("root");
    for d in ["etc", "usr", "proc", "dev", "tmp", "game"] {
        std::fs::create_dir_all(root.join(d)).unwrap();
    }
    for alias in ["bin", "sbin", "lib", "lib64"] {
        std::os::unix::fs::symlink(format!("usr/{alias}"), root.join(alias)).unwrap();
    }
    std::fs::create_dir_all(dir.join("empty")).unwrap();
    let big: Vec<u8> = (0..200_000u32).map(|i| (i % 251) as u8).collect();
    let files: BTreeMap<String, Arc<[u8]>> = BTreeMap::from([
        ("data.txt".into(), Arc::from(&b"hello from the view\n"[..])),
        ("dir/inner.txt".into(), Arc::from(&b"inner\n"[..])),
        ("big.bin".into(), Arc::from(big.as_slice())),
        (
            "echo".into(),
            Arc::from(std::fs::read("/usr/bin/echo").unwrap().as_slice()),
        ),
    ]);
    let source = Arc::new(banana_content::MemorySource { files });
    let view = Arc::new(
        View::new(
            source,
            &dir.join("overlay"),
            "fixture",
            BTreeMap::new(),
            BTreeMap::new(),
        )
        .unwrap(),
    );
    let socket = dir.join("view.sock");
    let server = Server::start(view.clone(), &socket, "/game").unwrap();

    let script = "cat data.txt; pwd; cat dir/inner.txt; ls /game | tr '\n' ' '; echo; \
        wc -c < big.bin; tail -c 3 big.bin | od -An -tu1; stat -c '%s %F' data.txt; \
        echo written > new.txt && cat new.txt; mkdir sub && mv new.txt sub/ && ls sub; \
        cd dir && pwd && cat inner.txt && cd /tmp && pwd && cd /game; \
        /game/echo from the view; \
        python3 -c \"import mmap;f=open('big.bin','rb');m=mmap.mmap(f.fileno(),0,access=mmap.ACCESS_READ);print(list(m[150000:150003]))\"; \
        python3 -c \"import os;fd=os.open('data.txt',os.O_RDONLY);d=os.dup(fd);os.lseek(fd,6,0);print(os.read(d,4).decode());print(os.readlink('/proc/self/fd/%d'%fd))\"";
    let output = Command::new(env!("CARGO_BIN_EXE_banana-tracer"))
        .args(["--kill-on-exit", "-0", "-r"])
        .arg(&root)
        .args(["-b", "/usr:/usr", "-b", "/proc", "-b", "/dev"])
        .args(["-w", "/game", "/bin/sh", "-c", script])
        .env("BANANA_VFS_SOCKET", &socket)
        .env("BANANA_VFS_EMPTY", dir.join("empty"))
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "status {:?}\nstdout {stdout}\nstderr {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    let lines: Vec<&str> = stdout.lines().map(str::trim).collect();
    let expected = [
        "hello from the view",
        "/game",
        "inner",
        "big.bin data.txt dir echo",
        "200000",
        &format!("{} {} {}", big[199_997], big[199_998], big[199_999]),
        "20 regular file",
        "written",
        "new.txt",
        "/game/dir",
        "inner",
        "/tmp",
        "from the view",
        &format!("[{}, {}, {}]", big[150_000], big[150_001], big[150_002]),
        "from",
        "/game/data.txt",
    ];
    assert_eq!(
        lines,
        expected,
        "stderr {}",
        String::from_utf8_lossy(&output.stderr)
    );
    drop(server);
    let mut bytes = [0u8; 8];
    let n = view.read("sub/new.txt", 0, &mut bytes).unwrap();
    assert_eq!(&bytes[..n], b"written\n");
    assert!(
        !Path::new(&root.join("game/new.txt")).exists(),
        "nothing reached the guest root"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
