//! Wine execution backend for Windows guests on Linux hosts.
use banana_api::{Availability, BackendKind, ExecutionBackend};

pub struct Wine;
pub static BACKEND: Wine = Wine;

impl ExecutionBackend for Wine {
    fn kind(&self) -> BackendKind {
        BackendKind::Wine
    }
    fn name(&self) -> &'static str {
        "Wine"
    }
    fn formats(&self) -> &'static [&'static str] {
        &["PE"]
    }
    fn availability(&self) -> Availability {
        if !cfg!(unix) {
            return Availability::Unsupported;
        }
        let found = std::env::var_os("PATH").is_some_and(|path| {
            std::env::split_paths(&path).any(|dir| dir.join("wine").is_file())
        });
        if found {
            Availability::Available
        } else {
            Availability::Missing("wine is not installed".into())
        }
    }
}
