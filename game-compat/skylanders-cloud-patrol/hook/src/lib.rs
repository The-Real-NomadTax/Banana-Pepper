//! Skylanders Cloud Patrol touch adapter: a freestanding 32-bit Windows DLL
//! that replaces the PQ Labs MultiTouch client (see `hook.rs`). On any other
//! target this crate is empty, so host workspace builds and tests are
//! unaffected.
#![cfg_attr(all(windows, target_arch = "x86"), no_std)]
// The library keeps the original DLL's name.
#![allow(non_snake_case)]

#[cfg(all(windows, target_arch = "x86"))]
mod hook;
