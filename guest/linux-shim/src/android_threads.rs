//! Cabinet file workers request real-time scheduling unavailable to Android apps.
use std::ffi::{c_int, c_void};
unsafe extern "C" {
    fn pthread_attr_getdetachstate(attributes: *const libc::pthread_attr_t, state: *mut c_int) -> c_int;
}

type Start = unsafe extern "C" fn(*mut c_void) -> *mut c_void;
type Create = unsafe extern "C" fn(
    *mut libc::pthread_t, *const libc::pthread_attr_t, Start, *mut c_void,
) -> c_int;

/// Build independent attributes: Box32 attributes contain an owned native pointer
/// and must never be copied as bytes. Preserve the caller's stack and detach mode.
unsafe fn ordinary_attributes(
    source: *const libc::pthread_attr_t,
    target: *mut libc::pthread_attr_t,
) -> c_int {
    let result = unsafe { libc::pthread_attr_init(target) };
    if result != 0 { return result; }
    let mut detach = 0;
    let mut guard = 0;
    let mut stack = std::ptr::null_mut();
    let mut size = 0;
    let result = unsafe {
        (|| {
            for result in [
                pthread_attr_getdetachstate(source, &mut detach),
                libc::pthread_attr_getguardsize(source, &mut guard),
                libc::pthread_attr_getstack(source, &mut stack, &mut size),
                libc::pthread_attr_setinheritsched(target, libc::PTHREAD_INHERIT_SCHED),
            ] {
                if result != 0 { return result; }
            }
            let result = libc::pthread_attr_setdetachstate(target, detach);
            if result != 0 { return result; }
            let result = libc::pthread_attr_setguardsize(target, guard);
            if result != 0 { return result; }
            if !stack.is_null() {
                libc::pthread_attr_setstack(target, stack, size)
            } else if size != 0 {
                libc::pthread_attr_setstacksize(target, size)
            } else { 0 }
        })()
    };
    if result != 0 { unsafe { libc::pthread_attr_destroy(target) }; }
    result
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn pthread_create(
    thread: *mut libc::pthread_t,
    attributes: *const libc::pthread_attr_t,
    start: Start,
    argument: *mut c_void,
) -> c_int {
    static ORIGINAL: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    let next = *ORIGINAL.get_or_init(|| unsafe {
        // Box32's RTLD_NEXT lookup can return this interposer again. An explicit
        // library handle selects the wrapped pthread implementation instead.
        let handle = libc::dlopen(c"libpthread.so.0".as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL);
        if handle.is_null() { return 0; }
        let next = libc::dlsym(handle, c"pthread_create".as_ptr());
        libc::dlclose(handle);
        if next == pthread_create as *const () as *mut c_void { 0 } else { next as usize }
    });
    if next == 0 { return libc::ENOSYS; }
    let create: Create = unsafe { std::mem::transmute(next) };
    if attributes.is_null()
        || !std::env::var_os("ARCADE_ANDROID").is_some_and(|s| s == "1") {
        return unsafe { create(thread, attributes, start, argument) };
    }
    let mut policy = 0;
    let policy_result = unsafe { libc::pthread_attr_getschedpolicy(attributes, &mut policy) };
    if policy_result != 0
        || ![libc::SCHED_FIFO, libc::SCHED_RR].contains(&policy) {
        return unsafe { create(thread, attributes, start, argument) };
    }
    let mut fallback = std::mem::MaybeUninit::<libc::pthread_attr_t>::uninit();
    let error = unsafe { ordinary_attributes(attributes, fallback.as_mut_ptr()) };
    if error != 0 { return error; }
    let result = unsafe { create(thread, fallback.as_ptr(), start, argument) };
    unsafe { libc::pthread_attr_destroy(fallback.as_mut_ptr()) };
    // Box32 currently retains allocations when native pthread_create fails.
    // Normalize the known unsupported Android request before that allocation.
    static NOTICE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if result != 0 || !NOTICE.swap(true, std::sync::atomic::Ordering::Relaxed) {
        eprintln!("[arcade] Android real-time thread uses ordinary scheduling: {result}");
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn explicit_lookup_starts_and_joins_a_real_thread() {
        unsafe extern "C" fn start(argument: *mut c_void) -> *mut c_void { argument }
        unsafe {
            let mut thread = std::mem::MaybeUninit::uninit();
            let argument = 123usize as *mut c_void;
            assert_eq!(pthread_create(thread.as_mut_ptr(), std::ptr::null(), start, argument), 0);
            let mut result = std::ptr::null_mut();
            assert_eq!(libc::pthread_join(thread.assume_init(), &mut result), 0);
            assert_eq!(result, argument);
        }
    }
    #[test]
    fn fallback_keeps_stack_and_detach_without_mutating_original() {
        unsafe {
            let mut source = std::mem::MaybeUninit::uninit();
            assert_eq!(libc::pthread_attr_init(source.as_mut_ptr()), 0);
            let source = source.as_mut_ptr();
            assert_eq!(libc::pthread_attr_setdetachstate(source, libc::PTHREAD_CREATE_DETACHED), 0);
            assert_eq!(libc::pthread_attr_setstacksize(source, 2 * 1024 * 1024), 0);
            assert_eq!(libc::pthread_attr_setguardsize(source, 8192), 0);
            assert_eq!(libc::pthread_attr_setschedpolicy(source, libc::SCHED_FIFO), 0);
            assert_eq!(libc::pthread_attr_setinheritsched(source, libc::PTHREAD_EXPLICIT_SCHED), 0);
            let mut target = std::mem::MaybeUninit::uninit();
            assert_eq!(ordinary_attributes(source, target.as_mut_ptr()), 0);
            let mut value = 0;
            let mut size = 0;
            assert_eq!(pthread_attr_getdetachstate(target.as_ptr(), &mut value), 0);
            assert_eq!(value, libc::PTHREAD_CREATE_DETACHED);
            assert_eq!(libc::pthread_attr_getstacksize(target.as_ptr(), &mut size), 0);
            assert_eq!(size, 2 * 1024 * 1024);
            assert_eq!(libc::pthread_attr_getguardsize(target.as_ptr(), &mut size), 0);
            assert_eq!(size, 8192);
            assert_eq!(libc::pthread_attr_getinheritsched(target.as_ptr(), &mut value), 0);
            assert_eq!(value, libc::PTHREAD_INHERIT_SCHED);
            assert_eq!(libc::pthread_attr_getinheritsched(source, &mut value), 0);
            assert_eq!(value, libc::PTHREAD_EXPLICIT_SCHED);
            libc::pthread_attr_destroy(target.as_mut_ptr());
            libc::pthread_attr_destroy(source);
        }
    }
}
