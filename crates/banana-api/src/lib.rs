//! The Banana Pepper game compatibility API.
//!
//! Every supported title lives in its own crate under `game-compat/` and
//! implements [`GameCompat`]. Execution backends under `backends/` implement
//! [`ExecutionBackend`]. Shared crates and frontends only ever talk to games
//! through these traits; they never name an individual game.
use serde::{Deserialize, Serialize};
use std::{path::PathBuf, sync::OnceLock};

pub use banana_input::controls::Game as ControlLayout;
pub use banana_settings::Profile as SettingsProfile;

/// The cabinet's primary control scheme.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Controls {
    LightGuns,
    Driving,
    Joysticks,
}

/// How far a game's support has been confirmed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SupportLevel {
    /// Boots into scored gameplay with saves confirmed.
    Verified,
    Experimental,
}

#[derive(Debug, Clone, Copy)]
pub struct Status {
    pub level: SupportLevel,
    /// Short label shown in the library, e.g. "Desktop gameplay".
    pub label: &'static str,
}

/// Static metadata parsed from an executable header.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BinaryInfo {
    pub format: String,
    pub bits: u8,
    pub machine: u16,
    pub entry: u64,
    pub interpreter: Option<String>,
    pub libraries: Vec<String>,
    pub search_paths: Vec<String>,
    pub section_count: usize,
}

/// A recognised executable build, identified by its SHA-256.
#[derive(Debug, Clone, Copy)]
pub struct KnownBuild {
    pub name: &'static str,
    pub sha256: &'static str,
}

/// Something a frontend can ask a game to do. `Launch` is always offered
/// once an executable is found; games add others through
/// [`GameCompat::extra_actions`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Action {
    /// Stable identifier passed back to [`GameCompat::plan`].
    pub id: &'static str,
    /// Button label.
    pub label: &'static str,
}

impl Action {
    pub const LAUNCH: Action = Action {
        id: "launch",
        label: "Play game",
    };
    pub const PROBE: Action = Action {
        id: "probe",
        label: "Startup diagnostics",
    };
}

/// A command-line entry point, e.g. `--launch-aliens [games-root]`.
#[derive(Debug, Clone, Copy)]
pub struct CliCommand {
    pub flag: &'static str,
    /// [`Action::id`] passed to [`GameCompat::plan`].
    pub action: &'static str,
    /// Printed before the run starts (may be empty).
    pub banner: &'static str,
}

/// Families of execution backend a game may require.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BackendKind {
    /// The host runs the executable directly (Linux ELF in a sandbox,
    /// Windows PE on Windows).
    Native,
    /// Windows executables through Wine.
    Wine,
    /// Linux executables translated to Windows by Water.
    Water,
}

/// Whether a backend can run on this host right now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Availability {
    Available,
    /// Supported on this host but a component is missing.
    Missing(String),
    /// Not supported on this host at all.
    Unsupported,
}

/// A reusable mechanism for running guest executables. Backends never
/// contain game-specific patches.
pub trait ExecutionBackend: Sync {
    fn kind(&self) -> BackendKind;
    fn name(&self) -> &'static str;
    /// Executable formats this backend can run (e.g. "ELF", "PE").
    fn formats(&self) -> &'static [&'static str];
    fn availability(&self) -> Availability;
    fn check(&self, plan: &LaunchPlan) -> Result<(), CapabilityError>;
    /// Generic guest components this backend places in the view for a plan
    /// (resolved like title adapters), e.g. the Windows view shim.
    fn adapters(&self, _plan: &LaunchPlan) -> Vec<Adapter> {
        Vec::new()
    }
    fn launch(&self, session: PreparedSession) -> Result<Box<dyn SessionHandle>, String>;
}

/// Everything unique to one supported title.
pub trait GameCompat: Sync {
    /// Library identifier, e.g. `"big-buck"`.
    fn id(&self) -> &'static str;
    /// Section name in `settings.cfg`/`controls.cfg`, e.g. `"big_buck"`.
    fn config_key(&self) -> &'static str;
    fn title(&self) -> &'static str;
    fn controls(&self) -> Controls;
    /// Executable path relative to a games library root.
    fn default_executable(&self) -> &'static str;
    /// Recognised executable builds.
    fn known_builds(&self) -> &'static [KnownBuild] {
        &[]
    }
    fn status(&self) -> Status;
    /// Compatibility notes shown in the library. `None` uses the shared
    /// notes derived from the executable format.
    fn compatibility_notes(&self) -> Option<&'static [&'static str]> {
        None
    }
    /// Execution backends this game can use.
    fn backends(&self) -> &'static [BackendKind];
    /// Settings the game honours; `None` hides graphics/audio settings.
    fn settings_profile(&self) -> Option<&'static SettingsProfile> {
        None
    }
    /// Remappable cabinet actions; `None` hides the controls tab.
    fn control_layout(&self) -> Option<&'static ControlLayout> {
        None
    }
    /// Additional buttons offered next to "Play game".
    fn extra_actions(&self) -> &'static [Action] {
        &[]
    }
    fn cli_commands(&self) -> &'static [CliCommand] {
        &[]
    }
    fn android_profile(&self) -> Option<AndroidProfile> {
        None
    }
    fn content_executable(&self) -> &'static str {
        self.default_executable().rsplit('/').next().unwrap()
    }
    fn session_actions(&self) -> &'static [SessionAction] {
        &[]
    }
    fn validate_session_action(&self, action: &ActionRequest) -> Result<(), String> {
        if action.id == "cancel" {
            Ok(())
        } else {
            Err(format!("Unsupported session action {}", action.id))
        }
    }
    fn legacy_state(&self) -> &'static [LegacyImport] {
        &[]
    }
    fn source_roots(&self) -> &'static [&'static str] {
        &[]
    }
    fn executable_names(&self) -> &'static [&'static str] {
        &[]
    }
    fn identify(
        &self,
        source: &dyn banana_content::ContentSource,
    ) -> Result<IdentifiedBuild, String> {
        identify(self, source)
    }
    fn plan(
        &self,
        build: &IdentifiedBuild,
        context: &PlanContext,
        action: &ActionRequest,
    ) -> Result<LaunchPlan, String>;
}

static GAMES: OnceLock<Vec<&'static dyn GameCompat>> = OnceLock::new();
static BACKENDS: OnceLock<Vec<&'static dyn ExecutionBackend>> = OnceLock::new();

/// Register the built-in games and backends. Called once by `banana-compat`;
/// later calls are ignored.
pub fn install(games: Vec<&'static dyn GameCompat>, backends: Vec<&'static dyn ExecutionBackend>) {
    let _ = GAMES.set(games);
    let _ = BACKENDS.set(backends);
}

/// Registered games, in library order.
pub fn games() -> &'static [&'static dyn GameCompat] {
    GAMES.get().map(Vec::as_slice).unwrap_or(&[])
}

pub fn game(id: &str) -> Option<&'static dyn GameCompat> {
    games().iter().copied().find(|g| g.id() == id)
}

/// Registered execution backends.
pub fn backends() -> &'static [&'static dyn ExecutionBackend] {
    BACKENDS.get().map(Vec::as_slice).unwrap_or(&[])
}

pub fn backend(kind: BackendKind) -> Option<&'static dyn ExecutionBackend> {
    backends().iter().copied().find(|b| b.kind() == kind)
}

mod launch;
pub use launch::*;
pub mod process;

/// Platform components required by the common plan on an Android host.
#[derive(Debug, Clone, Serialize)]
pub struct AndroidProfile {
    pub executable: &'static str,
    pub requirements: &'static [&'static str],
    pub status: &'static str,
    pub verified: bool,
}
