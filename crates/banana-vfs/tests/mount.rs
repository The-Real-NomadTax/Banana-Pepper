//! Real kernel mount and executable-loading checks. Run with --ignored on a configured host.
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
    #[cfg(windows)]
    unsafe {
        use std::os::windows::io::AsRawHandle;
        use windows::Win32::System::Memory::*;
        use windows::core::PCWSTR;
        let mapping = CreateFileMappingW(
            windows::Win32::Foundation::HANDLE(file.as_raw_handle()),
            None,
            PAGE_READWRITE,
            0,
            4096,
            PCWSTR::null(),
        )
        .unwrap();
        let address = MapViewOfFile(mapping, FILE_MAP_READ | FILE_MAP_WRITE, 0, 0, 4096);
        assert!(!address.Value.is_null());
        let p = address.Value.cast::<u8>();
        assert_eq!(*p.add(7), 1);
        *p.add(7) = 9;
        FlushViewOfFile(address.Value, 4096).unwrap();
        UnmapViewOfFile(address).unwrap();
        windows::Win32::Foundation::CloseHandle(mapping).unwrap();
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
#[ignore = "Requires WinFsp/FUSE and permission to mount"]
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
