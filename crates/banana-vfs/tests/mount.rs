//! Real FUSE mount and executable-loading checks. Run with --ignored on a
//! configured Linux host. Windows has no OS mount: its backends use `wire`.
#![cfg(target_os = "linux")]
use banana_content::{ContentSource, Folder, MemorySource, Zar};
use std::{
    collections::BTreeMap,
    fs::{self, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::Path,
    sync::Arc,
};
#[test]
#[ignore = "Runs only as a child of the mounted execution test"]
fn child_probe() {
    let Some(root) = std::env::var_os("BANANA_MOUNT_PROBE") else {
        return;
    };
    let root = Path::new(&root);
    assert_eq!(fs::read(root.join("seed.txt")).unwrap(), b"seed");
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(root.join("data.bin"))
        .unwrap();
    #[cfg(target_os = "linux")]
    unsafe {
        use std::os::fd::AsRawFd;
        let p = libc::mmap(
            std::ptr::null_mut(),
            4096,
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_SHARED,
            file.as_raw_fd(),
            0,
        );
        assert_ne!(p, libc::MAP_FAILED);
        assert_eq!(*(p as *const u8).add(7), 1);
        *(p as *mut u8).add(7) = 9;
        assert_eq!(libc::msync(p, 4096, libc::MS_SYNC), 0);
        assert_eq!(libc::munmap(p, 4096), 0);
    }
    file.sync_all().unwrap();
    drop(file);
    assert_eq!(fs::read(root.join("data.bin")).unwrap()[7], 9);
    fs::write(root.join("save.txt"), b"saved").unwrap();
    fs::rename(root.join("save.txt"), root.join("renamed.txt")).unwrap();
    let mut f = OpenOptions::new()
        .read(true)
        .write(true)
        .open(root.join("renamed.txt"))
        .unwrap();
    fs::remove_file(root.join("renamed.txt")).unwrap();
    f.seek(SeekFrom::Start(0)).unwrap();
    f.write_all(b"kept!").unwrap();
    f.seek(SeekFrom::Start(0)).unwrap();
    let mut b = String::new();
    f.read_to_string(&mut b).unwrap();
    assert_eq!(b, "kept!");
    fs::write(root.join("persistent.txt"), b"persistent").unwrap();
    println!("mounted probe complete");
}
#[test]
#[ignore = "Requires FUSE and permission to mount"]
fn mounted_sources_execute_and_preserve_dirty_pages() {
    #[cfg(target_os = "linux")]
    let temp = tempfile::tempdir().unwrap();
    #[cfg(not(target_os = "linux"))]
    let temp =
        tempfile::tempdir_in(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target")).unwrap();
    let input = temp.path().join("source");
    fs::create_dir(&input).unwrap();
    fs::write(input.join("seed.txt"), b"seed").unwrap();
    fs::write(input.join("data.bin"), vec![1; 4096]).unwrap();
    let archive = temp.path().join("source.zar");
    zarchive_rs::pack_dir(&input, &archive).unwrap();
    let original = fs::read(&archive).unwrap();
    let program: Arc<[u8]> = fs::read(std::env::current_exe().unwrap()).unwrap().into();
    let executable = if cfg!(windows) { "probe.exe" } else { "probe" };
    let providers: Vec<(&str, Arc<dyn ContentSource>)> = vec![
        ("folder", Arc::new(Folder::new(&input).unwrap())),
        ("archive", Arc::new(Zar::new(&archive).unwrap())),
        (
            "memory",
            Arc::new(MemorySource {
                files: BTreeMap::from([
                    ("seed.txt".into(), Arc::from(&b"seed"[..])),
                    ("data.bin".into(), Arc::from(vec![1; 4096])),
                ]),
            }),
        ),
    ];
    for (name, source) in providers {
        let fingerprint = source.fingerprint().unwrap();
        let generated = BTreeMap::from([(executable.into(), program.clone())]);
        let overlay = temp.path().join(format!("overlay-{name}"));
        let view = Arc::new(
            banana_vfs::View::new(
                source.clone(),
                &overlay,
                &fingerprint,
                generated.clone(),
                BTreeMap::new(),
            )
            .unwrap(),
        );
        let point = temp.path().join(format!("mount-{name}"));
        let mount = banana_vfs::mount(view.clone(), &point).unwrap();
        let output = std::process::Command::new(mount.path.join(executable))
            .args(["--ignored", "--exact", "child_probe", "--nocapture"])
            .env("BANANA_MOUNT_PROBE", &mount.path)
            .current_dir(&mount.path)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        drop(mount);
        assert!(!point.exists());
        drop(view);
        let view =
            banana_vfs::View::new(source, &overlay, &fingerprint, generated, BTreeMap::new())
                .unwrap();
        let mut b = [0; 4096];
        view.read("data.bin", 0, &mut b).unwrap();
        assert_eq!(b[7], 9);
        assert!(view.metadata("save.txt").is_err());
        assert!(view.metadata("renamed.txt").is_err());
        assert_eq!(view.metadata("persistent.txt").unwrap().len, 10);
        assert!(!overlay.join(executable).exists());
    }
    assert_eq!(fs::read(&archive).unwrap(), original);
    assert_eq!(fs::read(input.join("data.bin")).unwrap(), vec![1; 4096]);
    assert_eq!(fs::read_dir(input).unwrap().count(), 2);
}

#[cfg(target_os = "linux")]
#[test]
#[ignore = "Runs only as a child of the crash cleanup test"]
fn crash_mount_owner() {
    let Some(root) = std::env::var_os("BANANA_CRASH_MOUNT") else {
        return;
    };
    let root = Path::new(&root);
    let source = Arc::new(MemorySource {
        files: BTreeMap::from([("data".into(), Arc::from(&b"ready"[..]))]),
    });
    let view = Arc::new(
        banana_vfs::View::new(
            source,
            &root.join("overlay"),
            "crash",
            BTreeMap::new(),
            BTreeMap::new(),
        )
        .unwrap(),
    );
    let _mount = banana_vfs::mount(view, &root.join("view")).unwrap();
    fs::write(root.join("ready"), b"ready").unwrap();
    loop {
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
}
#[cfg(target_os = "linux")]
#[test]
#[ignore = "Requires permission to mount FUSE"]
fn reclaims_a_mount_after_its_owner_crashes() {
    let root = tempfile::tempdir().unwrap();
    let child = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--ignored", "--exact", "crash_mount_owner"])
        .env("BANANA_CRASH_MOUNT", root.path())
        .stdout(std::process::Stdio::null())
        .spawn()
        .unwrap();
    struct Guard(std::process::Child);
    impl Drop for Guard {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let mut child = Guard(child);
    let start = std::time::Instant::now();
    while !root.path().join("ready").exists() {
        assert!(start.elapsed() < std::time::Duration::from_secs(5));
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert_eq!(fs::read(root.path().join("view/data")).unwrap(), b"ready");
    child.0.kill().unwrap();
    child.0.wait().unwrap();
    banana_vfs::cleanup_stale_mounts(root.path()).unwrap();
    assert!(!root.path().join("view").exists());
    assert!(!root.path().join("view.banana-mount-lock").exists());
}

#[cfg(target_os = "linux")]
#[test]
#[ignore = "Requires the patched PRoot path in BANANA_TEST_PROOT"]
fn rootless_namespace_executes_and_preserves_mapped_writes() {
    let executable = std::env::var_os("BANANA_TEST_PROOT").expect("BANANA_TEST_PROOT is required");
    let temp = tempfile::tempdir().unwrap();
    let source: Arc<dyn ContentSource> = Arc::new(MemorySource {
        files: BTreeMap::from([
            ("seed.txt".into(), Arc::from(&b"seed"[..])),
            ("data.bin".into(), Arc::from(vec![1; 4096])),
        ]),
    });
    let generated = BTreeMap::from([(
        "probe".into(),
        Arc::from(fs::read(std::env::current_exe().unwrap()).unwrap()),
    )]);
    let overlay = temp.path().join("overlay");
    let view = Arc::new(
        banana_vfs::View::new(
            source.clone(),
            &overlay,
            "rootless",
            generated,
            BTreeMap::new(),
        )
        .unwrap(),
    );
    let empty = temp.path().join("empty");
    fs::create_dir(&empty).unwrap();
    let server = banana_vfs::rootless::Server::start(
        view.clone(),
        &temp.path().join("vfs.sock"),
        "/banana-test",
    )
    .unwrap();
    use std::os::unix::process::CommandExt;
    let mut child = std::process::Command::new(executable)
        .args([
            "-r",
            "/",
            "-w",
            "/banana-test",
            "/banana-test/probe",
            "--ignored",
            "--exact",
            "rootless_child_probe",
            "--nocapture",
        ])
        .env("BANANA_VFS_SOCKET", &server.socket)
        .env("BANANA_VFS_EMPTY", empty)
        .env("BANANA_MOUNT_PROBE", "/banana-test")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .process_group(0)
        .spawn()
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    loop {
        if child.try_wait().unwrap().is_some() {
            break;
        }
        if std::time::Instant::now() >= deadline {
            unsafe {
                libc::kill(-(child.id() as i32), libc::SIGKILL);
            };
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    server.flush().unwrap();
    drop(server);
    drop(view);
    let view = banana_vfs::View::new(
        source.clone(),
        &overlay,
        "rootless",
        BTreeMap::new(),
        BTreeMap::new(),
    )
    .unwrap();
    let mut bytes = [0; 4096];
    view.read("data.bin", 0, &mut bytes).unwrap();
    assert_eq!(bytes[7], 9);
    assert_eq!(view.metadata("persistent.txt").unwrap().len, 10);
    assert!(view.metadata("save.txt").is_err());
    assert!(view.metadata("renamed.txt").is_err());
    assert_eq!(
        banana_content::read_all(source.as_ref(), "data.bin", 4096).unwrap(),
        vec![1; 4096]
    );
    assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 2);
}

#[cfg(target_os = "linux")]
#[test]
#[ignore = "Child helper for the rootless namespace test"]
fn rootless_child_probe() {
    let Some(root) = std::env::var_os("BANANA_MOUNT_PROBE") else {
        return;
    };
    child_probe();
    let root = Path::new(&root);
    let file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(root.join("data.bin"))
        .unwrap();
    assert_eq!(file.metadata().unwrap().len(), 4096);
    fs::create_dir(root.join("directory")).unwrap();
    fs::write(root.join("directory/new"), b"new directory data").unwrap();
    let names = fs::read_dir(root.join("directory"))
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect::<Vec<_>>();
    assert_eq!(names, ["new"]);
    fs::rename(root.join("directory"), root.join("moved-directory")).unwrap();
    assert_eq!(
        fs::read(root.join("moved-directory/new")).unwrap(),
        b"new directory data"
    );
    fs::remove_file(root.join("moved-directory/new")).unwrap();
    fs::remove_dir(root.join("moved-directory")).unwrap();
    fs::copy(root.join("data.bin"), root.join("copied.bin")).unwrap();
    assert_eq!(
        fs::read(root.join("copied.bin")).unwrap(),
        fs::read(root.join("data.bin")).unwrap()
    );
    use std::os::fd::AsRawFd;
    let mapping = unsafe {
        libc::mmap(
            std::ptr::null_mut(),
            4096,
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_PRIVATE,
            file.as_raw_fd(),
            0,
        )
    };
    assert_ne!(mapping, libc::MAP_FAILED);
    unsafe {
        *(mapping as *mut u8).add(7) = 77;
        assert_eq!(libc::msync(mapping, 4096, libc::MS_SYNC), 0);
        libc::munmap(mapping, 4096);
    }
    assert_eq!(fs::read(root.join("data.bin")).unwrap()[7], 9);
    let child = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--ignored", "--exact", "rootless_grandchild", "--nocapture"])
        .status()
        .unwrap();
    assert!(child.success());
}
#[cfg(target_os = "linux")]
#[test]
#[ignore = "Child process inheritance test"]
fn rootless_grandchild() {
    if let Some(root) = std::env::var_os("BANANA_MOUNT_PROBE") {
        fs::write(Path::new(&root).join("child-save"), b"child process").unwrap();
    }
}
