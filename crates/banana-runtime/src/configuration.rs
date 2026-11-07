//! Declarative property updates applied to the current view, including saved overrides.
use banana_api::TextConfig;
use std::collections::BTreeSet;
pub fn apply(view: &banana_vfs::View, config: &TextConfig) -> Result<(), String> {
    let m = view.metadata(&config.path).map_err(|e| e.to_string())?;
    if m.len > 1024 * 1024 {
        return Err("Configuration exceeds 1 MiB".into());
    }
    let mut bytes = vec![0; m.len as usize];
    let n = view
        .read(&config.path, 0, &mut bytes)
        .map_err(|e| e.to_string())?;
    bytes.truncate(n);
    let source = std::str::from_utf8(&bytes).map_err(|e| e.to_string())?;
    let mut seen = BTreeSet::new();
    let mut lines = Vec::new();
    for line in source.lines() {
        if let Some((key, _)) = line.split_once('=') {
            let key = key.trim();
            if let Some(value) = config.values.get(key) {
                seen.insert(key.to_owned());
                lines.push(format!("{key}={value}"));
                continue;
            }
        }
        lines.push(line.to_owned());
    }
    for (key, value) in &config.values {
        if !seen.contains(key) {
            lines.push(format!("{key}={value}"));
        }
    }
    let mut updated = lines.join(config.newline);
    if source.ends_with('\n') {
        updated.push_str(config.newline);
    }
    if updated.as_bytes() != bytes {
        view.replace_content(&config.path, updated.as_bytes())
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::{collections::BTreeMap, path::Path, sync::Arc};
    #[test]
    fn settings_merge_current_game_writes_without_copying_originals() {
        let t = tempfile::tempdir_in(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target"))
            .unwrap();
        let source = Arc::new(banana_content::MemorySource {
            files: BTreeMap::from([(
                "settings.ini".into(),
                Arc::from(&b"Resolution=640x480\nOther=original\n"[..]),
            )]),
        });
        let view = banana_vfs::View::new(
            source.clone(),
            t.path(),
            "fixture",
            BTreeMap::new(),
            BTreeMap::new(),
        )
        .unwrap();
        view.replace_content("settings.ini", b"Resolution=800x600\nOther=user\n")
            .unwrap();
        apply(
            &view,
            &TextConfig {
                path: "settings.ini".into(),
                values: BTreeMap::from([
                    ("Resolution".into(), "1024x768".into()),
                    ("Fullscreen".into(), "off".into()),
                ]),
                newline: "\n",
            },
        )
        .unwrap();
        let mut bytes = vec![0; view.metadata("settings.ini").unwrap().len as usize];
        view.read("settings.ini", 0, &mut bytes).unwrap();
        assert_eq!(
            std::str::from_utf8(&bytes).unwrap(),
            "Resolution=1024x768\nOther=user\nFullscreen=off\n"
        );
        assert_eq!(
            banana_content::read_all(source.as_ref(), "settings.ini", 1024).unwrap(),
            b"Resolution=640x480\nOther=original\n"
        );
    }
}
