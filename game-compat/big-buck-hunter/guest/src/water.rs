//! Water frontend: the game's preload adapter for Water (`AdapterV2`). Water
//! runs the game and supplies GLUT; like the Linux preload, this adapter
//! only overrides the shared GLUT functions (`glut.rs`) and patches the
//! JAMMA board in. Controllers are read through XInput and DirectInput,
//! mice and light guns through Raw Input on Water's window.
#![allow(unsafe_op_in_unsafe_fn)]
use crate::{glut, jamma, pad};
use banana_water::extension::{ADAPTER_VERSION, AdapterV2, HostServicesV2};
use std::{
    ffi::{CStr, c_char, c_void},
    sync::OnceLock,
};
#[path = "../../../../guest/win32/controllers.rs"]
#[allow(dead_code)]
mod controllers;
#[path = "../../../../guest/win32/dinput.rs"]
mod dinput;
#[path = "water/probe.rs"]
mod probe;
#[path = "water/rawinput.rs"]
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

static HOST: OnceLock<&'static HostServicesV2> = OnceLock::new();
fn host() -> &'static HostServicesV2 {
    HOST.get().expect("Water initialized the adapter")
}

/// Water's own implementation of a library function (the real GLUT).
pub fn next(name: &CStr) -> usize {
    unsafe { (host().next)(name.as_ptr()) }
}

/// Controllers are idle while another window has the keyboard.
pub fn focused() -> bool {
    let window = unsafe { (host().window)() };
    window != 0
        && unsafe { windows_sys::Win32::UI::WindowsAndMessaging::GetForegroundWindow() } as isize
            == window
}

/// Raw Input on the new window, and the diagnostic probe when asked for.
pub fn window_created() {
    unsafe {
        (host().message_hook)(message);
        rawinput::register((host().window)() as *mut c_void);
    }
    probe::start();
}

/// Save the current frame in the session state.
pub fn capture(name: &CStr) {
    unsafe { (host().capture)(name.as_ptr()) }
}

pub fn install() {}

unsafe extern "C" fn message(_window: isize, message: u32, _w: usize, l: isize) -> i32 {
    if message == windows_sys::Win32::UI::WindowsAndMessaging::WM_INPUT {
        rawinput::input(l);
    }
    0
}

unsafe extern "C" fn initialize(
    _root: *const c_char,
    _state: *const c_char,
    host: *const HostServicesV2,
) -> i32 {
    let Some(host) = host.as_ref() else { return -1 };
    if host.size != std::mem::size_of::<HostServicesV2>() as u32 || HOST.set(host).is_err() {
        return -1;
    }
    0
}
unsafe extern "C" fn resolve(name: *const c_char) -> usize {
    match CStr::from_ptr(name).to_bytes() {
        b"glutEnterGameMode" => glut::enter_game_mode as *const () as usize,
        b"glutLeaveGameMode" => glut::leave_game_mode as *const () as usize,
        b"glutSwapBuffers" => glut::swap_buffers as *const () as usize,
        _ => 0,
    }
}
unsafe extern "C" fn patch() -> i32 {
    match jamma::install() {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("[arcade] {e}");
            -1
        }
    }
}
static ADAPTER: AdapterV2 = AdapterV2 {
    version: ADAPTER_VERSION,
    size: std::mem::size_of::<AdapterV2>() as u32,
    initialize,
    resolve,
    install: patch,
};
#[unsafe(no_mangle)]
pub extern "C" fn banana_water_adapter_v2() -> *const AdapterV2 {
    &ADAPTER
}
