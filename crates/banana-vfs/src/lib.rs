//! Shared composed filesystems, persistent overlays and their exposure to
//! guests: the [`wire`] protocol for intercepting backends (Water, the
//! Windows file shim), FUSE connections served into a session's own Linux
//! namespace, and the banana-tracer protocol on Android ([`rootless`]).
//! Nothing here mounts on the host.
mod view;
pub mod wire;
pub use view::*;
#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::serve_fuse;
#[cfg(any(target_os = "linux", target_os = "android"))]
pub mod rootless;
#[cfg(test)]
mod tests;
