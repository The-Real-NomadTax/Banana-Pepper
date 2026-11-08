//! The Android display/audio bridges against a real Xvfb (Linux hosts with
//! Xvfb installed): frames reach the frame file in RGBA order, input lines
//! become X events, and the PCM stream is relayed with the app's header.
#![cfg(target_os = "linux")]
use banana_android::bridge::Bridges;
use std::{
    io::{Read, Write},
    os::linux::net::SocketAddrExt,
    os::unix::net::{SocketAddr, UnixListener, UnixStream},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};
use x11rb::{
    connection::Connection,
    protocol::xproto::{ChangeWindowAttributesAux, ConnectionExt as _},
};

struct Server(Child);
impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn until(what: &str, mut ready: impl FnMut() -> bool) {
    let start = Instant::now();
    while !ready() {
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "timed out: {what}"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn word(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

/// Xvfb with its socket directory bound to `work/x11` inside a private user
/// and mount namespace, as the app's sessions keep theirs in session state.
fn xvfb(work: &Path) -> Option<(Server, PathBuf)> {
    use std::os::unix::process::CommandExt;
    if Command::new("Xvfb")
        .arg("-help")
        .stderr(Stdio::null())
        .status()
        .is_err()
    {
        eprintln!("Xvfb is not installed; skipping the bridge test");
        return None;
    }
    let sockets = work.join("x11");
    std::fs::create_dir_all(&sockets).unwrap();
    let display = 90 + std::process::id() % 400;
    let (uid, gid) = unsafe { (libc::getuid(), libc::getgid()) };
    let source = std::ffi::CString::new(sockets.to_str().unwrap()).unwrap();
    let maps = [
        (c"/proc/self/setgroups", "deny".to_owned()),
        (c"/proc/self/uid_map", format!("{uid} {uid} 1")),
        (c"/proc/self/gid_map", format!("{gid} {gid} 1")),
    ];
    let mut command = Command::new("Xvfb");
    command
        .args([
            format!(":{display}"),
            "-screen".into(),
            "0".into(),
            "320x240x24".into(),
        ])
        .args(["-nolisten", "tcp", "-ac"])
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    // SAFETY: only async-signal-safe calls between fork and exec.
    unsafe {
        command.pre_exec(move || {
            let check = |r: libc::c_int| {
                if r < 0 {
                    Err(std::io::Error::last_os_error())
                } else {
                    Ok(())
                }
            };
            check(libc::unshare(libc::CLONE_NEWUSER | libc::CLONE_NEWNS))?;
            for (path, text) in &maps {
                let fd = libc::open(path.as_ptr(), libc::O_WRONLY);
                check(fd)?;
                let written = libc::write(fd, text.as_ptr().cast(), text.len());
                libc::close(fd);
                check(written as libc::c_int)?;
            }
            check(libc::mount(
                source.as_ptr(),
                c"/tmp/.X11-unix".as_ptr(),
                std::ptr::null(),
                libc::MS_BIND,
                std::ptr::null(),
            ))
        });
    }
    let server = Server(command.spawn().unwrap());
    let socket = sockets.join(format!("X{display}"));
    until("Xvfb socket", || socket.exists());
    Some((server, socket))
}

fn client(socket: &Path) -> x11rb::rust_connection::RustConnection {
    let stream = UnixStream::connect(socket).unwrap();
    let (stream, _) = x11rb::rust_connection::DefaultStream::from_unix_stream(stream).unwrap();
    x11rb::rust_connection::RustConnection::connect_to_stream(stream, 0).unwrap()
}

#[test]
fn display_and_input_reach_the_app() {
    let work = std::env::temp_dir().join(format!("banana-bridge-{}", std::process::id()));
    std::fs::create_dir_all(&work).unwrap();
    let Some((_server, socket)) = xvfb(&work) else {
        return;
    };
    let frame = work.join("frame.bin");
    let input = format!("banana.test.{}.input", std::process::id());
    let mut bridges = Bridges::new(&work.join("bridge.log"));
    bridges.display(&socket, &frame, &input, 60).unwrap();

    // Paint the root window a known color.
    let x = client(&socket);
    let root = x.setup().roots[0].root;
    x.change_window_attributes(
        root,
        &ChangeWindowAttributesAux::new().background_pixel(0x0012_3456),
    )
    .unwrap();
    x.clear_area(false, root, 0, 0, 0, 0).unwrap();
    x11rb::wrapper::ConnectionExt::sync(&x).unwrap();
    let painted = |bytes: &[u8]| bytes.len() >= 36 && bytes[32..36] == [0x12, 0x34, 0x56, 0xff];
    until("painted frame", || {
        std::fs::read(&frame).is_ok_and(|b| painted(&b))
    });
    let bytes = std::fs::read(&frame).unwrap();
    assert_eq!(word(&bytes, 0), 0x4246_4641);
    assert_eq!((word(&bytes, 4), word(&bytes, 8)), (320, 240));
    assert_eq!(bytes.len(), 32 + 320 * 240 * 4);
    assert!(word(&bytes, 12) > 0 && word(&bytes, 16) > 0);

    let mut client =
        UnixStream::connect_addr(&SocketAddr::from_abstract_name(input.as_bytes()).unwrap())
            .unwrap();
    client.write_all(b"m 100 50\nk a 1\n").unwrap();
    until("pointer move", || {
        let p = x.query_pointer(root).unwrap().reply().unwrap();
        (p.root_x, p.root_y) == (100, 50)
    });
    let a = {
        let setup = x.setup();
        let (first, count) = (setup.min_keycode, setup.max_keycode - setup.min_keycode + 1);
        let map = x
            .get_keyboard_mapping(first, count)
            .unwrap()
            .reply()
            .unwrap();
        let per = usize::from(map.keysyms_per_keycode);
        first
            + map
                .keysyms
                .chunks(per)
                .position(|s| s.contains(&0x61))
                .unwrap() as u8
    };
    let held = |x: &x11rb::rust_connection::RustConnection| {
        let keys = x.query_keymap().unwrap().reply().unwrap().keys;
        keys[usize::from(a / 8)] & (1 << (a % 8)) != 0
    };
    until("key press", || held(&x));
    client.write_all(b"k a 0\n").unwrap();
    until("key release", || !held(&x));
    assert_eq!(bridges.failure(), None);
    drop(bridges);
    let log = std::fs::read_to_string(work.join("bridge.log")).unwrap();
    assert!(log.contains("shm=true"), "frames should use MIT-SHM: {log}");
    let _ = std::fs::remove_dir_all(work);
}

#[test]
fn audio_is_relayed_with_the_app_header() {
    let work = std::env::temp_dir().join(format!("banana-audio-{}", std::process::id()));
    std::fs::create_dir_all(&work).unwrap();
    let pcm_path = work.join("pulse-pcm");
    let pcm = UnixListener::bind(&pcm_path).unwrap();
    let name = format!("banana.test.{}.audio", std::process::id());
    let app =
        UnixListener::bind_addr(&SocketAddr::from_abstract_name(name.as_bytes()).unwrap()).unwrap();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = pcm.accept().unwrap();
        stream.write_all(&[1, 2, 3, 4, 5, 6, 7, 8]).unwrap();
        std::thread::sleep(Duration::from_millis(300));
    });
    let mut bridges = Bridges::new(&work.join("bridge.log"));
    bridges.audio(Path::new(&pcm_path), &name).unwrap();
    let (mut received, _) = app.accept().unwrap();
    let mut bytes = [0u8; 20];
    received.read_exact(&mut bytes).unwrap();
    assert_eq!(&bytes[..4], b"AFA1");
    assert_eq!((word(&bytes, 4), word(&bytes, 8)), (48_000, 2));
    assert_eq!(&bytes[12..], &[1, 2, 3, 4, 5, 6, 7, 8]);
    server.join().unwrap();
    until("closed stream reported", || bridges.failure().is_some());
    drop(bridges);
    let _ = std::fs::remove_dir_all(work);
}
