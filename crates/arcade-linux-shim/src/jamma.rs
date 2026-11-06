//! Build-specific game-side JAMMA API adapter, not a wire-level serial emulator.
use crate::inputs::Switches;
use std::{
    ffi::c_int,
    sync::{
        Mutex,
        atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
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
static INSTALLED: AtomicBool = AtomicBool::new(false);

core::arch::global_asm!(
    r#"
.hidden arcade_jamma_dispatch
.global arcade_jamma_op
.hidden arcade_jamma_op
.type arcade_jamma_op,@function
arcade_jamma_op:
    push ebp
    mov ebp, esp
    and esp, -16
    sub esp, 12
    lea eax, [ebp + 8]
    push eax
    call arcade_jamma_dispatch
    mov esp, ebp
    pop ebp
    ret
"#
);
unsafe extern "C" {
    fn arcade_jamma_op(op: u32, ...) -> c_int;
}

#[unsafe(no_mangle)]
unsafe extern "C" fn arcade_jamma_dispatch(args: *const u32) -> c_int {
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
extern "C" fn success() -> c_int {
    0
}

/// (ASCII key, switch index); matches arcade-core's default controls.cfg.
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
static KEYS: std::sync::OnceLock<Vec<(u8, usize)>> = std::sync::OnceLock::new();

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
            Some((index @ (1 | 5 | 6 | 7 | 8 | 10 | 11), key))
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

pub fn key(key: u8, down: bool) {
    let Some(index) = switch_for(key) else {
        return;
    };
    set_switch(index, down);
}

fn set_switch(index: usize, down: bool) {
    let mut switches = SWITCHES.lock().unwrap_or_else(|e| e.into_inner());
    switches.set(index, down);
    BITS.store(switches.held >> 1, Ordering::Relaxed);
    drop(switches);
    eprintln!(
        "[arcade] switch {index} {}",
        if down { "down" } else { "up" }
    );
}

/// Only called in the hash-verified ELF32 guest on its main thread.
pub unsafe fn install() {
    if !std::env::var_os("ARCADE_BBH_ADAPTER").is_some_and(|v| v == "1") {
        return;
    }
    if INSTALLED.swap(true, Ordering::Relaxed) {
        return;
    }
    // Check every expected prologue before changing any code.
    let hooks = [
        (0x80d9ce5usize, success as *const () as usize),
        (0x80d9e81, success as *const () as usize),
        (0x80dbf32, success as *const () as usize),
        (0x80da943, arcade_jamma_op as *const () as usize),
    ];
    for &(address, _) in &hooks {
        if unsafe { std::slice::from_raw_parts(address as *const u8, 3) } != [0x55, 0x89, 0xe5] {
            eprintln!("[arcade] incompatible JAMMA prologue at {address:#x}; adapter disabled");
            return;
        }
    }
    for (address, target) in hooks {
        let mut patch = [0xb8, 0, 0, 0, 0, 0xff, 0xe0];
        patch[1..5].copy_from_slice(&(target as u32).to_le_bytes());
        unsafe {
            std::ptr::copy_nonoverlapping(patch.as_ptr(), address as *mut u8, patch.len());
        }
    }
    eprintln!("[arcade] virtual JAMMA API installed");
}

/// Last aimed position (mouse or controller), in 640x480 game coordinates.
static AIM_X: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(320);
static AIM_Y: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(240);

/// controls.cfg's default controller buttons for Big Buck.
const DEFAULT_PAD: &str = "5:16,7:32,256:131072,256:4096,257:65536,257:8192";
/// Controller-only targets: aimed shot and pump.
const PAD_FIRE: u32 = 0x100;
const PAD_PUMP: u32 = 0x101;
/// Crosshair speed at full stick deflection, in game pixels per second.
const CROSSHAIR_SPEED: i32 = 700;

struct Controller {
    reader: crate::evdev_pad::Reader,
    bindings: Vec<(u32, u32)>,
    edges: crate::evdev_pad::Edges,
    last: std::time::Instant,
}
static CONTROLLER: Mutex<Option<Controller>> = Mutex::new(None);

/// One controller frame (GLUT timer): crosshair, shots, pump and switches.
pub fn controller_frame() {
    let mut guard = CONTROLLER.lock().unwrap_or_else(|e| e.into_inner());
    let c = guard.get_or_insert_with(|| Controller {
        reader: crate::evdev_pad::Reader::new(),
        bindings: crate::evdev_pad::bindings(DEFAULT_PAD),
        edges: Default::default(),
        last: std::time::Instant::now(),
    });
    let state = c.reader.read();
    if state.held(crate::pad::BACK) && state.held(crate::pad::START) {
        eprintln!("[arcade] Back + Start: exiting");
        std::process::exit(0);
    }
    let elapsed = c.last.elapsed().as_millis() as u32;
    c.last = std::time::Instant::now();
    let mut cursor = crate::pad::Cursor::new(
        AIM_X.load(Ordering::Relaxed),
        AIM_Y.load(Ordering::Relaxed),
    );
    if cursor.step(state.aim(), elapsed, CROSSHAIR_SPEED, 640, 480) {
        aim(cursor.x, cursor.y, false);
    }
    let mut changes = Vec::new();
    c.edges
        .update(&state, &c.bindings, |target, down| changes.push((target, down)));
    drop(guard);
    for (target, down) in changes {
        match target {
            PAD_FIRE => {
                if down {
                    aim(cursor.x, cursor.y, true);
                }
                set_switch(1, down);
            }
            PAD_PUMP => set_switch(3, down),
            1 | 5 | 6 | 7 | 8 | 10 | 11 => set_switch(target as usize, down),
            _ => {}
        }
    }
}

/// Where the crosshair is drawn, in 640x480 game coordinates.
pub fn aim_position() -> (c_int, c_int) {
    (AIM_X.load(Ordering::Relaxed), AIM_Y.load(Ordering::Relaxed))
}

/// Deliver the observed gun callback shape on the GLUT/game thread.
pub fn aim(x: c_int, y: c_int, shoot: bool) {
    AIM_X.store(x.clamp(0, 639), Ordering::Relaxed);
    AIM_Y.store(y.clamp(0, 479), Ordering::Relaxed);
    let callback = CALLBACK.load(Ordering::Relaxed);
    if callback == 0 {
        return;
    }
    let callback: unsafe extern "C" fn(u32, i32, i32, i32, i32, u32, u32) -> c_int =
        unsafe { std::mem::transmute(callback as usize) };
    // The game's callback copies these empty tracking lists synchronously.
    let empty = [0u32; 1];
    let event = if shoot { 1 } else { 9 };
    unsafe {
        callback(
            event,
            x.clamp(0, 639),
            y.clamp(0, 479),
            255,
            255,
            empty.as_ptr() as u32,
            empty.as_ptr() as u32,
        );
    }
    if shoot {
        eprintln!("[arcade] gun shot {x},{y}");
    }
}

pub fn mouse(button: c_int, down: bool, x: c_int, y: c_int) {
    aim(x, y, button == 0 && down);
    let index = match button {
        0 => 1,
        2 => 3,
        _ => return,
    };
    let mut switches = SWITCHES.lock().unwrap_or_else(|e| e.into_inner());
    switches.set(index, down);
    BITS.store(switches.held >> 1, Ordering::Relaxed);
    drop(switches);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn variadic_bridge_returns_edges_and_acknowledges_dirty_state() {
        let mut dirty = 0u32;
        let mut counters = 0u32;
        key(b't', true);
        key(b't', true);
        let result = unsafe {
            arcade_jamma_op(
                0x112,
                &mut dirty as *mut u32 as u32,
                &mut counters as *mut u32 as u32,
            )
        };
        assert_eq!(result, 0);
        assert_eq!(dirty, 1);
        assert_eq!(unsafe { *(counters as *const u32).add(11) }, 1);
        unsafe {
            arcade_jamma_op(0x112, &mut dirty as *mut u32 as u32, 0u32);
        }
        assert_eq!(dirty, 0);
        key(b't', false);
        assert_eq!(unsafe { *(counters as *const u32).add(11) }, 2);
        assert_eq!(unsafe { arcade_jamma_op(0xff) }, -9000);
    }

    #[test]
    fn configured_keys_parse_and_reject_unknown_switches() {
        // Same order as arcade-core's default controls.cfg output.
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
