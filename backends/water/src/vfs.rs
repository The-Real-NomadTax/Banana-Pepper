//! Guest path translation into the same mounted namespace used by other backends.
use std::{
    io,
    path::{Path, PathBuf},
};
pub struct Vfs {
    root: PathBuf,
}
impl Vfs {
    pub fn new(root: &Path) -> io::Result<Self> {
        Ok(Self {
            root: root.canonicalize()?,
        })
    }
    pub fn resolve(&self, name: &str, _write: bool) -> io::Result<PathBuf> {
        if name.contains(['\\', ':', '\0']) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "Guest path leaves its namespace",
            ));
        }
        let guest = std::env::var("BANANA_GUEST_ROOT").unwrap_or_else(|_| "/game".into());
        let name = name
            .strip_prefix(&format!("{guest}/"))
            .unwrap_or(name)
            .trim_start_matches('/');
        let mut components = Vec::new();
        for part in name.split('/') {
            match part {
                "" | "." => {}
                ".." => {
                    if components.pop().is_none() {
                        return Err(io::Error::new(
                            io::ErrorKind::PermissionDenied,
                            "Guest path leaves its namespace",
                        ));
                    }
                }
                p => components.push(p),
            }
        }
        let normalized = components.join("/");
        let name = if normalized == "proc/cpuinfo" {
            ".banana/proc/cpuinfo"
        } else {
            &normalized
        };
        banana_content::path(name)?;
        Ok(self.root.join(name))
    }
}
