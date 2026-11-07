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
        self.handle.wait()
    }
    pub fn next_event(&mut self) -> Result<Option<SessionEvent>, String> {
        self.handle.next_event()
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.handle.cancel();
        let _ = self.handle.wait();
        self.mount.take();
        if let Some(v) = &self.view {
            let _ = v.flush();
        }
        let _ = std::fs::remove_dir(&self.point);
    }
}
pub fn start(id: &str, source_path: &Path, action: ActionRequest) -> Result<Session, String> {
    let game = banana_api::game(id).ok_or_else(|| format!("Unknown game {id}"))?;
    let resources = crate::project_root()?;
    let paths = Paths::discover()?;
    paths.initialize(&resources)?;
    let source = banana_content::open(source_path).map_err(|e| e.to_string())?;
    let build = game.identify(source.as_ref())?;
    let source: Arc<dyn banana_content::ContentSource> = Arc::new(banana_content::Subtree {
        source,
        root: build.source_root.clone(),
    });
    let fingerprint = source.fingerprint().map_err(|e| e.to_string())?;
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
    let settings = banana_settings::Settings::load(&paths.config.join("settings.cfg"))?
        .game(game.config_key())
        .clone();
    let controls = banana_input::controls::Bindings::load(&paths.config.join("controls.cfg"))?;
    let context = PlanContext {
        source: source.clone(),
        settings: settings.clone(),
        controls,
        config: paths.config.clone(),
        state: state.clone(),
        dependencies: paths.data.join("dependencies"),
        resources: resources.clone(),
    };
    let mut plan = game.plan(&build, &context, &action)?;
    if plan.operation == PlanOperation::PrepareDependencies {
        for dep in &plan.dependencies {
            crate::dependencies::prepare(
                dep,
                &context.dependencies.join(&dep.id),
                &paths.cache.join("packages").join(&dep.id),
            )?;
        }
        return Ok(Session {
            _entry_lock: entry_lock,
            game,
            state: state.clone(),
            handle: Box::new(Completed),
            mount: None,
            view: None,
            point: PathBuf::new(),
        });
    }
    let mut failures = Vec::new();
    let selected = plan
        .backends
        .iter()
        .filter_map(|k| banana_api::backend(*k))
        .find(|backend| match backend.check(&plan) {
            Ok(()) => true,
            Err(e) => {
                failures.push(e.to_string());
                false
            }
        })
        .ok_or_else(|| failures.join("\n"))?;
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
    for dep in &plan.dependencies {
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
    if view.has_content_override(&plan.executable) {
        return Err(format!(
            "Executable was modified in the persistent overlay. Reset {} before applying compatibility patches.",
            view.canonical_path(&plan.executable)
                .map_err(|e| e.to_string())?
        ));
    }
    for config in &plan.text_configs {
        crate::configuration::apply(&view, config)?;
    }
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
    let devices = crate::pad_devices(&settings.input_assignment());
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
    let mount = banana_vfs::mount(view.clone(), &point).map_err(|e| e.to_string())?;
    let handle = selected.launch(PreparedSession {
        devices,
        plan,
        mount: mount.path.clone(),
        state: state.clone(),
        resources,
        dependencies: context.dependencies,
    })?;
    Ok(Session {
        _entry_lock: entry_lock,
        game,
        state,
        handle,
        mount: Some(mount),
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
fn resource(root: &Path, artifact: &str) -> Result<PathBuf, String> {
    if crate::is_distribution(root) {
        return Ok(root.join(artifact));
    }
    let catalog = root.join("target/artifacts.json");
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
