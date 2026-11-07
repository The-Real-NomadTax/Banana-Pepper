//! Desktop-only preferences, separate from game runtime settings.
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

#[derive(Default, Clone, Debug, PartialEq)]
pub struct Preferences {
    pub root: String,
    pub theme: crate::appearance::Theme,
    pub games: BTreeMap<String, String>,
    pub added: BTreeSet<String>,
}

fn preference_path() -> Result<PathBuf, String> {
    let base = if cfg!(target_os = "windows") {
        std::env::var_os("APPDATA").map(PathBuf::from)
    } else if cfg!(target_os = "macos") {
        std::env::var_os("HOME").map(|home| PathBuf::from(home).join("Library/Application Support"))
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .filter(|path| Path::new(path).is_absolute())
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
    };
    base.map(|base| base.join("banana-pepper/library.json"))
        .ok_or_else(|| "Cannot locate your user configuration directory.".into())
}

pub fn load() -> Result<Preferences, String> {
    load_from(&preference_path()?)
}

fn load_from(path: &Path) -> Result<Preferences, String> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Preferences {
                root: "games".into(),
                ..Default::default()
            });
        }
        Err(error) => return Err(format!("Could not read saved folders: {error}")),
    };
    let value: serde_json::Value = serde_json::from_slice(&bytes)
        .map_err(|error| format!("Could not read saved folders: {error}"))?;
    // Retain the single-folder preference from earlier desktop versions.
    if let Some(root) = value.as_str() {
        return Ok(Preferences {
            root: root.into(),
            ..Default::default()
        });
    }
    let root = value
        .get("root")
        .and_then(|v| v.as_str())
        .ok_or("Invalid saved library folder")?;
    let games: BTreeMap<String, String> =
        serde_json::from_value(value.get("games").cloned().unwrap_or(serde_json::json!({})))
            .map_err(|error| format!("Invalid saved game folders: {error}"))?;
    // Existing individually configured games were explicitly chosen by the user.
    let added = match value.get("added") {
        Some(value) => serde_json::from_value(value.clone())
            .map_err(|error| format!("Invalid saved user library: {error}"))?,
        None => games.keys().cloned().collect(),
    };
    Ok(Preferences {
        root: root.into(),
        games,
        added,
        theme: crate::appearance::Theme::from_id(
            value
                .get("theme")
                .and_then(|value| value.as_str())
                .unwrap_or_default(),
        ),
    })
}

pub fn save(preferences: &Preferences) -> Result<(), String> {
    write_to(&preference_path()?, preferences)
}

fn write_to(path: &Path, preferences: &Preferences) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(
        &serde_json::json!({"root": preferences.root, "games": preferences.games, "theme": preferences.theme.id(), "added": preferences.added}),
    )
    .map_err(|error| error.to_string())?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("Could not save folders: {error}"))?;
    }
    std::fs::write(path, bytes).map_err(|error| format!("Could not save folders: {error}"))
}

pub fn validate(folder: &str) -> Result<String, String> {
    normalize(folder)
}

fn normalize(folder: &str) -> Result<String, String> {
    if folder.trim().is_empty() {
        return Err("Enter a games folder before saving.".into());
    }
    let folder = std::fs::canonicalize(folder)
        .map_err(|error| format!("Cannot open games folder: {error}"))?;
    if !banana_content::supports(&folder) {
        return Err("Choose a supported game folder or content file.".into());
    }
    let folder = folder
        .to_str()
        .ok_or("The games folder path must contain valid Unicode.")?
        .to_owned();
    Ok(folder)
}

/// Selected source is passed intact to provider inspection and session creation.
pub fn runtime_root(game: &str, folder: &str) -> Result<PathBuf, String> {
    if banana_api::game(game).is_none() {
        return Err("Unknown game".into());
    }
    normalize(folder).map(PathBuf::from)
}

pub struct LibraryScan {
    pub reports: Vec<banana_core::GameReport>,
    pub game_roots: BTreeMap<String, Result<PathBuf, String>>,
}

/// Run on the inspection worker: both extraction and preference links can do I/O.
pub fn scan(root: &Path, games: &BTreeMap<String, String>) -> Result<LibraryScan, String> {
    let mut reports = banana_runtime::content::scan(root)?;
    let mut game_roots = BTreeMap::new();
    for (id, folder) in games {
        let root = runtime_root(id, folder).and_then(|root| {
            banana_content::open(&root)
                .map(|source| (root, source))
                .map_err(|e| e.to_string())
        });
        match root {
            Ok((root, source)) => {
                if let Some(game) = banana_api::game(id) {
                    if let Some(report) = reports.iter_mut().find(|r| r.profile.id == *id) {
                        *report = banana_core::report_source(game, &root, Ok(source.as_ref()));
                    }
                }
                game_roots.insert(id.clone(), Ok(root));
            }
            Err(error) => {
                if let Some(report) = reports.iter_mut().find(|r| r.profile.id == *id) {
                    report.binary = None;
                    report.error = Some(error.clone());
                }
                game_roots.insert(id.clone(), Err(error));
            }
        }
    }
    Ok(LibraryScan {
        reports,
        game_roots,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collection_is_explicit_persistent_and_independent_of_game_files() {
        banana_compat::install();
        let temp = tempfile::tempdir().unwrap();
        let config = temp.path().join("library.json");
        let mut prefs = load_from(&config).unwrap();
        assert!(prefs.added.is_empty());
        prefs
            .games
            .insert("aliens".into(), "local game files".into());
        // Finding or configuring files does not add a game to a new collection.
        write_to(&config, &prefs).unwrap();
        assert!(load_from(&config).unwrap().added.is_empty());
        prefs.added.insert("aliens".into());
        prefs.added.insert("aliens".into());
        write_to(&config, &prefs).unwrap();
        let mut reloaded = load_from(&config).unwrap();
        assert_eq!(reloaded.added.len(), 1);
        assert!(reloaded.added.contains("aliens"));
        reloaded.added.remove("aliens");
        write_to(&config, &reloaded).unwrap();
        let removed = load_from(&config).unwrap();
        assert!(removed.added.is_empty());
        assert_eq!(removed.games["aliens"], "local game files");
        // Migrate only explicitly configured games from older preferences.
        std::fs::write(
            &config,
            br#"{"root":"games","games":{"jurassic":"old folder"}}"#,
        )
        .unwrap();
        let legacy = load_from(&config).unwrap();
        assert_eq!(legacy.added, BTreeSet::from(["jurassic".into()]));
    }

    #[test]
    fn archive_preferences_persist_and_bad_archives_only_disable_their_game() {
        banana_compat::install();
        let temp = tempfile::tempdir().unwrap();
        let archive = temp.path().join("broken.ZAR");
        std::fs::write(&archive, b"not a ZArchive").unwrap();
        // Saving a source only validates its path; extraction belongs to the worker.
        let source = validate(archive.to_str().unwrap()).unwrap();
        let preferences = Preferences {
            root: temp.path().to_str().unwrap().into(),
            games: BTreeMap::from([("minecraft".into(), source)]),
            theme: crate::appearance::Theme::FieryRed,
            added: BTreeSet::from(["minecraft".into()]),
        };
        let config = temp.path().join("preferences/library.json");
        write_to(&config, &preferences).unwrap();
        assert_eq!(load_from(&config).unwrap(), preferences);
        let result = scan(temp.path(), &preferences.games).unwrap();
        assert_eq!(result.reports.len(), banana_core::profiles().len());
        assert!(result.game_roots["minecraft"].is_err());
        let minecraft = result
            .reports
            .iter()
            .find(|r| r.profile.id == "minecraft")
            .unwrap();
        assert!(
            minecraft
                .error
                .as_deref()
                .unwrap()
                .contains("Cannot load ZArchive")
        );
        for report in result
            .reports
            .iter()
            .filter(|r| r.profile.id != "minecraft")
        {
            assert!(
                !report
                    .error
                    .as_deref()
                    .unwrap()
                    .contains("Cannot load ZArchive")
            );
        }
        assert!(
            scan(&archive, &BTreeMap::new())
                .err()
                .unwrap()
                .contains("Cannot load ZArchive")
        );
    }

    #[test]
    fn independent_folders_survive_reload_and_old_settings_migrate() {
        banana_compat::install();
        let temp = std::env::temp_dir().join(format!("arcade-library-{}", std::process::id()));
        std::fs::create_dir_all(&temp).unwrap();
        let config = temp.join("config/library.json");
        assert_eq!(load_from(&config).unwrap().root, "games");
        std::fs::create_dir_all(config.parent().unwrap()).unwrap();
        std::fs::write(&config, br#""old library""#).unwrap();
        let mut prefs = load_from(&config).unwrap();
        assert_eq!(prefs.root, "old library");
        assert_eq!(prefs.theme, crate::appearance::Theme::Teal);
        prefs.theme = crate::appearance::Theme::FieryRed;
        for id in banana_core::profiles().into_iter().map(|p| p.id) {
            let folder = temp.join(format!("{id} game files"));
            std::fs::create_dir_all(&folder).unwrap();
            prefs
                .games
                .insert(id.into(), validate(folder.to_str().unwrap()).unwrap());
        }
        write_to(&config, &prefs).unwrap();
        assert_eq!(load_from(&config).unwrap(), prefs);
        for invalid in [
            "",
            "   ",
            config.to_str().unwrap(),
            temp.join("missing").to_str().unwrap(),
        ] {
            assert!(validate(invalid).is_err());
            assert_eq!(load_from(&config).unwrap(), prefs);
        }
        #[cfg(unix)]
        for profile in banana_core::profiles()
            .into_iter()
            .filter(|p| p.id != "minecraft")
        {
            let folder = Path::new(&prefs.games[&profile.id]);
            std::fs::write(
                folder.join(profile.executable.file_name().unwrap()),
                b"game data",
            )
            .unwrap();
            let root = runtime_root(&profile.id, folder.to_str().unwrap()).unwrap();
            assert_eq!(
                std::fs::read(root.join(profile.executable.file_name().unwrap())).unwrap(),
                b"game data"
            );
            assert_eq!(
                runtime_root(&profile.id, folder.to_str().unwrap()).unwrap(),
                root
            );
        }
        std::fs::write(&config, b"broken json").unwrap();
        assert!(load_from(&config).is_err());
        std::fs::remove_dir_all(temp).unwrap();
    }
}
