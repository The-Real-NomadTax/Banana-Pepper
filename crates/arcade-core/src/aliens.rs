//! Build-specific Aliens snapshot preparation. Original assets stay read-only.
use sha2::{Digest, Sha256};
use std::{fs, io::Write, path::Path};

pub const ORIGINAL_SHA256: &str =
    "297c07db82b531e276d4ea7ceb4a11c9ce3aacb436f63526c179c2fd4de3e4db";

pub fn prepare(source: &Path, destination: &Path) -> Result<(), String> {
    let mut bytes = fs::read(source).map_err(|e| e.to_string())?;
    if format!("{:x}", Sha256::digest(&bytes)) != ORIGINAL_SHA256 {
        return Err("Unrecognized Aliens build; no changes applied".into());
    }
    let elf = goblin::elf::Elf::parse(&bytes).map_err(|e| e.to_string())?;
    let dynamic = elf
        .program_headers
        .iter()
        .find(|p| p.p_type == goblin::elf::program_header::PT_DYNAMIC)
        .ok_or("Missing dynamic segment")?;
    let offset = dynamic.p_offset as usize;
    let size = dynamic.p_filesz as usize;
    // ESD has no direct imports; audio tests select SDL's dummy backend. The
    // sole CSV import, csv_write2, is implemented by our preloaded Rust shim.
    let mut entries = Vec::new();
    let mut removed = Vec::new();
    for item in bytes[offset..offset + size].as_chunks::<8>().0 {
        let tag = u32::from_le_bytes(item[..4].try_into().unwrap());
        let value = u32::from_le_bytes(item[4..].try_into().unwrap());
        if tag == 1
            && matches!(
                elf.dynstrtab.get_at(value as usize),
                Some("libesd.so.0" | "/usr/lib/libcsv.so")
            )
        {
            removed.push(elf.dynstrtab.get_at(value as usize).unwrap().to_owned());
        } else {
            entries.extend_from_slice(item);
        }
    }
    if removed != ["libesd.so.0", "/usr/lib/libcsv.so"] {
        return Err("Unexpected Aliens legacy dependency table".into());
    }
    drop(elf);
    bytes[offset..offset + size].fill(0);
    bytes[offset..offset + entries.len()].copy_from_slice(&entries);
    // The cabinet supervisor forks a ptrace monitor and invokes an audit error
    // handler before normal initialization when tracing is unavailable. Desktop
    // sessions use the host's process supervision instead of that cabinet loop.
    const SUPERVISOR: usize = 0x08347966 - 0x08048000;
    if bytes.get(SUPERVISOR..SUPERVISOR + 9)
        != Some(&[0x55, 0x89, 0xe5, 0x81, 0xec, 0x38, 0x04, 0x00, 0x00])
    {
        return Err("Unsupported Aliens supervisor site; no snapshot written".into());
    }
    bytes[SUPERVISOR..SUPERVISOR + 6].copy_from_slice(&[0xb8, 1, 0, 0, 0, 0xc3]);
    // Keep -fWIDTHxHEIGHT's requested size, but create an ordinary desktop
    // window. The original flag requests cabinet fullscreen presentation.
    const FULLSCREEN: usize = 0x08083bfa - 0x08048000;
    if bytes.get(FULLSCREEN..FULLSCREEN + 7) != Some(&[0xc6, 0x05, 0x51, 0xb5, 0x90, 0x08, 0x01]) {
        return Err("Unsupported Aliens window-mode site; no snapshot written".into());
    }
    bytes[FULLSCREEN + 6] = 0;
    // Select the engine's existing desktop input mode at both flag-reset sites.
    // Cabinet mode refuses Start while its physical guns are disconnected.
    // Desktop mode also installs the engine's own mouse maps.
    for address in [0x08080c39usize, 0x0808486c] {
        let offset = address - 0x08048000;
        if bytes.get(offset..offset + 7) != Some(&[0xc6, 0x05, 0x52, 0xb5, 0x90, 0x08, 0]) {
            return Err("Unsupported Aliens desktop-mode site; no snapshot written".into());
        }
        bytes[offset + 6] = 1;
    }
    // Minimal progression input: the cabinet's player-one Start switch becomes
    // keyboard 1. Keep its menu-confirm mapping consistent. Gun controls follow
    // separately; Start and coin let us verify actual level loading.
    for (address, original, key) in [
        (0x08081376usize, 0x154u32, 49u32),
        (0x0808138a, 0x154, 49),
        (0x08081466, 0x14c, 53),
    ] {
        let offset = address - 0x08048000;
        if bytes.get(offset..offset + 3) != Some(&[0xc7, 0x04, 0x24])
            || bytes.get(offset + 3..offset + 7) != Some(original.to_le_bytes().as_slice())
        {
            return Err("Unsupported Aliens progression mapping; no snapshot written".into());
        }
        bytes[offset + 3..offset + 7].copy_from_slice(&key.to_le_bytes());
    }
    // Supplied cabinet-response records contain the decoded asset seed tables.
    // Restore those tables in the runtime copy so the game's native filename
    // mixer, AES reader and decompressor can consume its production assets.
    // This does not emulate dongle operations or report a successful licence.
    let directory = source.parent().ok_or("Missing Aliens directory")?;
    for (name, length, hash, offset) in [
        (
            "2a3ccf10ef8dff97539ed4b3cd113701",
            512,
            "369ff9829782a47ad878183bb26fae5c5566923c23a1d58d237ef435487756b8",
            0x0082eea0usize,
        ),
        (
            "91a0dda258cc5df430a5678cc72cd603",
            256,
            "411f22ab8a0bb1ead127f7206e7668ce88ad1155fc61ca39b40e065eb3a561dc",
            0x0082f6a0,
        ),
    ] {
        let seed = fs::read(directory.join("hasp").join(name))
            .map_err(|e| format!("Missing Aliens asset seed {name}: {e}"))?;
        if seed.len() != length || format!("{:x}", Sha256::digest(&seed)) != hash {
            return Err(format!(
                "Unsupported Aliens asset seed {name}; no snapshot written"
            ));
        }
        bytes[offset..offset + length].copy_from_slice(&seed);
    }
    if destination.exists()
        && (source.canonicalize().ok() == destination.canonicalize().ok()
            || fs::symlink_metadata(destination).is_ok_and(|m| m.file_type().is_symlink()))
    {
        return Err("Runtime snapshot must be separate from original".into());
    }
    let temp = destination.with_extension("preparing");
    let mut file = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temp)
        .map_err(|e| e.to_string())?;
    file.write_all(&bytes)
        .and_then(|_| file.sync_all())
        .map_err(|e| e.to_string())?;
    drop(file);
    fs::rename(temp, destination).map_err(|e| e.to_string())?;
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
    fn unknown_build_leaves_source_and_destination_untouched() {
        let dir = std::env::temp_dir().join(format!("arcade-aliens-reject-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let source = dir.join("source");
        let destination = dir.join("output");
        fs::write(&source, b"unknown game").unwrap();
        fs::write(&destination, b"existing snapshot").unwrap();
        assert!(prepare(&source, &destination).is_err());
        assert_eq!(fs::read(&source).unwrap(), b"unknown game");
        assert_eq!(fs::read(&destination).unwrap(), b"existing snapshot");
        assert!(!destination.with_extension("preparing").exists());
        fs::remove_dir_all(dir).unwrap();
    }
}
