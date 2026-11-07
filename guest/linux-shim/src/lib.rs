//! Reusable Linux guest services. Title patches live in game-compat guest crates.
#![cfg(target_os = "linux")]
pub use banana_input::{crosshair, pad};
pub mod audio;
pub mod crosshair_gl;
pub mod display;
pub mod evdev_pad;
pub mod legacy_csv;

#[cfg(target_arch = "x86")]
pub mod android_threads;
#[cfg(target_arch = "x86")]
pub mod box32_abi;
