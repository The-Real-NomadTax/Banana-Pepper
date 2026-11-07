//! Shared input: keyboard/controller bindings (`controls.cfg`), the
//! XInput-style controller model and light-gun crosshairs.
//!
//! Game layouts are declared by game compatibility crates and registered
//! through [`controls::install`]; this crate names no individual game.
pub mod controls;
pub mod crosshair;
#[cfg(target_os = "linux")]
pub mod evdev;
pub mod pad;

pub mod transport;
