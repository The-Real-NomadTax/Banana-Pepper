//! Jurassic Park controllers. Button presses become SDL 1.2 key events for
//! private world keysyms (SDLK_WORLD_0 + n) that the desktop input maps route
//! to each bound action, so they behave exactly like keyboard keys. Either
//! stick moves the aim: synthetic SDL_MOUSEMOTION events plus SDL_GetMouseState
//! report the controller pointer until the real mouse moves again.
use crate::evdev_pad::{Edges, Reader, bindings};
use crate::pad::Cursor;
use std::collections::VecDeque;
use std::ffi::c_void;
use std::sync::Mutex;
use std::sync::atomic::{AtomicI32, Ordering};
use std::time::Instant;

/// controls.cfg's default controller buttons for Jurassic Park.
pub const DEFAULT_PAD: &str = "371:16,379:32,383:1,384:2,385:4096,386:8192,333:131072,334:65536";
/// First private keysym (SDLK_WORLD_0); SDL 1.2 defines 96 of them.
pub const KEYSYM_BASE: i32 = 160;
/// Pointer speed at full stick deflection, as window widths per second.
const SPEED_PER_WIDTH: i32 = 1;
/// SDL 1.2's SDL_Event union is 20 bytes on i386.
const EVENT_SIZE: usize = 20;

pub static WIDTH: AtomicI32 = AtomicI32::new(0);
pub static HEIGHT: AtomicI32 = AtomicI32::new(0);

struct State {
    reader: Reader,
    bindings: Vec<(u32, u32)>,
    /// (target, keysym) for each bound action.
    sources: Vec<(u32, i32)>,
    edges: Edges,
    cursor: Cursor,
    /// The controller moved the pointer more recently than the mouse.
    aiming: bool,
    last_real: (i32, i32),
    last: Instant,
    queue: VecDeque<[u8; EVENT_SIZE]>,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);

/// Distinct pad-bound targets, each with its own private keysym.
pub fn sources(bindings: &[(u32, u32)]) -> Vec<(u32, i32)> {
    let mut sources: Vec<(u32, i32)> = Vec::new();
    for &(target, _) in bindings {
        if sources.len() < 96 && !sources.iter().any(|s| s.0 == target) {
            sources.push((target, KEYSYM_BASE + sources.len() as i32));
        }
    }
    sources
}

pub fn configured_bindings() -> Vec<(u32, u32)> {
    bindings(DEFAULT_PAD)
}

fn with<R>(f: impl FnOnce(&mut State) -> R) -> R {
    let mut guard = STATE.lock().unwrap_or_else(|e| e.into_inner());
    let state = guard.get_or_insert_with(|| {
        let bindings = configured_bindings();
        State {
            reader: Reader::new(),
            sources: sources(&bindings),
            bindings,
            edges: Edges::default(),
            cursor: Cursor::new(
                WIDTH.load(Ordering::Relaxed) / 2,
                HEIGHT.load(Ordering::Relaxed) / 2,
            ),
            aiming: false,
            last_real: (-1, -1),
            last: Instant::now(),
            queue: VecDeque::new(),
        }
    });
    f(state)
}

fn key_event(keysym: i32, down: bool) -> [u8; EVENT_SIZE] {
    let mut e = [0u8; EVENT_SIZE];
    e[0] = if down { 2 } else { 3 }; // SDL_KEYDOWN / SDL_KEYUP
    e[2] = u8::from(down); // SDL_PRESSED / SDL_RELEASED
    e[8..12].copy_from_slice(&keysym.to_ne_bytes());
    e
}

fn motion_event(x: i32, y: i32, dx: i32, dy: i32) -> [u8; EVENT_SIZE] {
    let mut e = [0u8; EVENT_SIZE];
    e[0] = 4; // SDL_MOUSEMOTION
    e[4..6].copy_from_slice(&(x as u16).to_ne_bytes());
    e[6..8].copy_from_slice(&(y as u16).to_ne_bytes());
    e[8..10].copy_from_slice(&(dx as i16).to_ne_bytes());
    e[10..12].copy_from_slice(&(dy as i16).to_ne_bytes());
    e
}

/// Once per frame (start of the engine's event drain): read controllers.
pub fn frame() {
    with(|s| {
        let state = s.reader.read();
        if state.held(crate::pad::BACK) && state.held(crate::pad::START) {
            eprintln!("[jurassic] Back + Start: exiting");
            std::process::exit(0);
        }
        let mut changes = Vec::new();
        s.edges.update(&state, &s.bindings, |target, down| {
            changes.push((target, down))
        });
        for (target, down) in changes {
            if let Some(&(_, keysym)) = s.sources.iter().find(|x| x.0 == target) {
                s.queue.push_back(key_event(keysym, down));
            }
        }
        let elapsed = s.last.elapsed().as_millis() as u32;
        s.last = Instant::now();
        let (width, height) = (
            WIDTH.load(Ordering::Relaxed),
            HEIGHT.load(Ordering::Relaxed),
        );
        if width <= 0 || height <= 0 {
            return;
        }
        let (old_x, old_y) = (s.cursor.x, s.cursor.y);
        if s.cursor
            .step(state.aim(), elapsed, width * SPEED_PER_WIDTH, width, height)
        {
            s.aiming = true;
            s.queue.push_back(motion_event(
                s.cursor.x,
                s.cursor.y,
                s.cursor.x - old_x,
                s.cursor.y - old_y,
            ));
        }
    })
}

/// Pops one synthetic event. `event` may be null (a pending-events check).
pub fn next(event: *mut c_void) -> bool {
    with(|s| {
        let Some(front) = s.queue.front() else {
            return false;
        };
        if !event.is_null() {
            unsafe {
                std::ptr::copy_nonoverlapping(front.as_ptr(), event.cast::<u8>(), EVENT_SIZE)
            };
            s.queue.pop_front();
        }
        true
    })
}

/// SDL_GetMouseState position: the controller pointer while it is in use.
pub fn pointer(real_x: i32, real_y: i32) -> (i32, i32) {
    with(|s| {
        if (real_x, real_y) != s.last_real {
            // The real mouse moved: it takes over, and the stick continues from there.
            s.last_real = (real_x, real_y);
            s.aiming = false;
            s.cursor = Cursor::new(real_x, real_y);
        }
        if s.aiming {
            (s.cursor.x, s.cursor.y)
        } else {
            (real_x, real_y)
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_target_gets_one_private_keysym() {
        let b = crate::evdev_pad::bindings(DEFAULT_PAD);
        let s = sources(&b);
        assert_eq!(s.len(), 8);
        assert_eq!(s[0], (371, 160));
        assert_eq!(s[7], (334, 167));
        assert_eq!(sources(&[(5, 1), (5, 2), (6, 4)]), [(5, 160), (6, 161)]);
    }

    #[test]
    fn sdl12_event_layouts() {
        let e = key_event(161, true);
        assert_eq!((e[0], e[2]), (2, 1));
        assert_eq!(i32::from_ne_bytes(e[8..12].try_into().unwrap()), 161);
        let m = motion_event(300, 200, -4, 2);
        assert_eq!(m[0], 4);
        assert_eq!(u16::from_ne_bytes([m[4], m[5]]), 300);
        assert_eq!(i16::from_ne_bytes([m[8], m[9]]), -4);
    }
}
