//! The game's GLUT frontend, shared by every platform: it replaces the
//! cabinet's fullscreen game mode with a desktop window, forwards GLUT
//! keyboard and mouse callbacks to the JAMMA adapter, polls controllers from
//! a GLUT timer and draws the crosshairs before each buffer swap.
//!
//! The same three functions override GLUT on Linux (exported from the
//! preloaded library) and in Water (returned from the adapter's `resolve`);
//! the real GLUT functions come from the next library either way.
use crate::{crosshair_gl, jamma, platform};
use std::{
    ffi::{CStr, c_char, c_int, c_uint},
    sync::{
        OnceLock,
        atomic::{AtomicI32, Ordering},
    },
};

/// The GLUT functions this frontend calls: the real ones, not ours.
pub struct Glut {
    init_window_size: unsafe extern "C" fn(c_int, c_int),
    create_window: unsafe extern "C" fn(*const c_char) -> c_int,
    destroy_window: unsafe extern "C" fn(c_int),
    full_screen: unsafe extern "C" fn(),
    get: unsafe extern "C" fn(c_int) -> c_int,
    keyboard_func: unsafe extern "C" fn(extern "C" fn(u8, c_int, c_int)),
    keyboard_up_func: unsafe extern "C" fn(extern "C" fn(u8, c_int, c_int)),
    mouse_func: unsafe extern "C" fn(extern "C" fn(c_int, c_int, c_int, c_int)),
    motion_func: unsafe extern "C" fn(extern "C" fn(c_int, c_int)),
    passive_motion_func: unsafe extern "C" fn(extern "C" fn(c_int, c_int)),
    set_cursor: unsafe extern "C" fn(c_int),
    timer_func: unsafe extern "C" fn(c_uint, extern "C" fn(c_int), c_int),
    swap_buffers: unsafe extern "C" fn(),
}

impl Glut {
    /// Look up every function through `next` (the platform's next library).
    fn load(next: impl Fn(&CStr) -> usize) -> Result<Self, String> {
        macro_rules! table {
            ($($field:ident = $name:literal),* $(,)?) => {
                Self {$(
                    $field: {
                        let address = next($name);
                        if address == 0 {
                            return Err(format!("GLUT has no {}", $name.to_string_lossy()));
                        }
                        // SAFETY: the named GLUT function has this signature.
                        unsafe { std::mem::transmute::<usize, _>(address) }
                    },
                )*}
            };
        }
        Ok(table!(
            init_window_size = c"glutInitWindowSize",
            create_window = c"glutCreateWindow",
            destroy_window = c"glutDestroyWindow",
            full_screen = c"glutFullScreen",
            get = c"glutGet",
            keyboard_func = c"glutKeyboardFunc",
            keyboard_up_func = c"glutKeyboardUpFunc",
            mouse_func = c"glutMouseFunc",
            motion_func = c"glutMotionFunc",
            passive_motion_func = c"glutPassiveMotionFunc",
            set_cursor = c"glutSetCursor",
            timer_func = c"glutTimerFunc",
            swap_buffers = c"glutSwapBuffers",
        ))
    }
}

fn glut() -> &'static Glut {
    static GLUT: OnceLock<Glut> = OnceLock::new();
    GLUT.get_or_init(|| {
        Glut::load(platform::next).unwrap_or_else(|e| {
            eprintln!("[arcade] {e}");
            std::process::exit(125)
        })
    })
}

const GLUT_WINDOW_WIDTH: c_int = 0x66;
const GLUT_WINDOW_HEIGHT: c_int = 0x67;
const GLUT_CURSOR_NONE: c_int = 0x65;
static WINDOW: AtomicI32 = AtomicI32::new(0);

/// `glutEnterGameMode`: a desktop window instead of the cabinet's
/// fullscreen mode switch.
pub extern "C" fn enter_game_mode() -> c_int {
    platform::install();
    let g = glut();
    let (width, height) = window_size();
    unsafe {
        (g.init_window_size)(width, height);
        let id = (g.create_window)(c"Banana pepper - Big Buck Hunter Pro".as_ptr());
        WINDOW.store(id, Ordering::Relaxed);
        if std::env::var_os("ARCADE_FULLSCREEN").is_some_and(|v| v == "1") {
            (g.full_screen)();
        }
        (g.keyboard_func)(key_down);
        (g.keyboard_up_func)(key_up);
        (g.mouse_func)(mouse);
        (g.motion_func)(motion);
        (g.passive_motion_func)(motion);
        (g.timer_func)(16, controller_frame, 0);
        if crosshair_gl::enabled() {
            (g.set_cursor)(GLUT_CURSOR_NONE); // the crosshair replaces it
        }
        platform::window_created();
        id
    }
}

/// `glutLeaveGameMode`.
pub extern "C" fn leave_game_mode() {
    let window = WINDOW.swap(0, Ordering::Relaxed);
    if window > 0 {
        unsafe { (glut().destroy_window)(window) };
    }
}

/// `glutSwapBuffers`: draw the crosshairs over the finished frame first.
pub extern "C" fn swap_buffers() {
    let g = glut();
    if WINDOW.load(Ordering::Relaxed) > 0 && crosshair_gl::enabled() {
        let (width, height) = unsafe {
            (
                (g.get)(GLUT_WINDOW_WIDTH).max(1),
                (g.get)(GLUT_WINDOW_HEIGHT).max(1),
            )
        };
        for gun in 0..jamma::players() {
            let (x, y) = jamma::aim_position(gun);
            unsafe { crosshair_gl::draw(gun, x, y, width, height) };
        }
    }
    unsafe { (g.swap_buffers)() };
}

/// Desktop window size from ARCADE_RESOLUTION (settings.cfg); 640x480 otherwise.
fn window_size() -> (c_int, c_int) {
    parse_size(&std::env::var("ARCADE_RESOLUTION").unwrap_or_default()).unwrap_or((640, 480))
}

fn parse_size(value: &str) -> Option<(c_int, c_int)> {
    let (w, h) = value.split_once('x')?;
    let (w, h) = (w.parse::<c_int>().ok()?, h.parse::<c_int>().ok()?);
    ((320..=7680).contains(&w) && (240..=4320).contains(&h)).then_some((w, h))
}

extern "C" fn key_down(key: u8, _x: c_int, _y: c_int) {
    if key == 27 {
        jamma::release();
        std::process::exit(0);
    }
    jamma::key(key, true);
}
extern "C" fn key_up(key: u8, _x: c_int, _y: c_int) {
    jamma::key(key, false);
}

/// Controllers are polled from a GLUT timer, so aim callbacks run on the
/// GLUT thread exactly like the mouse callbacks.
extern "C" fn controller_frame(_value: c_int) {
    jamma::controller_frame(platform::focused());
    unsafe { (glut().timer_func)(16, controller_frame, 0) };
}

extern "C" fn mouse(button: c_int, state: c_int, x: c_int, y: c_int) {
    let (x, y) = game_coordinates(x, y);
    jamma::mouse(button, state == 0, x, y);
}
extern "C" fn motion(x: c_int, y: c_int) {
    let (x, y) = game_coordinates(x, y);
    jamma::aim(x, y, false);
}

/// Window pixels to the game's 640x480 coordinates.
fn game_coordinates(x: c_int, y: c_int) -> (c_int, c_int) {
    let g = glut();
    let width = unsafe { (g.get)(GLUT_WINDOW_WIDTH) }.max(1);
    let height = unsafe { (g.get)(GLUT_WINDOW_HEIGHT) }.max(1);
    (
        (i64::from(x) * 640 / i64::from(width)).clamp(0, 639) as c_int,
        (i64::from(y) * 480 / i64::from(height)).clamp(0, 479) as c_int,
    )
}

#[cfg(test)]
#[test]
fn window_size_parsing() {
    assert_eq!(parse_size("1280x960"), Some((1280, 960)));
    assert_eq!(parse_size("12x9"), None);
    assert_eq!(parse_size("wide"), None);
}
