//! Wine execution backend for Windows guests on Linux hosts. Wine, its
//! server and the game run inside one session namespace that holds the
//! served view (no host mount); the prefix links its game drive there.
use banana_api::{Availability, BackendKind, ExecutionBackend};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::{Command, ExitStatus},
};

/// Private prefix lifecycle and Wine registry operations, independent of titles.
/// Puts a command into the session namespace (with a working directory there).
pub type Enter = std::sync::Arc<dyn Fn(&mut Command, Option<&Path>) + Send + Sync>;

pub struct Prefix {
    pub path: PathBuf,
    pub env: BTreeMap<String, String>,
    wine: PathBuf,
    log: PathBuf,
    server: PathBuf,
    closed: bool,
    enter: Option<Enter>,
}
impl Prefix {
    pub fn new(path: &Path, log: &Path, architecture: &str) -> Result<Self, String> {
        Self::new_in(path, log, architecture, None)
    }
    /// A prefix whose Wine processes (and server) all run through `enter`.
    pub fn new_in(
        path: &Path,
        log: &Path,
        architecture: &str,
        enter: Option<Enter>,
    ) -> Result<Self, String> {
        fs::File::create(log).map_err(|e| e.to_string())?;
        let wine = std::env::var_os("ARCADE_WINE")
            .map(PathBuf::from)
            .or_else(|| find("wine"))
            .ok_or("Wine is required; install it or set ARCADE_WINE")?;
        let server = server_for(&wine).ok_or("wineserver is required for owned Wine sessions")?;
        let p = Self {
            server,
            closed: false,
            enter,
            path: path.to_owned(),
            wine,
            log: log.to_owned(),
            env: BTreeMap::from([
                ("WINEPREFIX".into(), path.to_string_lossy().into_owned()),
                ("WINEARCH".into(), architecture.into()),
                ("WINEDEBUG".into(), "-all".into()),
                ("WINEDLLOVERRIDES".into(), "mscoree,mshtml=".into()),
            ]),
        };
        if !path.join("system.reg").exists() {
            let mut c = p.command();
            c.args(["wineboot", "-i"]);
            if !p.run(&mut c)?.success() {
                return Err("Wine prefix initialization failed".into());
            }
            let mut c = p.server_command();
            c.arg("-w");
            if !p.run(&mut c)?.success() {
                return Err("Wine prefix setup did not complete".into());
            }
        }
        let z = path.join("dosdevices/z:");
        if z.is_symlink() {
            fs::remove_file(z).map_err(|e| e.to_string())?;
        }
        Ok(p)
    }
    fn close(&mut self) -> Result<(), String> {
        if self.closed {
            return Ok(());
        }
        let output = banana_api::process::utilities::bounded_output(
            self.server_command().args(["-k", "-w"]),
            std::time::Duration::from_secs(5),
            65536,
        )?;
        match output {
            Some(output) if output.status.success() => {
                self.closed = true;
                Ok(())
            }
            Some(output) => Err(format!(
                "Wine server cleanup failed: {}",
                String::from_utf8_lossy(&output.stderr)
            )),
            None => Err("Wine server cleanup timed out".into()),
        }
    }
    pub fn command(&self) -> Command {
        self.command_in(None)
    }
    /// A Wine command starting in `cwd` (inside the session namespace).
    pub fn command_in(&self, cwd: Option<&Path>) -> Command {
        let mut c = Command::new(&self.wine);
        c.envs(&self.env).env_remove("ARCADE_GUEST_ENV");
        if let Some(enter) = &self.enter {
            enter(&mut c, cwd);
        } else if let Some(cwd) = cwd {
            c.current_dir(cwd);
        }
        c
    }
    fn server_command(&self) -> Command {
        let mut c = Command::new(&self.server);
        c.envs(&self.env);
        if let Some(enter) = &self.enter {
            enter(&mut c, None);
        }
        c
    }
    pub fn run(&self, c: &mut Command) -> Result<ExitStatus, String> {
        let log = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.log)
            .map_err(|e| e.to_string())?;
        c.stdout(log.try_clone().map_err(|e| e.to_string())?)
            .stderr(log)
            .status()
            .map_err(|e| e.to_string())
    }
    pub fn desktop(
        &mut self,
        name: &str,
        resolution: &str,
        fullscreen: bool,
    ) -> Result<(), String> {
        let key = r"HKCU\Software\Wine\Explorer";
        if fullscreen {
            let mut c = self.command();
            c.args(["reg", "delete", key, "/v", "Desktop", "/f"]);
            self.run(&mut c)?;
        } else {
            for args in [
                vec!["reg", "add", key, "/v", "Desktop", "/d", name, "/f"],
                vec![
                    "reg",
                    "add",
                    r"HKCU\Software\Wine\Explorer\Desktops",
                    "/v",
                    name,
                    "/d",
                    resolution,
                    "/f",
                ],
            ] {
                let mut c = self.command();
                c.args(args);
                if !self.run(&mut c)?.success() {
                    return Err("Could not configure Wine desktop".into());
                }
            }
        }
        Ok(())
    }
}
fn find(name: &str) -> Option<PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|p| p.join(name))
        .find(|p| p.is_file())
}
pub fn find_dxvk() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(p) = std::env::var_os("ARCADE_DXVK") {
        candidates.push(PathBuf::from(p));
    }
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        for base in [home.join(".local/share/Steam"), home.join(".steam/steam")] {
            for (parent, proton) in [
                (base.join("steamapps/common"), true),
                (base.join("compatibilitytools.d"), false),
            ] {
                let mut found: Vec<_> = fs::read_dir(parent)
                    .into_iter()
                    .flatten()
                    .filter_map(Result::ok)
                    .filter(|e| !proton || e.file_name().to_string_lossy().starts_with("Proton"))
                    .map(|e| e.path().join("files/lib/wine/dxvk/i386-windows"))
                    .collect();
                found.sort();
                found.reverse();
                candidates.extend(found);
            }
        }
    }
    candidates.extend([
        PathBuf::from("/usr/share/dxvk/x32"),
        PathBuf::from("/usr/lib32/dxvk"),
    ]);
    candidates
        .into_iter()
        .find(|p| p.join("d3d8.dll").is_file() && p.join("d3d9.dll").is_file())
}

pub struct Wine;
pub static BACKEND: Wine = Wine;

impl ExecutionBackend for Wine {
    fn kind(&self) -> BackendKind {
        BackendKind::Wine
    }
    fn name(&self) -> &'static str {
        "Wine"
    }
    fn formats(&self) -> &'static [&'static str] {
        &["PE"]
    }
    fn availability(&self) -> Availability {
        if !cfg!(target_os = "linux") {
            return Availability::Unsupported;
        }
        let found = std::env::var_os("ARCADE_WINE")
            .map(PathBuf::from)
            .or_else(|| find("wine"))
            .is_some_and(|wine| wine.is_file() && server_for(&wine).is_some());
        if found {
            Availability::Available
        } else {
            Availability::Missing("Wine and wineserver are required".into())
        }
    }
    fn check(&self, p: &banana_api::LaunchPlan) -> Result<(), banana_api::CapabilityError> {
        banana_api::check_host(self, p)
    }
    /// The view is served into the session's own namespace instead.
    fn needs_mount(&self, _plan: &banana_api::LaunchPlan) -> bool {
        false
    }
    fn launch(
        &self,
        s: banana_api::PreparedSession,
    ) -> Result<Box<dyn banana_api::SessionHandle>, String> {
        self.check(&s.plan).map_err(|e| e.to_string())?;
        // The view appears at this (otherwise empty) directory only inside
        // the session namespace.
        let mount = s.state.join("view");
        #[cfg(target_os = "linux")]
        let holder = std::sync::Arc::new(banana_native::namespace::Holder::start(
            &s.resources,
            &mount,
            s.view.clone(),
            &s.state.join("sandbox.log"),
        )?);
        #[cfg(target_os = "linux")]
        let enter: Option<Enter> = {
            let holder = holder.clone();
            Some(std::sync::Arc::new(move |c: &mut Command, cwd: Option<&Path>| {
                holder.enter(c, cwd)
            }))
        };
        #[cfg(not(target_os = "linux"))]
        let enter: Option<Enter> = None;
        let mut prefix = Prefix::new_in(
            &s.state.join("prefix"),
            &s.state.join("game.log"),
            if s.plan.binary.bits == 32 {
                "win32"
            } else {
                "win64"
            },
            enter,
        )?;
        #[cfg(unix)]
        if let Some(relative) = s.plan.guest_root.strip_prefix("C:/") {
            let target = prefix.path.join("drive_c").join(relative);
            if let Ok(m) = fs::symlink_metadata(&target) {
                if !m.file_type().is_symlink() {
                    return Err("Wine guest mapping conflicts with an existing directory".into());
                }
                fs::remove_file(&target).map_err(|e| e.to_string())?;
            }
            if let Some(p) = target.parent() {
                fs::create_dir_all(p).map_err(|e| e.to_string())?;
            }
            std::os::unix::fs::symlink(&mount, &target).map_err(|e| e.to_string())?;
        }
        if let Some(resolution) = s.plan.env.get("ARCADE_RESOLUTION") {
            prefix.desktop(
                "BananaPepper",
                resolution,
                s.plan
                    .env
                    .get("ARCADE_FULLSCREEN")
                    .is_some_and(|v| v == "1"),
            )?;
        }
        let mut command = prefix.command_in(Some(&mount.join(&s.plan.cwd)));
        command
            .arg(mount.join(&s.plan.executable))
            .args(&s.plan.args)
            .envs(&s.plan.env);
        let log = fs::File::create(s.state.join("game.log")).map_err(|e| e.to_string())?;
        command
            .stdout(log.try_clone().map_err(|e| e.to_string())?)
            .stderr(log);
        let process = banana_api::process::ProcessSession::spawn(&mut command, s.plan.timeout_ms)?;
        Ok(Box::new(WineSession {
            process,
            prefix,
            #[cfg(target_os = "linux")]
            _holder: holder,
        }))
    }
}

fn server_for(wine: &Path) -> Option<PathBuf> {
    std::env::var_os("ARCADE_WINESERVER")
        .map(PathBuf::from)
        .filter(|p| p.is_file())
        .or_else(|| {
            let p = wine.with_file_name("wineserver");
            p.is_file().then_some(p)
        })
        .or_else(|| find("wineserver"))
}
impl Drop for Prefix {
    fn drop(&mut self) {
        let _ = self.close();
    }
}
struct WineSession {
    process: Box<dyn banana_api::SessionHandle>,
    prefix: Prefix,
    /// Dropped after the prefix closes, ending the session namespace.
    #[cfg(target_os = "linux")]
    _holder: std::sync::Arc<banana_native::namespace::Holder>,
}
impl banana_api::SessionHandle for WineSession {
    fn next_event(&mut self) -> Result<Option<banana_api::SessionEvent>, String> {
        self.process.next_event()
    }
    fn command(&mut self, a: banana_api::ActionRequest) -> Result<(), String> {
        self.process.command(a)
    }
    fn cancel(&mut self) -> Result<(), String> {
        let result = self.process.cancel();
        self.prefix.close()?;
        result
    }
    fn wait(&mut self) -> Result<i32, String> {
        let code = self.process.wait()?;
        self.prefix.close()?;
        Ok(code)
    }
}
impl Drop for WineSession {
    fn drop(&mut self) {
        let _ = self.process.cancel();
        let _ = self.process.wait();
        let _ = self.prefix.close();
    }
}
