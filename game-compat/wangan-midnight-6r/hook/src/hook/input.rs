//! Desktop controls to cabinet inputs, read each time the game polls the
//! JVS board.
//!
//! Bindings come from controls.cfg through the session environment:
//! `ARCADE_WMMT6R_KEYS` (`target:dik,...`), `ARCADE_PAD` (`target:mask,...`)
//! and `ARCADE_MOVE` (which stick steers). Targets match the plan crate's
//! `controls.rs`.
use super::{controllers, pad};
use crate::board::Inputs;

#[link(name = "kernel32", kind = "raw-dylib")]
unsafe extern "system" {
    fn GetEnvironmentVariableA(name: *const u8, value: *mut u8, size: u32) -> u32;
    fn GetTickCount() -> u32;
}
#[link(name = "user32", kind = "raw-dylib")]
unsafe extern "system" {
    fn GetAsyncKeyState(key: i32) -> i16;
    fn MapVirtualKeyA(code: u32, kind: u32) -> u32;
}

const STEER_LEFT: u32 = 1;
const STEER_RIGHT: u32 = 2;
const GAS: u32 = 3;
const BRAKE: u32 = 4;
const SHIFT_UP: u32 = 0x200;
const SHIFT_DOWN: u32 = 0x201;
const NEUTRAL: u32 = 0x210;
const COIN: u32 = 0x103;

/// Switch targets: (target, switch byte, bit). Byte 0 is the system byte,
/// 1-2 player one's.
const SWITCHES: &[(u32, usize, u8)] = &[
    (0x100, 1, 0x02), // enter / view change
    (0x101, 2, 0x02), // perspective
    (0x102, 2, 0x01), // interruption
    (0x104, 0, 0x80), // test
    (0x105, 1, 0x40), // service
    (0x106, 1, 0x20), // operator menu up
    (0x107, 1, 0x10), // operator menu down
];

/// The shifter's gate sensors per gear (neutral, 1-6): rows up (0x80) and
/// down (0x40), columns left (0x20) and right (0x10); the centre column has
/// no column sensor.
const GEARS: [u8; 7] = [0, 0xa0, 0x60, 0x80, 0x40, 0x90, 0x50];

/// Steering travel in 1/4096ths of half the range per millisecond.
const STEER_RATE: i32 = 6;
const CENTRE_RATE: i32 = 10;
const MAX_BINDINGS: usize = 96;

pub struct Controls {
    keys: [(u32, u8); MAX_BINDINGS],
    key_count: usize,
    pads: [(u32, u32); MAX_BINDINGS],
    pad_count: usize,
    moves: pad::Movement,
    steer: i32,
    last_tick: u32,
    gear: usize,
    held: [bool; 4], // shift up, shift down, coin, exit
    exit_armed: bool,
}

pub fn env<'a>(name: &[u8], buffer: &'a mut [u8]) -> Option<&'a [u8]> {
    let n = unsafe { GetEnvironmentVariableA(name.as_ptr(), buffer.as_mut_ptr(), buffer.len() as u32) }
        as usize;
    (n > 0 && n < buffer.len()).then(|| &buffer[..n])
}

/// The virtual key for a DirectInput scan code.
fn virtual_key(dik: u32) -> u8 {
    match dik {
        0xc8 => 0x26, // up
        0xd0 => 0x28, // down
        0xcb => 0x25, // left
        0xcd => 0x27, // right
        0xc7 => 0x24, // home
        0xcf => 0x23, // end
        0xc9 => 0x21, // page up
        0xd1 => 0x22, // page down
        0xd2 => 0x2d, // insert
        0xd3 => 0x2e, // delete
        0x9c => 0x0d, // keypad enter
        0x9d => 0xa3, // right control
        0xb8 => 0xa5, // right alt
        0xb5 => 0x6f, // keypad divide
        code if code < 0x80 => (unsafe { MapVirtualKeyA(code, 1) }) as u8,
        _ => 0,
    }
}

fn key_down(vk: u8) -> bool {
    vk != 0 && unsafe { GetAsyncKeyState(i32::from(vk)) } as u16 & 0x8000 != 0
}

impl Controls {
    pub fn new() -> Self {
        let mut c = Self {
            keys: [(0, 0); MAX_BINDINGS],
            key_count: 0,
            pads: [(0, 0); MAX_BINDINGS],
            pad_count: 0,
            moves: pad::Movement::STEERING,
            steer: 0,
            last_tick: 0,
            gear: 0,
            held: [false; 4],
            exit_armed: false,
        };
        let mut buffer = [0u8; 2048];
        let mut raw = [(0u32, 0u32); MAX_BINDINGS];
        if let Some(text) = env(b"ARCADE_WMMT6R_KEYS\0", &mut buffer) {
            let n = pad::parse_bindings(text, &mut raw);
            for &(target, dik) in &raw[..n] {
                let vk = virtual_key(dik);
                if vk != 0 {
                    c.keys[c.key_count] = (target, vk);
                    c.key_count += 1;
                }
            }
        }
        if let Some(text) = env(b"ARCADE_PAD\0", &mut buffer) {
            c.pad_count = pad::parse_bindings(text, &mut c.pads);
        }
        c.moves = pad::Movement::parse(env(b"ARCADE_MOVE\0", &mut buffer), pad::Movement::STEERING);
        let mut h = [0u8; 16];
        super::log(&[
            b"controls: ",
            super::hex(c.key_count as u64, &mut h),
            b" key bindings (hex)",
        ]);
        c
    }

    /// The board's inputs now, and whether a coin was just inserted.
    pub fn poll(&mut self) -> (Inputs, bool) {
        let pad = controllers::read();
        let keys = &self.keys[..self.key_count];
        let pads = &self.pads[..self.pad_count];
        let key = |target: u32| keys.iter().any(|&(t, vk)| t == target && key_down(vk));
        let held = |target: u32| key(target) || pad::target_held(&pad, pads, target);

        // Escape or Back + Start exits, once released and pressed again.
        let exit = key_down(0x1b) || (pad.held(pad::BACK) && pad.held(pad::START));
        if !exit {
            self.exit_armed = true;
        } else if self.exit_armed {
            super::log(&[b"exit requested"]);
            unsafe { super::ExitProcess(0) }
        }

        let mut inputs = Inputs::default();
        for &(target, byte, bit) in SWITCHES {
            if held(target) {
                inputs.switches[byte] |= bit;
            }
        }

        // Sequential shifting steps through neutral; a gear key selects
        // that gear directly. The lever stays where it was left.
        let edges = [held(SHIFT_UP), held(SHIFT_DOWN), held(COIN)];
        let pressed = |i: usize| edges[i] && !self.held[i];
        if pressed(0) {
            self.gear = (self.gear + 1).min(6);
        }
        if pressed(1) {
            self.gear = self.gear.saturating_sub(1);
        }
        let coin = pressed(2);
        self.held[..3].copy_from_slice(&edges);
        for gear in 0..7 {
            if held(NEUTRAL + gear as u32) {
                self.gear = gear;
            }
        }
        inputs.switches[2] |= GEARS[self.gear];

        // Steering keys ramp like a wheel and recentre; the stick is an
        // absolute wheel position.
        let tick = unsafe { GetTickCount() };
        let elapsed = if self.last_tick == 0 {
            16
        } else {
            tick.wrapping_sub(self.last_tick).min(100)
        } as i32;
        self.last_tick = tick;
        let goal = match (held(STEER_LEFT), held(STEER_RIGHT)) {
            (true, false) => -pad::AXIS_MAX,
            (false, true) => pad::AXIS_MAX,
            _ => 0,
        };
        let (stick, pedals) = pad.movement(&self.moves);
        let rate = if goal == 0 { CENTRE_RATE } else { STEER_RATE } * elapsed;
        if goal == 0 && stick != 0 {
            self.steer = stick;
        } else if self.steer < goal {
            self.steer = (self.steer + rate).min(goal);
        } else if self.steer > goal {
            self.steer = (self.steer - rate).max(goal);
        }
        let pedal = |target: u32| -> u16 {
            if key(target) {
                return 0xffff;
            }
            // Movement Y: up = gas, down = brake.
            let axis = match target {
                GAS => (-pedals).max(0),
                _ => pedals.max(0),
            } * 0xffff
                / pad::AXIS_MAX;
            (i32::from(pad::target_level(&pad, pads, target)) * 0x101).max(axis) as u16
        };
        inputs.analog[0] = (0x8000 + self.steer * 0x7fff / pad::AXIS_MAX) as u16;
        inputs.analog[1] = pedal(GAS);
        inputs.analog[2] = pedal(BRAKE);
        (inputs, coin)
    }
}
