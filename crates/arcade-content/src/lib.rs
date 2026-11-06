//! Materialize a ZArchived game folder for ordinary filesystem-based backends.
//! Sources are immutable; verified, complete extractions are published atomically.
use sha2::{Digest, Sha256};
use std::{
    fs::{self, OpenOptions},
    io::{BufReader, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};
use zarchive_rs::{Archive, ExtractOptions};

const CACHE_VERSION: &str = "v1";
const MAX_ENTRIES: usize = 1_000_000;
const MAX_DEPTH: usize = 64;
const MAX_UNPACKED_BYTES: u64 = 2 * 1024 * 1024 * 1024 * 1024;

/// A directory passes through unchanged. An archive is verified and extracted
/// once beneath `cache_root`; identical archive bytes share the same cache.
/// No archive handle or extraction lock needs to outlive this call.
pub fn prepare(source: &Path, cache_root: &Path) -> Result<PathBuf, String> {
    if source.is_dir() || (!source.is_file() && !archive_extension(source)) {
        return Ok(source.to_path_buf());
    }
    prepare_archive(source, cache_root)
        .map_err(|e| format!("Cannot load ZArchive {}: {e}", source.display()))
}

fn archive_extension(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| {
        ["zar", "zarchive", "wua"]
            .iter()
            .any(|candidate| e.eq_ignore_ascii_case(candidate))
    })
}

fn real_directory(path: &Path) -> Result<bool, String> {
    match fs::symlink_metadata(path) {
        Ok(m) if m.is_dir() && !m.file_type().is_symlink() => Ok(true),
        Ok(_) => Err(format!(
            "Cache directory is a link or non-directory: {}",
            path.display()
        )),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e.to_string()),
    }
}

fn prepare_archive(source: &Path, cache_root: &Path) -> Result<PathBuf, String> {
    let mut options = OpenOptions::new();
    options.read(true);
    // Do not accept an archive concurrently rewritten or replaced on Windows.
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(1);
    }
    let file = options.open(source).map_err(|e| e.to_string())?;
    let before = file.metadata().map_err(|e| e.to_string())?;
    let mut reader = BufReader::with_capacity(1024 * 1024, file);
    let mut hasher = Sha256::new();
    let mut buffer = vec![0; 1024 * 1024];
    loop {
        let count = reader.read(&mut buffer).map_err(|e| e.to_string())?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    let digest = format!("{:x}", hasher.finalize());
    let key = format!("{CACHE_VERSION}-{digest}");
    reader.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
    let mut archive = Archive::new(reader).map_err(|e| e.to_string())?;

    if !real_directory(cache_root)? {
        fs::create_dir_all(cache_root).map_err(|e| e.to_string())?;
    }
    let cache_root = cache_root.canonicalize().map_err(|e| e.to_string())?;
    let destination = cache_root.join(&key);
    let lock_path = cache_root.join(format!("{key}.lock"));
    if fs::symlink_metadata(&lock_path).is_ok_and(|m| !m.is_file() || m.file_type().is_symlink()) {
        return Err("Invalid archive cache lock file".into());
    }
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(lock_path)
        .map_err(|e| e.to_string())?;
    lock.lock()
        .map_err(|e| format!("Cannot lock archive extraction: {e}"))?;
    let ready = format!("arcade-content {CACHE_VERSION}\n{digest}\n");
    if real_directory(&destination)? {
        let marker = destination.join("complete");
        if fs::symlink_metadata(&marker).is_ok_and(|m| m.is_file() && !m.file_type().is_symlink())
            && fs::read_to_string(&marker).is_ok_and(|s| s == ready)
            && real_directory(&destination.join("tree"))?
        {
            return Ok(destination.join("tree"));
        }
        return Err(format!(
            "Incomplete archive cache at {}; remove that cache entry and retry",
            destination.display()
        ));
    }

    archive.verify_integrity().map_err(|e| e.to_string())?;
    // Validate all paths and declared sizes before creating any game files.
    preflight(&mut archive)?;
    let staging = tempfile::Builder::new()
        .prefix(".extract-")
        .tempdir_in(&cache_root)
        .map_err(|e| e.to_string())?;
    let tree = staging.path().join("tree");
    archive
        .extract_to(&tree, ExtractOptions::new())
        .map_err(|e| format!("Extracting archive entries: {e}"))?;
    restore_executables(&tree)?;
    let after = archive
        .into_inner()
        .into_inner()
        .metadata()
        .map_err(|e| e.to_string())?;
    if before.len() != after.len() || before.modified().ok() != after.modified().ok() {
        return Err("Archive changed during extraction; finish writing it and retry".into());
    }
    fs::write(staging.path().join("complete"), ready).map_err(|e| e.to_string())?;
    fs::rename(staging.path(), &destination)
        .map_err(|e| format!("Publishing archive cache: {e}"))?;
    // TempDir now points to an absent staging name, so Drop cannot remove the
    // published tree. Errors before publication clean only that private tree.
    Ok(destination.join("tree"))
}

fn preflight<R: Read + Seek>(archive: &mut Archive<R>) -> Result<(), String> {
    let mut pending = vec![(Vec::<u8>::new(), 0usize)];
    let mut count = 0usize;
    let mut bytes = 0u64;
    while let Some((directory, depth)) = pending.pop() {
        if depth > MAX_DEPTH {
            return Err("Archive nesting exceeds 64 directories".into());
        }
        for entry in archive.read_dir(&directory).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            count += 1;
            if count > MAX_ENTRIES {
                return Err("Archive contains more than one million entries".into());
            }
            let path = entry.path_str().ok_or("Archive filenames must be UTF-8")?;
            validate_path(path)?;
            if entry.metadata().is_dir() {
                pending.push((entry.path_bytes().to_vec(), depth + 1));
            } else {
                bytes = bytes
                    .checked_add(entry.metadata().len())
                    .ok_or("Archive size overflow")?;
                if bytes > MAX_UNPACKED_BYTES {
                    return Err("Archive expands beyond the 2 TiB game-source limit".into());
                }
            }
        }
    }
    Ok(())
}

fn validate_path(path: &str) -> Result<(), String> {
    let normalized = path.replace('\\', "/");
    for component in normalized.split('/') {
        if component.is_empty()
            || matches!(component, "." | "..")
            || component.contains(['\0', ':'])
        {
            return Err(format!("Unsafe archive path: {path}"));
        }
        #[cfg(windows)]
        {
            let base = component
                .split('.')
                .next()
                .unwrap_or("")
                .to_ascii_uppercase();
            let reserved = ["CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$"]
                .contains(&base.as_str())
                || (base.len() == 4
                    && (base.starts_with("COM") || base.starts_with("LPT"))
                    && matches!(base.as_bytes()[3], b'1'..=b'9'));
            if reserved
                || component.ends_with(['.', ' '])
                || component.contains(['<', '>', '"', '|', '?', '*'])
            {
                return Err(format!(
                    "Archive filename cannot be represented on Windows: {path}"
                ));
            }
        }
    }
    Ok(())
}

#[cfg(unix)]
fn restore_executables(root: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(directory).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            if entry.file_type().map_err(|e| e.to_string())?.is_dir() {
                pending.push(entry.path());
            } else {
                let mut magic = [0u8; 4];
                let count = fs::File::open(entry.path())
                    .and_then(|mut f| f.read(&mut magic))
                    .map_err(|e| e.to_string())?;
                if (count == 4 && magic == *b"\x7fELF") || (count >= 2 && magic[..2] == *b"#!") {
                    fs::set_permissions(entry.path(), fs::Permissions::from_mode(0o755))
                        .map_err(|e| e.to_string())?;
                }
            }
        }
    }
    Ok(())
}
#[cfg(not(unix))]
fn restore_executables(_root: &Path) -> Result<(), String> {
    Ok(())
}

#[cfg(test)]
mod tests;
