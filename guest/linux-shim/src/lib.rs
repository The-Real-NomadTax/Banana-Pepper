//! Small guest-side compatibility layer; build for i686-unknown-linux-gnu.
#![cfg(target_os = "linux")]
#[cfg(target_arch = "x86")]
mod aliens;
#[cfg(target_arch = "x86")]
mod aliens_exit;
#[cfg(any(target_arch = "x86", test))]
#[path = "../../../crates/banana-input/src/crosshair.rs"]
mod crosshair;
#[cfg(any(target_arch = "x86", test))]
mod crosshair_gl;
#[cfg(any(target_arch = "x86", test))]
mod evdev_pad;
#[cfg(any(target_arch = "x86", test))]
mod inputs;
#[cfg(target_arch = "x86")]
mod jamma;
#[cfg(target_arch = "x86")]
mod jurassic;
#[cfg(any(target_arch = "x86", test))]
mod jurassic_pad;

#[cfg(target_arch = "x86")]
mod audio;
mod display;
mod legacy_csv;
#[cfg(any(target_arch = "x86", test))]
#[allow(dead_code)]
#[path = "../../../crates/banana-input/src/pad.rs"]
mod pad;

use std::ffi::{c_char, c_int};

#[link(name = "glut")]
unsafe extern "C" {
    fn glutInitWindowSize(width: c_int, height: c_int);
    fn glutCreateWindow(title: *const c_char) -> c_int;
    fn glutDestroyWindow(window: c_int);
    fn glutFullScreen();
    #[cfg(target_arch = "x86")]
    fn glutGet(query: c_int) -> c_int;
    #[cfg(target_arch = "x86")]
    fn glutKeyboardFunc(callback: extern "C" fn(u8, c_int, c_int));
    #[cfg(target_arch = "x86")]
    fn glutKeyboardUpFunc(callback: extern "C" fn(u8, c_int, c_int));
    #[cfg(target_arch = "x86")]
    fn glutMouseFunc(callback: extern "C" fn(c_int, c_int, c_int, c_int));
    #[cfg(target_arch = "x86")]
    fn glutMotionFunc(callback: extern "C" fn(c_int, c_int));
    #[cfg(target_arch = "x86")]
    fn glutPassiveMotionFunc(callback: extern "C" fn(c_int, c_int));
    #[cfg(target_arch = "x86")]
    fn glutSetCursor(cursor: c_int);
    #[cfg(target_arch = "x86")]
    fn glutTimerFunc(milliseconds: std::ffi::c_uint, callback: extern "C" fn(c_int), value: c_int);
}

use std::sync::atomic::{AtomicI32, Ordering};
static WINDOW: AtomicI32 = AtomicI32::new(0);

/// Cabinet fullscreen mode switching is replaced with a desktop GL window.
#[unsafe(no_mangle)]
pub extern "C" fn glutEnterGameMode() -> c_int {
    unsafe {
        #[cfg(target_arch = "x86")]
        jamma::install();
        let (width, height) = window_size();
        glutInitWindowSize(width, height);
        let id = glutCreateWindow(c"Banana pepper - Big Buck Hunter Pro".as_ptr());
        WINDOW.store(id, Ordering::Relaxed);
        if std::env::var_os("ARCADE_FULLSCREEN").is_some_and(|v| v == "1") {
            glutFullScreen();
        }
        #[cfg(target_arch = "x86")]
        {
            glutKeyboardFunc(key_down);
            glutKeyboardUpFunc(key_up);
            glutMouseFunc(mouse);
            glutMotionFunc(motion);
            glutPassiveMotionFunc(motion);
            glutTimerFunc(16, controller_frame, 0);
            if crosshair_gl::enabled() {
                glutSetCursor(0x65); // GLUT_CURSOR_NONE: the crosshair replaces it
            }
        }
        id
    }
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

#[cfg(test)]
#[test]
fn window_size_parsing() {
    assert_eq!(parse_size("1280x960"), Some((1280, 960)));
    assert_eq!(parse_size("12x9"), None);
    assert_eq!(parse_size("wide"), None);
}

#[unsafe(no_mangle)]
pub extern "C" fn glutLeaveGameMode() {
    let window = WINDOW.swap(0, Ordering::Relaxed);
    if window > 0 {
        unsafe {
            glutDestroyWindow(window);
        }
    }
}

#[cfg(target_arch = "x86")]
extern "C" fn key_down(key: u8, _x: c_int, _y: c_int) {
    if key == 27 {
        std::process::exit(0);
    }
    jamma::key(key, true);
}
#[cfg(target_arch = "x86")]
extern "C" fn key_up(key: u8, _x: c_int, _y: c_int) {
    jamma::key(key, false);
}

#[cfg(target_arch = "x86")]
extern "C" fn initialize() {
    jurassic::initialize();
    aliens_exit::start();
    if std::env::var_os("ARCADE_BBH_ADAPTER").is_some_and(|v| v == "1") {
        unsafe {
            jamma::install();
        }
    }
}
#[cfg(target_arch = "x86")]
#[used]
#[unsafe(link_section = ".init_array")]
static INITIALIZE: extern "C" fn() = initialize;

/// The game's buffer swap: draw the crosshair over the finished frame first.
#[cfg(target_arch = "x86")]
#[unsafe(no_mangle)]
pub extern "C" fn glutSwapBuffers() {
    static REAL: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    let real = *REAL.get_or_init(|| unsafe {
        libc::dlsym(libc::RTLD_NEXT, c"glutSwapBuffers".as_ptr()) as usize
    });
    if WINDOW.load(Ordering::Relaxed) > 0 && crosshair_gl::enabled() {
        let (width, height) = unsafe { (glutGet(0x66).max(1), glutGet(0x67).max(1)) };
        for gun in 0..jamma::players() {
            let (x, y) = jamma::aim_position(gun);
            unsafe { crosshair_gl::draw(gun, x, y, width, height) };
        }
    }
    if real != 0 {
        let real: extern "C" fn() = unsafe { std::mem::transmute(real) };
        real();
    }
}

/// Controllers are polled from a GLUT timer, so aim callbacks run on the
/// GLUT thread exactly like the mouse callbacks.
#[cfg(target_arch = "x86")]
extern "C" fn controller_frame(_value: c_int) {
    jamma::controller_frame();
    unsafe { glutTimerFunc(16, controller_frame, 0) };
}

#[cfg(target_arch = "x86")]
extern "C" fn mouse(button: c_int, state: c_int, x: c_int, y: c_int) {
    let (x, y) = game_coordinates(x, y);
    jamma::mouse(button, state == 0, x, y);
}
#[cfg(target_arch = "x86")]
extern "C" fn motion(x: c_int, y: c_int) {
    let (x, y) = game_coordinates(x, y);
    jamma::aim(x, y, false);
}

#[cfg(target_arch = "x86")]
fn game_coordinates(x: c_int, y: c_int) -> (c_int, c_int) {
    let width = unsafe { glutGet(0x66) }.max(1);
    let height = unsafe { glutGet(0x67) }.max(1);
    (
        (i64::from(x) * 640 / i64::from(width)).clamp(0, 639) as c_int,
        (i64::from(y) * 480 / i64::from(height)).clamp(0, 479) as c_int,
    )
}
