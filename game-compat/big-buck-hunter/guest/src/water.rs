//! Water frontend: the versioned Water adapter (`AdapterV1`). Water owns the
//! window and forwards its keyboard, mouse and Raw Input messages here;
//! controllers are read through XInput and DirectInput.
#![allow(unsafe_op_in_unsafe_fn)]
use crate::{jamma, pad};
use banana_water::extension::{AdapterV1, DisplayV1, HostServicesV1};
use std::{ffi::c_void, sync::OnceLock};
#[path = "../../../../guest/win32/controllers.rs"]
#[allow(dead_code)]
mod controllers;
#[path = "../../../../guest/win32/dinput.rs"]
mod dinput;
mod probe;
mod rawinput;
#[path = "../../../../guest/win32/xinput.rs"]
mod xinput;

/// XInput and DirectInput controllers plus Raw Input mice and light guns.
pub struct Devices {
    connected: u32,
}
impl Devices {
    pub fn new(inputs: &pad::Inputs) -> Self {
        if inputs.uses(pad::Method::XInput) && !xinput::available() {
            eprintln!("[arcade] XInput unavailable; controllers disabled");
        }
        Self { connected: 0 }
    }
    pub fn read_players(&mut self, inputs: &pad::Inputs) -> [pad::Pad; pad::MAX_PLAYERS] {
        let [c1, c2] = controllers::read_players(inputs);
        let [r1, r2] = unsafe { rawinput::read_players(inputs) };
        if controllers::connected() != self.connected {
            self.connected = controllers::connected();
            eprintln!(
                "[arcade] {} controller(s) connected (XInput + DirectInput)",
                self.connected
            );
        }
        [c1.merge(r1), c2.merge(r2)]
    }
}

static HOST: OnceLock<&'static HostServicesV1> = OnceLock::new();
unsafe extern "C" fn initialize(
    _root: *const i8,
    _state: *const i8,
    host: *const HostServicesV1,
) -> i32 {
    let Some(host) = host.as_ref() else { return -1 };
    if host.size != std::mem::size_of::<HostServicesV1>() as u32 || HOST.set(host).is_err() {
        return -1;
    }
    0
}
unsafe extern "C" fn install() -> i32 {
    match jamma::install() {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("[arcade] {e}");
            -1
        }
    }
}
unsafe extern "C" fn resolve(_name: *const i8) -> usize {
    0
}
unsafe extern "C" fn title() -> *const i8 {
    c"Banana pepper - Big Buck Hunter Pro".as_ptr()
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
static DISPLAY: DisplayV1 = DisplayV1 {
    size: std::mem::size_of::<DisplayV1>() as u32,
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
unsafe extern "C" fn display() -> *const DisplayV1 {
    &DISPLAY
}
static ADAPTER: AdapterV1 = AdapterV1 {
    version: 1,
    size: std::mem::size_of::<AdapterV1>() as u32,
    initialize,
    install,
    resolve,
    display,
};
#[unsafe(no_mangle)]
pub extern "C" fn banana_water_adapter_v1() -> *const AdapterV1 {
    &ADAPTER
}
