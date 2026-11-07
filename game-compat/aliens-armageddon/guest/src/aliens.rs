//! Desktop graphics adapters for Aliens' statically linked SDL 1.2 engine.
use std::ffi::{c_char, c_void};
use std::sync::atomic::{AtomicBool, Ordering};

#[link(name = "dl")]
unsafe extern "C" {
    fn dlsym(handle: *mut c_void, name: *const c_char) -> *mut c_void;
}
#[link(name = "GL")]
unsafe extern "C" {
    fn glXGetCurrentDisplay() -> *mut c_void;
    fn glXGetCurrentDrawable() -> libc::c_ulong;
}

// G6 sends zero/negative values through the old positive-only SGI interface,
// sometimes before a context exists. Modern NVIDIA rejects that X request.
#[unsafe(no_mangle)]
pub extern "C" fn glXSwapIntervalSGI(interval: i32) -> i32 {
    if !std::env::var_os("ARCADE_ALIENS").is_some_and(|v| v == "1") {
        let next = unsafe { dlsym(libc::RTLD_NEXT, c"glXSwapIntervalSGI".as_ptr()) };
        if next.is_null() {
            return -1;
        }
        let real: unsafe extern "C" fn(i32) -> i32 = unsafe { std::mem::transmute(next) };
        return unsafe { real(interval) };
    }
    static LOGGED: AtomicBool = AtomicBool::new(false);
    if !LOGGED.swap(true, Ordering::Relaxed) {
        eprintln!("[aliens] using context-aware EXT swap control for legacy SGI requests");
    }
    let display = unsafe { glXGetCurrentDisplay() };
    let drawable = unsafe { glXGetCurrentDrawable() };
    if !display.is_null() && drawable != 0 {
        let next = unsafe { dlsym(libc::RTLD_NEXT, c"glXSwapIntervalEXT".as_ptr()) };
        if !next.is_null() {
            let real: unsafe extern "C" fn(*mut c_void, libc::c_ulong, i32) =
                unsafe { std::mem::transmute(next) };
            unsafe {
                real(display, drawable, interval.clamp(0, 4));
            }
        }
    }
    0
}
