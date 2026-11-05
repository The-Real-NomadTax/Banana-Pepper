//! Build-specific preparation. Never alters the source game file.
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{self, Write},
    path::Path,
};

pub const SOURCE_SHA256: &str = "c569677c4e4bcf3449939b6e9a44afd3346c153133391c24dc06c012a58c1244";
const CODE_OFFSET: usize = 0x2d80;
const CODE_SIZE: usize = 0x937c0;
const ENTRY: u32 = 0x0804ad80;
const MASK: [u8; 4] = [0x84, 0xeb, 0x67, 0xf1];

pub fn prepare(source: &Path, destination: &Path) -> Result<String, String> {
    let bytes = fs::read(source).map_err(|e| e.to_string())?;
    let hash = format!("{:x}", Sha256::digest(&bytes));
    if hash != SOURCE_SHA256 {
        return Err(format!(
            "Unrecognized Big Buck Hunter executable: SHA256 {hash}. No changes made."
        ));
    }
    let decoded = decode_known_build(bytes);
    // Confirm the reconstructed Linux CRT startup before writing anything.
    if &decoded[CODE_OFFSET..CODE_OFFSET + 11] != b"\x31\xed\x5e\x89\xe1\x83\xe4\xf0\x50\x54\x52" {
        return Err("Reconstructed entry validation failed".into());
    }
    if fs::symlink_metadata(destination).is_ok_and(|m| m.file_type().is_symlink())
        || destination.canonicalize().ok() == source.canonicalize().ok()
    {
        return Err(
            "Runtime destination must be separate from the source and cannot be a symlink".into(),
        );
    }
    let temporary = destination.with_extension("preparing");
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|e| e.to_string())?;
    file.write_all(&decoded).map_err(|e| e.to_string())?;
    file.sync_all().map_err(|e| e.to_string())?;
    drop(file);
    fs::rename(&temporary, destination).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(destination, fs::Permissions::from_mode(0o755))
            .map_err(|e| e.to_string())?;
    }
    Ok(format!("{:x}", Sha256::digest(&decoded)))
}

fn decode_known_build(mut bytes: Vec<u8>) -> Vec<u8> {
    for (i, byte) in bytes[CODE_OFFSET..CODE_OFFSET + CODE_SIZE]
        .iter_mut()
        .enumerate()
    {
        *byte ^= MASK[i % MASK.len()];
    }
    bytes[24..28].copy_from_slice(&ENTRY.to_le_bytes());
    bytes[9..16].fill(0); // Reserved ELF identification padding.
    bytes
}

/// Copy seed saves once; reject symlinks rather than following paths outside the game.
pub fn seed_saves(source: &Path, destination: &Path) -> io::Result<()> {
    fs::create_dir_all(destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        let target = destination.join(entry.file_name());
        if fs::symlink_metadata(&target).is_ok_and(|m| m.file_type().is_symlink())
            || kind.is_symlink()
        {
            return Err(io::Error::other(
                "Symlinks are not allowed in runtime save data",
            ));
        }
        if kind.is_dir() {
            seed_saves(&entry.path(), &target)?;
        } else if kind.is_file() && !target.exists() {
            fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn transform_is_limited_to_the_expected_ranges() {
        let bytes = vec![0xa5; CODE_OFFSET + CODE_SIZE + 10];
        let transformed = decode_known_build(bytes.clone());
        for i in 0..bytes.len() {
            let expected = if (CODE_OFFSET..CODE_OFFSET + CODE_SIZE).contains(&i) {
                bytes[i] ^ MASK[(i - CODE_OFFSET) % 4]
            } else if (24..28).contains(&i) {
                ENTRY.to_le_bytes()[i - 24]
            } else if (9..16).contains(&i) {
                0
            } else {
                bytes[i]
            };
            assert_eq!(transformed[i], expected, "offset {i:#x}");
        }
    }
    #[test]
    fn unknown_build_is_rejected_without_creating_output() {
        let dir = std::env::temp_dir().join(format!("arcade-build-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("source"), b"unknown build").unwrap();
        assert!(prepare(&dir.join("source"), &dir.join("destination")).is_err());
        assert!(!dir.join("destination").exists());
        fs::remove_dir_all(dir).unwrap();
    }
}
