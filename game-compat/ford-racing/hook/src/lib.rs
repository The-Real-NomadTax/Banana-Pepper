//! Ford Racing desktop cabinet adapter, using the game's imported Out32 DLL.
//! No kernel driver, third-party loader, or physical port access.
#![cfg_attr(all(windows, target_arch = "x86"), no_std)]
#[cfg(all(windows, target_arch = "x86"))]
mod hook;
