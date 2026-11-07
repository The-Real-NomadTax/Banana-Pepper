//! Immutable folder and archive providers. Game content is never extracted.
use std::io::{Read, Seek};
use zarchive_rs::Archive;
pub mod pack;
mod source;
pub use source::*;
const MAX_ENTRIES: usize = 1_000_000;
const MAX_DEPTH: usize = 64;
const MAX_UNPACKED_BYTES: u64 = 2 * 1024 * 1024 * 1024 * 1024;
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

#[cfg(test)]
mod provider_tests;

#[cfg(test)]
mod pack_tests;
