use crate::storage::Paths;
use banana_api::*;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Arc,
};
pub struct Session {
    _entry_lock: std::fs::File,
    game: &'static dyn GameCompat,
    state: PathBuf,
    handle: Box<dyn SessionHandle>,
    mount: Option<banana_vfs::MountLease>,
    view: Option<Arc<banana_vfs::View>>,
    point: PathBuf,
}
impl Session {
    pub fn command(&mut self, a: ActionRequest) -> Result<(), String> {
        self.game.validate_session_action(&a)?;
        self.handle.command(a)
    }
    pub fn cancel(&mut self) -> Result<(), String> {
        self.handle.cancel()
    }
    pub fn wait(&mut self) -> Result<i32, String> {
        let code = self.handle.wait()?;
        self.release_view();
        if let Some(mount) = &self.mount {
            mount.flush().map_err(|e| e.to_string())?;
        }
        Ok(code)
    }
    pub fn next_event(&mut self) -> Result<Option<SessionEvent>, String> {
        self.handle.next_event()
    }
}
impl Session {
    /// Wait (briefly) for backend threads serving the view to let go of it
    /// after the guest exits, so the overlay is free for the next session.
    fn release_view(&self) {
        // A mount keeps its own reference for the whole session.
        let (Some(view), None) = (&self.view, &self.mount) else {
            return;
        };
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while Arc::strong_count(view) > 1 && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.handle.cancel();
        let _ = self.handle.wait();
        self.release_view();
        self.mount.take();
        if let Some(v) = &self.view {
            let _ = v.flush();
        }
        let _ = std::fs::remove_dir(&self.point);
    }
}
pub struct PreparedView {
    _entry_lock: std::fs::File,
    pub game: &'static dyn GameCompat,
    pub plan: LaunchPlan,
    pub view: Arc<banana_vfs::View>,
    pub state: PathBuf,
    pub resources: PathBuf,
    pub paths: Paths,
    pub dependencies: PathBuf,
    pub backend: Option<BackendKind>,
}
pub fn prepare_view(
    id: &str,
    source_path: &Path,
    action: ActionRequest,
    paths: Paths,
    resources: PathBuf,
    configure: impl FnOnce(&mut banana_settings::GameSettings),
) -> Result<PreparedView, String> {
    let game = banana_api::game(id).ok_or_else(|| format!("Unknown game {id}"))?;
    paths.initialize(&resources)?;
    let source = banana_content::open(source_path).map_err(|e| e.to_string())?;
    let build = game.identify(source.as_ref())?;
    let source: Arc<dyn banana_content::ContentSource> = Arc::new(banana_content::Subtree {
        source,
        root: build.source_root.clone(),
    });
    let location = source_path.canonicalize().map_err(|e| e.to_string())?;
    let fingerprint = paths.fingerprint(&location, source.as_ref())?;
    let entry = paths.entry(game.id(), source_path, &fingerprint)?;
    let state = paths.state.join("entries").join(&entry);
    std::fs::create_dir_all(&state).map_err(|e| e.to_string())?;
    // Protect state imports and planning as well as the eventual writable view.
    let entry_lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(state.join("session.lock"))
        .map_err(|e| e.to_string())?;
    entry_lock
        .try_lock()
        .map_err(|_| "This library entry already has a writable session".to_owned())?;
    crate::migration::import(game, &resources, &state, None)?;
    #[cfg(target_os = "android")]
    if let Some(previous) = resources.parent() {
        crate::migration::import(game, previous, &state, None)?;
    }
    let mut settings = banana_settings::Settings::load(&paths.config.join("settings.cfg"))?
        .game(game.config_key())
        .clone();
    configure(&mut settings);
    let controls = banana_input::controls::Bindings::load(&paths.config.join("controls.cfg"))?;
    let context = PlanContext {
        source: source.clone(),
        settings: settings.clone(),
        controls,
        config: paths.config.clone(),
        state: state.clone(),
        dependencies: if cfg!(target_os = "android") {
            resources.join("dependencies")
        } else {
            paths.data.join("dependencies")
        },
        resources: resources.clone(),
    };
    let mut plan = game.plan(&build, &context, &action)?;
    // Reject unsupported hosts before resolving adapters for another platform.
    let backend = if plan.operation == PlanOperation::Launch {
        let mut failures = Vec::new();
        Some(
            plan.backends
                .iter()
                .filter_map(|kind| banana_api::backend(*kind))
                .find(|backend| match backend.check(&plan) {
                    Ok(()) => true,
                    Err(error) => {
                        failures.push(error.to_string());
                        false
                    }
                })
                .map(|backend| backend.kind())
                .ok_or_else(|| {
                    if failures.is_empty() {
                        "No registered backend supports this plan".into()
                    } else {
                        failures.join("\n")
                    }
                })?,
        )
    } else {
        None
    };
    if let Some(selected) = backend.and_then(banana_api::backend) {
        let extra = selected.adapters(&plan);
        plan.adapters.extend(extra);
    }
    plan.env
        .insert("BANANA_GUEST_ROOT".into(), plan.guest_root.clone());
    plan.generated
        .insert(".banana/proc/cpuinfo".into(), cpuinfo().into_bytes().into());

    for (k, v) in settings.session_env() {
        plan.env.entry(k.into()).or_insert(v);
    }
    plan.env
        .insert("ARCADE_INPUTS".into(), settings.inputs_env());
    plan.env
        .insert("ARCADE_PLAYERS".into(), settings.players.to_string());
    if !settings.audio_sink.is_empty() {
        plan.env
            .insert("PULSE_SINK".into(), settings.audio_sink.clone());
    }
    let config = &context.controls;
    for (k, v) in [
        ("ARCADE_PAD", config.pad_env_value(game.config_key())?),
        ("ARCADE_PAD_P2", config.pad_env_value_p2(game.config_key())?),
        (
            "ARCADE_PAD_KEYS",
            config.pad_key_env_value(game.config_key())?,
        ),
        (
            "ARCADE_PAD_KEYS_P2",
            config.pad_key_env_value_p2(game.config_key())?,
        ),
        ("ARCADE_MOVE", config.move_env_value(game.config_key(), 0)),
        (
            "ARCADE_MOVE_P2",
            config.move_env_value(game.config_key(), 1),
        ),
    ] {
        plan.env.insert(k.into(), v);
    }
    for adapter in &plan.adapters {
        let path = resource(&resources, &adapter.artifact)?;
        let bytes = std::fs::read(&path).map_err(|e| {
            format!(
                "Missing compiled adapter {}: {e}. Run just build.",
                path.display()
            )
        })?;
        plan.generated
            .insert(adapter.destination.clone(), bytes.into());
        if adapter.preload {
            plan.env.insert(
                "LD_PRELOAD".into(),
                format!("{}/{}", plan.guest_root, adapter.destination),
            );
        }
    }
    let mut mounts = BTreeMap::new();
    for dep in plan
        .dependencies
        .iter()
        .filter(|_| plan.operation == PlanOperation::Launch)
    {
        let directory = context.dependencies.join(&dep.id);
        if !directory.is_dir() {
            return Err(format!(
                "Missing dependency {} in {}",
                dep.id,
                directory.display()
            ));
        }
        mounts.insert(
            dep.destination.clone(),
            banana_content::open(&directory).map_err(|e| e.to_string())?,
        );
    }
    let source: Arc<dyn banana_content::ContentSource> = Arc::new(banana_content::MountedSources {
        base: source,
        mounts,
    });
    let view = Arc::new(
        banana_vfs::View::with_case(
            source,
            &paths.data.join("overlays").join(entry),
            &fingerprint,
            plan.generated.clone(),
            plan.aliases.clone(),
            plan.case_sensitive,
        )
        .map_err(|e| e.to_string())?,
    );
    crate::migration::import(game, &resources, &state, Some(&view))?;
    #[cfg(target_os = "android")]
    if let Some(previous) = resources.parent() {
        crate::migration::import(game, previous, &state, Some(&view))?;
    }
    if view.has_content_override(&plan.executable) {
        return Err(format!(
            "Executable was modified in the persistent overlay. Reset {} before applying compatibility patches.",
            view.canonical_path(&plan.executable)
                .map_err(|e| e.to_string())?
        ));
    }
    for config in &plan.binary_configs {
        crate::configuration::apply_binary(&view, config)?;
    }
    for config in &plan.text_configs {
        crate::configuration::apply(&view, config)?;
    }
    Ok(PreparedView {
        _entry_lock: entry_lock,
        game,
        plan,
        view,
        state,
        resources,
        dependencies: context.dependencies,
        backend,
        paths,
    })
}
pub fn start(id: &str, source_path: &Path, action: ActionRequest) -> Result<Session, String> {
    start_with(
        id,
        source_path,
        action,
        Paths::discover()?,
        crate::project_root()?,
        |_| {},
    )
}
pub fn start_with(
    id: &str,
    source_path: &Path,
    action: ActionRequest,
    paths: Paths,
    resources: PathBuf,
    configure: impl FnOnce(&mut banana_settings::GameSettings),
) -> Result<Session, String> {
    start_with_env(
        id,
        source_path,
        action,
        paths,
        resources,
        configure,
        BTreeMap::new(),
    )
}
pub fn start_with_env(
    id: &str,
    source_path: &Path,
    action: ActionRequest,
    paths: Paths,
    resources: PathBuf,
    configure: impl FnOnce(&mut banana_settings::GameSettings),
    environment: BTreeMap<String, String>,
) -> Result<Session, String> {
    let PreparedView {
        _entry_lock: entry_lock,
        game,
        mut plan,
        view,
        state,
        resources,
        paths,
        dependencies,
        backend,
    } = prepare_view(id, source_path, action, paths, resources, configure)?;
    plan.env.extend(environment);
    if plan.operation == PlanOperation::PrepareDependencies {
        for dep in &plan.dependencies {
            crate::dependencies::prepare(
                dep,
                &dependencies.join(&dep.id),
                &paths.cache.join("packages").join(&dep.id),
            )?;
        }
        return Ok(Session {
            _entry_lock: entry_lock,
            game,
            state,
            handle: Box::new(Completed),
            mount: None,
            view: None,
            point: PathBuf::new(),
        });
    }
    let selected = banana_api::backend(backend.ok_or("Missing prepared backend")?)
        .ok_or("Execution backend is no longer registered")?;
    let point = paths.cache.join("mounts").join(format!(
        "{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(point.parent().unwrap()).map_err(|e| e.to_string())?;
    #[cfg(target_os = "linux")]
    let devices = crate::pad_devices(
        &banana_settings::Settings::load(&paths.config.join("settings.cfg"))?
            .game(game.config_key())
            .input_assignment(),
    );
    #[cfg(not(target_os = "linux"))]
    let devices: Vec<PathBuf> = Vec::new();
    if !devices.is_empty() {
        plan.env.insert(
            "ARCADE_PAD_DEVICES".into(),
            devices
                .iter()
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>()
                .join(":"),
        );
    }
    // Only backends whose guest opens files by host path get an OS mount;
    // the others are served the view directly.
    let mount = if selected.needs_mount(&plan) {
        let mount = banana_vfs::mount(view.clone(), &point).map_err(|e| e.to_string())?;
        selected.prepare_namespace(&mut plan, &Namespace(&mount))?;
        Some(mount)
    } else {
        None
    };
    let handle = selected.launch(PreparedSession {
        devices,
        plan,
        mount: mount.as_ref().map(|m| m.path.clone()),
        view: Arc::new(ServedView(view.clone())),
        state: state.clone(),
        resources,
        dependencies,
    })?;
    Ok(Session {
        _entry_lock: entry_lock,
        game,
        state,
        handle,
        mount,
        view: Some(view),
        point,
    })
}
#[derive(Debug)]
pub struct SessionReport {
    pub message: String,
    pub state: PathBuf,
    pub capture: Option<PathBuf>,
}
pub fn run_report(id: &str, source: &Path, action: ActionRequest) -> Result<SessionReport, String> {
    let probe = action.id == "probe";
    let mut session = start(id, source, action)?;
    let code = session.wait()?;
    if code != 0 && !(probe && code == 124) {
        return Err(format!(
            "Game session exited with code {code}. Logs: {}",
            session.state.display()
        ));
    }
    let capture = session.state.join("screen.png");
    Ok(SessionReport {
        message: format!("Session closed. Logs: {}", session.state.display()),
        state: session.state.clone(),
        capture: capture.is_file().then_some(capture),
    })
}
pub fn run(id: &str, source: &Path, action: ActionRequest) -> Result<String, String> {
    run_report(id, source, action).map(|r| r.message)
}
/// Resolve a crate-declared artifact by name: packaged location in a
/// distribution (`artifacts.json`), build output in a checkout.
fn resource(root: &Path, artifact: &str) -> Result<PathBuf, String> {
    let catalog = if crate::is_distribution(root) {
        root.join("artifacts.json")
    } else {
        root.join("target/artifacts.json")
    };
    let map: BTreeMap<String, String> = serde_json::from_slice(
        &std::fs::read(&catalog)
            .map_err(|_| "Build artifact catalog is missing. Run just build.")?,
    )
    .map_err(|e| e.to_string())?;
    map.get(artifact)
        .map(|p| root.join(p))
        .ok_or_else(|| format!("Artifact {artifact} is not in the build catalog"))
}

fn cpuinfo() -> String {
    #[cfg(target_os="android")]
    return "processor\t: 0\nvendor_id\t: BananaPepper\ncpu MHz\t\t: 1000.000\nflags\t\t: fpu tsc mmx sse sse2\n".into();
    #[cfg(target_os = "linux")]
    if let Ok(text) = std::fs::read_to_string("/proc/cpuinfo") {
        return text;
    }
    #[cfg(all(windows, target_arch = "x86_64"))]
    {
        let start = std::time::Instant::now();
        let ticks = unsafe { std::arch::x86_64::_rdtsc() };
        std::thread::sleep(std::time::Duration::from_millis(50));
        let mhz = (unsafe { std::arch::x86_64::_rdtsc() } - ticks) as f64
            / start.elapsed().as_secs_f64()
            / 1_000_000.0;
        return format!(
            "processor\t: 0\nvendor_id\t: BananaPepper\ncpu MHz\t\t: {mhz:.3}\nflags\t\t: fpu tsc mmx sse sse2\n"
        );
    }
    #[allow(unreachable_code)]
    String::new()
}

struct ServedView(Arc<banana_vfs::View>);
impl SessionView for ServedView {
    fn serve(
        &self,
        reader: Box<dyn std::io::Read + Send>,
        writer: Box<dyn std::io::Write + Send>,
    ) -> std::io::Result<()> {
        banana_vfs::wire::serve(self.0.clone(), reader, writer)
    }
    fn read(&self, path: &str, limit: u64) -> std::io::Result<Vec<u8>> {
        let file = self.0.open_file(path)?;
        let len = file.metadata()?.len;
        if len > limit {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "View file exceeds read limit",
            ));
        }
        let mut bytes = vec![0; len as usize];
        let mut done = 0;
        while done < bytes.len() {
            match file.read(done as u64, &mut bytes[done..])? {
                0 => break,
                n => done += n,
            }
        }
        bytes.truncate(done);
        Ok(bytes)
    }
    fn read_dir(&self, path: &str) -> std::io::Result<Vec<banana_content::Entry>> {
        self.0.read_dir(path)
    }
    #[cfg(target_os = "linux")]
    fn serve_fuse(&self, device: std::fs::File) -> std::io::Result<()> {
        banana_vfs::serve_fuse(self.0.clone(), device.into())
    }
}

struct Namespace<'a>(&'a banana_vfs::MountLease);
impl GuestNamespace for Namespace<'_> {
    fn root(&self) -> &Path {
        &self.0.path
    }
    fn map(&self, guest: &str, source: &str, readonly: bool) -> Result<(), String> {
        self.0
            .map_namespace(guest, source, readonly)
            .map_err(|e| e.to_string())
    }
    fn endpoint(&self) -> Option<&Path> {
        self.0.namespace_endpoint()
    }
}

struct Completed;
impl SessionHandle for Completed {
    fn next_event(&mut self) -> Result<Option<SessionEvent>, String> {
        Ok(None)
    }
    fn command(&mut self, _: ActionRequest) -> Result<(), String> {
        Err("Session has completed".into())
    }
    fn cancel(&mut self) -> Result<(), String> {
        Ok(())
    }
    fn wait(&mut self) -> Result<i32, String> {
        Ok(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adapters_resolve_by_artifact_name() {
        let temp = tempfile::tempdir().unwrap();
        let dist = temp.path().join("dist");
        std::fs::create_dir_all(&dist).unwrap();
        std::fs::write(dist.join(crate::DIST_MARKER), "v2").unwrap();
        std::fs::write(
            dist.join("artifacts.json"),
            r#"{"hook":"games/demo/windows-x86/hook.dll"}"#,
        )
        .unwrap();
        assert_eq!(
            resource(&dist, "hook").unwrap(),
            dist.join("games/demo/windows-x86/hook.dll")
        );
        assert!(resource(&dist, "games/demo/windows-x86/hook.dll").is_err());

        let checkout = temp.path().join("checkout");
        std::fs::create_dir_all(checkout.join("target")).unwrap();
        let built = checkout.join("target/release/hook.dll");
        std::fs::write(
            checkout.join("target/artifacts.json"),
            serde_json::to_vec(&BTreeMap::from([("hook", &built)])).unwrap(),
        )
        .unwrap();
        assert_eq!(resource(&checkout, "hook").unwrap(), built);
    }
}
