//! The shared Banana Pepper application API for native frontends.
//!
//! Android's Kotlin/Jetpack Compose app calls these functions through
//! UniFFI-generated bindings, so game detection, compatibility logic and
//! settings validation are never reimplemented in Kotlin. Generate the
//! bindings with `uniffi-bindgen generate --library libbanana_ffi.so
//! --language kotlin`.
use std::path::Path;

uniffi::setup_scaffolding!();

#[derive(Debug, uniffi::Error)]
pub enum FfiError {
    Failed { message: String },
}

impl std::fmt::Display for FfiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Failed { message } => f.write_str(message),
        }
    }
}

impl std::error::Error for FfiError {}

impl From<String> for FfiError {
    fn from(message: String) -> Self {
        Self::Failed { message }
    }
}

#[derive(Debug, Clone, uniffi::Enum)]
pub enum Support {
    Verified,
    Experimental,
}

/// One action a frontend may offer for a game.
#[derive(Debug, Clone, uniffi::Record)]
pub struct GameAction {
    pub id: String,
    pub label: String,
}

/// A library entry: the game's identity plus what was found on disk.
#[derive(Debug, Clone, uniffi::Record)]
pub struct LibraryEntry {
    pub id: String,
    pub title: String,
    pub support: Support,
    pub status: String,
    pub executable: String,
    pub found: bool,
    pub error: Option<String>,
    pub notes: Vec<String>,
    pub actions: Vec<GameAction>,
    /// Settings summary, when the game declares settings.
    pub settings_summary: Option<String>,
}

/// Register the built-in games. Safe to call more than once.
#[uniffi::export]
pub fn initialize() {
    banana_compat::install();
}

/// Inspect every supported game below `root` (a folder or ZArchive).
#[uniffi::export]
pub fn scan_library(root: String, settings_path: Option<String>) -> Result<Vec<LibraryEntry>, FfiError> {
    initialize();
    let reports = banana_runtime::content::scan(Path::new(&root))?;
    let settings = match settings_path {
        Some(path) => banana_settings::Settings::load(Path::new(&path))?,
        None => banana_settings::Settings::default(),
    };
    Ok(reports
        .into_iter()
        .filter_map(|report| {
            let game = banana_api::game(&report.profile.id)?;
            let status = game.status();
            let mut actions = vec![GameAction { id: "launch".into(), label: "Play game".into() }];
            actions.extend(game.extra_actions().iter().map(|a| GameAction { id: a.id.into(), label: a.label.into() }));
            Some(LibraryEntry {
                id: report.profile.id.clone(),
                title: report.profile.title.clone(),
                support: match status.level {
                    banana_api::SupportLevel::Verified => Support::Verified,
                    banana_api::SupportLevel::Experimental => Support::Experimental,
                },
                status: status.label.into(),
                executable: report.path.display().to_string(),
                found: report.binary.is_some(),
                error: report.error.clone(),
                notes: report.blockers.clone(),
                actions,
                settings_summary: game
                    .settings_profile()
                    .map(|p| settings.game(p.id).summary()),
            })
        })
        .collect())
}

/// Run a game action (blocking). Returns the session's closing message.
#[uniffi::export]
pub fn run_game(id: String, root: String, action: String) -> Result<String, FfiError> {
    initialize();
    Ok(banana_runtime::run(&id, Path::new(&root), &action)?)
}

pub mod android;
