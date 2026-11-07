//! Linux cabinet joystick presence and focus-aware keyboard forwarding.
//! uinput and evdev are accessed directly; no Python evdev dependency.
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::{fd::AsRawFd, unix::fs::OpenOptionsExt},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
struct Pad {
    file: File,
    path: PathBuf,
}
impl Drop for Pad {
    fn drop(&mut self) {
        unsafe {
            libc::ioctl(self.file.as_raw_fd(), 0x5502 as libc::c_ulong);
        }
    }
}
impl Pad {
    fn new(player: usize) -> Result<Self, String> {
        let mut file = OpenOptions::new()
            .write(true)
            .custom_flags(libc::O_NONBLOCK | libc::O_CLOEXEC)
            .open("/dev/uinput")
            .map_err(|e| {
                format!("Virtual cabinet controllers require write access to /dev/uinput: {e}")
            })?;
        let fd = file.as_raw_fd();
        for (request, value) in [(0x40045564, 1), (0x40045564, 3)]
            .into_iter()
            .chain(
                [
                    0x130, 0x131, 0x133, 0x134, 0x136, 0x137, 0x13a, 0x13b, 0x13d, 0x13e,
                ]
                .into_iter()
                .map(|n| (0x40045565, n)),
            )
            .chain([0, 1, 16, 17].into_iter().map(|n| (0x40045567, n)))
        {
            if unsafe { libc::ioctl(fd, request as libc::c_ulong, value as libc::c_int) } < 0 {
                return Err(std::io::Error::last_os_error().to_string());
            }
        }
        let mut device: libc::uinput_user_dev = unsafe { std::mem::zeroed() };
        for (slot, byte) in device
            .name
            .iter_mut()
            .zip(format!("Banana pepper Pad {}", player + 1).bytes())
        {
            *slot = byte as libc::c_char;
        }
        device.id = libc::input_id {
            bustype: 3,
            vendor: 0x045e,
            product: 0x028e,
            version: 0x110 + player as u16,
        };
        for axis in [0, 1] {
            device.absmin[axis] = -32768;
            device.absmax[axis] = 32767;
            device.absfuzz[axis] = 16;
            device.absflat[axis] = 128;
        }
        for axis in [16, 17] {
            device.absmin[axis] = -1;
            device.absmax[axis] = 1;
        }
        let bytes = unsafe {
            std::slice::from_raw_parts(
                (&device as *const libc::uinput_user_dev).cast::<u8>(),
                std::mem::size_of_val(&device),
            )
        };
        file.write_all(bytes).map_err(|e| e.to_string())?;
        if unsafe { libc::ioctl(fd, 0x5501 as libc::c_ulong) } < 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        let mut pad = Self {
            file,
            path: PathBuf::new(),
        };
        let mut name = [0u8; 128];
        let request = (2u32 << 30) | (128 << 16) | ((b'U' as u32) << 8) | 44;
        if unsafe { libc::ioctl(fd, request as libc::c_ulong, name.as_mut_ptr()) } < 0 {
            return Err("Could not identify virtual input device".into());
        }
        let end = name.iter().position(|b| *b == 0).unwrap_or(name.len());
        let sys = Path::new("/sys/devices/virtual/input")
            .join(std::str::from_utf8(&name[..end]).map_err(|e| e.to_string())?);
        let until = Instant::now() + Duration::from_secs(3);
        loop {
            if let Ok(entries) = fs::read_dir(&sys) {
                for e in entries.flatten() {
                    if e.file_name().to_string_lossy().starts_with("event") {
                        let p = Path::new("/dev/input").join(e.file_name());
                        if p.exists() {
                            pad.path = p;
                            return Ok(pad);
                        }
                    }
                }
            }
            if Instant::now() >= until {
                return Err("udev did not create a virtual controller node".into());
            }
            thread::sleep(Duration::from_millis(25));
        }
    }
}
pub struct Bridge {
    pads: Vec<Pad>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}
impl Bridge {
    pub fn new(output: &Path, config: banana_api::InputBridge) -> Result<Self, String> {
        let pads = (0..config.virtual_pads as usize)
            .map(Pad::new)
            .collect::<Result<Vec<_>, _>>()?;
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let path = output.join("input.sock");
        let thread = (!config.keys.is_empty()).then(|| {
            thread::spawn(move || {
                if let Err(e) = forward(&path, &flag, &config) {
                    eprintln!("Input transport: {e}");
                }
            })
        });
        Ok(Self { pads, stop, thread })
    }
    pub fn paths(&self) -> impl Iterator<Item = &Path> {
        self.pads.iter().map(|p| p.path.as_path())
    }
}
impl Drop for Bridge {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}
fn focused(titles: &[String]) -> bool {
    banana_api::process::utilities::bounded_output(
        std::process::Command::new("xdotool").args(["getwindowfocus", "getwindowname"]),
        Duration::from_secs(1),
        65536,
    )
    .ok()
    .flatten()
    .is_some_and(|o| {
        titles
            .iter()
            .any(|title| String::from_utf8_lossy(&o.stdout).trim() == title)
    })
}
fn forward(path: &Path, stop: &AtomicBool, config: &banana_api::InputBridge) -> Result<(), String> {
    let mut keyboards = Vec::new();
    for e in fs::read_dir("/dev/input")
        .map_err(|e| e.to_string())?
        .flatten()
    {
        if !e.file_name().to_string_lossy().starts_with("event") {
            continue;
        }
        if let Ok(f) = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NONBLOCK | libc::O_CLOEXEC)
            .open(e.path())
        {
            let mut keys = [0u8; 96];
            let req = (2u32 << 30) | (96 << 16) | ((b'E' as u32) << 8) | 0x21;
            if unsafe { libc::ioctl(f.as_raw_fd(), req as libc::c_ulong, keys.as_mut_ptr()) } >= 0
                && keys[44 / 8] & (1 << (44 % 8)) != 0
                && keys[2 / 8] & (1 << (2 % 8)) != 0
            {
                keyboards.push((f, BTreeSet::<u16>::new()));
            }
        }
    }
    let output = std::os::unix::net::UnixDatagram::unbound().map_err(|e| e.to_string())?;
    output.set_nonblocking(true).map_err(|e| e.to_string())?;
    let mut sequence = 0;
    let mut edges = Vec::new();
    let mut counts = BTreeMap::<u16, usize>::new();
    let mut focus = false;
    let mut last = Instant::now() - Duration::from_secs(1);
    while !stop.load(Ordering::Acquire) {
        if last.elapsed() > Duration::from_millis(300) {
            last = Instant::now();
            focus = focused(&config.focus_titles);
            if !focus {
                for code in counts.keys() {
                    edges.push((*code, false));
                }
                counts.clear();
                for (_, held) in &mut keyboards {
                    held.clear();
                }
            }
        }
        for (file, held) in &mut keyboards {
            loop {
                let mut event: libc::input_event = unsafe { std::mem::zeroed() };
                let bytes = unsafe {
                    std::slice::from_raw_parts_mut(
                        (&mut event as *mut libc::input_event).cast::<u8>(),
                        std::mem::size_of::<libc::input_event>(),
                    )
                };
                match file.read(bytes) {
                    Ok(n) if n == bytes.len() => {
                        if event.type_ != 1 || event.value == 2 {
                            continue;
                        }
                        let Some(code) = config
                            .keys
                            .iter()
                            .find(|(key, _)| *key == event.code)
                            .map(|(_, code)| *code)
                        else {
                            continue;
                        };
                        if event.value != 0 && focus && held.insert(code) {
                            let n = counts.entry(code).or_default();
                            if *n == 0 {
                                edges.push((code, true));
                            }
                            *n += 1;
                        } else if event.value == 0 && held.remove(&code) {
                            if let Some(n) = counts.get_mut(&code) {
                                *n -= 1;
                                if *n == 0 {
                                    counts.remove(&code);
                                    edges.push((code, false));
                                }
                            }
                        }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                    _ => {
                        for code in std::mem::take(held) {
                            if let Some(n) = counts.get_mut(&code) {
                                *n -= 1;
                                if *n == 0 {
                                    counts.remove(&code);
                                    edges.push((code, false));
                                }
                            }
                        }
                        break;
                    }
                }
            }
        }
        sequence += 1;
        let frame = banana_input::transport::InputFrame {
            sequence,
            focused: focus,
            sources: config.keys.iter().map(|(_, c)| *c).collect(),
            held: counts.keys().copied().collect(),
            edges: std::mem::take(&mut edges),
        };
        if let Ok(bytes) = serde_json::to_vec(&frame) {
            let _ = output.send_to(&bytes, path);
        }
        thread::sleep(Duration::from_millis(25));
    }
    for code in counts.keys() {
        edges.push((*code, false));
    }
    let frame = banana_input::transport::InputFrame {
        sequence: sequence + 1,
        focused: false,
        sources: config.keys.iter().map(|(_, c)| *c).collect(),
        held: vec![],
        edges,
    };
    if let Ok(bytes) = serde_json::to_vec(&frame) {
        let _ = output.send_to(&bytes, path);
    }
    Ok(())
}
