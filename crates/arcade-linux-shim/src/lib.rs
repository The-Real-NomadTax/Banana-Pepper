//! Small guest-side compatibility layer; build for i686-unknown-linux-gnu.
#[cfg(target_arch = "x86")]
mod aliens;
#[cfg(any(target_arch = "x86", test))]
mod inputs;
#[cfg(target_arch = "x86")]
mod jamma;
#[cfg(target_arch = "x86")]
mod jurassic;

#[cfg(target_arch = "x86")]
mod audio;
mod display;
mod legacy_csv;

use std::ffi::{c_char, c_int};

#[link(name = "glut")]
unsafe extern "C" {
    fn glutInitWindowSize(width: c_int, height: c_int);
    fn glutCreateWindow(title: *const c_char) -> c_int;
    fn glutDestroyWindow(window: c_int);
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
}

use std::sync::atomic::{AtomicI32, Ordering};
static WINDOW: AtomicI32 = AtomicI32::new(0);

/// Cabinet fullscreen mode switching is replaced with a desktop GL window.
#[unsafe(no_mangle)]
pub extern "C" fn glutEnterGameMode() -> c_int {
    unsafe {
        #[cfg(target_arch = "x86")]
        jamma::install();
        glutInitWindowSize(640, 480);
        let id = glutCreateWindow(c"Arcade Foundry - Big Buck Hunter Pro".as_ptr());
        WINDOW.store(id, Ordering::Relaxed);
        #[cfg(target_arch = "x86")]
        {
            glutKeyboardFunc(key_down);
            glutKeyboardUpFunc(key_up);
            glutMouseFunc(mouse);
            glutMotionFunc(motion);
            glutPassiveMotionFunc(motion);
        }
        id
    }
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
