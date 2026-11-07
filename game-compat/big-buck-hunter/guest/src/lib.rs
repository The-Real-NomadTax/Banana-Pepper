//! Big Buck Hunter Pro's in-process patch, written once for the game's own
//! platform (32-bit Linux) and built for every way the game runs:
//!
//! - Linux and Android: `libarcade_big_buck.so`, preloaded into the game
//!   (`linux.rs`: GLUT window and callbacks, evdev controllers).
//! - Windows: `arcade_big_buck.dll`, loaded by Water next to the game
//!   (`water.rs`: Water's window and messages, XInput, DirectInput and Raw
//!   Input).
//!
//! The game patch itself — the virtual JAMMA board, switch mapping, guns
//! and controller aiming — is `jamma.rs`, shared by both.
#![allow(dead_code)]
use banana_input::pad;
mod inputs;
mod jamma;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
use linux as platform;
#[cfg(all(windows, target_arch = "x86"))]
mod water;
#[cfg(all(windows, target_arch = "x86"))]
use water as platform;
/// Hosts the game never runs on (unit tests of the shared patch).
#[cfg(not(any(target_os = "linux", all(windows, target_arch = "x86"))))]
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
#[cfg(target_os = "linux")]
use banana_input::crosshair;
#[cfg(target_os = "linux")]
#[path = "../../../../guest/linux-shim/src/audio.rs"]
mod audio;
#[cfg(target_os = "linux")]
#[path = "../../../../guest/linux-shim/src/crosshair_gl.rs"]
mod crosshair_gl;
#[cfg(target_os = "linux")]
#[path = "../../../../guest/linux-shim/src/display.rs"]
mod display;
#[cfg(target_os = "linux")]
#[path = "../../../../guest/linux-shim/src/evdev_pad.rs"]
mod evdev_pad;
#[cfg(target_os = "linux")]
#[path = "../../../../guest/linux-shim/src/legacy_csv.rs"]
mod legacy_csv;
#[cfg(all(target_os = "linux", target_arch = "x86"))]
#[path = "../../../../guest/linux-shim/src/box32_abi.rs"]
mod box32_abi;
#[cfg(all(target_os = "linux", target_arch = "x86"))]
#[path = "../../../../guest/linux-shim/src/android_threads.rs"]
mod android_threads;
