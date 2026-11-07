use super::*;
use banana_content::{ContentSource, read_all};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, sync::Arc};
#[derive(Clone)]
pub struct IdentifiedBuild {
    pub source_root: String,
    pub executable: String,
    pub sha256: String,
    pub bytes: Arc<[u8]>,
    pub binary: BinaryInfo,
    pub known: bool,
}
pub fn identify<G: GameCompat + ?Sized>(
    game: &G,
    source: &dyn ContentSource,
) -> Result<IdentifiedBuild, String> {
    let default = game.default_executable().replace('\\', "/");
    let file = game.content_executable();
    let dir = default
        .strip_suffix(file)
        .unwrap_or("")
        .trim_end_matches('/');
    let basename = dir.rsplit('/').next().unwrap_or("");
    let mut roots = game
        .source_roots()
        .iter()
        .map(|p| p.to_string())
        .collect::<Vec<_>>();
    roots.extend([dir.to_owned(), basename.to_owned(), String::new()]);
    roots.dedup();
    let names = if game.executable_names().is_empty() {
        vec![file]
    } else {
        game.executable_names().to_vec()
    };
    for root in roots {
        for name in &names {
            let p = if root.is_empty() {
                name.to_string()
            } else {
                format!("{root}/{name}")
            };
            if source.metadata(&p).is_err() {
                continue;
            }
            let bytes = read_all(source, &p, 256 * 1024 * 1024).map_err(|e| e.to_string())?;
            let hash = format!("{:x}", Sha256::digest(&bytes));
            let binary = inspect_bytes(&bytes)?;
            return Ok(IdentifiedBuild {
                source_root: root,
                executable: name.to_string(),
                known: game.known_builds().iter().any(|b| b.sha256 == hash),
                sha256: hash,
                bytes: bytes.into(),
                binary,
            });
        }
    }
    Err(format!(
        "{} executable not found in the selected content source",
        game.title()
    ))
}
pub fn inspect_bytes(bytes: &[u8]) -> Result<BinaryInfo, String> {
    match goblin::Object::parse(bytes).map_err(|e| e.to_string())? {
        goblin::Object::Elf(e) => Ok(BinaryInfo {
            format: "ELF".into(),
            bits: if e.is_64 { 64 } else { 32 },
            machine: e.header.e_machine,
            entry: e.entry,
            interpreter: e.interpreter.map(str::to_owned),
            libraries: e.libraries.iter().map(|s| s.to_string()).collect(),
            search_paths: e
                .rpaths
                .iter()
                .chain(e.runpaths.iter())
                .map(|s| s.to_string())
                .collect(),
            section_count: e.section_headers.len(),
        }),
        goblin::Object::PE(p) => Ok(BinaryInfo {
            format: "PE".into(),
            bits: if p.is_64 { 64 } else { 32 },
            machine: p.header.coff_header.machine,
            entry: p.image_base + p.entry as u64,
            interpreter: None,
            libraries: p.libraries.iter().map(|s| s.to_string()).collect(),
            search_paths: vec![],
            section_count: p.sections.len(),
        }),
        _ => Err("Expected ELF or PE executable".into()),
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ActionRequest {
    pub id: String,
    #[serde(default)]
    pub payload: serde_json::Value,
}
impl ActionRequest {
    pub fn new(id: &str) -> Self {
        Self {
            id: id.into(),
            payload: serde_json::Value::Null,
        }
    }
}
pub struct PlanContext {
    pub source: Arc<dyn ContentSource>,
    pub settings: banana_settings::GameSettings,
    pub controls: banana_input::controls::Bindings,
    pub config: PathBuf,
    pub state: PathBuf,
    pub dependencies: PathBuf,
    pub resources: PathBuf,
}
#[derive(Clone, Debug)]
pub struct Adapter {
    /// Artifact `name` from the owning crate's `[[package.metadata.banana.artifacts]]`;
    /// the build catalog resolves where it was built or packaged.
    pub artifact: String,
    pub destination: String,
    pub preload: bool,
}
#[derive(Clone, Debug)]
pub struct GuestMount {
    pub source: String,
    pub target: String,
    pub readonly: bool,
}
#[derive(Clone, Debug)]
pub struct Dependency {
    pub id: String,
    pub destination: String,
    pub recipe: Option<DependencyRecipe>,
}
#[derive(Clone, Debug)]
pub struct DependencyRecipe {
    pub deb: &'static [&'static str],
    pub downloads: Vec<DownloadSpec>,
}
#[derive(Clone, Debug)]
pub struct DownloadSpec {
    pub url: &'static str,
    pub sha256: &'static str,
    pub limit: u64,
    pub members: Vec<(&'static str, &'static str)>,
}
#[derive(Clone, Debug)]
pub struct TextConfig {
    pub path: String,
    pub values: BTreeMap<String, String>,
    pub newline: &'static str,
}
#[derive(Clone, Debug)]
pub struct ByteEdit {
    pub offset: usize,
    pub accepted: Vec<Vec<u8>>,
    pub replacement: Vec<u8>,
}
#[derive(Clone, Debug)]
pub struct BinaryConfig {
    pub path: String,
    pub checks: Vec<(usize, Vec<u8>)>,
    pub edits: Vec<ByteEdit>,
}
#[derive(Clone)]
pub struct LaunchPlan {
    pub game: String,
    pub build: String,
    pub binary: BinaryInfo,
    pub executable: String,
    pub cwd: String,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub aliases: BTreeMap<String, String>,
    pub generated: BTreeMap<String, Arc<[u8]>>,
    pub text_configs: Vec<TextConfig>,
    pub binary_configs: Vec<BinaryConfig>,
    pub adapters: Vec<Adapter>,
    pub dependencies: Vec<Dependency>,
    pub requirements: Vec<String>,
    pub backends: Vec<BackendKind>,
    pub timeout_ms: Option<u64>,
    pub action: ActionRequest,
    pub guest_root: String,
    pub inert_tools: Vec<String>,
    pub case_sensitive: bool,
    pub input: Option<InputBridge>,
    pub operation: PlanOperation,
    pub command_channel: Option<String>,
    pub guest_mounts: Vec<GuestMount>,
}
impl LaunchPlan {
    pub fn new(
        game: &dyn GameCompat,
        build: &IdentifiedBuild,
        action: &ActionRequest,
    ) -> Result<Self, String> {
        if !matches!(action.id.as_str(), "launch" | "probe")
            && !game.extra_actions().iter().any(|a| a.id == action.id)
        {
            return Err(format!("Unknown action {}", action.id));
        }
        if !build.known {
            return Err(format!(
                "Unrecognized {} build: SHA256 {}. No patches applied.",
                game.title(),
                build.sha256
            ));
        }
        Ok(Self {
            game: game.id().into(),
            build: build.sha256.clone(),
            binary: build.binary.clone(),
            executable: build.executable.clone(),
            cwd: String::new(),
            args: vec![],
            env: BTreeMap::new(),
            aliases: BTreeMap::new(),
            generated: BTreeMap::new(),
            text_configs: vec![],
            binary_configs: vec![],
            adapters: vec![],
            dependencies: vec![],
            requirements: vec![],
            backends: game.backends().to_vec(),
            timeout_ms: (action.id == "probe").then_some(45000),
            action: action.clone(),
            guest_root: "/pm".into(),
            inert_tools: vec![],
            case_sensitive: build.binary.format != "PE",
            input: None,
            operation: PlanOperation::Launch,
            command_channel: None,
            guest_mounts: vec![],
        })
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CapabilityError {
    pub backend: String,
    pub missing: Vec<String>,
}
impl std::fmt::Display for CapabilityError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} cannot run this build: {}",
            self.backend,
            self.missing.join("; ")
        )
    }
}
impl std::error::Error for CapabilityError {}
pub struct PreparedSession {
    pub devices: Vec<PathBuf>,
    pub plan: LaunchPlan,
    /// Host directory exposing the view, for backends whose guest opens
    /// files by host path ([`ExecutionBackend::needs_mount`]).
    pub mount: Option<PathBuf>,
    /// The session's composed filesystem, reachable without a mount.
    pub view: Arc<dyn SessionView>,
    pub state: PathBuf,
    pub resources: PathBuf,
    pub dependencies: PathBuf,
}
impl PreparedSession {
    /// The mount directory; an error for backends that run without one.
    pub fn mount(&self) -> Result<&std::path::Path, String> {
        self.mount
            .as_deref()
            .ok_or_else(|| "This session's view is not mounted".into())
    }
}

/// A session's composed filesystem (source, generated files and overlay),
/// owned by the session host. Backends that intercept guest file calls serve
/// it to the guest instead of mounting it.
pub trait SessionView: Send + Sync {
    /// Serve the `banana_vfs::wire` protocol on one connection until the
    /// client closes it.
    fn serve(
        &self,
        reader: Box<dyn std::io::Read + Send>,
        writer: Box<dyn std::io::Write + Send>,
    ) -> std::io::Result<()>;
    /// Read a whole view file, bounded by `limit` bytes.
    fn read(&self, path: &str, limit: u64) -> std::io::Result<Vec<u8>>;
    /// List a view directory.
    fn read_dir(&self, path: &str) -> std::io::Result<Vec<banana_content::Entry>>;
    /// Serve a FUSE connection (an opened `/dev/fuse` that the session's own
    /// namespace mounted) until that mount ends. Linux only.
    fn serve_fuse(&self, _device: std::fs::File) -> std::io::Result<()> {
        Err(std::io::ErrorKind::Unsupported.into())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum SessionEvent {
    Started,
    ActionDispatched(String),
    TimedOut,
    Exited(i32),
}
pub trait SessionHandle: Send {
    fn next_event(&mut self) -> Result<Option<SessionEvent>, String>;
    fn command(&mut self, action: ActionRequest) -> Result<(), String>;
    fn cancel(&mut self) -> Result<(), String>;
    fn wait(&mut self) -> Result<i32, String>;
}
pub fn check_host(
    backend: &dyn ExecutionBackend,
    plan: &LaunchPlan,
) -> Result<(), CapabilityError> {
    let mut missing = Vec::new();
    match backend.availability() {
        Availability::Available => {}
        Availability::Missing(s) => missing.push(s),
        Availability::Unsupported => missing.push("Backend is unavailable on this host".into()),
    }
    if !backend.formats().contains(&plan.binary.format.as_str()) {
        missing.push(format!(
            "Unsupported executable format {}",
            plan.binary.format
        ))
    }
    if missing.is_empty() {
        Ok(())
    } else {
        Err(CapabilityError {
            backend: backend.name().into(),
            missing,
        })
    }
}

#[derive(Clone, Copy)]
pub enum ImportKind {
    Overlay,
    State,
}
#[derive(Clone, Copy)]
pub struct LegacyImport {
    pub from: &'static str,
    pub to: &'static str,
    pub kind: ImportKind,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PlanOperation {
    Launch,
    PrepareDependencies,
}

#[derive(Clone, Debug)]
pub struct InputBridge {
    pub virtual_pads: u8,
    pub keys: Vec<(u16, u16)>,
    pub focus_titles: Vec<String>,
}

#[derive(Clone, Copy)]
pub struct SessionAction {
    pub id: &'static str,
    pub label: &'static str,
    pub schema: &'static str,
}
