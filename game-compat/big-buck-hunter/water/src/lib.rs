#![cfg(all(windows, target_arch = "x86"))]
#![allow(unsafe_op_in_unsafe_fn)]
#[path = "../../../../guest/win32/controllers.rs"]
#[allow(dead_code)]
mod controllers;
#[path = "../../../../guest/win32/dinput.rs"]
mod dinput;
mod jamma;
mod probe;
mod rawinput;
#[path = "../../../../guest/win32/xinput.rs"]
mod xinput;
use banana_input::pad;
use std::{
    ffi::{CString, c_void},
    ptr,
    sync::OnceLock,
};
static HOST: OnceLock<&'static banana_water::extension::HostServicesV1> = OnceLock::new();
unsafe extern "C" fn initialize(
    _root: *const i8,
    _state: *const i8,
    host: *const banana_water::extension::HostServicesV1,
) -> i32 {
    let Some(host) = host.as_ref() else { return -1 };
    if host.size != std::mem::size_of::<banana_water::extension::HostServicesV1>() as u32
        || HOST.set(host).is_err()
    {
        return -1;
    }
    0
}
unsafe extern "C" fn install() -> i32 {
    match unsafe { jamma::install() } {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("{e}");
            -1
        }
    }
}
unsafe extern "C" fn resolve(_name: *const i8) -> usize {
    0
}
unsafe extern "C" fn title() -> *const i8 {
    c"Banana pepper - Big Buck Hunter Pro (Water)".as_ptr()
}
unsafe extern "C" fn players() -> usize {
    jamma::players()
}
unsafe extern "C" fn aim_position(p: usize, x: *mut i32, y: *mut i32) {
    let (a, b) = jamma::aim_position(p);
    *x = a;
    *y = b;
}
unsafe extern "C" fn release() {
    jamma::release();
}
unsafe extern "C" fn key(k: u8, d: bool) {
    jamma::key(k, d);
}
unsafe extern "C" fn aim(x: i32, y: i32, s: bool) {
    jamma::aim(x, y, s);
}
unsafe extern "C" fn mouse(b: i32, d: bool, x: i32, y: i32) {
    jamma::mouse(b, d, x, y);
}
unsafe extern "C" fn controller_frame(f: bool) {
    jamma::controller_frame(f);
}
unsafe extern "C" fn register_raw(w: usize) {
    rawinput::register(w as *mut c_void);
}
unsafe extern "C" fn raw_input(l: isize) {
    rawinput::input(l);
}
static DISPLAY: banana_water::extension::DisplayV1 = banana_water::extension::DisplayV1 {
    size: std::mem::size_of::<banana_water::extension::DisplayV1>() as u32,
    width: 640,
    height: 480,
    title,
    players,
    aim_position,
    release,
    key,
    aim,
    mouse,
    controller_frame,
    register_raw,
    raw_input,
    probe: probe::probe,
};
unsafe extern "C" fn display() -> *const banana_water::extension::DisplayV1 {
    &DISPLAY
}
static ADAPTER: banana_water::extension::AdapterV1 = banana_water::extension::AdapterV1 {
    version: 1,
    size: std::mem::size_of::<banana_water::extension::AdapterV1>() as u32,
    initialize,
    install,
    resolve,
    display,
};
#[unsafe(no_mangle)]
pub extern "C" fn banana_water_adapter_v1() -> *const banana_water::extension::AdapterV1 {
    &ADAPTER
}
