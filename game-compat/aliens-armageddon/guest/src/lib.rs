// The game is 32-bit Linux; on other targets this crate is empty.
#![cfg(all(target_os = "linux", target_arch = "x86"))]
#![allow(dead_code)]
use banana_input::pad;
#[path = "../../../../guest/linux-shim/src/audio.rs"]
mod audio;
#[path = "../../../../guest/linux-shim/src/crosshair_gl.rs"]
mod crosshair_gl;
#[path = "../../../../guest/linux-shim/src/display.rs"]
mod display;
#[path = "../../../../guest/linux-shim/src/evdev_pad.rs"]
mod evdev_pad;
#[path = "../../../../guest/linux-shim/src/legacy_csv.rs"]
mod legacy_csv;
use banana_input::crosshair;
mod aliens;
mod aliens_exit;
extern "C" fn initialize() {
    aliens_exit::start();
}
#[used]
#[unsafe(link_section = ".init_array")]
static INITIALIZE: extern "C" fn() = initialize;

#[cfg(target_arch = "x86")]
#[path = "../../../../guest/linux-shim/src/box32_abi.rs"]
mod box32_abi;

#[cfg(target_arch = "x86")]
#[path = "../../../../guest/linux-shim/src/android_threads.rs"]
mod android_threads;
