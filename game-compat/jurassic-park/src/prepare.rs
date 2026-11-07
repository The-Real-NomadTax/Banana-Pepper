//! Identify and snapshot the two supplied Jurassic Park builds without modification.
use sha2::{Digest, Sha256};
use std::{fs, io::Write, path::Path};

/// The game folder for a library root (`Jurassic Park 2015/`, or the older
/// `linux based/Jurassic Park 2015/`) or the game's own folder.
pub fn directory(root: &Path) -> std::path::PathBuf {
    [root.join("Jurassic Park 2015"), root.join("linux based/Jurassic Park 2015"), root.to_path_buf()]
        .into_iter()
        .find(|p| p.join("game_original").is_file() || p.join("game_legacy_patched").is_file())
        .unwrap_or_else(|| root.join("Jurassic Park 2015"))
}

#[derive(Clone, Copy, Debug)]
pub enum Build {
    Original,
    Legacy,
    Native,
}
impl Build {
    pub fn filename(self) -> &'static str {
        match self {
            Self::Original => "game_original",
            Self::Legacy | Self::Native => "game_legacy_patched",
        }
    }
    pub fn sha256(self) -> &'static str {
        match self {
            Self::Original => "6bb8a70dc0ee3f9d7ac3672d7dc8cf4aadec44b0f96c050ace96a7633d26af11",
            Self::Legacy | Self::Native => {
                "c18a9329e72a363937155d245440694e58b5a1e27984016ae7648af3413b4269"
            }
        }
    }
}

pub fn prepare(source: &Path, destination: &Path, build: Build) -> Result<(), String> {
    prepare_impl(source, destination, build, false)
}

/// Desktop profile for the supplied plaintext asset backups. No HASP cryptography
/// is implemented: unavailable cabinet security initialization is omitted.
pub fn prepare_desktop(source: &Path, destination: &Path) -> Result<(), String> {
    prepare_impl(source, destination, Build::Native, true)
}

fn desktop_patches(bytes: &mut [u8]) -> Result<(), String> {
    const INIT: usize = 0x81bab40 - 0x8048000;
    const BRANCH: usize = 0x827e11b - 0x8048000;
    if bytes.get(INIT..INIT + 8) != Some(&[0x55, 0x57, 0x56, 0x53, 0x81, 0xec, 0x3c, 0x03])
        || bytes.get(BRANCH..BRANCH + 6) != Some(&[0x0f, 0x84, 0x5f, 0x01, 0, 0])
    {
        return Err("Unsupported Jurassic desktop adapter sites; no patches applied".into());
    }
    bytes[INIT..INIT + 3].copy_from_slice(&[0x31, 0xc0, 0xc3]);
    // The launcher requires complete //\n-marked plaintext script backups and
    // binds that directory read-only before allowing this profile to execute.
    bytes[BRANCH..BRANCH + 6].fill(0x90);
    Ok(())
}

fn prepare_impl(
    source: &Path,
    destination: &Path,
    build: Build,
    desktop: bool,
) -> Result<(), String> {
    let mut bytes = fs::read(source).map_err(|e| e.to_string())?;
    let hash = format!("{:x}", Sha256::digest(&bytes));
    if hash != build.sha256() {
        return Err(format!(
            "Unrecognized Jurassic Park build: {hash}. No changes made."
        ));
    }
    if matches!(build, Build::Native) {
        let elf = goblin::elf::Elf::parse(&bytes).map_err(|e| e.to_string())?;
        let indices: Vec<_> = elf
            .dynsyms
            .iter()
            .enumerate()
            .filter_map(|(i, s)| (s.st_shndx == 0 && s.st_value != 0).then_some(i))
            .collect();
        if indices.len() != 9 {
            return Err("Unexpected Jurassic import metadata; repair not applied".into());
        }
        for index in indices {
            bytes[0x26680 + 16 * index + 4..0x26680 + 16 * index + 8].fill(0);
        }
    }
    if desktop {
        desktop_patches(&mut bytes)?;
    }
    if source.canonicalize().ok() == destination.canonicalize().ok()
        || fs::symlink_metadata(destination).is_ok_and(|m| m.file_type().is_symlink())
    {
        return Err("Runtime snapshot must be separate from the original file".into());
    }
    let temporary = destination.with_extension("preparing");
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|e| e.to_string())?;
    file.write_all(&bytes)
        .and_then(|_| file.sync_all())
        .map_err(|e| e.to_string())?;
    drop(file);
    fs::rename(&temporary, destination).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(destination, fs::Permissions::from_mode(0o755))
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn desktop_preflight_failure_does_not_partially_patch_code() {
        let mut bytes = vec![0; 0x240000];
        let at = 0x81bab40 - 0x8048000;
        bytes[at..at + 8].copy_from_slice(&[0x55, 0x57, 0x56, 0x53, 0x81, 0xec, 0x3c, 0x03]);
        let before = bytes.clone();
        assert!(desktop_patches(&mut bytes).is_err());
        assert_eq!(bytes, before);
    }
    #[test]
    fn unknown_build_does_not_create_snapshot() {
        let root = std::env::temp_dir().join(format!("jurassic-test-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("source"), b"wrong build").unwrap();
        for build in [Build::Original, Build::Legacy, Build::Native] {
            assert!(prepare(&root.join("source"), &root.join("copy"), build).is_err());
        }
        assert!(!root.join("copy").exists());
        fs::remove_dir_all(root).unwrap();
    }
}
