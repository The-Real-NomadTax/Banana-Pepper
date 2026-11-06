//! Controller state shared by every game adapter.
//!
//! Backends read controllers into [`Pad`]: XInput on Windows and Wine (the
//! freestanding hook DLLs and Water), evdev on Linux (the guest shim). Button
//! bits are XInput's `wButtons` (Xbox layout) plus [`LT`]/[`RT`] for the
//! triggers, matching `controls::PAD_BUTTONS`. Bindings arrive as
//! `target:mask,...` (`ARCADE_PAD`, from controls.cfg).
//!
//! This file has no dependencies and no `std` use so the `no_std` hook DLLs
//! can include it with `#[path]`.

pub const UP: u32 = 0x0001;
pub const DOWN: u32 = 0x0002;
pub const LEFT: u32 = 0x0004;
pub const RIGHT: u32 = 0x0008;
pub const START: u32 = 0x0010;
pub const BACK: u32 = 0x0020;
pub const LS: u32 = 0x0040;
pub const RS: u32 = 0x0080;
pub const LB: u32 = 0x0100;
pub const RB: u32 = 0x0200;
pub const A: u32 = 0x1000;
pub const B: u32 = 0x2000;
pub const X: u32 = 0x4000;
pub const Y: u32 = 0x8000;
pub const LT: u32 = 0x1_0000;
pub const RT: u32 = 0x2_0000;

/// A trigger counts as a pressed button past a quarter of its travel.
pub const TRIGGER_PRESSED: u8 = 64;
/// Stick dead zone (XInput's recommended left-stick value, rounded).
pub const DEADZONE: i32 = 8000;
/// Full deflection after [`Pad::axis`].
pub const AXIS_MAX: i32 = 4096;

/// One frame of controller state. Sticks are XInput-style: -32768..=32767,
/// Y positive up. Triggers are 0..=255.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Pad {
    pub buttons: u32,
    pub lx: i16,
    pub ly: i16,
    pub rx: i16,
    pub ry: i16,
    pub lt: u8,
    pub rt: u8,
}

impl Pad {
    /// From an `XINPUT_GAMEPAD`.
    pub fn from_xinput(buttons: u16, lt: u8, rt: u8, lx: i16, ly: i16, rx: i16, ry: i16) -> Self {
        let mut pad = Self {
            buttons: u32::from(buttons) & 0xf3ff,
            lx,
            ly,
            rx,
            ry,
            lt,
            rt,
        };
        pad.update_trigger_bits();
        pad
    }

    pub fn update_trigger_bits(&mut self) {
        self.buttons &= !(LT | RT);
        if self.lt >= TRIGGER_PRESSED {
            self.buttons |= LT;
        }
        if self.rt >= TRIGGER_PRESSED {
            self.buttons |= RT;
        }
    }

    /// Several controllers act as one: buttons combine, the larger stick
    /// deflection and trigger travel win.
    pub fn merge(self, other: Self) -> Self {
        fn wider(a: i16, b: i16) -> i16 {
            if i32::from(b).abs() > i32::from(a).abs() {
                b
            } else {
                a
            }
        }
        Self {
            buttons: self.buttons | other.buttons,
            lx: wider(self.lx, other.lx),
            ly: wider(self.ly, other.ly),
            rx: wider(self.rx, other.rx),
            ry: wider(self.ry, other.ry),
            lt: self.lt.max(other.lt),
            rt: self.rt.max(other.rt),
        }
    }

    pub fn held(&self, mask: u32) -> bool {
        self.buttons & mask != 0
    }

    /// Analog level 0..=255 for a bound button: triggers report their travel,
    /// every other button is off or full.
    pub fn level(&self, mask: u32) -> u8 {
        let mut level = if self.buttons & mask & !(LT | RT) != 0 {
            255
        } else {
            0
        };
        if mask & LT != 0 {
            level = level.max(self.lt);
        }
        if mask & RT != 0 {
            level = level.max(self.rt);
        }
        level
    }

    /// A stick axis with the dead zone removed, scaled to -4096..=4096.
    pub fn axis(value: i16) -> i32 {
        let v = i32::from(value).clamp(-32767, 32767);
        let magnitude = v.abs();
        if magnitude <= DEADZONE {
            return 0;
        }
        let scaled = (magnitude - DEADZONE) * AXIS_MAX / (32767 - DEADZONE);
        if v < 0 { -scaled } else { scaled }
    }

    /// The more deflected stick (either stick aims in the gun games), as
    /// (x, y) in -4096..=4096 with Y positive *down* (screen space).
    pub fn aim(&self) -> (i32, i32) {
        let left = (Self::axis(self.lx), -Self::axis(self.ly));
        let right = (Self::axis(self.rx), -Self::axis(self.ry));
        let size = |(x, y): (i32, i32)| x.abs().max(y.abs());
        if size(right) > size(left) {
            right
        } else {
            left
        }
    }
}

/// Most players any game adapter supports (the gun games are two-player).
pub const MAX_PLAYERS: usize = 2;

/// Who plays with which device, from `ARCADE_PLAYERS` and `ARCADE_INPUTS`
/// (settings.cfg `players`, `p1_input`, `p2_input`).
///
/// * One player: every controller, the mouse and the keyboard play player one.
/// * Two players on controllers (`pad,pad`): the first connected controller
///   plays player one, the rest player two; the mouse stays player one.
/// * `pad,kbm`: every controller plays player one; the mouse (and keyboard
///   aim/fire keys) play player two.
/// * `kbm,pad`: the mouse plays player one; every controller plays player two.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Inputs {
    pub players: usize,
    /// The player the mouse aims for (0 = player one).
    pub mouse: usize,
    /// Players driven by controllers, in connection order.
    pads: [usize; MAX_PLAYERS],
    pad_count: usize,
}

impl Inputs {
    pub const ONE_PLAYER: Self = Self {
        players: 1,
        mouse: 0,
        pads: [0; MAX_PLAYERS],
        pad_count: 1,
    };

    pub fn parse(players: Option<&[u8]>, inputs: Option<&[u8]>) -> Self {
        if players != Some(b"2") {
            return Self::ONE_PLAYER;
        }
        let (mouse, pads, pad_count) = match inputs {
            Some(b"pad,kbm") => (1, [0, 0], 1),
            Some(b"kbm,pad") => (0, [1, 1], 1),
            _ => (0, [0, 1], 2),
        };
        Self {
            players: 2,
            mouse,
            pads,
            pad_count,
        }
    }

    /// The player the `index`th connected controller drives; any controllers
    /// beyond the last listed player join that player.
    pub fn pad_player(&self, index: usize) -> usize {
        self.pads[index.min(self.pad_count - 1)]
    }

    /// True if any controller drives `player`.
    pub fn pad_drives(&self, player: usize) -> bool {
        self.pads[..self.pad_count].contains(&player)
    }

    /// True if `player` aims with a controller only (no mouse), so its aim
    /// must reach the game some other way than the mouse pointer.
    pub fn pad_aims(&self, player: usize) -> bool {
        player < self.players && player != self.mouse && self.pad_drives(player)
    }
}

/// Moves a pointer with stick input. Speed grows with deflection (squared
/// response, so small movements are precise).
#[derive(Clone, Copy, Debug)]
pub struct Cursor {
    pub x: i32,
    pub y: i32,
    /// Sub-pixel remainder, in 1/4096 pixels.
    fx: i32,
    fy: i32,
}

impl Cursor {
    pub const fn new(x: i32, y: i32) -> Self {
        Self { x, y, fx: 0, fy: 0 }
    }

    /// Advance by `elapsed_ms` with `aim` from [`Pad::aim`]; `speed` is the
    /// full-deflection speed in pixels per second. Returns true if it moved.
    pub fn step(
        &mut self,
        aim: (i32, i32),
        elapsed_ms: u32,
        speed: i32,
        width: i32,
        height: i32,
    ) -> bool {
        if aim == (0, 0) {
            return false;
        }
        let elapsed = elapsed_ms.min(100) as i64;
        let curve = |v: i32| -> i64 {
            let v = i64::from(v);
            v * v.abs() / i64::from(AXIS_MAX)
        };
        // pixels/s * ms / 1000 * deflection/4096, kept in 1/4096 pixels.
        let dx = curve(aim.0) * i64::from(speed) * elapsed / 1000 + i64::from(self.fx);
        let dy = curve(aim.1) * i64::from(speed) * elapsed / 1000 + i64::from(self.fy);
        let (px, py) = (dx / i64::from(AXIS_MAX), dy / i64::from(AXIS_MAX));
        self.fx = (dx % i64::from(AXIS_MAX)) as i32;
        self.fy = (dy % i64::from(AXIS_MAX)) as i32;
        let (old_x, old_y) = (self.x, self.y);
        self.x = (self.x + px as i32).clamp(0, (width - 1).max(0));
        self.y = (self.y + py as i32).clamp(0, (height - 1).max(0));
        (self.x, self.y) != (old_x, old_y)
    }
}

/// Parses `target:mask,target:mask` (decimal) into `out`; returns the count.
/// Malformed pairs are skipped.
pub fn parse_bindings(text: &[u8], out: &mut [(u32, u32)]) -> usize {
    let mut count = 0;
    let (mut target, mut value, mut have_target, mut digits) = (0u32, 0u32, false, false);
    for &c in text.iter().chain(core::iter::once(&b',')) {
        match c {
            b'0'..=b'9' => {
                value = value.saturating_mul(10).saturating_add(u32::from(c - b'0'));
                digits = true;
            }
            b':' => {
                target = value;
                value = 0;
                have_target = digits;
                digits = false;
            }
            b',' => {
                if have_target && digits && value != 0 && count < out.len() {
                    out[count] = (target, value);
                    count += 1;
                }
                (value, have_target, digits) = (0, false, false);
            }
            b' ' | b'\t' | b'\r' | b'\n' => {}
            _ => {
                // Poison this pair.
                have_target = false;
                digits = false;
            }
        }
    }
    count
}

/// True if any binding for `target` is held.
pub fn target_held(pad: &Pad, bindings: &[(u32, u32)], target: u32) -> bool {
    bindings.iter().any(|&(t, m)| t == target && pad.held(m))
}

/// Highest analog level (0..=255) over the bindings for `target`.
pub fn target_level(pad: &Pad, bindings: &[(u32, u32)], target: u32) -> u8 {
    bindings
        .iter()
        .filter(|&&(t, _)| t == target)
        .map(|&(_, m)| pad.level(m))
        .max()
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xinput_state_triggers_and_levels() {
        let pad = Pad::from_xinput(0x1010 | 0x0400, 200, 10, 0, 0, 0, 0);
        assert!(pad.held(A) && pad.held(START) && pad.held(LT) && !pad.held(RT));
        // Guide-area bit 0x0400 is reserved and dropped.
        assert_eq!(pad.buttons & 0x0400, 0);
        assert_eq!(pad.level(LT), 200);
        assert_eq!(pad.level(RT), 10);
        assert_eq!(pad.level(A), 255);
        assert_eq!(pad.level(B), 0);
    }

    #[test]
    fn axes_have_a_dead_zone_and_full_range() {
        assert_eq!(Pad::axis(5000), 0);
        assert_eq!(Pad::axis(-8000), 0);
        assert_eq!(Pad::axis(32767), AXIS_MAX);
        assert_eq!(Pad::axis(-32768), -AXIS_MAX);
        assert!(Pad::axis(20000) > 0 && Pad::axis(20000) < AXIS_MAX);
        let pad = Pad {
            lx: 32767,
            ry: 32767,
            ..Pad::default()
        };
        // Equal deflection: left stick wins; screen Y is down.
        assert_eq!(pad.aim(), (AXIS_MAX, 0));
        let pad = Pad {
            lx: 9000,
            ry: 32767,
            ..Pad::default()
        };
        assert_eq!(pad.aim().1, -AXIS_MAX);
    }

    #[test]
    fn merging_and_bindings() {
        let a = Pad {
            buttons: A,
            lx: 100,
            lt: 50,
            ..Pad::default()
        };
        let b = Pad {
            buttons: B,
            lx: -2000,
            lt: 10,
            ..Pad::default()
        };
        let m = a.merge(b);
        assert_eq!((m.buttons, m.lx, m.lt), (A | B, -2000, 50));
        let mut out = [(0, 0); 4];
        let n = parse_bindings(b"3:131072, 65536:16,bad:1,4:,:7,5:4096,6:1", &mut out);
        assert_eq!(&out[..n], &[(3, RT), (65536, START), (5, A), (6, 1)]);
        let pad = Pad {
            buttons: RT,
            rt: 128,
            ..Pad::default()
        };
        assert_eq!(target_level(&pad, &out[..n], 3), 128);
        assert!(target_held(&pad, &out[..n], 3));
        assert!(!target_held(&pad, &out[..n], 5));
    }

    #[test]
    fn controllers_and_mouse_are_assigned_to_players() {
        let one = Inputs::parse(None, Some(b"pad,kbm"));
        assert_eq!(one, Inputs::ONE_PLAYER);
        assert_eq!((one.pad_player(0), one.pad_player(3), one.mouse), (0, 0, 0));
        assert_eq!(Inputs::parse(Some(b"4"), None), Inputs::ONE_PLAYER);
        let pads = Inputs::parse(Some(b"2"), None);
        assert_eq!((pads.pad_player(0), pads.pad_player(1), pads.pad_player(2)), (0, 1, 1));
        assert_eq!(pads.mouse, 0);
        assert!(!pads.pad_aims(0) && pads.pad_aims(1));
        let p2_mouse = Inputs::parse(Some(b"2"), Some(b"pad,kbm"));
        assert_eq!((p2_mouse.pad_player(0), p2_mouse.pad_player(1), p2_mouse.mouse), (0, 0, 1));
        assert!(p2_mouse.pad_aims(0) && !p2_mouse.pad_aims(1) && !p2_mouse.pad_drives(1));
        let p1_mouse = Inputs::parse(Some(b"2"), Some(b"kbm,pad"));
        assert_eq!((p1_mouse.pad_player(0), p1_mouse.pad_player(1), p1_mouse.mouse), (1, 1, 0));
        assert!(!p1_mouse.pad_drives(0) && p1_mouse.pad_aims(1));
    }

    #[test]
    fn cursor_moves_with_deflection_and_stays_on_screen() {
        let mut c = Cursor::new(320, 240);
        assert!(!c.step((0, 0), 16, 800, 640, 480));
        // Full right for 100 ms at 800 px/s = 80 px.
        assert!(c.step((AXIS_MAX, 0), 100, 800, 640, 480));
        assert_eq!((c.x, c.y), (400, 240));
        // Half deflection moves a quarter as fast (squared response).
        c.step((0, AXIS_MAX / 2), 100, 800, 640, 480);
        assert_eq!((c.x, c.y), (400, 260));
        for _ in 0..50 {
            c.step((-AXIS_MAX, -AXIS_MAX), 100, 800, 640, 480);
        }
        assert_eq!((c.x, c.y), (0, 0));
    }
}
