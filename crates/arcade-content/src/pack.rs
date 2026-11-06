//! Create loader-compatible archives without modifying the source or replacing outputs.
use crate::{MAX_DEPTH, MAX_ENTRIES, MAX_UNPACKED_BYTES, preflight, validate_path};
use std::{
    collections::HashSet,
    fs::{self, File},
    io::{BufReader, Read, Write},
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
};
use zarchive_rs::{Archive, ArchiveWriter};

#[derive(Clone, Debug, Default)]
pub struct Progress {
    pub phase: &'static str,
    pub path: String,
    pub bytes_done: u64,
    pub bytes_total: u64,
    pub files_done: usize,
    pub files_total: usize,
}

fn check_cancel(cancel: &AtomicBool) -> Result<(), String> {
    if cancel.load(Ordering::Relaxed) {
        Err("Conversion cancelled; no archive was published.".into())
    } else {
        Ok(())
    }
}

/// Archive the folder's contents (no extra enclosing directory). The output parent
/// must exist and be outside the source. Links and special files are rejected.
/// Callers should keep the game closed while converting it.
pub fn convert(
    source: &Path,
    output: &Path,
    cancel: &AtomicBool,
    mut report: impl FnMut(Progress),
) -> Result<(), String> {
    let mut work = || -> Result<(), String> {
        check_cancel(cancel)?;
        let source = source.canonicalize().map_err(|e| e.to_string())?;
        if !source.is_dir() {
            return Err("Choose a game folder.".into());
        }
        if !output
            .extension()
            .is_some_and(|s| s.eq_ignore_ascii_case("zar"))
        {
            return Err("Output filename must end in .zar.".into());
        }
        let parent = output
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let parent = parent
            .canonicalize()
            .map_err(|e| format!("Output folder: {e}"))?;
        if parent.starts_with(&source) {
            return Err("Save the archive outside the source folder.".into());
        }
        let output = parent.join(output.file_name().ok_or("Choose an output filename.")?);
        if fs::symlink_metadata(&output).is_ok() {
            return Err("Output already exists; choose a new filename.".into());
        }
        let mut progress = Progress {
            phase: "Scanning folder",
            ..Default::default()
        };
        report(progress.clone());
        let mut entries = Vec::new();
        let mut pending = vec![(source.clone(), String::new(), 0)];
        let mut names = HashSet::new();
        while let Some((directory, prefix, depth)) = pending.pop() {
            check_cancel(cancel)?;
            if depth > MAX_DEPTH {
                return Err("Folder nesting exceeds 64 directories.".into());
            }
            let mut children = fs::read_dir(&directory)
                .map_err(|e| format!("{}: {e}", directory.display()))?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| e.to_string())?;
            children.sort_by_key(|e| e.file_name());
            for entry in children {
                check_cancel(cancel)?;
                let name = entry
                    .file_name()
                    .into_string()
                    .map_err(|_| "Filenames must be UTF-8.")?;
                if name.contains('\\') {
                    return Err(format!("Filename contains a backslash: {name}"));
                }
                let path = if prefix.is_empty() {
                    name
                } else {
                    format!("{prefix}/{name}")
                };
                validate_path(&path)?;
                if !names.insert(path.to_ascii_lowercase()) {
                    return Err(format!(
                        "ZArchive cannot store names differing only by case: {path}"
                    ));
                }
                let metadata = fs::symlink_metadata(entry.path()).map_err(|e| e.to_string())?;
                if metadata.file_type().is_symlink() || !(metadata.is_file() || metadata.is_dir()) {
                    return Err(format!("Links and special files are unsupported: {path}"));
                }
                if metadata.is_dir() {
                    pending.push((entry.path(), path.clone(), depth + 1));
                } else {
                    progress.bytes_total = progress
                        .bytes_total
                        .checked_add(metadata.len())
                        .ok_or("Folder size overflow")?;
                    progress.files_total += 1;
                }
                entries.push((entry.path(), path, metadata));
                if entries.len() > MAX_ENTRIES || progress.bytes_total > MAX_UNPACKED_BYTES {
                    return Err(
                        "Folder exceeds the loader's one million entries / 2 TiB limits.".into(),
                    );
                }
            }
        }
        // Parents precede their children; ordering also gives reproducible traversal.
        entries.sort_by(|a, b| a.1.cmp(&b.1));
        let mut temporary = tempfile::Builder::new()
            .prefix(".arcade-zar-")
            .tempfile_in(parent)
            .map_err(|e| e.to_string())?;
        let mut writer = ArchiveWriter::new(temporary.as_file_mut()).map_err(|e| e.to_string())?;
        let mut buffer = vec![0; 1024 * 1024];
        progress.phase = "Compressing";
        report(progress.clone());
        for (disk_path, path, before) in entries {
            check_cancel(cancel)?;
            let current = fs::symlink_metadata(&disk_path).map_err(|e| e.to_string())?;
            if current.file_type() != before.file_type()
                || current.len() != before.len()
                || current.modified().ok() != before.modified().ok()
            {
                return Err(format!("Source changed during conversion: {path}"));
            }
            if before.is_dir() {
                writer.create_dir_all(&path).map_err(|e| e.to_string())?;
                continue;
            }
            let mut input = File::open(&disk_path).map_err(|e| format!("{path}: {e}"))?;
            let mut entry = writer.start_file(&path).map_err(|e| e.to_string())?;
            let mut copied = 0;
            progress.path = path.clone();
            loop {
                check_cancel(cancel)?;
                let count = input.read(&mut buffer).map_err(|e| e.to_string())?;
                if count == 0 {
                    break;
                }
                entry
                    .write_all(&buffer[..count])
                    .map_err(|e| e.to_string())?;
                copied += count as u64;
                progress.bytes_done += count as u64;
                report(progress.clone());
            }
            let after = input.metadata().map_err(|e| e.to_string())?;
            if copied != before.len() || after.modified().ok() != before.modified().ok() {
                return Err(format!("Source changed during conversion: {path}"));
            }
            progress.files_done += 1;
            report(progress.clone());
        }
        check_cancel(cancel)?;
        progress.phase = "Finalizing";
        report(progress.clone());
        writer.finish().map_err(|e| e.to_string())?;
        temporary.as_file().sync_all().map_err(|e| e.to_string())?;
        check_cancel(cancel)?;
        progress.phase = "Verifying archive";
        report(progress.clone());
        let mut archive = Archive::new(BufReader::new(
            File::open(temporary.path()).map_err(|e| e.to_string())?,
        ))
        .map_err(|e| e.to_string())?;
        archive.verify_integrity().map_err(|e| e.to_string())?;
        preflight(&mut archive)?;
        drop(archive);
        check_cancel(cancel)?;
        temporary
            .persist_noclobber(&output)
            .map_err(|e| format!("Publishing archive: {}", e.error))?;
        progress.phase = "Complete";
        report(progress);
        Ok(())
    };
    work().map_err(|e| format!("Cannot create ZArchive: {e}"))
}
