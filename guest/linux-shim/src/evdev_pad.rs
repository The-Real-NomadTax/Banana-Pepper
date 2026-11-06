//! Linux controllers through evdev, read into banana-input's XInput-style
//! [`Pad`]. Buttons follow the kernel's Xbox (xpad) layout: BTN_A/B/X/Y,
//! bumpers, Back/Start, stick clicks; ABS_X/Y and ABS_RX/RY sticks, ABS_Z/RZ
//! (or ABS_BRAKE/GAS) triggers, a hat or BTN_DPAD_* for the d-pad.
//!
//! The launchers bind only controller device nodes into the sandbox
//! (`ARCADE_PAD_DEVICES`); without that, every `/dev/input/event*` that
//! reports gamepad buttons is used. Reads never block.
use crate::pad::{self, Pad};
use std::ffi::CString;
use std::time::{Duration, Instant};

const EV_KEY: u16 = 1;
const EV_ABS: u16 = 3;
const BTN_SOUTH: usize = 0x130;
/// Keyboards (including virtual ones) can report gamepad buttons too.
const KEY_A: usize = 30;

const ABS_X: usize = 0x00;
const ABS_Y: usize = 0x01;
const ABS_Z: usize = 0x02;
const ABS_RX: usize = 0x03;
const ABS_RY: usize = 0x04;
const ABS_RZ: usize = 0x05;
const ABS_GAS: usize = 0x09;
const ABS_BRAKE: usize = 0x0a;
const ABS_HAT0X: usize = 0x10;
const ABS_HAT0Y: usize = 0x11;
const AXES: [usize; 10] = [
    ABS_X, ABS_Y, ABS_Z, ABS_RX, ABS_RY, ABS_RZ, ABS_GAS, ABS_BRAKE, ABS_HAT0X, ABS_HAT0Y,
];

/// evdev key code -> pad mask.
fn button(code: u16) -> u32 {
    match code {
        0x130 => pad::A,
        0x131 => pad::B,
        0x133 => pad::X, // BTN_X (xpad); BTN_NORTH on some other drivers
        0x134 => pad::Y,
        0x136 => pad::LB,
        0x137 => pad::RB,
        0x138 => pad::LT, // digital triggers on some pads
        0x139 => pad::RT,
        0x13a => pad::BACK,
        0x13b => pad::START,
        0x13d => pad::LS,
        0x13e => pad::RS,
        0x220 => pad::UP,
        0x221 => pad::DOWN,
        0x222 => pad::LEFT,
        0x223 => pad::RIGHT,
        _ => 0,
    }
}

#[repr(C)]
#[derive(Default, Clone, Copy)]
struct AbsInfo {
    value: i32,
    minimum: i32,
    maximum: i32,
    fuzz: i32,
    flat: i32,
    resolution: i32,
}

const fn ioc_read(nr: u32, size: u32) -> libc::c_ulong {
    ((2u32 << 30) | (size << 16) | ((b'E' as u32) << 8) | nr) as libc::c_ulong
}

struct Device {
    fd: libc::c_int,
    path: String,
    ranges: [Option<(i32, i32)>; 0x12],
    buttons: u32,
    axes: [i32; 0x12],
}

impl Device {
    fn open(path: &str) -> Option<Self> {
        let name = CString::new(path).ok()?;
        let fd = unsafe {
            libc::open(
                name.as_ptr(),
                libc::O_RDONLY | libc::O_NONBLOCK | libc::O_CLOEXEC,
            )
        };
        if fd < 0 {
            return None;
        }
        let mut keys = [0u8; 0x300 / 8];
        let ok = unsafe {
            libc::ioctl(
                fd,
                ioc_read(0x20 + u32::from(EV_KEY), keys.len() as u32) as _,
                keys.as_mut_ptr(),
            )
        } >= 0;
        let has = |code: usize| keys[code / 8] & (1 << (code % 8)) != 0;
        if !ok || !has(BTN_SOUTH) || has(KEY_A) {
            unsafe { libc::close(fd) };
            return None;
        }
        let mut device = Self {
            fd,
            path: path.to_owned(),
            ranges: [None; 0x12],
            buttons: 0,
            axes: [0; 0x12],
        };
        for axis in AXES {
            let mut info = AbsInfo::default();
            let size = std::mem::size_of::<AbsInfo>() as u32;
            if unsafe { libc::ioctl(fd, ioc_read(0x40 + axis as u32, size) as _, &mut info) } >= 0
                && info.maximum > info.minimum
            {
                device.ranges[axis] = Some((info.minimum, info.maximum));
                device.axes[axis] = info.value;
            }
        }
        Some(device)
    }

    /// Drains pending events. Returns false once the device is gone.
    fn read(&mut self) -> bool {
        let mut events = [unsafe { std::mem::zeroed::<libc::input_event>() }; 32];
        loop {
            let size = std::mem::size_of_val(&events);
            let n = unsafe { libc::read(self.fd, events.as_mut_ptr().cast(), size) };
            if n < 0 {
                let error = std::io::Error::last_os_error().raw_os_error();
                return error == Some(libc::EAGAIN) || error == Some(libc::EINTR);
            }
            if n == 0 {
                return false;
            }
            let count = n as usize / std::mem::size_of::<libc::input_event>();
            for event in &events[..count] {
                match event.type_ {
                    EV_KEY => {
                        let mask = button(event.code);
                        if event.value != 0 {
                            self.buttons |= mask;
                        } else {
                            self.buttons &= !mask;
                        }
                    }
                    EV_ABS if usize::from(event.code) < self.axes.len() => {
                        self.axes[usize::from(event.code)] = event.value;
                    }
                    _ => {}
                }
            }
            if count < events.len() {
                return true;
            }
        }
    }

    /// -32768..=32767 for a stick axis, centred on its range.
    fn stick(&self, axis: usize) -> i16 {
        let Some((min, max)) = self.ranges[axis] else {
            return 0;
        };
        let v = i64::from(self.axes[axis].clamp(min, max) - min);
        ((v * 65535 / i64::from(max - min)) - 32768).clamp(-32768, 32767) as i16
    }

    /// 0..=255 for a trigger axis.
    fn trigger(&self, axis: usize) -> u8 {
        let Some((min, max)) = self.ranges[axis] else {
            return 0;
        };
        let v = i64::from(self.axes[axis].clamp(min, max) - min);
        (v * 255 / i64::from(max - min)) as u8
    }

    fn hat(&self, axis: usize) -> i32 {
        match self.ranges[axis] {
            Some(_) => self.axes[axis].signum(),
            None => 0,
        }
    }

    fn state(&self) -> Pad {
        let mut buttons = self.buttons;
        match self.hat(ABS_HAT0X) {
            -1 => buttons |= pad::LEFT,
            1 => buttons |= pad::RIGHT,
            _ => {}
        }
        match self.hat(ABS_HAT0Y) {
            -1 => buttons |= pad::UP,
            1 => buttons |= pad::DOWN,
            _ => {}
        }
        let digital_lt = buttons & pad::LT != 0;
        let digital_rt = buttons & pad::RT != 0;
        // Generic HID layout (e.g. Xbox controllers over Bluetooth with
        // hid-microsoft): right stick on Z/RZ, triggers on BRAKE/GAS.
        let hid = self.ranges[ABS_RX].is_none()
            && (self.ranges[ABS_GAS].is_some() || self.ranges[ABS_BRAKE].is_some());
        let (right_x, right_y, lt, rt) = if hid {
            (
                ABS_Z,
                ABS_RZ,
                self.trigger(ABS_BRAKE),
                self.trigger(ABS_GAS),
            )
        } else {
            (
                ABS_RX,
                ABS_RY,
                self.trigger(ABS_Z).max(self.trigger(ABS_BRAKE)),
                self.trigger(ABS_RZ).max(self.trigger(ABS_GAS)),
            )
        };
        let mut state = Pad {
            buttons,
            lx: self.stick(ABS_X),
            // evdev Y grows downwards; XInput's grows upwards.
            ly: invert(self.stick(ABS_Y)),
            rx: self.stick(right_x),
            ry: invert(self.stick(right_y)),
            lt: if digital_lt { 255 } else { lt },
            rt: if digital_rt { 255 } else { rt },
        };
        state.update_trigger_bits();
        state
    }
}

fn invert(v: i16) -> i16 {
    (-(i32::from(v))).clamp(-32768, 32767) as i16
}

impl Drop for Device {
    fn drop(&mut self) {
        unsafe { libc::close(self.fd) };
    }
}

pub struct Reader {
    devices: Vec<Device>,
    last_scan: Option<Instant>,
    announced: usize,
}

impl Reader {
    pub const fn new() -> Self {
        Self {
            devices: Vec::new(),
            last_scan: None,
            announced: 0,
        }
    }

    fn candidates() -> Vec<String> {
        if let Ok(list) = std::env::var("ARCADE_PAD_DEVICES") {
            return list
                .split(':')
                .filter(|p| !p.is_empty())
                .map(str::to_owned)
                .collect();
        }
        let mut paths: Vec<String> = std::fs::read_dir("/dev/input")
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| e.file_name().into_string().ok())
            .filter(|n| n.starts_with("event"))
            .map(|n| format!("/dev/input/{n}"))
            .collect();
        paths.sort();
        paths
    }

    fn scan(&mut self) {
        for path in Self::candidates() {
            if self.devices.iter().any(|d| d.path == path) {
                continue;
            }
            if let Some(device) = Device::open(&path) {
                self.devices.push(device);
            }
        }
        if self.devices.len() != self.announced {
            self.announced = self.devices.len();
            eprintln!("[arcade] {} controller(s) connected", self.devices.len());
        }
    }

    /// Controllers per player ([`pad::player_for`]), rescanning for new ones
    /// every 2 s. One player merges every controller; with two, the first
    /// connected controller drives player one and the rest player two.
    pub fn read_players(&mut self, players: usize) -> [Pad; pad::MAX_PLAYERS] {
        if self
            .last_scan
            .is_none_or(|t| t.elapsed() >= Duration::from_secs(2))
        {
            self.last_scan = Some(Instant::now());
            self.scan();
        }
        self.devices.retain_mut(Device::read);
        if self.devices.len() != self.announced {
            self.announced = self.devices.len();
            eprintln!("[arcade] {} controller(s) connected", self.devices.len());
        }
        let mut out = [Pad::default(); pad::MAX_PLAYERS];
        for (index, device) in self.devices.iter().enumerate() {
            let player = pad::player_for(index, players);
            out[player] = out[player].merge(device.state());
        }
        out
    }
}

/// `ARCADE_PLAYERS` (settings.cfg `players`): 1 or 2.
pub fn players() -> usize {
    pad::players(std::env::var("ARCADE_PLAYERS").ok().as_deref().map(str::as_bytes))
}

/// Controller 2's bindings (`ARCADE_PAD_P2`), or `defaults` when unset.
pub fn bindings_p2(defaults: &str) -> Vec<(u32, u32)> {
    parse(&std::env::var("ARCADE_PAD_P2").unwrap_or_else(|_| defaults.to_owned()))
}

fn parse(text: &str) -> Vec<(u32, u32)> {
    let mut out = vec![(0, 0); 128];
    let n = pad::parse_bindings(text.as_bytes(), &mut out);
    out.truncate(n);
    out
}

/// `ARCADE_PAD` bindings (`target:mask,...`), or `defaults` when unset.
pub fn bindings(defaults: &str) -> Vec<(u32, u32)> {
    parse(&std::env::var("ARCADE_PAD").unwrap_or_else(|_| defaults.to_owned()))
}

/// Reports press/release edges of pad-bound targets, so controller input
/// never overrides a key that is held at the same time.
#[derive(Default)]
pub struct Edges {
    held: Vec<(u32, bool)>,
}

impl Edges {
    /// Each player's controller with its own bindings. A target shared by
    /// both (menus, test) is held while either player holds it.
    pub fn update_players(
        &mut self,
        players: &[(Pad, &[(u32, u32)])],
        mut changed: impl FnMut(u32, bool),
    ) {
        for (_, bindings) in players {
            for &(target, _) in *bindings {
                if !self.held.iter().any(|h| h.0 == target) {
                    self.held.push((target, false));
                }
            }
        }
        for (target, was) in &mut self.held {
            let now = players
                .iter()
                .any(|(state, bindings)| pad::target_held(state, bindings, *target));
            if now != *was {
                *was = now;
                changed(*target, now);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edges_report_changes_once() {
        let bindings = [(5, pad::START), (5, pad::A), (7, pad::BACK)];
        let mut edges = Edges::default();
        let mut log = Vec::new();
        let pressed = Pad {
            buttons: pad::A,
            ..Pad::default()
        };
        edges.update_players(&[(pressed, &bindings)], |t, d| log.push((t, d)));
        edges.update_players(&[(pressed, &bindings)], |t, d| log.push((t, d)));
        edges.update_players(&[(Pad::default(), &bindings)], |t, d| log.push((t, d)));
        assert_eq!(log, [(5, true), (5, false)]);
    }

    #[test]
    fn each_player_uses_its_own_bindings() {
        let p1 = [(5, pad::START), (11, pad::BACK)];
        let p2 = [(6, pad::START), (11, pad::BACK)];
        let start = Pad {
            buttons: pad::START,
            ..Pad::default()
        };
        let back = Pad {
            buttons: pad::BACK,
            ..Pad::default()
        };
        let mut edges = Edges::default();
        let mut log = Vec::new();
        edges.update_players(&[(Pad::default(), &p1), (start, &p2)], |t, d| log.push((t, d)));
        assert_eq!(log, [(6, true)]);
        log.clear();
        // A shared target stays held while either player holds it.
        edges.update_players(&[(back, &p1), (back, &p2)], |t, d| log.push((t, d)));
        edges.update_players(&[(back, &p1), (Pad::default(), &p2)], |t, d| log.push((t, d)));
        assert_eq!(log, [(11, true), (6, false)]);
    }

    #[test]
    fn ioctl_numbers_match_the_kernel() {
        // EVIOCGBIT(EV_KEY, 96) and EVIOCGABS(ABS_X) as in <linux/input.h>.
        assert_eq!(ioc_read(0x21, 96), 0x8060_4521);
        assert_eq!(ioc_read(0x40, 24), 0x8018_4540);
    }

    /// Reads attached controllers for a moment: `cargo test -p
    /// banana-linux-shim live_controllers -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn live_controllers() {
        let mut reader = Reader::new();
        for _ in 0..30 {
            let [state, _] = reader.read_players(1);
            eprintln!("{state:?} aim={:?}", state.aim());
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    #[test]
    fn missing_devices_are_ignored() {
        assert!(Device::open("/nonexistent/event0").is_none());
    }
}
