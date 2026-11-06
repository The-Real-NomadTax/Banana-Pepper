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
    /// An absolute aim point from a mouse or light gun read directly:
    /// (x, y) in 0..=[`POINTER_MAX`] across the game window.
    pub pointer: Option<(u16, u16)>,
}

/// Full scale of [`Pad::pointer`].
pub const POINTER_MAX: u16 = u16::MAX;

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
            pointer: None,
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
            pointer: self.pointer.or(other.pointer),
        }
    }

    /// The aim point in a `width` x `height` window, if this is a pointer.
    pub fn pointer_in(&self, width: i32, height: i32) -> Option<(i32, i32)> {
        let (x, y) = self.pointer?;
        let scale = |v: u16, size: i32| {
            (i64::from(v) * i64::from(size.max(1)) / (i64::from(POINTER_MAX) + 1)) as i32
        };
        Some((scale(x, width), scale(y, height)))
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

    /// An axis as -32768..=32767, right/down/pressed positive (see
    /// [`Axis`]); pointers read 0 here (see [`Pad::pointer_moved`]).
    pub fn axis_raw(&self, axis: Axis) -> i32 {
        match axis {
            Axis::LeftX => i32::from(self.lx),
            Axis::LeftY => -i32::from(self.ly),
            Axis::RightX => i32::from(self.rx),
            Axis::RightY => -i32::from(self.ry),
            Axis::LeftTrigger => i32::from(self.lt) * 32767 / 255,
            Axis::RightTrigger => i32::from(self.rt) * 32767 / 255,
            Axis::PointerX | Axis::PointerY => 0,
        }
        .clamp(-32767, 32767)
    }

    /// Movement as (x, y) in -4096..=4096 (dead zone removed; Y positive
    /// down) from the bound axes; the more deflected binding wins.
    pub fn movement(&self, movement: &Movement) -> (i32, i32) {
        let along = |binds: &[Option<AxisBind>; 2]| {
            binds
                .iter()
                .flatten()
                .map(|b| {
                    let v = Self::axis(self.axis_raw(b.axis) as i16);
                    if b.invert { -v } else { v }
                })
                .max_by_key(|v| v.abs())
                .unwrap_or(0)
        };
        (along(&movement.x), along(&movement.y))
    }

    /// A Raw Input pointer's aim point in a `width` x `height` window, with
    /// the pointer axes bound to X and Y (default PointerX / PointerY).
    pub fn pointer_moved(&self, movement: &Movement, width: i32, height: i32) -> Option<(i32, i32)> {
        let (px, py) = self.pointer?;
        let pick = |binds: &[Option<AxisBind>; 2], fallback: Axis, size: i32| {
            let bind = binds
                .iter()
                .flatten()
                .find(|b| b.axis.is_pointer())
                .copied()
                .unwrap_or(AxisBind { axis: fallback, invert: false });
            let value = if bind.axis == Axis::PointerX { px } else { py };
            let scaled = (i64::from(value) * i64::from(size.max(1)) / (i64::from(POINTER_MAX) + 1)) as i32;
            if bind.invert { size.max(1) - 1 - scaled } else { scaled }
        };
        Some((pick(&movement.x, Axis::PointerX, width), pick(&movement.y, Axis::PointerY, height)))
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

/// An analog input that can drive movement (aim or steering). Values are
/// "raw device" direction: right, down and pressed are positive, like
/// DirectInput and evdev (so a stick's Y needs no inverting to aim down).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    LeftX,
    LeftY,
    RightX,
    RightY,
    LeftTrigger,
    RightTrigger,
    /// A Raw Input mouse or light gun's aim point.
    PointerX,
    PointerY,
}

impl Axis {
    pub const ALL: [Self; 8] = [
        Self::LeftX,
        Self::LeftY,
        Self::RightX,
        Self::RightY,
        Self::LeftTrigger,
        Self::RightTrigger,
        Self::PointerX,
        Self::PointerY,
    ];

    /// The controls.cfg name (`AxisLX`, ...; `-` appended when inverted).
    pub const fn name(self) -> &'static str {
        match self {
            Self::LeftX => "AxisLX",
            Self::LeftY => "AxisLY",
            Self::RightX => "AxisRX",
            Self::RightY => "AxisRY",
            Self::LeftTrigger => "AxisLT",
            Self::RightTrigger => "AxisRT",
            Self::PointerX => "PointerX",
            Self::PointerY => "PointerY",
        }
    }

    pub const fn is_pointer(self) -> bool {
        matches!(self, Self::PointerX | Self::PointerY)
    }
}

/// One axis bound to a movement direction, possibly inverted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AxisBind {
    pub axis: Axis,
    pub invert: bool,
}

impl AxisBind {
    /// `AxisLX`, `axislx`, `AxisLY-` (inverted), ...
    pub fn parse(word: &[u8]) -> Option<Self> {
        let (word, invert) = match word.split_last() {
            Some((b'-', rest)) => (rest, true),
            _ => (word, false),
        };
        let axis = Axis::ALL
            .into_iter()
            .find(|a| a.name().as_bytes().eq_ignore_ascii_case(word))?;
        Some(Self { axis, invert })
    }
}

/// Which axes move a player (controls.cfg `move_x` / `move_y`): up to two
/// each; the more deflected one wins. Aim X/Y in the gun games, steering and
/// gas (up) / brake (down) in the racers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Movement {
    pub x: [Option<AxisBind>; 2],
    pub y: [Option<AxisBind>; 2],
}

const fn bind(axis: Axis) -> Option<AxisBind> {
    Some(AxisBind { axis, invert: false })
}

impl Movement {
    /// Either stick aims (the gun games' default).
    pub const STICKS: Self = Self {
        x: [bind(Axis::LeftX), bind(Axis::RightX)],
        y: [bind(Axis::LeftY), bind(Axis::RightY)],
    };
    /// The left stick steers (the racers' default); nothing on Y.
    pub const STEERING: Self = Self {
        x: [bind(Axis::LeftX), None],
        y: [None, None],
    };

    /// `x=AxisLX,AxisRX;y=AxisLY` (`ARCADE_MOVE`); `default` when absent,
    /// empty or malformed. A direction left empty (`y=`) is unbound.
    pub fn parse(text: Option<&[u8]>, default: Self) -> Self {
        let Some(text) = text.filter(|t| !t.is_empty()) else {
            return default;
        };
        let mut out = Self {
            x: [None; 2],
            y: [None; 2],
        };
        for part in text.split(|&b| b == b';') {
            let Some(eq) = part.iter().position(|&b| b == b'=') else {
                return default;
            };
            let slots = match &part[..eq] {
                b"x" => &mut out.x,
                b"y" => &mut out.y,
                _ => return default,
            };
            let words = part[eq + 1..].split(|&b| b == b',').filter(|w| !w.is_empty());
            for (slot, word) in slots.iter_mut().zip(words) {
                match AxisBind::parse(word) {
                    Some(bind) => *slot = Some(bind),
                    None => return default,
                }
            }
        }
        out
    }
}

/// Most players any game adapter supports (the gun games are two-player).
pub const MAX_PLAYERS: usize = 2;

/// What a player plays with (settings.cfg `p1_input` / `p2_input`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Method {
    /// The desktop's keyboard and mouse (the system pointer).
    KeyboardMouse,
    /// Xbox-style controllers (XInput on Windows/Wine, evdev gamepads with
    /// the Xbox layout on Linux).
    XInput,
    /// Other game controllers: generic gamepads, arcade sticks and wheels
    /// (DirectInput on Windows/Wine, evdev joysticks on Linux).
    DirectInput,
    /// A mouse or light gun of the player's own, read directly (Raw Input on
    /// Windows, evdev mice on Linux), so two of them can be told apart.
    RawInput,
}

impl Method {
    pub const ALL: [Self; 4] = [
        Self::KeyboardMouse,
        Self::XInput,
        Self::DirectInput,
        Self::RawInput,
    ];

    /// The settings.cfg / `ARCADE_INPUTS` word.
    pub const fn id(self) -> &'static str {
        match self {
            Self::KeyboardMouse => "keyboard_mouse",
            Self::XInput => "xinput",
            Self::DirectInput => "dinput",
            Self::RawInput => "rawinput",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::KeyboardMouse => "Keyboard & mouse",
            Self::XInput => "XInput",
            Self::DirectInput => "DirectInput",
            Self::RawInput => "Raw Input",
        }
    }

    /// From an id; also the older `controller` (= XInput), `pad` and `kbm`.
    pub fn parse(value: &[u8]) -> Option<Self> {
        Some(match value {
            b"keyboard_mouse" | b"kbm" => Self::KeyboardMouse,
            b"xinput" | b"controller" | b"pad" => Self::XInput,
            b"dinput" | b"directinput" => Self::DirectInput,
            b"rawinput" | b"raw" => Self::RawInput,
            _ => return None,
        })
    }

    /// The kind of device this method reads, if any.
    pub const fn device(self) -> Option<Device> {
        match self {
            Self::KeyboardMouse => None,
            Self::XInput => Some(Device::XInput),
            Self::DirectInput => Some(Device::DirectInput),
            Self::RawInput => Some(Device::Mouse),
        }
    }
}

/// Kinds of input device the backends read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Device {
    /// Xbox-style gamepad.
    XInput,
    /// Generic gamepad, arcade stick or wheel.
    DirectInput,
    /// A mouse or light gun (relative or absolute pointer).
    Mouse,
}

/// Classify an evdev device from its key capabilities (`key(code)`) and
/// whether it reports relative or absolute X/Y. Keyboards (KEY_A) and
/// touchpads (BTN_TOOL_FINGER) are never used.
pub fn classify(key: impl Fn(u16) -> bool, rel_xy: bool, abs_xy: bool) -> Option<Device> {
    const KEY_A: u16 = 30;
    const BTN_LEFT: u16 = 0x110;
    const BTN_JOYSTICK: u16 = 0x120;
    const BTN_SOUTH: u16 = 0x130;
    const BTN_TOOL_FINGER: u16 = 0x145;
    if key(KEY_A) || key(BTN_TOOL_FINGER) {
        return None;
    }
    if key(BTN_SOUTH) {
        return Some(Device::XInput);
    }
    if (BTN_JOYSTICK..BTN_JOYSTICK + 16).any(&key) && abs_xy {
        return Some(Device::DirectInput);
    }
    if key(BTN_LEFT) && (rel_xy || abs_xy) {
        return Some(Device::Mouse);
    }
    None
}

/// Which buttons a DirectInput-style controller's numbered buttons become,
/// in order (button 1 = A, ...): the usual DirectInput pad layout. evdev
/// joysticks number them from BTN_JOYSTICK, DirectInput from rgbButtons[0].
pub const DINPUT_BUTTONS: [u32; 12] = [A, B, X, Y, LB, RB, LT, RT, BACK, START, LS, RS];

/// Raw mouse / light gun buttons: left, right, middle, side, extra.
pub const MOUSE_BUTTONS: [u32; 5] = [RT, LT, A, START, BACK];

/// Who plays with which device, from `ARCADE_PLAYERS` and `ARCADE_INPUTS`
/// (settings.cfg `players`, `p1_input`, `p2_input`).
///
/// Each kind of device goes to the players using its method, in connection
/// order (any extra devices join the last of those players). The system
/// mouse aims for the keyboard-and-mouse player, or player one when nobody
/// has picked keyboard and mouse. While any player uses Raw Input, every
/// mouse is read directly instead: the Raw Input players' first, any others
/// for the keyboard-and-mouse player, and the system mouse aims for nobody.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Inputs {
    pub players: usize,
    pub methods: [Method; MAX_PLAYERS],
}

impl Inputs {
    pub const ONE_PLAYER: Self = Self {
        players: 1,
        methods: [Method::XInput; MAX_PLAYERS],
    };

    /// `players`: `2` or one player. `inputs`: comma-separated methods per
    /// player (missing or unknown = XInput).
    pub fn parse(players: Option<&[u8]>, inputs: Option<&[u8]>) -> Self {
        let mut out = Self::ONE_PLAYER;
        if players == Some(b"2") {
            out.players = 2;
        }
        if let Some(inputs) = inputs {
            for (slot, word) in out.methods.iter_mut().zip(inputs.split(|&b| b == b',')) {
                *slot = Method::parse(word).unwrap_or(Method::XInput);
            }
        }
        out
    }

    fn method(&self, player: usize) -> Method {
        self.methods[player.min(MAX_PLAYERS - 1)]
    }

    /// True if a player in this session uses `method`.
    pub fn uses(&self, method: Method) -> bool {
        (0..self.players).any(|p| self.method(p) == method)
    }

    fn keyboard_mouse_player(&self) -> Option<usize> {
        (0..self.players).find(|&p| self.method(p) == Method::KeyboardMouse)
    }

    /// The player the system mouse aims for (see [`Inputs`]).
    pub fn mouse(&self) -> Option<usize> {
        if self.uses(Method::RawInput) {
            return None;
        }
        Some(self.keyboard_mouse_player().unwrap_or(0))
    }

    /// The player the `index`th connected device of `kind` drives, or `None`
    /// when nobody uses that kind.
    pub fn device_player(&self, kind: Device, index: usize) -> Option<usize> {
        let mut players = [0; MAX_PLAYERS + 1];
        let mut count = 0;
        for p in 0..self.players {
            if self.method(p).device() == Some(kind) {
                players[count] = p;
                count += 1;
            }
        }
        if kind == Device::Mouse
            && count > 0
            && let Some(p) = self.keyboard_mouse_player()
        {
            players[count] = p;
            count += 1;
        }
        (count > 0).then(|| players[index.min(count - 1)])
    }

    /// True if `player` aims without the system mouse (a controller stick or
    /// its own mouse/light gun), so its aim reaches the game another way.
    pub fn own_aim(&self, player: usize) -> bool {
        player < self.players && self.mouse() != Some(player)
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
    fn devices_and_mouse_are_assigned_by_method() {
        use Device::*;
        let one = Inputs::parse(None, None);
        assert_eq!(one, Inputs::ONE_PLAYER);
        assert_eq!((one.device_player(XInput, 3), one.device_player(DirectInput, 0)), (Some(0), None));
        assert_eq!((one.mouse(), one.own_aim(0)), (Some(0), false));
        // Two Xbox pads: one each; the mouse stays player one.
        let pads = Inputs::parse(Some(b"2"), Some(b"xinput,xinput"));
        assert_eq!((pads.device_player(XInput, 0), pads.device_player(XInput, 5)), (Some(0), Some(1)));
        assert_eq!(pads.mouse(), Some(0));
        assert!(!pads.own_aim(0) && pads.own_aim(1));
        // The older words still work.
        assert_eq!(Inputs::parse(Some(b"2"), Some(b"pad,kbm")).methods, [Method::XInput, Method::KeyboardMouse]);
        // Player two on keyboard and mouse.
        let kbm = Inputs::parse(Some(b"2"), Some(b"dinput,keyboard_mouse"));
        assert_eq!((kbm.device_player(DirectInput, 2), kbm.device_player(XInput, 0)), (Some(0), None));
        assert_eq!(kbm.mouse(), Some(1));
        assert!(kbm.own_aim(0) && !kbm.own_aim(1));
        // Raw Input: every mouse is read directly; the system mouse aims for nobody.
        let raw = Inputs::parse(Some(b"2"), Some(b"rawinput,keyboard_mouse"));
        assert_eq!(raw.mouse(), None);
        assert_eq!((raw.device_player(Mouse, 0), raw.device_player(Mouse, 1)), (Some(0), Some(1)));
        assert!(raw.own_aim(0) && raw.own_aim(1));
        let guns = Inputs::parse(Some(b"2"), Some(b"rawinput,rawinput"));
        assert_eq!((guns.device_player(Mouse, 0), guns.device_player(Mouse, 1), guns.device_player(Mouse, 2)), (Some(0), Some(1), Some(1)));
        // One player only reads player one's method.
        let solo = Inputs::parse(None, Some(b"dinput,rawinput"));
        assert_eq!((solo.players, solo.device_player(Mouse, 0), solo.mouse()), (1, None, Some(0)));
    }

    #[test]
    fn movement_follows_the_bound_axes() {
        let right_down = Pad { lx: 32767, ly: -32767, ..Pad::default() };
        // Either stick by default; stick down is positive Y.
        assert_eq!(right_down.movement(&Movement::STICKS), (AXIS_MAX, AXIS_MAX));
        let parsed = Movement::parse(Some(b"x=AxisRX;y=axisly-"), Movement::STICKS);
        assert_eq!(parsed.x, [Some(AxisBind { axis: Axis::RightX, invert: false }), None]);
        assert_eq!(parsed.y[0], Some(AxisBind { axis: Axis::LeftY, invert: true }));
        assert_eq!(right_down.movement(&parsed), (0, -AXIS_MAX));
        // Malformed or absent: the default. `y=` leaves Y unbound.
        assert_eq!(Movement::parse(Some(b"x=Bogus"), Movement::STEERING), Movement::STEERING);
        assert_eq!(Movement::parse(None, Movement::STICKS), Movement::STICKS);
        assert_eq!(Movement::parse(Some(b"x=AxisLX;y="), Movement::STICKS), Movement::STEERING);
        // A trigger as an axis: pressed is positive.
        let pedal = Pad { rt: 255, ..Pad::default() };
        let gas = Movement::parse(Some(b"x=;y=AxisRT-"), Movement::STICKS);
        assert_eq!(pedal.movement(&gas), (0, -AXIS_MAX));
        // Pointers: default X/Y, or swapped/inverted by binding.
        let gun = Pad { pointer: Some((0, POINTER_MAX)), ..Pad::default() };
        assert_eq!(gun.pointer_moved(&Movement::STICKS, 640, 480), Some((0, 479)));
        let swapped = Movement::parse(Some(b"x=PointerY;y=PointerX-"), Movement::STICKS);
        assert_eq!(gun.pointer_moved(&swapped, 640, 480), Some((639, 479)));
    }

    #[test]
    fn devices_are_classified_like_the_backends_expect() {
        let has = |codes: &'static [u16]| move |c: u16| codes.contains(&c);
        assert_eq!(classify(has(&[0x130, 0x131]), false, true), Some(Device::XInput));
        assert_eq!(classify(has(&[0x120, 0x121]), false, true), Some(Device::DirectInput));
        assert_eq!(classify(has(&[0x110, 0x111]), true, false), Some(Device::Mouse));
        assert_eq!(classify(has(&[0x110]), false, true), Some(Device::Mouse)); // light gun
        assert_eq!(classify(has(&[0x110, 0x145]), false, true), None); // touchpad
        assert_eq!(classify(has(&[30, 0x110, 0x130]), true, true), None); // keyboard
        let pad = Pad { pointer: Some((u16::MAX, 0)), ..Pad::default() };
        assert_eq!(pad.pointer_in(640, 480), Some((639, 0)));
        assert_eq!(Pad::default().merge(pad).pointer, pad.pointer);
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
