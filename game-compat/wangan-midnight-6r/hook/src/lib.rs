//! Wangan Midnight Maximum Tune 6R cabinet devices: a freestanding 64-bit
//! Windows DLL that takes `bngrw.dll`'s place in the game's view. The plan
//! points the game's HASP and USB-finder imports at it too (see the plan
//! crate's `image.rs`), so it serves:
//!
//! - the HASP dongle API (`hasp.rs`),
//! - the USB dongle finder and the card reader API (both report no device),
//! - a JVS I/O board on COM3 (`board.rs`), fed from the keyboard and
//!   XInput/DirectInput controllers (`controls.rs`).
//!
//! On other targets only the protocol code is built, for its tests.
#![cfg_attr(all(windows, target_arch = "x86_64"), no_std)]

pub mod board;
pub mod game_server;
pub mod pki;
pub mod tls;
pub mod terminal;
#[cfg(all(windows, target_arch = "x86_64"))]
mod hook;
