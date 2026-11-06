//! Shared source preparation for inspection and every game backend.
use std::path::{Path, PathBuf};
pub fn resolve(source: &Path) -> Result<PathBuf, String> {
    let cache = std::env::var_os("ARCADE_ARCHIVE_CACHE")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../runtime/archive-cache")
        });
    banana_content::prepare(source, &cache)
}
pub fn scan(source: &Path) -> Result<Vec<banana_core::GameReport>, String> {
    Ok(banana_core::scan(&resolve(source)?))
}
