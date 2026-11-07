//! Versioned C ABI between the generic Water worker and an optional preload
//! adapter: Water's `LD_PRELOAD`. Water runs a Linux program by itself; an
//! adapter only changes it, exactly as a preloaded library would on Linux:
//! it may replace any library function the program imports (and call
//! Water's own implementation through [`HostServicesV2::next`]), patch the
//! program's code once it is loaded, and observe the window's messages.
//!
//! The adapter exports `banana_water_adapter_v2`, returning [`AdapterV2`].
use std::ffi::c_char;

/// Services Water offers an adapter. Sizes are checked before use.
#[repr(C)]
pub struct HostServicesV2 {
    pub size: u32,
    /// Water's implementation of a library function (the next definition,
    /// like `dlsym(RTLD_NEXT, name)`), or 0 when Water has none.
    pub next: unsafe extern "C" fn(*const c_char) -> usize,
    /// Save the current frame as a PNG named `name` in the session state.
    pub capture: unsafe extern "C" fn(*const c_char),
    /// The program's window (`HWND`), or 0 before it has one.
    pub window: unsafe extern "C" fn() -> isize,
    /// Observe every message of the program's window before Water handles
    /// it (`hwnd, message, wparam, lparam`); a nonzero result consumes it.
    pub message_hook: unsafe extern "C" fn(MessageHook),
}
pub type MessageHook = unsafe extern "C" fn(isize, u32, usize, isize) -> i32;

/// An adapter. Every entry is required; return 0 for "nothing to do".
#[repr(C)]
pub struct AdapterV2 {
    pub version: u32,
    pub size: u32,
    /// Before the program is loaded: the guest root (e.g. `/game`), the
    /// session state directory and Water's services. Nonzero fails the launch.
    pub initialize:
        unsafe extern "C" fn(*const c_char, *const c_char, *const HostServicesV2) -> i32,
    /// The adapter's replacement for an imported function, or 0 to keep
    /// Water's. Asked first for every import, like a preloaded library.
    pub resolve: unsafe extern "C" fn(*const c_char) -> usize,
    /// After the program is mapped and relocated, before it runs: code
    /// patches go here. Nonzero fails the launch.
    pub install: unsafe extern "C" fn() -> i32,
}
pub const ADAPTER_VERSION: u32 = 2;
/// The adapter DLL's exported factory.
pub const ADAPTER_ENTRY: &str = "banana_water_adapter_v2";
