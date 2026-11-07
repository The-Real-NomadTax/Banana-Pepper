//! Press-to-bind for controllers, joysticks, mice and light guns: after a
//! binding is clicked, the next button pressed or axis moved on a device of
//! the player's input method becomes the binding, like keyboard keys.
//!
//! Devices are read live with the same reader as the games: evdev on Linux
//! (`banana_input::evdev`, every device of the method's kind). On Windows the
//! app reads XInput controllers; DirectInput and Raw Input buttons are picked
//! from the list there.
use banana_input::controls::{self, PAD_BUTTONS};
use banana_input::pad::{Axis, Method, Pad};
use std::time::{Duration, Instant};

/// How long a capture waits for input.
pub const TIMEOUT: Duration = Duration::from_secs(15);
/// An axis must move this far from where it rested (of 32767).
const AXIS_THRESHOLD: i32 = 20000;
/// A pointer must move this far (of `pad::POINTER_MAX`).
const POINTER_THRESHOLD: i32 = 6000;

/// What is being bound.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Want {
    /// A button for an action.
    Button(&'static str),
    /// An axis for movement X (0) or Y (1); `append` adds a second axis.
    Axis { which: usize, append: bool },
}

/// A capture in progress.
pub struct Capture {
    pub game: &'static str,
    pub player: usize,
    pub method: Method,
    pub want: Want,
    baseline: Option<Pad>,
    started: Instant,
}

/// The result of a finished capture.
#[derive(Debug, PartialEq, Eq)]
pub enum Captured {
    /// A controller button (`PadA`, ...).
    Button(&'static str),
    /// An axis name (`AxisLX`, `PointerY-`, ...).
    Axis(String),
}

impl Capture {
    pub fn new(game: &'static str, player: usize, method: Method, want: Want) -> Self {
        Self {
            game,
            player,
            method,
            want,
            baseline: None,
            started: Instant::now(),
        }
    }

    pub fn timed_out(&self) -> bool {
        self.started.elapsed() > TIMEOUT
    }

    /// What to tell the player while waiting.
    pub fn prompt(&self, labels: [&str; 2]) -> String {
        let device = match self.method {
            Method::XInput => "the XInput controller",
            Method::DirectInput => "the DirectInput controller",
            Method::RawInput => "the mouse or light gun",
            Method::KeyboardMouse => "the device",
        };
        match self.want {
            Want::Button(_) => format!("Press a button on {device}… (Esc cancels)"),
            Want::Axis { which, .. } => {
                let direction = if which == 0 { "right" } else { "down" };
                format!(
                    "Move {device}'s stick, axis or pointer {direction} for {}… (Esc cancels)",
                    labels[which.min(1)]
                )
            }
        }
    }

    /// Feeds the current device state; returns the binding once found.
    pub fn update(&mut self, now: Pad) -> Option<Captured> {
        let Some(mut baseline) = self.baseline else {
            // Whatever is held or deflected when the capture starts is the
            // resting state, so only a new press or movement counts.
            self.baseline = Some(now);
            return None;
        };
        if baseline.pointer.is_none() && now.pointer.is_some() {
            // A relative mouse reports its first point only once it moves.
            baseline.pointer = now.pointer;
        }
        let found = match self.want {
            Want::Button(_) => pressed_button(&baseline, &now, self.method).map(Captured::Button),
            Want::Axis { .. } => moved_axis(&baseline, &now, self.method).map(Captured::Axis),
        };
        // Buttons released since the start may be pressed again.
        baseline.buttons &= now.buttons;
        self.baseline = Some(baseline);
        found
    }
}

/// A button newly pressed since `before`, if the method's devices have it.
pub fn pressed_button(before: &Pad, now: &Pad, method: Method) -> Option<&'static str> {
    let new = now.buttons & !before.buttons;
    PAD_BUTTONS
        .iter()
        .find(|(name, mask, _)| new & mask != 0 && controls::pad_label(method, name).is_some())
        .map(|(name, _, _)| *name)
}

/// The axis moved furthest from `before` (inverted when moved negative), if
/// it moved far enough. Raw Input devices bind pointer axes.
pub fn moved_axis(before: &Pad, now: &Pad, method: Method) -> Option<String> {
    let mut best: Option<(Axis, i32)> = None;
    if method == Method::RawInput {
        let (Some((bx, by)), Some((nx, ny))) = (before.pointer, now.pointer) else {
            return None;
        };
        for (axis, delta) in [
            (Axis::PointerX, i32::from(nx) - i32::from(bx)),
            (Axis::PointerY, i32::from(ny) - i32::from(by)),
        ] {
            if delta.abs() >= POINTER_THRESHOLD && best.is_none_or(|(_, d)| delta.abs() > d.abs()) {
                best = Some((axis, delta));
            }
        }
    } else {
        for axis in Axis::ALL.into_iter().filter(|a| !a.is_pointer()) {
            if controls::axis_label(method, axis.name()).is_none() {
                continue;
            }
            let delta = now.axis_raw(axis) - before.axis_raw(axis);
            if delta.abs() >= AXIS_THRESHOLD && best.is_none_or(|(_, d)| delta.abs() > d.abs()) {
                best = Some((axis, delta));
            }
        }
    }
    best.map(|(axis, delta)| format!("{}{}", axis.name(), if delta < 0 { "-" } else { "" }))
}

/// Live devices for press-to-bind.
#[derive(Default)]
pub struct Devices {
    #[cfg(target_os = "linux")]
    reader: Option<banana_input::evdev::Reader>,
}

impl Devices {
    /// Every device of `method`'s kind, merged; `None` where this system
    /// cannot read that kind here.
    pub fn read(&mut self, method: Method) -> Option<Pad> {
        method.device()?;
        #[cfg(target_os = "linux")]
        {
            use banana_input::pad;
            let inputs = pad::Inputs {
                players: 1,
                methods: [method; pad::MAX_PLAYERS],
            };
            let reader = self.reader.get_or_insert_with(banana_input::evdev::Reader::new);
            Some(reader.read_players(&inputs)[0])
        }
        #[cfg(windows)]
        {
            (method == Method::XInput).then(windows::xinput)
        }
        #[cfg(not(any(target_os = "linux", windows)))]
        {
            None
        }
    }
}

#[cfg(windows)]
mod windows {
    use super::Pad;

    #[repr(C)]
    #[derive(Default)]
    struct State {
        packet: u32,
        buttons: u16,
        lt: u8,
        rt: u8,
        lx: i16,
        ly: i16,
        rx: i16,
        ry: i16,
    }

    #[link(name = "xinput1_4", kind = "raw-dylib")]
    unsafe extern "system" {
        fn XInputGetState(index: u32, state: *mut State) -> u32;
    }

    /// Every connected XInput controller, merged.
    pub fn xinput() -> Pad {
        let mut pad = Pad::default();
        for index in 0..4 {
            let mut state = State::default();
            if unsafe { XInputGetState(index, &mut state) } == 0 {
                pad = pad.merge(Pad::from_xinput(
                    state.buttons,
                    state.lt,
                    state.rt,
                    state.lx,
                    state.ly,
                    state.rx,
                    state.ry,
                ));
            }
        }
        pad
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use banana_input::pad;

    #[test]
    fn buttons_are_captured_on_a_new_press() {
        let rest = Pad::default();
        let a = Pad { buttons: pad::A, ..Pad::default() };
        let mut capture = Capture::new("jurassic", 0, Method::XInput, Want::Button("fire"));
        // A held button at the start is not taken.
        assert_eq!(capture.update(a), None);
        assert_eq!(capture.update(a), None);
        assert_eq!(capture.update(rest), None);
        assert_eq!(capture.update(a), Some(Captured::Button("PadA")));
        // Methods without the button never take it.
        let y = Pad { buttons: pad::Y, ..Pad::default() };
        assert_eq!(pressed_button(&rest, &y, Method::RawInput), None);
        let left = Pad { buttons: pad::RT, rt: 255, ..Pad::default() };
        assert_eq!(pressed_button(&rest, &left, Method::RawInput), Some("PadRT"));
    }

    #[test]
    fn axes_are_captured_with_their_direction() {
        let rest = Pad::default();
        let right = Pad { rx: 30000, ..Pad::default() };
        assert_eq!(moved_axis(&rest, &right, Method::XInput).as_deref(), Some("AxisRX"));
        // Stick pushed up while binding "down": inverted.
        let up = Pad { ly: 30000, ..Pad::default() };
        assert_eq!(moved_axis(&rest, &up, Method::DirectInput).as_deref(), Some("AxisLY-"));
        // Small wobbles are ignored; a pressed pedal is positive.
        assert_eq!(moved_axis(&rest, &Pad { lx: 9000, ..Pad::default() }, Method::XInput), None);
        let pedal = Pad { lt: 255, ..Pad::default() };
        assert_eq!(moved_axis(&rest, &pedal, Method::DirectInput).as_deref(), Some("AxisLT"));
        // A light gun moved up and a little right: pointer Y, inverted.
        let before = Pad { pointer: Some((30000, 30000)), ..Pad::default() };
        let after = Pad { pointer: Some((32000, 10000)), ..Pad::default() };
        assert_eq!(moved_axis(&before, &after, Method::RawInput).as_deref(), Some("PointerY-"));
        // Sticks do not count for Raw Input, pointers not for controllers.
        assert_eq!(moved_axis(&rest, &right, Method::RawInput), None);
        assert_eq!(moved_axis(&before, &after, Method::XInput), None);
    }

    #[test]
    fn a_relative_mouse_starts_from_its_first_point() {
        let mut capture = Capture::new("big_buck", 1, Method::RawInput, Want::Axis { which: 0, append: false });
        assert_eq!(capture.update(Pad::default()), None);
        let first = Pad { pointer: Some((32768, 32768)), ..Pad::default() };
        assert_eq!(capture.update(first), None);
        let moved = Pad { pointer: Some((45000, 33000)), ..Pad::default() };
        assert_eq!(capture.update(moved), Some(Captured::Axis("PointerX".into())));
    }
}
