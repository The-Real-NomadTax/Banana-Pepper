//! Build-specific game-side JAMMA API adapter, not a wire-level serial
//! emulator. One implementation for every platform: the Linux preload and
//! the Water adapter differ only in where input events and controller states
//! come from (`crate::platform`).
use crate::inputs::Switches;
use crate::pad;
use banana_input::session;
use std::{
    ffi::c_int,
    sync::{
        Mutex, OnceLock,
        atomic::{AtomicI32, AtomicU32, AtomicU64, Ordering},
    },
};
static SWITCHES: Mutex<Switches> = Mutex::new(Switches {
    counters: [0; 20],
    held: 0,
    dirty: false,
});
static CALLBACK: AtomicU32 = AtomicU32::new(0);
static BITS: AtomicU32 = AtomicU32::new(0);
static SEEN: AtomicU64 = AtomicU64::new(0);
static INSTALLED: OnceLock<Result<(), String>> = OnceLock::new();

/// The game's variadic `JammaOp(op, ...)`: hand the address of the first
/// argument to [`dispatch`] on a 16-byte aligned stack.
#[cfg(target_arch = "x86")]
#[unsafe(naked)]
unsafe extern "C" fn op() -> c_int {
    core::arch::naked_asm!(
        "push ebp",
        "mov ebp, esp",
        "and esp, -16",
        "sub esp, 12",
        "lea eax, [ebp + 8]",
        "push eax",
        "call {dispatch}",
        "mov esp, ebp",
        "pop ebp",
        "ret",
        dispatch = sym dispatch,
    );
}

unsafe extern "C" fn dispatch(args: *const u32) -> c_int {
    let op = unsafe { *args };
    let code = op & 255;
    if code < 64 && SEEN.fetch_or(1u64 << code, Ordering::Relaxed) & (1u64 << code) == 0 {
        eprintln!("[arcade] JAMMA op {op:#x}");
    }
    let arg = |n| unsafe { *args.add(n) };
    match code {
        1 => 0x30001, // Connected adapter, version 1 (game checks these bits).
        2 => {
            if arg(1) != 0 {
                unsafe {
                    *(arg(1) as *mut u32) = BITS.as_ptr() as u32;
                }
            }
            0
        }
        3 => 0, // Virtual board reset has no serial transport.
        6 => {
            CALLBACK.store(arg(1), Ordering::Relaxed);
            0
        }
        0x12 => {
            let mut state = SWITCHES.lock().unwrap_or_else(|e| e.into_inner());
            if arg(1) != 0 {
                unsafe {
                    *(arg(1) as *mut u32) = u32::from(state.dirty);
                }
            }
            if arg(2) != 0 {
                unsafe {
                    *(arg(2) as *mut u32) = state.counters.as_ptr() as u32;
                }
            }
            if op & 0x100 != 0 {
                state.dirty = false;
            }
            0
        }
        // Observed setup/output requests: no physical lamps or gun hardware.
        4 | 5 | 0xb | 0xd | 0x11 | 0x14 | 0x15 | 0x17 | 0x18 | 0x1f | 0x20 => 0,
        _ => -9000,
    }
}
#[cfg(target_arch = "x86")]
extern "C" fn success() -> c_int {
    0
}

/// Replace the build's JAMMA Init, Close, Loop and Op functions in memory,
/// after checking every expected prologue. Runs once, on the game's main
/// thread, inside the hash-verified ELF32 image.
#[cfg(target_arch = "x86")]
pub unsafe fn install() -> Result<(), String> {
    INSTALLED
        .get_or_init(|| {
            let hooks = [
                (crate::JAMMA_HOOKS[0], success as *const () as usize),
                (crate::JAMMA_HOOKS[1], success as *const () as usize),
                (crate::JAMMA_HOOKS[2], success as *const () as usize),
                (crate::JAMMA_HOOKS[3], op as *const () as usize),
            ];
            for &(address, _) in &hooks {
                if unsafe { std::slice::from_raw_parts(address as *const u8, 3) }
                    != [0x55, 0x89, 0xe5]
                {
                    return Err(format!("incompatible JAMMA prologue at {address:#x}"));
                }
            }
            crate::install_extra()?;
            for (address, target) in hooks {
                let mut patch = [0xb8, 0, 0, 0, 0, 0xff, 0xe0];
                patch[1..5].copy_from_slice(&(target as u32).to_le_bytes());
                unsafe {
                    std::ptr::copy_nonoverlapping(patch.as_ptr(), address as *mut u8, patch.len());
                }
            }
            eprintln!("[arcade] virtual JAMMA API installed");
            Ok(())
        })
        .clone()
}

/// (ASCII key, switch index); matches banana-input's default controls.cfg.
const DEFAULT_KEYS: &[(u8, usize)] = &[
    (b'1', 5),
    (13, 5),
    (b'2', 6),
    (b'5', 7),
    (b'6', 8),
    (b't', 11),
    (b'9', 10),
    (b' ', 1),
];
static KEYS: OnceLock<Vec<(u8, usize)>> = OnceLock::new();

/// Parses ARCADE_BBH_KEYS ("switch:ascii,..."), written from controls.cfg.
fn parse_keys(value: &str) -> Result<Vec<(u8, usize)>, String> {
    let mut keys = Vec::new();
    for pair in value.split(',').filter(|p| !p.trim().is_empty()) {
        let parsed = pair.split_once(':').and_then(|(index, key)| {
            Some((
                index.trim().parse::<usize>().ok()?,
                key.trim().parse::<u8>().ok()?,
            ))
        });
        match parsed {
            Some((index @ (1 | 2 | 5 | 6 | 7 | 8 | 10 | 11), key))
                if (1..128).contains(&key) && key != 27 =>
            {
                keys.push((key.to_ascii_lowercase(), index));
            }
            _ => return Err(format!("unsupported key binding \"{pair}\"")),
        }
    }
    Ok(keys)
}

fn switch_for(key: u8) -> Option<usize> {
    let keys = KEYS.get_or_init(|| match std::env::var("ARCADE_BBH_KEYS") {
        Ok(value) => parse_keys(&value).unwrap_or_else(|error| {
            eprintln!("[arcade] {error}; using default keys");
            DEFAULT_KEYS.to_vec()
        }),
        Err(_) => DEFAULT_KEYS.to_vec(),
    });
    let key = key.to_ascii_lowercase();
    keys.iter()
        .find(|(k, _)| *k == key)
        .map(|(_, index)| *index)
}

/// A keyboard key (ASCII, either case) went down or up.
pub fn key(key: u8, down: bool) {
    let Some(index) = switch_for(key) else {
        return;
    };
    set_switch(follow_mouse(index, inputs().mouse()), down);
}

/// Release every held switch (focus lost, window closed, exiting).
pub fn release() {
    for index in 0..20 {
        set_switch(index, false);
    }
}

/// Gun one's trigger and pump switches belong to whoever has the system
/// mouse: with player two on keyboard and mouse they become gun two's.
fn follow_mouse(index: usize, mouse: Option<usize>) -> usize {
    match (index, mouse) {
        (1, Some(1)) => GUN2_TRIGGER,
        (3, Some(1)) => GUN2_PUMP,
        _ => index,
    }
}

fn set_switch(index: usize, down: bool) {
    let mut switches = SWITCHES.lock().unwrap_or_else(|e| e.into_inner());
    let before = switches.held;
    switches.set(index, down);
    BITS.store(switches.held >> 1, Ordering::Relaxed);
    let changed = switches.held != before;
    drop(switches);
    if changed {
        eprintln!(
            "[arcade] switch {index} {}",
            if down { "down" } else { "up" }
        );
    }
}

/// Last aimed position per gun (mouse or controller), in 640x480 game
/// coordinates. The mouse always aims gun one.
static AIM: [[AtomicI32; 2]; pad::MAX_PLAYERS] = [
    [AtomicI32::new(320), AtomicI32::new(240)],
    [AtomicI32::new(320), AtomicI32::new(240)],
];

/// controls.cfg's default controller buttons for Big Buck.
const DEFAULT_PAD: &str = "5:16,7:32,256:131072,256:4096,257:65536,257:8192";
/// Controller 2 with two players: the same buttons on player two's targets.
const DEFAULT_PAD_P2: &str = "6:16,8:32,258:131072,258:4096,259:65536,259:8192";
/// Controller-only targets: aimed shot and pump, per gun.
const PAD_FIRE: u32 = 0x100;
const PAD_PUMP: u32 = 0x101;
const PAD_FIRE_P2: u32 = 0x102;
const PAD_PUMP_P2: u32 = 0x103;
/// Switch-table indices for gun two (`g21` trigger, `g22` pump); gun one's
/// are `g11` = 1 and `g12` = 3.
const GUN2_TRIGGER: usize = 2;
const GUN2_PUMP: usize = 4;
/// Crosshair speed at full stick deflection, in game pixels per second.
const CROSSHAIR_SPEED: i32 = 700;

struct Controller {
    devices: crate::platform::Devices,
    players: session::Players,
}
static CONTROLLER: Mutex<Option<Controller>> = Mutex::new(None);

/// Who plays with which device (settings.cfg `players`, `p1_input`, `p2_input`).
pub fn inputs() -> pad::Inputs {
    session::inputs()
}

/// Players in this session.
pub fn players() -> usize {
    inputs().players
}

/// One controller frame (a GLUT timer on Linux, a message-loop pass in
/// Water): crosshairs, shots, pumps and switches. `focused` is false while
/// another window has the keyboard; controllers then read as idle.
pub fn controller_frame(focused: bool) {
    let mut guard = CONTROLLER.lock().unwrap_or_else(|e| e.into_inner());
    let c = guard.get_or_insert_with(|| {
        let players = session::Players::new(DEFAULT_PAD, DEFAULT_PAD_P2);
        let inputs = &players.inputs;
        if inputs.players > 1 {
            eprintln!(
                "[arcade] two players ({}, {}); the system mouse aims {}",
                inputs.methods[0].label(),
                inputs.methods[1].label(),
                inputs
                    .mouse()
                    .map_or("no gun".to_owned(), |g| format!("gun {}", g + 1))
            );
        }
        Controller {
            devices: crate::platform::Devices::new(inputs),
            players,
        }
    });
    let states = if focused {
        c.devices.read_players(&c.players.inputs)
    } else {
        [pad::Pad::default(); pad::MAX_PLAYERS]
    };
    if session::Players::quit(&states) {
        drop(guard);
        release();
        eprintln!("[arcade] Back + Start: exiting");
        std::process::exit(0);
    }
    let elapsed = c.players.tick();
    let mut cursors = [(0, 0); pad::MAX_PLAYERS];
    let mut aims = Vec::new();
    for gun in 0..c.players.inputs.players {
        let (x, y) = aim_position(gun);
        let mut cursor = pad::Cursor::new(x, y);
        if c.players.aim(
            gun,
            &states[gun],
            &mut cursor,
            elapsed,
            CROSSHAIR_SPEED,
            640,
            480,
        ) {
            aims.push((gun, cursor.x, cursor.y));
        }
        cursors[gun] = (cursor.x, cursor.y);
    }
    let mut changes = Vec::new();
    c.players
        .edges(&states, |target, down| changes.push((target, down)));
    // The game's callback runs without the controller lock held.
    drop(guard);
    for (gun, x, y) in aims {
        aim_gun(gun, x, y, false);
    }
    for (target, down) in changes {
        match target {
            PAD_FIRE | PAD_FIRE_P2 => {
                let gun = usize::from(target == PAD_FIRE_P2);
                if down {
                    aim_gun(gun, cursors[gun].0, cursors[gun].1, true);
                }
                set_switch(if gun == 0 { 1 } else { GUN2_TRIGGER }, down);
            }
            PAD_PUMP => set_switch(3, down),
            PAD_PUMP_P2 => set_switch(GUN2_PUMP, down),
            1 | 2 | 5 | 6 | 7 | 8 | 10 | 11 => set_switch(target as usize, down),
            _ => {}
        }
    }
}

/// Where a gun's crosshair is drawn, in 640x480 game coordinates.
pub fn aim_position(gun: usize) -> (c_int, c_int) {
    let [x, y] = &AIM[gun.min(pad::MAX_PLAYERS - 1)];
    (x.load(Ordering::Relaxed), y.load(Ordering::Relaxed))
}

/// The system mouse's aim: gun one, or gun two when player two has the
/// mouse; nobody's while every mouse is read directly (Raw Input).
pub fn aim(x: c_int, y: c_int, shoot: bool) {
    if let Some(gun) = inputs().mouse() {
        aim_gun(gun, x, y, shoot);
    }
}

/// The gun callback's event code: shot (1) or move (9), with the gun number
/// (0 = gun one) in bits 8-11, as the game's callback at 0x0804d9f0 decodes it.
fn gun_event(gun: usize, shoot: bool) -> u32 {
    (if shoot { 1 } else { 9 }) | ((gun as u32 & 0xf) << 8)
}

/// Deliver the observed gun callback shape on the game's thread.
fn aim_gun(gun: usize, x: c_int, y: c_int, shoot: bool) {
    let gun = gun.min(pad::MAX_PLAYERS - 1);
    let (x, y) = (x.clamp(0, 639), y.clamp(0, 479));
    AIM[gun][0].store(x, Ordering::Relaxed);
    AIM[gun][1].store(y, Ordering::Relaxed);
    let callback = CALLBACK.load(Ordering::Relaxed);
    if callback == 0 {
        return;
    }
    let callback: unsafe extern "C" fn(u32, i32, i32, i32, i32, u32, u32) -> c_int =
        unsafe { std::mem::transmute(callback as usize) };
    // The game's callback copies these empty tracking lists synchronously.
    let empty = [0u32; 1];
    unsafe {
        callback(
            gun_event(gun, shoot),
            x,
            y,
            255,
            255,
            empty.as_ptr() as u32,
            empty.as_ptr() as u32,
        );
    }
    if shoot {
        eprintln!("[arcade] gun {} shot {x},{y}", gun + 1);
    }
}

/// A system-mouse button (0 left = trigger, 2 right = pump) at a game position.
pub fn mouse(button: c_int, down: bool, x: c_int, y: c_int) {
    let mouse = inputs().mouse();
    if mouse.is_none() {
        return; // every mouse and light gun is read directly
    }
    aim(x, y, button == 0 && down);
    let index = match button {
        0 => 1,
        2 => 3,
        _ => return,
    };
    set_switch(follow_mouse(index, mouse), down);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_arch = "x86")]
    #[test]
    fn variadic_bridge_returns_edges_and_acknowledges_dirty_state() {
        let op: unsafe extern "C" fn(u32, ...) -> c_int =
            unsafe { std::mem::transmute(op as *const ()) };
        let mut dirty = 0u32;
        let mut counters = 0u32;
        key(b't', true);
        key(b't', true);
        let result = unsafe {
            op(
                0x112,
                &mut dirty as *mut u32 as u32,
                &mut counters as *mut u32 as u32,
            )
        };
        assert_eq!(result, 0);
        assert_eq!(dirty, 1);
        assert_eq!(unsafe { *(counters as *const u32).add(11) }, 1);
        unsafe {
            op(0x112, &mut dirty as *mut u32 as u32, 0u32);
        }
        assert_eq!(dirty, 0);
        key(b't', false);
        assert_eq!(unsafe { *(counters as *const u32).add(11) }, 2);
        assert_eq!(unsafe { op(0xff) }, -9000);
    }

    #[test]
    fn unknown_operations_are_refused() {
        assert_eq!(unsafe { dispatch([0xffu32].as_ptr()) }, -9000);
        assert_eq!(unsafe { dispatch([0x1u32].as_ptr()) }, 0x30001);
    }

    #[test]
    fn gun_two_events_carry_the_gun_number() {
        assert_eq!(gun_event(0, true), 1);
        assert_eq!(gun_event(0, false), 9);
        assert_eq!(gun_event(1, true), 0x101);
        assert_eq!(gun_event(1, false), 0x109);
    }

    #[test]
    fn mouse_trigger_and_pump_follow_the_mouse_player() {
        assert_eq!((follow_mouse(1, Some(0)), follow_mouse(3, Some(0))), (1, 3));
        assert_eq!((follow_mouse(1, Some(1)), follow_mouse(3, Some(1))), (2, 4));
        assert_eq!(
            (follow_mouse(5, Some(1)), follow_mouse(11, Some(1))),
            (5, 11)
        );
        assert_eq!(follow_mouse(1, None), 1);
    }

    #[test]
    fn configured_keys_parse_and_reject_unknown_switches() {
        // Same order as banana-input's default controls.cfg output.
        let parsed = parse_keys("5:49,5:13,6:50,7:53,8:54,11:116,10:57,1:32").unwrap();
        let mut expected = DEFAULT_KEYS.to_vec();
        expected.sort();
        let mut sorted = parsed.clone();
        sorted.sort();
        assert_eq!(sorted, expected);
        assert_eq!(parse_keys("11:70").unwrap(), [(b'f', 11)]);
        assert!(parse_keys("3:49").is_err());
        assert!(parse_keys("11:27").is_err());
        assert!(parse_keys("11").is_err());
    }
}
