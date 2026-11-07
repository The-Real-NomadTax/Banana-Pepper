//! Provider inspection never creates an extracted filesystem tree.
use std::path::Path;
pub fn scan(path: &Path) -> Result<Vec<banana_core::GameReport>, String> {
    if !path.exists() && path.extension().is_none() {
        return Ok(banana_core::scan(path));
    }
    let source = banana_content::open(path).map_err(|e| e.to_string())?;
    Ok(banana_api::games()
        .iter()
        .map(|game| banana_core::report_source(*game, path, Ok(source.as_ref())))
        .collect())
}
