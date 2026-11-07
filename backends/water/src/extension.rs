//! Versioned C ABI between a generic Water worker and a title-owned adapter.
use std::ffi::c_char;
#[repr(C)]
pub struct HostServicesV1 {
    pub size: u32,
    pub capture: unsafe extern "C" fn(*const c_char),
}
#[repr(C)]
pub struct DisplayV1 {
    pub size: u32,
    pub width: i32,
    pub height: i32,
    pub title: unsafe extern "C" fn() -> *const c_char,
    pub players: unsafe extern "C" fn() -> usize,
    pub aim_position: unsafe extern "C" fn(usize, *mut i32, *mut i32),
    pub release: unsafe extern "C" fn(),
    pub key: unsafe extern "C" fn(u8, bool),
    pub aim: unsafe extern "C" fn(i32, i32, bool),
    pub mouse: unsafe extern "C" fn(i32, bool, i32, i32),
    pub controller_frame: unsafe extern "C" fn(bool),
    pub register_raw: unsafe extern "C" fn(usize),
    pub raw_input: unsafe extern "C" fn(isize),
    pub probe: unsafe extern "C" fn(u64),
}
#[repr(C)]
pub struct AdapterV1 {
    pub version: u32,
    pub size: u32,
    pub initialize:
        unsafe extern "C" fn(*const c_char, *const c_char, *const HostServicesV1) -> i32,
    pub install: unsafe extern "C" fn() -> i32,
    pub resolve: unsafe extern "C" fn(*const c_char) -> usize,
    pub display: unsafe extern "C" fn() -> *const DisplayV1,
}
