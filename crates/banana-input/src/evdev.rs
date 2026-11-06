//! Linux input devices through evdev, read into the XInput-style
//! [`Pad`], by kind ([`pad::Device`]):
//!
//! * XInput-style gamepads: the kernel's Xbox (xpad) layout: BTN_A/B/X/Y,
//!   bumpers, Back/Start, stick clicks; ABS_X/Y and ABS_RX/RY sticks, ABS_Z/RZ
//!   (or ABS_BRAKE/GAS) triggers, a hat or BTN_DPAD_* for the d-pad.
//! * DirectInput-style controllers (generic gamepads, arcade sticks, wheels):
//!   numbered buttons from BTN_JOYSTICK in DirectInput order
//!   ([`pad::DINPUT_BUTTONS`]), X/Y, Z/RZ or RX/RY, hat, gas/brake.
//! * Mice and light guns: buttons ([`pad::MOUSE_BUTTONS`]) and an absolute
//!   aim point ([`Pad::pointer`]) from relative motion or absolute X/Y.
//!
//! The launchers bind only the device nodes the session's input methods need
//! into the sandbox (`ARCADE_PAD_DEVICES`; never keyboards); without that,
//! every `/dev/input/event*` of a wanted kind is used. Reads never block.
//!
//! Shared by the Linux guest shim (included with `#[path]`, next to its own
//! copy of `pad.rs`) and the desktop app's press-to-bind (`banana_input::evdev`).
use crate::pad::{self, Pad};
use std::ffi::CString;
use std::time::{Duration, Instant};

const EV_KEY: u16 = 1;
const EV_REL: u16 = 2;
const EV_ABS: u16 = 3;
const REL_X: u16 = 0;
const REL_Y: u16 = 1;

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
/// Relative mouse counts across the whole window (pointer sensitivity).
const MOUSE_COUNTS_PER_WINDOW: i64 = 1600;

/// evdev key code -> pad mask, for a device of `kind`.
fn button(kind: pad::Device, code: u16) -> u32 {
    match kind {
        pad::Device::XInput => match code {
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
        },
        pad::Device::DirectInput => code
            .checked_sub(0x120)
            .and_then(|i| pad::DINPUT_BUTTONS.get(usize::from(i)))
            .copied()
            .unwrap_or(0),
        pad::Device::Mouse => code
            .checked_sub(0x110)
            .and_then(|i| pad::MOUSE_BUTTONS.get(usize::from(i)))
            .copied()
            .unwrap_or(0),
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

/// One capability bitmap (EVIOCGBIT) of an open device.
fn bits<const N: usize>(fd: libc::c_int, kind: u16) -> Option<[u8; N]> {
    let mut bits = [0u8; N];
    let ok = unsafe {
        libc::ioctl(fd, ioc_read(0x20 + u32::from(kind), N as u32) as _, bits.as_mut_ptr())
    } >= 0;
    ok.then_some(bits)
}

fn has(bits: &[u8], code: usize) -> bool {
    bits.get(code / 8).is_some_and(|b| b & (1 << (code % 8)) != 0)
}

struct Device {
    fd: libc::c_int,
    path: String,
    kind: pad::Device,
    ranges: [Option<(i32, i32)>; 0x12],
    buttons: u32,
    axes: [i32; 0x12],
    /// Mice: the aim point in 0..=POINTER_MAX, once the device has moved.
    pointer: Option<(i64, i64)>,
}

impl Device {
    /// Opens `path` if it is a device of a kind in `wanted`.
    fn open(path: &str, wanted: &[pad::Device]) -> Option<Self> {
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
        let keys = bits::<{ 0x300 / 8 }>(fd, EV_KEY);
        let rel = bits::<1>(fd, EV_REL).unwrap_or_default();
        let abs = bits::<8>(fd, EV_ABS).unwrap_or_default();
        let kind = keys.and_then(|keys| {
            pad::classify(
                |code| has(&keys, usize::from(code)),
                has(&rel, usize::from(REL_X)) && has(&rel, usize::from(REL_Y)),
                has(&abs, ABS_X) && has(&abs, ABS_Y),
            )
        });
        let Some(kind) = kind.filter(|k| wanted.contains(k)) else {
            unsafe { libc::close(fd) };
            return None;
        };
        let mut device = Self {
            fd,
            path: path.to_owned(),
            kind,
            ranges: [None; 0x12],
            buttons: 0,
            axes: [0; 0x12],
            pointer: None,
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
                        let mask = button(self.kind, event.code);
                        if event.value != 0 {
                            self.buttons |= mask;
                        } else {
                            self.buttons &= !mask;
                        }
                    }
                    EV_REL if matches!(event.code, REL_X | REL_Y) => {
                        let full = i64::from(pad::POINTER_MAX);
                        let (x, y) = self.pointer.get_or_insert((full / 2, full / 2));
                        let axis = if event.code == REL_X { x } else { y };
                        *axis = (*axis + i64::from(event.value) * full / MOUSE_COUNTS_PER_WINDOW)
                            .clamp(0, full);
                    }
                    EV_ABS if usize::from(event.code) < self.axes.len() => {
                        self.axes[usize::from(event.code)] = event.value;
                        if self.kind == pad::Device::Mouse && usize::from(event.code) <= ABS_Y {
                            let point = (self.unit(ABS_X), self.unit(ABS_Y));
                            self.pointer = Some(point);
                        }
                    }
                    _ => {}
                }
            }
            if count < events.len() {
                return true;
            }
        }
    }

    /// An absolute axis as 0..=POINTER_MAX (light guns).
    fn unit(&self, axis: usize) -> i64 {
        let Some((min, max)) = self.ranges[axis] else {
            return i64::from(pad::POINTER_MAX) / 2;
        };
        i64::from(self.axes[axis].clamp(min, max) - min) * i64::from(pad::POINTER_MAX)
            / i64::from(max - min)
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
        if self.kind == pad::Device::Mouse {
            let mut state = Pad {
                buttons: self.buttons,
                pointer: self.pointer.map(|(x, y)| (x as u16, y as u16)),
                ..Pad::default()
            };
            state.lt = if state.held(pad::LT) { 255 } else { 0 };
            state.rt = if state.held(pad::RT) { 255 } else { 0 };
            return state;
        }
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
        // hid-microsoft, and most DirectInput-style pads): right stick on
        // Z/RZ, triggers on BRAKE/GAS.
        let hid = self.ranges[ABS_RX].is_none()
            && (self.kind == pad::Device::DirectInput
                || self.ranges[ABS_GAS].is_some()
                || self.ranges[ABS_BRAKE].is_some());
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
            pointer: None,
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

impl Default for Reader {
    fn default() -> Self {
        Self::new()
    }
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

    fn scan(&mut self, wanted: &[pad::Device]) {
        for path in Self::candidates() {
            if self.devices.iter().any(|d| d.path == path) {
                continue;
            }
            if let Some(device) = Device::open(&path, wanted) {
                self.devices.push(device);
            }
        }
        if self.devices.len() != self.announced {
            self.announced = self.devices.len();
            eprintln!("[arcade] {} controller(s) connected", self.devices.len());
        }
    }

    /// Each player's devices ([`pad::Inputs::device_player`]), rescanning
    /// for new ones every 2 s.
    pub fn read_players(&mut self, inputs: &pad::Inputs) -> [Pad; pad::MAX_PLAYERS] {
        if self
            .last_scan
            .is_none_or(|t| t.elapsed() >= Duration::from_secs(2))
        {
            self.last_scan = Some(Instant::now());
            let wanted: Vec<pad::Device> = [pad::Device::XInput, pad::Device::DirectInput, pad::Device::Mouse]
                .into_iter()
                .filter(|&kind| inputs.device_player(kind, 0).is_some())
                .collect();
            self.scan(&wanted);
        }
        self.devices.retain_mut(Device::read);
        if self.devices.len() != self.announced {
            self.announced = self.devices.len();
            eprintln!("[arcade] {} controller(s) connected", self.devices.len());
        }
        let mut out = [Pad::default(); pad::MAX_PLAYERS];
        let mut counts = [0usize; 3];
        for device in &self.devices {
            let slot = &mut counts[device.kind as usize];
            if let Some(player) = inputs.device_player(device.kind, *slot) {
                out[player] = out[player].merge(device.state());
            }
            *slot += 1;
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
            let [state, _] = reader.read_players(&pad::Inputs::ONE_PLAYER);
            eprintln!("{state:?} aim={:?}", state.aim());
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    #[test]
    fn missing_devices_are_ignored() {
        assert!(Device::open("/nonexistent/event0", &[pad::Device::XInput]).is_none());
    }

    #[test]
    fn buttons_follow_each_kind_of_device() {
        use pad::Device::*;
        assert_eq!(button(XInput, 0x130), pad::A);
        assert_eq!(button(DirectInput, 0x120), pad::A); // button 1
        assert_eq!(button(DirectInput, 0x127), pad::RT); // button 8
        assert_eq!(button(DirectInput, 0x129), pad::START); // button 10
        assert_eq!(button(DirectInput, 0x130), 0);
        assert_eq!(button(Mouse, 0x110), pad::RT); // left
        assert_eq!(button(Mouse, 0x111), pad::LT); // right
        assert_eq!(button(Mouse, 0x113), pad::START); // side
    }
}
