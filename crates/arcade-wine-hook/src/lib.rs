//! Super Cars Wine input adapter: a freestanding 32-bit Windows DLL (see
//! `hook.rs`). On any other target this crate is empty, so host workspace
//! builds and tests are unaffected.
#![cfg_attr(all(windows, target_arch = "x86"), no_std)]

#[cfg(all(windows, target_arch = "x86"))]
mod hook;
