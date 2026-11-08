//! Immutable folder and archive providers. Game content is never extracted.
use std::io::{Read, Seek};
use zarust::{ArchiveReader, NodeHandle, ROOT_NODE};
pub mod pack;
mod source;
pub use source::*;
const MAX_ENTRIES: usize = 1_000_000;
const MAX_DEPTH: usize = 64;
const MAX_UNPACKED_BYTES: u64 = 2 * 1024 * 1024 * 1024 * 1024;

/// One validated archive entry.
struct ArchiveNode {
    /// `/`-separated path below the archive root.
    path: String,
    handle: NodeHandle,
    directory: bool,
    len: u64,
}

/// Walk an archive's tree, checking it against the game-source limits and
/// path rules. Every entry is returned, parents before their children.
fn preflight<R: Read + Seek>(archive: &ArchiveReader<R>) -> Result<Vec<ArchiveNode>, String> {
    let mut pending = vec![(String::new(), ROOT_NODE, 0usize)];
    let mut nodes = Vec::new();
    let mut bytes = 0u64;
    while let Some((directory, handle, depth)) = pending.pop() {
        if depth > MAX_DEPTH {
            return Err("Archive nesting exceeds 64 directories".into());
        }
        for entry in archive
            .directory_entries(handle)
            .map_err(|e| e.to_string())?
        {
            if nodes.len() >= MAX_ENTRIES {
                return Err("Archive contains more than one million entries".into());
            }
            let name = std::str::from_utf8(entry.name).map_err(|_| "Archive filenames must be UTF-8")?;
            let path = if directory.is_empty() {
                name.to_owned()
            } else {
                format!("{directory}/{name}")
            };
            validate_path(&path)?;
            if entry.is_directory() {
                pending.push((path.clone(), entry.handle, depth + 1));
            } else {
                bytes = bytes.checked_add(entry.size).ok_or("Archive size overflow")?;
                if bytes > MAX_UNPACKED_BYTES {
                    return Err("Archive expands beyond the 2 TiB game-source limit".into());
                }
            }
            nodes.push(ArchiveNode {
                path,
                handle: entry.handle,
                directory: entry.is_directory(),
                len: entry.size,
            });
        }
    }
    Ok(nodes)
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
