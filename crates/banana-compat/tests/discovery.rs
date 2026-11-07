//! Layout discovery is provider-relative and never manufactures library folders.
use std::{collections::BTreeMap, sync::Arc};

#[test]
fn flat_legacy_and_own_folder_sources_identify_without_staging() {
    banana_compat::install();
    let image: Arc<[u8]> = std::fs::read(std::env::current_exe().unwrap())
        .unwrap()
        .into();
    for game in banana_api::games() {
        let mut roots = vec![String::new()];
        if let Some(flat) = game
            .default_executable()
            .strip_suffix(game.content_executable())
        {
            roots.push(flat.trim_end_matches('/').to_owned());
        }
        roots.extend(
            game.source_roots()
                .iter()
                .filter(|p| p.starts_with("windows based/") || p.starts_with("linux based/"))
                .map(|p| p.to_string()),
        );
        roots.sort();
        roots.dedup();
        for root in roots {
            let file = if root.is_empty() {
                game.content_executable().to_owned()
            } else {
                format!("{root}/{}", game.content_executable())
            };
            let source = banana_content::MemorySource {
                files: BTreeMap::from([(file, image.clone())]),
            };
            let identified = game
                .identify(&source)
                .unwrap_or_else(|error| panic!("{} in {root}: {error}", game.id()));
            assert_eq!(identified.source_root, root, "{} layout", game.id());
            assert!(
                !identified.known,
                "synthetic fixture must not become launchable"
            );
        }
    }
}
