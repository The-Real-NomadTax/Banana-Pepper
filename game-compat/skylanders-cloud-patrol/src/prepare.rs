//! Locating, verifying and staging the cabinet's DataPC folder.
//!
//! The staged copy (runtime/skylanders/game) mirrors the game with symlinks
//! for its 1.2 GB of Unity assets. Only the executable is copied, and the
//! PQ Labs touch client and Kaboom coin/ticket board client are replaced by
//! the Banana Pepper adapter. The
//! cabinet's updater, SSH tunnel, registry tweak and reboot script are not
//! staged, and nothing in the source folder is written.
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
};

pub const EXECUTABLE: &str = "Skylanders.exe";
pub const DATA: &str = "Skylanders_Data";
/// Skylanders.exe of the 2014 cabinet build (Unity 4.2.2f1).
pub const SOURCE_SHA256: &str = "c9fa7f38ab8cc58ba3e2daaa806d07b87e730a9d49e505f0a1881882b99fc2e9";
/// The cabinet clients replaced by the adapter: the PQ Labs touch screen and
/// the Kaboom coin/ticket board.
const ADAPTED: &[&str] = &["PQMTClient.dll", "KaboomComDll.dll"];
/// Cabinet-only files: the Java updater, its SSH tunnel and key, the launcher
/// and boot script (which reboots the PC on failure), a disk-cache registry
/// tweak, the desktop wallpaper and the previous session's log.
const SKIP: &[&str] = &[
    "skylanders.bat",
    "skylanders.jar",
    "gamelauncher.exe",
    "plink.exe",
    "sshlink.ppk",
    "key.reg",
    "aa_wallpaper.jpg",
    "need.to.unzip",
    "output_log.txt",
];

/// Folder names the game is found under in a games library.
const FOLDERS: &[&str] = &[
    "skylanders_cloud_patrol",
    "Skylanders Cloud Patrol",
    "Skylanders",
    "DataPC",
    "windows based/skylanders_cloud_patrol",
    "windows based/Skylanders Cloud Patrol",
];

/// The game folder (holding Skylanders.exe) for a library root or the game's
/// own folder.
pub fn directory(root: &Path) -> PathBuf {
    std::iter::once(root.to_path_buf())
        .chain(FOLDERS.iter().map(|name| root.join(name)))
        .chain(FOLDERS.iter().map(|name| root.join(name).join("DataPC")))
        .find(|p| p.join(EXECUTABLE).is_file() && p.join(DATA).is_dir())
        .unwrap_or_else(|| root.join(FOLDERS[0]))
}

fn sha256(path: &Path) -> Result<String, String> {
    let data = fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(format!("{:x}", Sha256::digest(&data)))
}

pub fn verify(source: &Path) -> Result<(), String> {
    let hash = sha256(&source.join(EXECUTABLE))?;
    if hash != SOURCE_SHA256 {
        return Err(format!(
            "Unsupported Skylanders.exe (SHA256 {hash}); only the 2014 Cloud Patrol cabinet build is supported"
        ));
    }
    if !source.join(DATA).join("mainData").is_file() {
        return Err("Skylanders_Data/mainData is missing; copy the complete DataPC folder".into());
    }
    Ok(())
}

fn skipped(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    SKIP.contains(&lower.as_str()) || ADAPTED.iter().any(|a| a.eq_ignore_ascii_case(&lower))
}

#[cfg(unix)]
fn link(source: &Path, destination: &Path) -> Result<(), String> {
    match fs::symlink_metadata(destination) {
        Ok(meta) if meta.file_type().is_symlink() => {
            if fs::read_link(destination).ok().as_deref() == Some(source) {
                return Ok(());
            }
            fs::remove_file(destination).map_err(|e| e.to_string())?;
        }
        Ok(_) => {
            return Err(format!(
                "{} is not a staging link; remove it to restage",
                destination.display()
            ));
        }
        Err(_) => {}
    }
    std::os::unix::fs::symlink(source, destination).map_err(|e| e.to_string())
}

#[cfg(not(unix))]
fn link(source: &Path, destination: &Path) -> Result<(), String> {
    if destination.exists() {
        return Ok(());
    }
    if source.is_dir() {
        copy_tree(source, destination)
    } else {
        fs::copy(source, destination).map(|_| ()).map_err(|e| e.to_string())
    }
}

#[cfg(not(unix))]
fn copy_tree(source: &Path, destination: &Path) -> Result<(), String> {
    fs::create_dir_all(destination).map_err(|e| e.to_string())?;
    for entry in fs::read_dir(source).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let target = destination.join(entry.file_name());
        if entry.file_type().map_err(|e| e.to_string())?.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else if !target.exists() {
            fs::copy(entry.path(), target).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

fn real_dir(path: &Path) -> Result<(), String> {
    if fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink()) {
        fs::remove_file(path).map_err(|e| e.to_string())?;
    }
    fs::create_dir_all(path).map_err(|e| e.to_string())
}

/// Mirror `source` into `destination`: entries named in `real` become real
/// directories filled by `fill`, everything else (bar skipped files) a link.
fn mirror(
    source: &Path,
    destination: &Path,
    real: &[&str],
    fill: &mut dyn FnMut(&str, &Path, &Path) -> Result<(), String>,
) -> Result<(), String> {
    real_dir(destination)?;
    for entry in fs::read_dir(source).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let name = entry.file_name();
        let text = name.to_string_lossy();
        if skipped(&text) {
            continue;
        }
        let target = destination.join(&name);
        if real.iter().any(|r| r.eq_ignore_ascii_case(&text)) {
            fill(&text, &entry.path(), &target)?;
        } else {
            link(&entry.path(), &target)?;
        }
    }
    Ok(())
}

/// Copy the adapter into `folder` under each replaced client's name.
fn install(adapter: &Path, folder: &Path) -> Result<(), String> {
    for name in ADAPTED {
        let destination = folder.join(name);
        if fs::symlink_metadata(&destination).is_ok_and(|m| m.file_type().is_symlink()) {
            fs::remove_file(&destination).map_err(|e| e.to_string())?;
        }
        fs::copy(adapter, &destination).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Stage `source` (a verified DataPC folder) into `destination`, installing
/// the touch adapter DLL at `adapter`.
pub fn stage(source: &Path, destination: &Path, adapter: &Path) -> Result<(), String> {
    verify(source)?;
    if !adapter.is_file() {
        return Err("Build the Skylanders touch adapter: cargo build -p banana-skylanders-touch --target i686-pc-windows-msvc --release".into());
    }
    let source = source.canonicalize().map_err(|e| e.to_string())?;
    fs::create_dir_all(destination).map_err(|e| e.to_string())?;
    if destination.canonicalize().map_err(|e| e.to_string())?.starts_with(&source) {
        return Err("The Skylanders runtime folder must be outside the game folder".into());
    }
    mirror(&source, destination, &[EXECUTABLE, DATA], &mut |name, from, to| {
        if name.eq_ignore_ascii_case(EXECUTABLE) {
            if sha256(to).ok().as_deref() != Some(SOURCE_SHA256) {
                if fs::symlink_metadata(to).is_ok() {
                    fs::remove_file(to).map_err(|e| e.to_string())?;
                }
                fs::copy(from, to).map_err(|e| e.to_string())?;
            }
            return Ok(());
        }
        // Skylanders_Data: Unity writes output_log.txt here, and Plugins
        // receives the adapter.
        mirror(from, to, &["Plugins"], &mut |_, from, to| {
            mirror(from, to, &[], &mut |_, _, _| Ok(()))?;
            install(adapter, to)
        })
    })?;
    // The executable's folder is searched first for plugin DLLs too.
    install(adapter, destination)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_library_and_own_folder_layouts() {
        let temp = std::env::temp_dir().join(format!("banana-skylanders-{}", std::process::id()));
        let game = temp.join("skylanders_cloud_patrol");
        fs::create_dir_all(game.join(DATA)).unwrap();
        fs::write(game.join(EXECUTABLE), b"MZ").unwrap();
        assert_eq!(directory(&temp), game);
        assert_eq!(directory(&game), game);
        assert_eq!(directory(&temp.join("missing")), temp.join("missing").join(FOLDERS[0]));
        fs::remove_dir_all(&temp).unwrap();
    }

    #[test]
    fn cabinet_scripts_are_not_staged() {
        for name in ["Skylanders.bat", "plink.exe", "SSHLINK.PPK", "PQMTClient.dll", "KaboomComDll.dll", "output_log.txt"] {
            assert!(skipped(name), "{name}");
        }
        assert!(!skipped("mainData"));
    }
}
