//! Respect the requested desktop DISPLAY even when the cabinet supplies :0.0.
use std::ffi::{CStr, CString, c_char, c_int, c_void};

#[link(name = "dl")]
unsafe extern "C" {
    fn dlsym(handle: *mut c_void, name: *const c_char) -> *mut c_void;
}

/// # Safety
/// `argc` and `argv` must satisfy GLUT's normal C initialization contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn glutInit(argc: *mut c_int, argv: *mut *mut c_char) {
    let next = unsafe { dlsym((-1isize) as *mut c_void, c"glutInit".as_ptr()) };
    if next.is_null() {
        eprintln!("[arcade] cannot locate real glutInit");
        std::process::exit(1);
    }
    let real: unsafe extern "C" fn(*mut c_int, *mut *mut c_char) =
        unsafe { std::mem::transmute(next) };
    let display = std::env::var("DISPLAY")
        .ok()
        .and_then(|d| CString::new(d).ok());
    let mut replaced = None;
    if !argc.is_null()
        && !argv.is_null()
        && let Some(display) = &display
    {
        let count = unsafe { *argc };
        if (1..1024).contains(&count) {
            for i in 0..count as usize - 1 {
                let value = unsafe { *argv.add(i) };
                if !value.is_null() && unsafe { CStr::from_ptr(value) }.to_bytes() == b"-display" {
                    let slot = unsafe { argv.add(i + 1) };
                    replaced = Some((slot, unsafe { *slot }));
                    unsafe {
                        *slot = display.as_ptr().cast_mut();
                    }
                    break;
                }
            }
        }
    }
    unsafe {
        real(argc, argv);
    }
    if let Some((slot, original)) = replaced {
        unsafe {
            *slot = original;
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn glutSetCursor(_cursor: c_int) {
    let next = unsafe { dlsym((-1isize) as *mut c_void, c"glutSetCursor".as_ptr()) };
    if !next.is_null() {
        let real: unsafe extern "C" fn(c_int) = unsafe { std::mem::transmute(next) };
        unsafe {
            real(1);
        } // Visible arrow for mouse testing.
    }
}
