//! Display and audio bridges between a session's Xvfb/PulseAudio and the
//! Kotlin transport. They run as threads of the session owner, outside the
//! traced rootfs, so the runtime image carries no bridge programs.
//!
//! * Display: copies the X screen into the shared frame file the app draws
//!   (32-byte header, then RGBA pixels) and injects the app's input lines with
//!   XTest, so the game sees ordinary X11 pointer and keyboard events.
//! * Audio: relays PulseAudio's simple-protocol PCM stream of the virtual sink
//!   to the app's AudioTrack socket.
//!
//! The app's sockets live in the abstract namespace: Android's SELinux policy
//! denies filesystem FIFOs and sockets in some app and shell directories.
use std::{
    collections::HashMap,
    fs::{File, OpenOptions},
    io::{BufRead, BufReader, ErrorKind, Read, Write},
    os::fd::{AsRawFd, FromRawFd, OwnedFd},
    os::unix::net::{SocketAddr, UnixListener, UnixStream},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU32, Ordering},
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

#[cfg(target_os = "android")]
use std::os::android::net::SocketAddrExt;
#[cfg(target_os = "linux")]
use std::os::linux::net::SocketAddrExt;

use x11rb::{
    connection::{Connection, RequestConnection},
    protocol::{
        shm::{self, ConnectionExt as _},
        xproto::{self, ConnectionExt as _, ImageFormat},
        xtest::ConnectionExt as _,
    },
    rust_connection::{DefaultStream, RustConnection},
};

/// Frame file header: magic, width, height, sequence, heartbeat, padding.
const HEADER: usize = 32;
const FRAME_MAGIC: u32 = 0x4246_4641; // "AFFB"
const AUDIO_MAGIC: u32 = 0x3141_4641; // "AFA1"
pub const AUDIO_RATE: u32 = 48_000;
pub const AUDIO_CHANNELS: u32 = 2;

/// Owns the bridge threads of one session; dropping it stops and joins them.
pub struct Bridges {
    stop: Arc<AtomicBool>,
    failure: Arc<Mutex<Option<String>>>,
    log: Arc<Mutex<Option<File>>>,
    threads: Vec<JoinHandle<()>>,
}

impl Bridges {
    pub fn new(log: &Path) -> Self {
        Self {
            stop: Arc::new(AtomicBool::new(false)),
            failure: Arc::new(Mutex::new(None)),
            log: Arc::new(Mutex::new(File::create(log).ok())),
            threads: Vec::new(),
        }
    }

    /// The first bridge failure, if any thread has stopped on an error.
    pub fn failure(&self) -> Option<String> {
        self.failure.lock().ok()?.clone()
    }

    /// Mirror the X screen served at `x_socket` into `frame` at `fps`, and
    /// accept input connections on the abstract socket `input`.
    pub fn display(
        &mut self,
        x_socket: &Path,
        frame: &Path,
        input: &str,
        fps: u32,
    ) -> Result<(), String> {
        let capture = connect(x_socket)?;
        let injector = Injector::new(connect(x_socket)?)?;
        let listener = UnixListener::bind_addr(&abstract_address(input)?)
            .map_err(|e| format!("Cannot listen for Android input on {input}: {e}"))?;
        listener.set_nonblocking(true).map_err(|e| e.to_string())?;
        let mirror = Mirror::new(capture, frame)?;
        self.note(&format!(
            "display {}x{} shm={} fps={fps}",
            mirror.width,
            mirror.height,
            mirror.shm.is_some()
        ));
        let period = Duration::from_secs(1) / fps.clamp(1, 120);
        self.spawn("display", move |stop| mirror.run(period, stop));
        let injector = Arc::new(Mutex::new(injector));
        let log = self.log.clone();
        self.spawn("input", move |stop| accept(listener, injector, log, stop));
        Ok(())
    }

    /// Relay the PCM stream served at `pcm` to the abstract socket `android`.
    pub fn audio(&mut self, pcm: &Path, android: &str) -> Result<(), String> {
        let mut source = connect_unix(pcm)
            .map_err(|e| format!("Cannot read the PulseAudio stream {}: {e}", pcm.display()))?;
        let mut sink = UnixStream::connect_addr(&abstract_address(android)?)
            .map_err(|e| format!("Cannot reach the Android audio socket {android}: {e}"))?;
        let mut header = Vec::with_capacity(12);
        for word in [AUDIO_MAGIC, AUDIO_RATE, AUDIO_CHANNELS] {
            header.extend_from_slice(&word.to_le_bytes());
        }
        sink.write_all(&header).map_err(|e| e.to_string())?;
        source
            .set_read_timeout(Some(Duration::from_millis(200)))
            .map_err(|e| e.to_string())?;
        self.note("audio relay started");
        self.spawn("audio", move |stop| {
            let mut buffer = [0u8; 8192];
            while !stop.load(Ordering::Relaxed) {
                match source.read(&mut buffer) {
                    Ok(0) => return Err("PulseAudio closed the stream".into()),
                    Ok(n) => sink
                        .write_all(&buffer[..n])
                        .map_err(|e| format!("Android audio socket: {e}"))?,
                    Err(e) if timed_out(&e) => {}
                    Err(e) => return Err(e.to_string()),
                }
            }
            Ok(())
        });
        Ok(())
    }

    fn spawn(
        &mut self,
        name: &'static str,
        body: impl FnOnce(&AtomicBool) -> Result<(), String> + Send + 'static,
    ) {
        let stop = self.stop.clone();
        let failure = self.failure.clone();
        self.threads.push(std::thread::spawn(move || {
            if let Err(e) = body(&stop)
                && !stop.load(Ordering::Relaxed)
                && let Ok(mut slot) = failure.lock()
            {
                slot.get_or_insert(format!("Android {name} bridge: {e}"));
            }
        }));
    }

    fn note(&self, message: &str) {
        note(&self.log, message)
    }
}

impl Drop for Bridges {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        for thread in self.threads.drain(..) {
            let _ = thread.join();
        }
    }
}

fn note(log: &Mutex<Option<File>>, message: &str) {
    if let Ok(mut log) = log.lock()
        && let Some(file) = log.as_mut()
    {
        let _ = writeln!(file, "bridge: {message}");
    }
}

fn timed_out(e: &std::io::Error) -> bool {
    matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut)
}

fn abstract_address(name: &str) -> Result<SocketAddr, String> {
    SocketAddr::from_abstract_name(name.as_bytes())
        .map_err(|e| format!("Invalid abstract socket name {name}: {e}"))
}

/// Connect to a socket file. Session paths can exceed `sun_path`; then the
/// socket is reached through a short descriptor path to its directory.
fn connect_unix(path: &Path) -> std::io::Result<UnixStream> {
    use std::os::unix::ffi::OsStrExt;
    if path.as_os_str().as_bytes().len() < 100 {
        return UnixStream::connect(path);
    }
    let directory = File::open(path.parent().unwrap_or(Path::new("/")))?;
    let short = PathBuf::from(format!("/proc/self/fd/{}", directory.as_raw_fd()))
        .join(path.file_name().unwrap_or_default());
    UnixStream::connect(short)
}

fn connect(x_socket: &Path) -> Result<RustConnection, String> {
    let stream = connect_unix(x_socket)
        .map_err(|e| format!("Cannot reach Xvfb at {}: {e}", x_socket.display()))?;
    let (stream, _) = DefaultStream::from_unix_stream(stream).map_err(|e| e.to_string())?;
    RustConnection::connect_to_stream(stream, 0).map_err(|e| format!("X11 setup failed: {e}"))
}

/// A shared, writable memory mapping released on drop.
struct Mapping {
    address: *mut u8,
    length: usize,
}
// The mapping is plain shared memory; the owner serializes its use.
unsafe impl Send for Mapping {}

impl Mapping {
    fn new(fd: i32, length: usize) -> Result<Self, String> {
        // SAFETY: a fresh shared mapping of a descriptor sized to `length`.
        let address = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                length,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED,
                fd,
                0,
            )
        };
        if address == libc::MAP_FAILED {
            return Err(std::io::Error::last_os_error().to_string());
        }
        Ok(Self {
            address: address.cast(),
            length,
        })
    }
    fn bytes(&self) -> &[u8] {
        // SAFETY: the mapping is live and `length` bytes long.
        unsafe { std::slice::from_raw_parts(self.address, self.length) }
    }
    fn bytes_mut(&mut self) -> &mut [u8] {
        // SAFETY: as above; `&mut self` makes this the only Rust view.
        unsafe { std::slice::from_raw_parts_mut(self.address, self.length) }
    }
    fn word(&self, offset: usize) -> &AtomicU32 {
        assert!(offset + 4 <= self.length && offset % 4 == 0);
        // SAFETY: aligned, in bounds, and only accessed atomically by readers.
        unsafe { &*(self.address.add(offset) as *const AtomicU32) }
    }
}

impl Drop for Mapping {
    fn drop(&mut self) {
        // SAFETY: unmaps exactly the region `new` mapped.
        unsafe { libc::munmap(self.address.cast(), self.length) };
    }
}

/// Copies the root window into the frame file.
struct Mirror {
    connection: RustConnection,
    root: xproto::Window,
    width: u16,
    height: u16,
    frame: Mapping,
    /// MIT-SHM segment shared with the server, when it accepts one.
    shm: Option<(shm::Seg, Mapping)>,
}

impl Mirror {
    fn new(connection: RustConnection, path: &Path) -> Result<Self, String> {
        let screen = &connection.setup().roots[0];
        let (root, width, height) = (screen.root, screen.width_in_pixels, screen.height_in_pixels);
        let pixels = usize::from(width) * usize::from(height) * 4;
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(path)
            .map_err(|e| format!("Cannot create the frame file {}: {e}", path.display()))?;
        file.set_len((HEADER + pixels) as u64)
            .map_err(|e| e.to_string())?;
        let frame = Mapping::new(file.as_raw_fd(), HEADER + pixels)?;
        let shm = shared_segment(&connection, pixels).ok();
        let mirror = Self {
            connection,
            root,
            width,
            height,
            frame,
            shm,
        };
        mirror.frame.word(4).store(width.into(), Ordering::Relaxed);
        mirror.frame.word(8).store(height.into(), Ordering::Relaxed);
        mirror.frame.word(0).store(FRAME_MAGIC, Ordering::Release);
        Ok(mirror)
    }

    fn run(mut self, period: Duration, stop: &AtomicBool) -> Result<(), String> {
        let mut next = Instant::now();
        while !stop.load(Ordering::Relaxed) {
            self.frame.word(16).fetch_add(1, Ordering::Relaxed);
            self.capture()?;
            next += period;
            let now = Instant::now();
            if now > next + Duration::from_secs(1) {
                next = now; // fell far behind; resynchronize
            }
            std::thread::sleep(next.saturating_duration_since(now));
        }
        Ok(())
    }

    fn capture(&mut self) -> Result<(), String> {
        let (width, height) = (self.width, self.height);
        let pixels = usize::from(width) * usize::from(height) * 4;
        let owned;
        let source: &[u8] = if let Some((seg, memory)) = &self.shm {
            shm::get_image(
                &self.connection,
                self.root,
                0,
                0,
                width,
                height,
                !0,
                ImageFormat::Z_PIXMAP.into(),
                *seg,
                0,
            )
            .map_err(|e| e.to_string())?
            .reply()
            .map_err(|e| e.to_string())?;
            memory.bytes()
        } else {
            owned = self
                .connection
                .get_image(ImageFormat::Z_PIXMAP, self.root, 0, 0, width, height, !0)
                .map_err(|e| e.to_string())?
                .reply()
                .map_err(|e| e.to_string())?
                .data;
            &owned
        };
        if source.len() < pixels {
            return Err(format!(
                "X screen is not 32 bits per pixel ({} bytes for {width}x{height})",
                source.len()
            ));
        }
        convert(&source[..pixels], &mut self.frame.bytes_mut()[HEADER..]);
        self.frame.word(12).fetch_add(1, Ordering::Release);
        Ok(())
    }
}

/// X's little-endian 0x00RRGGBB pixels to Android's RGBA memory order.
fn convert(source: &[u8], target: &mut [u8]) {
    for (s, t) in source.chunks_exact(4).zip(target.chunks_exact_mut(4)) {
        t.copy_from_slice(&[s[2], s[1], s[0], 0xff]);
    }
}

/// Share a memfd with the server (MIT-SHM 1.2), so frames skip the socket.
fn shared_segment(
    connection: &RustConnection,
    length: usize,
) -> Result<(shm::Seg, Mapping), String> {
    connection
        .extension_information(shm::X11_EXTENSION_NAME)
        .map_err(|e| e.to_string())?
        .ok_or("MIT-SHM unavailable")?;
    let version = connection
        .shm_query_version()
        .map_err(|e| e.to_string())?
        .reply()
        .map_err(|e| e.to_string())?;
    if (version.major_version, version.minor_version) < (1, 2) {
        return Err("MIT-SHM lacks descriptor passing".into());
    }
    // memfd_create through syscall(): bionic only exports it from API 30.
    // SAFETY: a static name and flags; the result is checked.
    let fd = unsafe {
        libc::syscall(
            libc::SYS_memfd_create,
            c"banana-frame".as_ptr(),
            libc::MFD_CLOEXEC,
        )
    };
    if fd < 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    // SAFETY: `fd` is a new descriptor this function owns.
    let fd = unsafe { OwnedFd::from_raw_fd(fd as i32) };
    File::from(fd.try_clone().map_err(|e| e.to_string())?)
        .set_len(length as u64)
        .map_err(|e| e.to_string())?;
    let memory = Mapping::new(fd.as_raw_fd(), length)?;
    let seg = connection.generate_id().map_err(|e| e.to_string())?;
    connection
        .shm_attach_fd(seg, fd, false)
        .map_err(|e| e.to_string())?
        .check()
        .map_err(|e| e.to_string())?;
    Ok((seg, memory))
}

/// Injects input lines with XTest:
/// `m X Y` pointer position, `b N 1|0` button, `k NAME 1|0` key.
struct Injector {
    connection: RustConnection,
    root: xproto::Window,
    keycodes: HashMap<u32, u8>,
}

impl Injector {
    fn new(connection: RustConnection) -> Result<Self, String> {
        connection
            .extension_information(x11rb::protocol::xtest::X11_EXTENSION_NAME)
            .map_err(|e| e.to_string())?
            .ok_or("The X server lacks XTest; input cannot be injected")?;
        let setup = connection.setup();
        let root = setup.roots[0].root;
        let (first, last) = (setup.min_keycode, setup.max_keycode);
        let mapping = connection
            .get_keyboard_mapping(first, last - first + 1)
            .map_err(|e| e.to_string())?
            .reply()
            .map_err(|e| e.to_string())?;
        let mut keycodes = HashMap::new();
        let per = usize::from(mapping.keysyms_per_keycode.max(1));
        for (index, symbols) in mapping.keysyms.chunks(per).enumerate() {
            for &symbol in symbols.iter().filter(|&&s| s != 0) {
                keycodes.entry(symbol).or_insert(first + index as u8);
            }
        }
        Ok(Self {
            connection,
            root,
            keycodes,
        })
    }

    fn handle(&self, line: &str) -> Result<(), String> {
        let mut words = line.split_ascii_whitespace();
        let (kind, a, b) = (words.next(), words.next(), words.next());
        let pressed = |v: Option<&str>| v.map(|v| v != "0");
        let (event, detail, x, y) = match (kind, a, b) {
            (Some("m"), Some(x), Some(y)) => {
                let x = x
                    .parse::<i16>()
                    .map_err(|_| format!("Bad pointer line {line}"))?;
                let y = y
                    .parse::<i16>()
                    .map_err(|_| format!("Bad pointer line {line}"))?;
                (xproto::MOTION_NOTIFY_EVENT, 0, x, y)
            }
            (Some("b"), Some(button), state) => {
                let button = button
                    .parse::<u8>()
                    .map_err(|_| format!("Bad button line {line}"))?;
                let event = if pressed(state).unwrap_or(false) {
                    xproto::BUTTON_PRESS_EVENT
                } else {
                    xproto::BUTTON_RELEASE_EVENT
                };
                (event, button, 0, 0)
            }
            (Some("k"), Some(name), state) => {
                let code = keysym(name)
                    .and_then(|s| self.keycodes.get(&s).copied())
                    .ok_or_else(|| format!("unknown key {name}"))?;
                let event = if pressed(state).unwrap_or(false) {
                    xproto::KEY_PRESS_EVENT
                } else {
                    xproto::KEY_RELEASE_EVENT
                };
                (event, code, 0, 0)
            }
            _ => return Err(format!("unknown input line {line:?}")),
        };
        self.connection
            .xtest_fake_input(event, detail, x11rb::CURRENT_TIME, self.root, x, y, 0)
            .map_err(|e| e.to_string())?;
        self.connection.flush().map_err(|e| e.to_string())
    }
}

fn accept(
    listener: UnixListener,
    injector: Arc<Mutex<Injector>>,
    log: Arc<Mutex<Option<File>>>,
    stop: &AtomicBool,
) -> Result<(), String> {
    let mut clients: Vec<JoinHandle<()>> = Vec::new();
    let stop_flag = Arc::new(AtomicBool::new(false));
    while !stop.load(Ordering::Relaxed) {
        match listener.accept() {
            Ok((client, _)) => {
                client.set_nonblocking(false).map_err(|e| e.to_string())?;
                client
                    .set_read_timeout(Some(Duration::from_millis(200)))
                    .map_err(|e| e.to_string())?;
                let (injector, log, stop) = (injector.clone(), log.clone(), stop_flag.clone());
                clients.retain(|c| !c.is_finished());
                clients.push(std::thread::spawn(move || {
                    serve(client, &injector, &log, &stop)
                }));
            }
            Err(e) if timed_out(&e) => std::thread::sleep(Duration::from_millis(20)),
            Err(e) if e.kind() == ErrorKind::Interrupted => {}
            Err(e) => {
                stop_flag.store(true, Ordering::Relaxed);
                return Err(e.to_string());
            }
        }
    }
    stop_flag.store(true, Ordering::Relaxed);
    for client in clients {
        let _ = client.join();
    }
    Ok(())
}

fn serve(
    client: UnixStream,
    injector: &Mutex<Injector>,
    log: &Mutex<Option<File>>,
    stop: &AtomicBool,
) {
    let mut reader = BufReader::new(client);
    let mut line = String::new();
    while !stop.load(Ordering::Relaxed) {
        match reader.read_line(&mut line) {
            Ok(0) => return,
            Ok(_) if line.ends_with('\n') => {
                let result = injector
                    .lock()
                    .map_err(|_| "input lock poisoned".to_owned())
                    .and_then(|i| i.handle(line.trim()));
                if let Err(e) = result {
                    note(log, &e);
                }
                line.clear();
            }
            Ok(_) => {}
            Err(e) if timed_out(&e) => {}
            Err(_) => return,
        }
    }
}

/// X keysym for an input name: X names (`Return`, `Shift_L`, `a`) and the
/// canonical control names from `banana-input` (`LeftShift`, `Keypad0`).
pub fn keysym(name: &str) -> Option<u32> {
    let bytes = name.as_bytes();
    if bytes.len() == 1 && bytes[0].is_ascii_graphic() {
        return Some(u32::from(bytes[0].to_ascii_lowercase()));
    }
    if let Some(n) = name.strip_prefix('F').and_then(|n| n.parse::<u32>().ok())
        && (1..=35).contains(&n)
    {
        return Some(0xffbd + n);
    }
    if let Some(d) = name
        .strip_prefix("Keypad")
        .or_else(|| name.strip_prefix("KP_"))
        .and_then(|d| d.parse::<u32>().ok())
        && d <= 9
    {
        return Some(0xffb0 + d);
    }
    let symbol = match name {
        "space" | "Space" => 0x20,
        "apostrophe" | "Quote" => 0x27,
        "comma" | "Comma" => 0x2c,
        "minus" | "Minus" => 0x2d,
        "period" | "Period" => 0x2e,
        "slash" | "Slash" => 0x2f,
        "semicolon" | "Semicolon" => 0x3b,
        "equal" | "Equals" => 0x3d,
        "bracketleft" | "LeftBracket" => 0x5b,
        "backslash" | "Backslash" => 0x5c,
        "bracketright" | "RightBracket" => 0x5d,
        "grave" | "Backquote" => 0x60,
        "BackSpace" | "Backspace" => 0xff08,
        "Tab" => 0xff09,
        "Return" | "Enter" => 0xff0d,
        "Pause" => 0xff13,
        "Escape" => 0xff1b,
        "Delete" => 0xffff,
        "Home" => 0xff50,
        "Left" => 0xff51,
        "Up" => 0xff52,
        "Right" => 0xff53,
        "Down" => 0xff54,
        "Prior" | "PageUp" => 0xff55,
        "Next" | "PageDown" => 0xff56,
        "End" => 0xff57,
        "Insert" => 0xff63,
        "KP_Enter" | "KeypadEnter" => 0xff8d,
        "KP_Multiply" | "KeypadMultiply" => 0xffaa,
        "KP_Add" | "KeypadPlus" => 0xffab,
        "KP_Subtract" | "KeypadMinus" => 0xffad,
        "KP_Decimal" | "KeypadPeriod" => 0xffae,
        "KP_Divide" | "KeypadDivide" => 0xffaf,
        "Shift_L" | "LeftShift" => 0xffe1,
        "Shift_R" | "RightShift" => 0xffe2,
        "Control_L" | "LeftCtrl" => 0xffe3,
        "Control_R" | "RightCtrl" => 0xffe4,
        "Alt_L" | "LeftAlt" => 0xffe9,
        "Alt_R" | "RightAlt" => 0xffea,
        _ => return None,
    };
    Some(symbol)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_resolve_to_x_keysyms() {
        for (name, symbol) in [
            ("a", 0x61),
            ("T", 0x74),
            ("1", 0x31),
            ("Return", 0xff0d),
            ("Enter", 0xff0d),
            ("space", 0x20),
            ("Escape", 0xff1b),
            ("F1", 0xffbe),
            ("F12", 0xffc9),
            ("Keypad0", 0xffb0),
            ("LeftShift", 0xffe1),
            ("Prior", 0xff55),
        ] {
            assert_eq!(keysym(name), Some(symbol), "{name}");
        }
        assert_eq!(keysym("NoSuchKey"), None);
    }

    #[test]
    fn pixels_become_rgba() {
        let mut target = [0u8; 8];
        convert(
            &[0x30, 0x20, 0x10, 0x00, 0xff, 0x00, 0x00, 0x00],
            &mut target,
        );
        assert_eq!(target, [0x10, 0x20, 0x30, 0xff, 0x00, 0x00, 0xff, 0xff]);
    }
}
