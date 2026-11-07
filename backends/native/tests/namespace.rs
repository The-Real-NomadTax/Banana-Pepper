//! End to end on Linux: a real view served into `banana-sandbox` namespaces,
//! with no fusermount, bubblewrap or host-visible mount.
#![cfg(target_os = "linux")]
use banana_native::{Sandbox, namespace};
use std::{collections::BTreeMap, fs, path::Path, sync::Arc, time::Duration};

struct Served(Arc<banana_vfs::View>);
impl banana_api::SessionView for Served {
    fn serve(
        &self,
        r: Box<dyn std::io::Read + Send>,
        w: Box<dyn std::io::Write + Send>,
    ) -> std::io::Result<()> {
        banana_vfs::wire::serve(self.0.clone(), r, w)
    }
    fn read(&self, path: &str, limit: u64) -> std::io::Result<Vec<u8>> {
        let mut b = vec![0; limit as usize];
        let n = self.0.read(path, 0, &mut b)?;
        b.truncate(n);
        Ok(b)
    }
    fn read_dir(&self, path: &str) -> std::io::Result<Vec<banana_content::Entry>> {
        self.0.read_dir(path)
    }
    fn serve_fuse(&self, device: fs::File) -> std::io::Result<()> {
        banana_vfs::serve_fuse(self.0.clone(), device.into())
    }
}
fn view(root: &Path) -> Arc<banana_vfs::View> {
    let game = root.join("game");
    fs::create_dir_all(game.join("data")).unwrap();
    fs::write(game.join("seed.txt"), "seed\n").unwrap();
    fs::write(game.join("data/level.bin"), vec![7u8; 300_000]).unwrap();
    Arc::new(
        banana_vfs::View::new(
            Arc::new(banana_content::Folder::new(&game).unwrap()),
            &root.join("overlay"),
            "test",
            BTreeMap::new(),
            BTreeMap::new(),
        )
        .unwrap(),
    )
}
fn read(view: &banana_vfs::View, path: &str) -> Vec<u8> {
    let mut b = vec![0; 1 << 20];
    let n = view.read(path, 0, &mut b).unwrap();
    b.truncate(n);
    b
}

#[test]
fn isolated_guest_reads_and_writes_the_view() {
    let t = tempfile::tempdir().unwrap();
    let view = view(t.path());
    let state = t.path().join("state");
    fs::create_dir_all(&state).unwrap();
    let connection = namespace::ViewConnection::new().unwrap();
    let mut sandbox = Sandbox::new(Path::new(env!("CARGO_BIN_EXE_banana-sandbox")));
    sandbox.command.args(connection.args());
    sandbox
        .view("/game")
        .bind(&state, "/work", false)
        .view_bind("data", "/levels", true)
        .chdir("/game")
        .exec(
            "/bin/sh",
            &[
                "-c".into(),
                "cat seed.txt; wc -c < /levels/level.bin; echo saved > new.txt; \
                 test -d /proc/self && echo proc-ok; echo $$; id -u; \
                 touch /usr/x 2>/dev/null || echo usr-readonly"
                    .into(),
            ],
        );
    let child = sandbox
        .command
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let served = connection.serve(Arc::new(Served(view.clone())));
    let out = child.wait_with_output().unwrap();
    served.unwrap_or_else(|e| panic!("{e}: {}", String::from_utf8_lossy(&out.stderr)));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}{}", String::from_utf8_lossy(&out.stderr));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.is_empty(), "{stderr}");
    let lines: Vec<_> = stdout.lines().collect();
    assert_eq!(lines[0], "seed");
    assert_eq!(lines[1].trim(), "300000");
    assert_eq!(lines[2], "proc-ok");
    assert_eq!(lines[3], "2", "the guest runs as PID 2 under a minimal init");
    assert_eq!(lines[4], unsafe { libc::getuid() }.to_string());
    assert_eq!(lines[5], "usr-readonly");
    // The write landed in the overlay; the source folder is untouched.
    assert_eq!(read(&view, "new.txt"), b"saved\n");
    assert!(!t.path().join("game/new.txt").exists());
}

#[test]
fn held_namespace_serves_processes_that_enter_it() {
    let t = tempfile::tempdir().unwrap();
    let view = view(t.path());
    let resources = t.path().join("resources");
    fs::create_dir_all(resources.join("helpers")).unwrap();
    std::os::unix::fs::symlink(
        env!("CARGO_BIN_EXE_banana-sandbox"),
        namespace::helper(&resources),
    )
    .unwrap();
    let target = t.path().join("state/view");
    let holder = namespace::Holder::start(
        &resources,
        &target,
        Arc::new(Served(view.clone())),
        &t.path().join("sandbox.log"),
    )
    .unwrap();
    let mut c = std::process::Command::new("/bin/sh");
    c.args(["-c", "cat seed.txt; echo held > made.txt"]);
    holder.enter(&mut c, Some(&target));
    let out = c.output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(out.stdout, b"seed\n");
    assert_eq!(read(&view, "made.txt"), b"held\n");
    // Outside the namespace the directory stays empty: nothing is mounted.
    assert_eq!(fs::read_dir(&target).unwrap().count(), 0);
    drop(holder);
    // The connection ends with the namespace.
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while Arc::strong_count(&view) > 1 && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(Arc::strong_count(&view), 1);
}
