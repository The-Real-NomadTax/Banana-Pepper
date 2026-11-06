//! Ford Racing Full Blown EU 1.1.6. The supplied shell/loaders are not used.
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
};
pub const SOURCE_SHA256: &str = "ef7011d4c59bcf68cbbe8a3efd31443638089a96ac2dc1747d9aa3d912b41e76";
pub fn directory(root: &Path) -> PathBuf {
    [
        root.join("windows based/Ford Racing"),
        root.join("Ford Racing"),
        root.to_path_buf(),
    ]
    .into_iter()
    .find(|p| p.join("FordRacing/fordracing.exe").is_file())
    .unwrap_or_else(|| root.join("windows based/Ford Racing"))
}
pub fn verify(root: &Path) -> Result<(), String> {
    let data = fs::read(root.join("FordRacing/fordracing.exe")).map_err(|e| e.to_string())?;
    let hash = format!("{:x}", Sha256::digest(&data));
    if hash != SOURCE_SHA256 {
        return Err(format!(
            "Unsupported Ford executable SHA256 {hash}; no adapter applied"
        ));
    }
    Ok(())
}
/// Stage ordinary game assets and seed saves; never import bundled loaders or
/// kernel port drivers. Existing runtime saves are kept across sessions.
pub fn prepare(source: &Path, destination: &Path, adapter: &Path) -> Result<(), String> {
    verify(source)?;
    if source.canonicalize().ok() == destination.canonicalize().ok() {
        return Err("Ford runtime must be separate from source".into());
    }
    if !adapter.is_file() {
        return Err("Build the Ford adapter: cargo build -p banana-ford-hook --target i686-pc-windows-msvc --release".into());
    }
    fn copy(source: &Path, dest: &Path) -> Result<(), String> {
        if fs::symlink_metadata(dest).is_ok_and(|m| m.file_type().is_symlink()) {
            return Err("Symlink runtime destination".into());
        }
        fs::create_dir_all(dest).map_err(|e| e.to_string())?;
        for entry in fs::read_dir(source).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let kind = entry.file_type().map_err(|e| e.to_string())?;
            if kind.is_symlink() {
                return Err("Symlink in Ford files".into());
            }
            let name = entry.file_name();
            let lower = name.to_string_lossy().to_ascii_lowercase();
            if [
                "dinput8.dll",
                "inpout32.dll",
                "jvsemufr.dll",
                "teknoparrot.ini",
            ]
            .contains(&lower.as_str())
            {
                continue;
            }
            let target = dest.join(name);
            if fs::symlink_metadata(&target).is_ok_and(|m| m.file_type().is_symlink()) {
                return Err("Symlink runtime file".into());
            }
            if kind.is_dir() {
                copy(&entry.path(), &target)?;
            } else if kind.is_file() && !target.exists() {
                fs::copy(entry.path(), target).map_err(|e| e.to_string())?;
            }
        }
        Ok(())
    }
    copy(&source.join("FordRacing"), &destination.join("FordRacing"))?;
    copy(&source.join("data"), &destination.join("data"))?;
    let exe = destination.join("FordRacing/fordracing.exe");
    let staged_hash = format!(
        "{:x}",
        Sha256::digest(fs::read(&exe).map_err(|e| e.to_string())?)
    );
    if staged_hash != SOURCE_SHA256 {
        return Err("Staged Ford executable differs from supported build".into());
    }
    let dll = destination.join("FordRacing/inpout32.dll");
    if fs::symlink_metadata(&dll).is_ok_and(|m| m.file_type().is_symlink()) {
        return Err("Symlink adapter destination".into());
    }
    fs::copy(adapter, dll).map_err(|e| e.to_string())?;
    Ok(())
}
