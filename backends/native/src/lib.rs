//! Native execution: the host runs the guest executable directly. On Linux,
//! ELF guests run inside a bubblewrap sandbox with a 32-bit userspace.
use banana_api::{Availability, BackendKind, ExecutionBackend};
pub mod sandbox;
pub use sandbox::Sandbox;

pub struct Native;
pub static BACKEND: Native = Native;

impl ExecutionBackend for Native {
    fn kind(&self) -> BackendKind {
        BackendKind::Native
    }
    fn name(&self) -> &'static str {
        "Native"
    }
    fn formats(&self) -> &'static [&'static str] {
        if cfg!(windows) { &["PE"] } else { &["ELF"] }
    }
    fn availability(&self) -> Availability {
        if cfg!(windows) {
            Availability::Available
        } else if cfg!(target_os = "linux") {
            match find_program("bwrap") {
                Some(_) => Availability::Available,
                None => Availability::Missing("bubblewrap (bwrap) is not installed".into()),
            }
        } else {
            Availability::Unsupported
        }
    }
}

/// First `name` on `PATH`.
pub fn find_program(name: &str) -> Option<std::path::PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|dir| dir.join(name))
        .find(|path| path.is_file())
}
