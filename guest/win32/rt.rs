//! Symbols a freestanding i686 MSVC-target DLL must provide itself (normally
//! from the MSVC CRT). Volatile accesses stop LLVM from turning these loops
//! back into calls to themselves.
use core::ffi::c_void;
use core::ptr::{read_volatile, write_volatile};

#[unsafe(no_mangle)]
pub static _fltused: i32 = 0;

#[unsafe(no_mangle)]
pub unsafe extern "C" fn memcpy(dest: *mut c_void, src: *const c_void, n: usize) -> *mut c_void {
    let (d, s) = (dest.cast::<u8>(), src.cast::<u8>());
    for i in 0..n {
        unsafe { write_volatile(d.add(i), read_volatile(s.add(i))) };
    }
    dest
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn memmove(dest: *mut c_void, src: *const c_void, n: usize) -> *mut c_void {
    if (dest as usize) <= (src as usize) {
        return unsafe { memcpy(dest, src, n) };
    }
    let (d, s) = (dest.cast::<u8>(), src.cast::<u8>());
    for i in (0..n).rev() {
        unsafe { write_volatile(d.add(i), read_volatile(s.add(i))) };
    }
    dest
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn memset(dest: *mut c_void, value: i32, n: usize) -> *mut c_void {
    let d = dest.cast::<u8>();
    for i in 0..n {
        unsafe { write_volatile(d.add(i), value as u8) };
    }
    dest
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn memcmp(a: *const c_void, b: *const c_void, n: usize) -> i32 {
    let (a, b) = (a.cast::<u8>(), b.cast::<u8>());
    for i in 0..n {
        let (x, y) = unsafe { (read_volatile(a.add(i)), read_volatile(b.add(i))) };
        if x != y {
            return i32::from(x) - i32::from(y);
        }
    }
    0
}

/// Referenced only by core's unwinding/u128 formatting paths, which this
/// panic=abort library never reaches (it uses no formatting or 64-bit division).
#[unsafe(no_mangle)]
pub extern "C" fn __CxxFrameHandler3() -> i32 {
    unreachable_abort()
}
#[unsafe(no_mangle)]
pub extern "C" fn _aulldiv() -> u64 {
    unreachable_abort()
}
#[unsafe(no_mangle)]
pub extern "C" fn _aullrem() -> u64 {
    unreachable_abort()
}

fn unreachable_abort() -> ! {
    #[cfg_attr(
        target_arch = "x86",
        link(name = "kernel32", kind = "raw-dylib", import_name_type = "undecorated")
    )]
    #[cfg_attr(not(target_arch = "x86"), link(name = "kernel32", kind = "raw-dylib"))]
    unsafe extern "system" {
        fn ExitProcess(code: u32) -> !;
    }
    unsafe { ExitProcess(4) }
}
