//! Big Buck Hunter Pro's in-process patch, written once for the game's own
//! platform (32-bit Linux) and built for every way the game runs:
//!
//! - Linux and Android: `libarcade_big_buck.so`, preloaded into the game
//!   (`linux.rs`: evdev controllers).
//! - Windows: `arcade_big_buck.dll`, Water's preload adapter for the game
//!   (`water.rs`: XInput, DirectInput and Raw Input).
//!
//! Everything else is shared: the virtual JAMMA board, switch mapping, guns
//! and controller aiming (`jamma.rs`), and the GLUT window, input callbacks
//! and crosshairs (`glut.rs`), which override the same GLUT functions on
//! both platforms.
#![allow(dead_code)]
use banana_input::pad;
mod inputs;
mod jamma;

#[cfg(all(any(target_os = "linux", windows), target_arch = "x86"))]
mod glut;
#[cfg(all(any(target_os = "linux", windows), target_arch = "x86"))]
use banana_input::crosshair;
#[cfg(all(any(target_os = "linux", windows), target_arch = "x86"))]
#[path = "../../../../guest/linux-shim/src/crosshair_gl.rs"]
mod crosshair_gl;
#[cfg(all(target_os = "linux", target_arch = "x86"))]
mod linux;
#[cfg(all(target_os = "linux", target_arch = "x86"))]
use linux as platform;
#[cfg(all(windows, target_arch = "x86"))]
mod water;
#[cfg(all(windows, target_arch = "x86"))]
use water as platform;
/// Targets the game never runs on (unit tests of the shared patch).
#[cfg(not(all(any(target_os = "linux", windows), target_arch = "x86")))]
mod platform {
    use crate::pad;
    pub struct Devices;
    impl Devices {
        pub fn new(_inputs: &pad::Inputs) -> Self {
            Self
        }
        pub fn read_players(&mut self, _inputs: &pad::Inputs) -> [pad::Pad; pad::MAX_PLAYERS] {
            [pad::Pad::default(); pad::MAX_PLAYERS]
        }
    }
}

// Linux guest-side services shared with the other Linux titles.
#[cfg(all(target_os = "linux", target_arch = "x86"))]
#[path = "../../../../guest/linux-shim/src/audio.rs"]
mod audio;
#[cfg(all(target_os = "linux", target_arch = "x86"))]
#[path = "../../../../guest/linux-shim/src/display.rs"]
mod display;
#[cfg(all(target_os = "linux", target_arch = "x86"))]
#[path = "../../../../guest/linux-shim/src/evdev_pad.rs"]
mod evdev_pad;
#[cfg(all(target_os = "linux", target_arch = "x86"))]
#[path = "../../../../guest/linux-shim/src/legacy_csv.rs"]
mod legacy_csv;
#[cfg(all(target_os = "linux", target_arch = "x86"))]
#[path = "../../../../guest/linux-shim/src/box32_abi.rs"]
mod box32_abi;
#[cfg(all(target_os = "linux", target_arch = "x86"))]
#[path = "../../../../guest/linux-shim/src/android_threads.rs"]
mod android_threads;

const JAMMA_HOOKS: [usize; 4] = [0x080d9ce5, 0x080d9e81, 0x080dbf32, 0x080da943];
const WINDOW_TITLE: &std::ffi::CStr = c"Banana pepper - Big Buck Hunter Pro";

fn install_extra() -> Result<(), String> {
    Ok(())
}
