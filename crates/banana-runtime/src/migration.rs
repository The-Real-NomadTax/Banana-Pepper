//! Idempotent imports selected by compatibility modules; old files are never removed.
use banana_api::{GameCompat, ImportKind};
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, fs, io, path::Path};
pub fn import(
    game: &dyn GameCompat,
    old: &Path,
    state: &Path,
    view: Option<&banana_vfs::View>,
) -> Result<(), String> {
    let ledger = state.join("imports.json");
    let mut done: BTreeSet<String> = match fs::read(&ledger) {
        Ok(b) => serde_json::from_slice(&b).map_err(|e| e.to_string())?,
        Err(e) if e.kind() == io::ErrorKind::NotFound => BTreeSet::new(),
        Err(e) => return Err(e.to_string()),
    };
    for rule in game.legacy_state() {
        if matches!(rule.kind, ImportKind::Overlay) && view.is_none() {
            continue;
        }
        banana_content::path(rule.from).map_err(|e| e.to_string())?;
        if !rule.to.is_empty() {
            banana_content::path(rule.to).map_err(|e| e.to_string())?;
        }
        let source = old.join(rule.from);
        if !source.exists() {
            continue;
        }
        let mut todo = vec![(source, rule.to.to_owned())];
        while let Some((source, target)) = todo.pop() {
            let m = fs::symlink_metadata(&source).map_err(|e| e.to_string())?;
            if m.file_type().is_symlink() {
                return Err(format!(
                    "Legacy state contains a link: {}",
                    source.display()
                ));
            }
            if m.is_dir() {
                for e in fs::read_dir(&source).map_err(|e| e.to_string())? {
                    let e = e.map_err(|e| e.to_string())?;
                    let name = e
                        .file_name()
                        .into_string()
                        .map_err(|_| "Legacy state names must be UTF-8")?;
                    todo.push((
                        e.path(),
                        if target.is_empty() {
                            name
                        } else {
                            format!("{target}/{name}")
                        },
                    ));
                }
                continue;
            }
            if !m.is_file() {
                return Err("Legacy state contains a special file".into());
            }
            let bytes = fs::read(&source).map_err(|e| e.to_string())?;
            let key = format!("{}:{:x}", source.display(), Sha256::digest(&bytes));
            if done.contains(&key) {
                continue;
            }
            match rule.kind {
                ImportKind::State => {
                    let destination = state.join(&target);
                    if let Ok(existing) = fs::read(&destination) {
                        if existing != bytes {
                            return Err(format!(
                                "Migration conflict at {}. Both copies have been retained.",
                                destination.display()
                            ));
                        }
                    } else {
                        fs::create_dir_all(destination.parent().unwrap())
                            .map_err(|e| e.to_string())?;
                        let mut f = fs::OpenOptions::new()
                            .create_new(true)
                            .write(true)
                            .open(destination)
                            .map_err(|e| e.to_string())?;
                        use std::io::Write;
                        f.write_all(&bytes).map_err(|e| e.to_string())?;
                        f.sync_all().map_err(|e| e.to_string())?;
                    }
                }
                ImportKind::Overlay => {
                    let view = view.ok_or("Overlay not prepared")?;
                    if view.has_overlay(&target) {
                        let len = view.metadata(&target).map_err(|e| e.to_string())?.len;
                        let mut existing = vec![0; len as usize];
                        view.read(&target, 0, &mut existing)
                            .map_err(|e| e.to_string())?;
                        if existing != bytes {
                            return Err(format!(
                                "Migration conflict for {target}. Existing overlay and legacy files have been retained."
                            ));
                        }
                    } else {
                        let components = target.split('/').collect::<Vec<_>>();
                        for i in 1..components.len() {
                            let parent = components[..i].join("/");
                            if view.metadata(&parent).is_err() {
                                view.create(&parent, true).map_err(|e| e.to_string())?;
                            }
                        }
                        if view.metadata(&target).is_err() {
                            view.create(&target, false).map_err(|e| e.to_string())?;
                        }
                        view.truncate(&target, 0).map_err(|e| e.to_string())?;
                        view.write(&target, 0, &bytes).map_err(|e| e.to_string())?;
                    }
                }
            }
            done.insert(key);
            let pending = ledger.with_extension("pending");
            fs::write(
                &pending,
                serde_json::to_vec(&done).map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?;
            fs::rename(pending, &ledger).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use banana_api::*;
    use std::{collections::BTreeMap, sync::Arc};
    struct TestGame;
    impl GameCompat for TestGame {
        fn id(&self) -> &'static str {
            "fixture"
        }
        fn config_key(&self) -> &'static str {
            "fixture"
        }
        fn title(&self) -> &'static str {
            "Fixture"
        }
        fn controls(&self) -> Controls {
            Controls::Joysticks
        }
        fn default_executable(&self) -> &'static str {
            "game"
        }
        fn status(&self) -> Status {
            Status {
                level: SupportLevel::Experimental,
                label: "fixture",
            }
        }
        fn play_hint(&self) -> &'static str {
            ""
        }
        fn backends(&self) -> &'static [BackendKind] {
            &[BackendKind::Native]
        }
        fn legacy_state(&self) -> &'static [LegacyImport] {
            &[
                LegacyImport {
                    from: "runtime/fixture/saves",
                    to: "saves",
                    kind: ImportKind::Overlay,
                },
                LegacyImport {
                    from: "runtime/fixture/seeds.json",
                    to: "seeds.json",
                    kind: ImportKind::State,
                },
            ]
        }
        fn plan(
            &self,
            _: &IdentifiedBuild,
            _: &PlanContext,
            _: &ActionRequest,
        ) -> Result<LaunchPlan, String> {
            Err("No fixture process".into())
        }
    }
    #[test]
    fn imports_are_idempotent_preserve_new_writes_and_report_conflicts() {
        let temp = tempfile::tempdir_in(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target"))
            .unwrap();
        let old = temp.path().join("old");
        let state = temp.path().join("state");
        fs::create_dir_all(old.join("runtime/fixture/saves")).unwrap();
        fs::create_dir(&state).unwrap();
        fs::write(old.join("runtime/fixture/seeds.json"), b"records").unwrap();
        fs::write(old.join("runtime/fixture/saves/save"), b"legacy").unwrap();
        fs::write(
            old.join("runtime/fixture/game"),
            b"never import the executable",
        )
        .unwrap();
        import(&TestGame, &old, &state, None).unwrap();
        assert_eq!(fs::read(state.join("seeds.json")).unwrap(), b"records");
        let source = Arc::new(banana_content::MemorySource::default());
        let view = banana_vfs::View::new(
            source,
            &temp.path().join("overlay"),
            "fixture",
            BTreeMap::new(),
            BTreeMap::new(),
        )
        .unwrap();
        import(&TestGame, &old, &state, Some(&view)).unwrap();
        let mut b = [0; 6];
        view.read("saves/save", 0, &mut b).unwrap();
        assert_eq!(&b, b"legacy");
        view.write("saves/save", 0, b"newest").unwrap();
        import(&TestGame, &old, &state, Some(&view)).unwrap();
        view.read("saves/save", 0, &mut b).unwrap();
        assert_eq!(&b, b"newest");
        fs::write(old.join("runtime/fixture/saves/save"), b"changed").unwrap();
        assert!(
            import(&TestGame, &old, &state, Some(&view))
                .unwrap_err()
                .contains("conflict")
        );
        assert!(view.metadata("game").is_err());
        assert_eq!(
            fs::read(old.join("runtime/fixture/game")).unwrap(),
            b"never import the executable"
        );
    }
}
