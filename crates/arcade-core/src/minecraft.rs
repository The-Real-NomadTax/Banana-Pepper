//! Discovery and build identity for the locally supplied Minecraft arcade files.
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
};

pub const DUMP_SHA256: &str = "663b147927e7b9d60cef7bd41cf49d3fd64058619880391083ea1ba872d6be3b";
pub const RELATIVE_DIRECTORY: &str = "linux based/Minecraft Dungeons Arcade";

/// Accept a game directory, the conventional library layout, or a named download.
/// Discovery is deliberately limited to two directory levels.
pub fn directory(root: &Path) -> PathBuf {
    let valid = |p: &Path| {
        let name = p
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_lowercase();
        (p.join("minecraft.hvc").is_file()
            || (name.contains("minecraft") && name.contains("dungeons")))
            && p.join("data").is_dir()
            && (p.join("gamedump.exec").is_file() || p.join("game").is_file())
    };
    for candidate in [root.to_path_buf(), root.join(RELATIVE_DIRECTORY)] {
        if valid(&candidate) {
            return candidate;
        }
    }
    for parent in [root.to_path_buf(), root.join("linux based")] {
        let mut candidates: Vec<_> = fs::read_dir(parent)
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
            .filter(|e| {
                let name = e.file_name().to_string_lossy().to_lowercase();
                name.contains("minecraft") && name.contains("dungeons")
            })
            .map(|e| e.path())
            .collect();
        candidates.sort();
        if let Some(found) = candidates.into_iter().find(|p| valid(p)) {
            return found;
        }
    }
    root.join(RELATIVE_DIRECTORY)
}

pub fn executable(root: &Path) -> PathBuf {
    let directory = directory(root);
    let dump = directory.join("gamedump.exec");
    if dump.is_file() {
        dump
    } else {
        directory.join("game")
    }
}

pub fn verify_dump(path: &Path) -> Result<String, String> {
    let data = fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let hash = format!("{:x}", Sha256::digest(&data));
    if hash != DUMP_SHA256 {
        return Err(format!(
            "Unrecognized Minecraft executable (SHA256 {hash}). Inspection is available; execution requires the verified gamedump.exec build."
        ));
    }
    let info = crate::inspect_bytes(&data)?;
    if info.format != "ELF" || info.bits != 64 || info.machine != 62 {
        return Err("Minecraft requires the x86-64 Linux build".into());
    }
    Ok(hash)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn discovers_direct_and_download_layouts() {
        let root = std::env::temp_dir().join(format!("arcade-minecraft-{}", std::process::id()));
        let game = root.join("Minecraft Dungeons Arcade (3.07)[TP]");
        fs::create_dir_all(game.join("data")).unwrap();
        fs::write(game.join("game"), b"original").unwrap();
        assert_eq!(executable(&root), game.join("game"));
        fs::write(game.join("gamedump.exec"), b"dump").unwrap();
        assert_eq!(executable(&root), game.join("gamedump.exec"));
        assert_eq!(executable(&game), game.join("gamedump.exec"));
        assert!(verify_dump(&game.join("gamedump.exec")).is_err());
        fs::remove_dir_all(root).unwrap();
    }
}
