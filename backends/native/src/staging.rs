//! Windows native sessions without an OS mount. The Windows loader needs real
//! files for executable images, so a session root receives the view's
//! directory tree and its PE images only; every other file is served from the
//! view by `banana_view.dll`, which the staged executable imports first.
use banana_api::{LaunchPlan, SessionView};
use std::{
    fs, io,
    path::{Path, PathBuf},
};

/// Where the shim sits in the view (placed there as a backend adapter).
pub const SHIM: &str = ".banana/banana_view.dll";
const SHIM_NAME: &str = "banana_view.dll";
const SHIM_ENTRY: &str = "banana_view_attach";

/// Recreate `root` from the view; returns the staged executable.
pub fn stage(view: &dyn SessionView, plan: &LaunchPlan, root: &Path) -> Result<PathBuf, String> {
    if root.exists() {
        fs::remove_dir_all(root).map_err(|e| format!("Cannot clear {}: {e}", root.display()))?;
    }
    fs::create_dir_all(root).map_err(|e| e.to_string())?;
    walk(view, "", root).map_err(|e| format!("Cannot stage the session view: {e}"))?;
    let executable = host_path(root, &plan.executable);
    let directory = executable.parent().ok_or("Executable has no directory")?;
    // Next to the executable, so the loader finds it first.
    let shim = view
        .read(SHIM, 64 * 1024 * 1024)
        .map_err(|e| format!("Missing Windows view shim: {e}"))?;
    fs::write(directory.join(SHIM_NAME), shim).map_err(|e| e.to_string())?;
    let image = fs::read(&executable).map_err(|e| e.to_string())?;
    let patched = crate::pe::add_import(&image, SHIM_NAME, SHIM_ENTRY)
        .map_err(|e| format!("Cannot prepare {}: {e}", plan.executable))?;
    fs::write(&executable, patched).map_err(|e| e.to_string())?;
    Ok(executable)
}

fn walk(view: &dyn SessionView, path: &str, root: &Path) -> io::Result<()> {
    for entry in view.read_dir(path)? {
        let child = if path.is_empty() {
            entry.name.clone()
        } else {
            format!("{path}/{}", entry.name)
        };
        let host = host_path(root, &child);
        if entry.metadata.directory {
            fs::create_dir_all(&host)?;
            walk(view, &child, root)?;
        } else if entry.metadata.executable {
            // Executables and DLLs (MZ images) only; their size is small.
            fs::write(&host, view.read(&child, 1 << 31)?)?;
        }
    }
    Ok(())
}

pub fn host_path(root: &Path, view: &str) -> PathBuf {
    view.split('/')
        .filter(|p| !p.is_empty())
        .fold(root.to_path_buf(), |p, part| p.join(part))
}
