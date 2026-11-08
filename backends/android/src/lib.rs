//! Platform execution/transport only. Titles use the same LaunchPlan and View
//! as desktop; this module contains no title identities or preparation code.
#[cfg(any(target_os = "android", target_os = "linux"))]
pub mod bridge;
/// Hosts without the bridges report the transport as unsupported.
#[cfg(not(any(target_os = "android", target_os = "linux")))]
mod bridge {
    pub const AUDIO_RATE: u32 = 48_000;
    pub const AUDIO_CHANNELS: u32 = 2;
    const UNSUPPORTED: &str = "The Android display/audio bridges need Android or Linux";
    pub struct Bridges;
    impl Bridges {
        pub fn new(_: &std::path::Path) -> Self {
            Self
        }
        pub fn failure(&self) -> Option<String> {
            None
        }
        pub fn display(&mut self, _: &std::path::Path, _: &std::path::Path, _: &str, _: u32) -> Result<(), String> {
            Err(UNSUPPORTED.into())
        }
        pub fn audio(&mut self, _: &std::path::Path, _: &str) -> Result<(), String> {
            Err(UNSUPPORTED.into())
        }
    }
}
use banana_api::*;
use std::{
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};
pub struct Android {
    kind: BackendKind,
}
pub static NATIVE: Android = Android {
    kind: BackendKind::Native,
};
pub static WINE: Android = Android {
    kind: BackendKind::Wine,
};
impl ExecutionBackend for Android {
    fn kind(&self) -> BackendKind {
        self.kind
    }
    fn name(&self) -> &'static str {
        if self.kind == BackendKind::Wine {
            "Android Wine/Box64"
        } else {
            "Android Box64"
        }
    }
    fn formats(&self) -> &'static [&'static str] {
        if self.kind == BackendKind::Wine {
            &["PE"]
        } else {
            &["ELF"]
        }
    }
    fn availability(&self) -> Availability {
        if cfg!(target_os = "android") {
            Availability::Available
        } else {
            Availability::Unsupported
        }
    }
    fn check(&self, p: &LaunchPlan) -> Result<(), CapabilityError> {
        check_host(self, p)
    }
    fn launch(&self, mut s: PreparedSession) -> Result<Box<dyn SessionHandle>, String> {
        self.check(&s.plan).map_err(|e| e.to_string())?;
        // PRoot brokers the guest's file calls into the view; the returned
        // empty directory only stands for the view's root, so the guest is
        // given the planned root's virtual path instead.
        let (_empty, view) = expose(&mut s)?;
        let guest_root = if let Some((drive, relative)) = s.plan.guest_root.split_once(":/") {
            PathBuf::from(format!(
                "/work/prefix/drive_{}/{relative}",
                drive.to_ascii_lowercase()
            ))
        } else {
            PathBuf::from(&s.plan.guest_root)
        };
        let files = s
            .resources
            .parent()
            .ok_or("Missing Android application directory")?
            .to_path_buf();
        let run = s
            .plan
            .env
            .get("BANANA_ANDROID_RUN")
            .map(PathBuf::from)
            .unwrap_or_else(|| s.state.join("run"));
        std::fs::create_dir_all(&run).map_err(|e| e.to_string())?;
        let tmp = s.state.join("tmp");
        std::fs::create_dir_all(&tmp).map_err(|e| e.to_string())?;
        for relative in [".X0-lock", ".X11-unix/X0", "pulse-native", "pulse-pcm"] {
            let path = tmp.join(relative);
            if std::fs::symlink_metadata(&path).is_ok() {
                std::fs::remove_file(path).map_err(|e| e.to_string())?;
            }
        }
        let proot = files.join("bin/proot");
        if !std::fs::read(&proot)
            .map_err(|e| format!("Missing Android PRoot: {e}"))?
            .windows(21)
            .any(|v| v == b"BANANA_VFS_PROTOCOL_2")
        {
            return Err("Android PRoot lacks virtual filesystem protocol 2. Rebuild/install the application runtime; stock PRoot cannot expose mounted game content.".into());
        }
        let mut transport = Transport {
            files,
            resources: s.resources.clone(),
            state: s.state.clone(),
            run: run.clone(),
            tmp,
            environment: s.plan.env.clone(),
            inert: s.plan.inert_tools.clone(),
        };
        let resolution = s
            .plan
            .env
            .get("ARCADE_RESOLUTION")
            .cloned()
            .unwrap_or_else(|| "640x480".into());
        let (w, h) = resolution
            .split_once('x')
            .and_then(|(w, h)| Some((w.parse::<u32>().ok()?, h.parse::<u32>().ok()?)))
            .ok_or("Invalid Android resolution")?;
        if !(320..=4096).contains(&w) || !(240..=4096).contains(&h) {
            return Err("Android display dimensions exceed supported bounds".into());
        }
        transport.environment.insert("DISPLAY".into(), ":0".into());
        transport
            .environment
            .insert("HOME".into(), "/work/home".into());
        std::fs::create_dir_all(s.state.join("home")).map_err(|e| e.to_string())?;
        transport
            .environment
            .insert("LIBGL_ALWAYS_SOFTWARE".into(), "1".into());
        transport
            .environment
            .insert("GALLIUM_DRIVER".into(), "llvmpipe".into());
        let mut owned = AndroidSession {
            game: None,
            support: Vec::new(),
            bridges: None,
            events: std::collections::VecDeque::new(),
            frame: run.join("frame.bin"),
            view: Some(view),
        };
        owned.support.push(transport.spawn(
            "xvfb",
            &[
                "/usr/bin/Xvfb".into(),
                ":0".into(),
                "-screen".into(),
                "0".into(),
                format!("{resolution}x24"),
                "-nolisten".into(),
                "tcp".into(),
                "-nolisten".into(),
                "local".into(),
                "-ac".into(),
                "+iglx".into(),
                "-nocursor".into(),
            ],
            None,
        )?);
        wait_for(&transport.tmp.join(".X11-unix/X0"), &mut owned.support)?;
        let input = s
            .plan
            .env
            .get("AF_INPUT_SOCKET")
            .ok_or("Android input socket is missing")?;
        let bridges = owned.bridges.insert(bridge::Bridges::new(&run.join("bridge.log")));
        bridges.display(&transport.tmp.join(".X11-unix/X0"), &owned.frame, input, 60)?;
        if let Some(audio) = s
            .plan
            .env
            .get("AF_AUDIO_SOCKET")
            .filter(|_| s.plan.env.get("ARCADE_AUDIO").is_none_or(|s| s != "0"))
        {
            transport
                .environment
                .insert("PULSE_SERVER".into(), "unix:/tmp/pulse-native".into());
            transport
                .environment
                .insert("XDG_RUNTIME_DIR".into(), "/tmp/pulse-runtime".into());
            std::fs::create_dir_all(transport.tmp.join("pulse-runtime"))
                .map_err(|e| e.to_string())?;
            // The game plays into a null sink; the simple protocol streams its
            // monitor as raw PCM for the audio bridge to relay.
            let (rate, channels) = (bridge::AUDIO_RATE, bridge::AUDIO_CHANNELS);
            let pulse = [
                "/usr/bin/pulseaudio".into(),
                "-n".into(),
                "--daemonize=no".into(),
                "--disable-shm=yes".into(),
                "--use-pid-file=no".into(),
                "--exit-idle-time=-1".into(),
                "--high-priority=no".into(),
                "--realtime=no".into(),
                "--log-target=stderr".into(),
                "--load=module-native-protocol-unix socket=/tmp/pulse-native auth-anonymous=1".into(),
                format!("--load=module-null-sink sink_name=arcade rate={rate} channels={channels}"),
                format!(
                    "--load=module-simple-protocol-unix socket=/tmp/pulse-pcm record=true playback=false source=arcade.monitor format=s16le rate={rate} channels={channels}"
                ),
            ];
            owned.support.push(transport.spawn("pulse", &pulse, None)?);
            wait_for(&transport.tmp.join("pulse-native"), &mut owned.support)?;
            wait_for(&transport.tmp.join("pulse-pcm"), &mut owned.support)?;
            bridges.audio(&transport.tmp.join("pulse-pcm"), audio)?;
        }
        let mut environment = transport.environment.clone();
        environment.insert("ARCADE_ANDROID".into(), "1".into());
        environment.insert("BOX64_ANDROID_NOEXEC".into(), "1".into());
        if let Some(preload) = environment.remove("LD_PRELOAD") {
            environment.insert("BOX64_LD_PRELOAD".into(), preload);
        }
        let libraries = environment.remove("LD_LIBRARY_PATH").unwrap_or_default();
        environment.insert(
            "BOX64_LD_LIBRARY_PATH".into(),
            format!("{libraries}:/opt/arcade/x64-libs:/opt/arcade/dependencies/legacy32"),
        );
        transport.environment.insert(
            "BANANA_ANDROID_CWD".into(),
            guest_root.join(&s.plan.cwd).to_string_lossy().into_owned(),
        );
        let exe = guest_root
            .join(&s.plan.executable)
            .to_string_lossy()
            .into_owned();
        let mut argv = vec!["/usr/local/bin/box64".into()];
        if self.kind == BackendKind::Wine {
            environment.insert("WINEPREFIX".into(), "/work/prefix".into());
            argv.extend([
                "/opt/arcade/wine/bin/wine".into(),
                "explorer".into(),
                format!("/desktop=BananaPepper,{resolution}"),
                exe,
            ]);
        } else {
            argv.push(exe)
        }
        argv.extend(s.plan.args.clone());
        environment.insert(
            "BANANA_ANDROID_CWD".into(),
            guest_root.join(&s.plan.cwd).to_string_lossy().into_owned(),
        );
        transport.environment = environment;
        owned.game = Some(transport.spawn("game", &argv, s.plan.timeout_ms)?);
        Ok(Box::new(owned))
    }
}
/// Serve the view to PRoot: map the planned root (and, for Wine, its prefix
/// drive), the generated cpuinfo and the plan's extra mounts, and publish
/// the endpoint in the plan's environment.
fn expose(s: &mut PreparedSession) -> Result<(PathBuf, Box<dyn TracedView>), String> {
    let root = s.state.join("view");
    std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    let socket = s.state.join("view.sock");
    if std::fs::symlink_metadata(&socket).is_ok() {
        std::fs::remove_file(&socket).map_err(|e| e.to_string())?;
    }
    let view = s
        .view
        .serve_traced(&socket, &root.to_string_lossy())
        .map_err(|e| format!("Cannot serve the view to PRoot: {e}"))?;
    let plan = &mut s.plan;
    let map = |guest: &str, source: &str, readonly: bool| {
        view.map(guest, source, readonly).map_err(|e| e.to_string())
    };
    if plan.guest_root.starts_with('/') {
        map(&plan.guest_root, "", false)?;
    }
    if plan.binary.format == "PE" {
        if let Some((drive, relative)) = plan.guest_root.split_once(":/") {
            if drive.len() != 1 || !drive.as_bytes()[0].is_ascii_alphabetic() {
                return Err("Invalid planned Windows drive".into());
            }
            banana_content::path(relative).map_err(|e| e.to_string())?;
            let guest = format!(
                "/work/prefix/drive_{}/{relative}",
                drive.to_ascii_lowercase()
            );
            map(&guest, "", false)?;
        }
    }
    map("/proc/cpuinfo", ".banana/proc/cpuinfo", true)?;
    for mapping in &plan.guest_mounts {
        map(&mapping.target, &mapping.source, mapping.readonly)?;
    }
    plan.env.insert(
        "BANANA_VFS_SOCKET".into(),
        socket.to_string_lossy().into_owned(),
    );
    plan.env.insert(
        "BANANA_VFS_EMPTY".into(),
        root.to_string_lossy().into_owned(),
    );
    Ok((root, view))
}
struct Transport {
    files: PathBuf,
    resources: PathBuf,
    state: PathBuf,
    run: PathBuf,
    tmp: PathBuf,
    environment: std::collections::BTreeMap<String, String>,
    inert: Vec<String>,
}
impl Transport {
    fn spawn(
        &self,
        name: &str,
        argv: &[String],
        timeout: Option<u64>,
    ) -> Result<Box<dyn SessionHandle>, String> {
        let mut command = Command::new(self.files.join("bin/proot"));
        command
            .args(["--kill-on-exit", "--link2symlink", "-0", "-r"])
            .arg(self.files.join("rootfs"));
        for (host, guest) in [
            (PathBuf::from("/dev"), "/dev"),
            (PathBuf::from("/proc"), "/proc"),
            (PathBuf::from("/sys"), "/sys"),
            (self.tmp.clone(), "/tmp"),
            (self.tmp.clone(), "/dev/shm"),
            (self.run.clone(), "/run/af"),
            (self.state.clone(), "/work"),
            (self.resources.clone(), "/opt/arcade"),
        ] {
            command.arg("-b").arg(format!("{}:{guest}", host.display()));
        }
        if name == "game" {
            // Developer tracing: a verbosity level in `config/proot-trace` makes
            // the game's tracer log intercepted system calls into game.log.
            if let Ok(level) = std::fs::read_to_string(self.files.join("config/proot-trace")) {
                let level = level.trim().parse::<u8>().unwrap_or(2).min(9);
                command.arg("-v").arg(level.to_string());
            }
            for tool in &self.inert {
                if tool.contains('/') || tool.contains('\\') {
                    return Err("Inert tool must be a basename".into());
                }
                for directory in ["usr/bin", "usr/sbin", "bin", "sbin"] {
                    let guest = format!("/{directory}/{tool}");
                    if self
                        .files
                        .join("rootfs")
                        .join(guest.trim_start_matches('/'))
                        .exists()
                    {
                        command.arg("-b").arg(format!(
                            "{}:{guest}",
                            self.files.join("rootfs/usr/bin/true").display()
                        ));
                    }
                }
            }
        }
        command
            .arg("-w")
            .arg(if name == "game" {
                self.environment
                    .get("BANANA_ANDROID_CWD")
                    .map(String::as_str)
                    .unwrap_or("/work")
            } else {
                "/work"
            })
            .args([
                "/usr/bin/env",
                "-i",
                "PATH=/usr/local/bin:/usr/bin:/bin",
                "LANG=C.UTF-8",
            ]);
        // Host-side broker variables are never interpolated into shell commands.
        for (k, v) in &self.environment {
            if !k.starts_with("BANANA_VFS_") && (name == "game" || k != "LD_PRELOAD") {
                command.arg(format!("{k}={v}"));
            }
        }
        command
            .args(argv)
            .env_remove("LD_PRELOAD")
            .env("LD_LIBRARY_PATH", self.files.join("bin"))
            .env("PROOT_LOADER", self.files.join("bin/loader"))
            .env("PROOT_TMP_DIR", &self.tmp);
        // Only the game tracer opens the namespace broker. Support processes don't
        // access game files and must not monopolize its serialized RPC connection.
        if name == "game" {
            for (k, v) in &self.environment {
                if k.starts_with("BANANA_VFS_") {
                    command.env(k, v);
                }
            }
        }
        let log = std::fs::File::create(self.run.join(format!("{name}.log")))
            .map_err(|e| e.to_string())?;
        command
            .stdin(Stdio::null())
            .stdout(log.try_clone().map_err(|e| e.to_string())?)
            .stderr(log);
        banana_api::process::ProcessSession::spawn(&mut command, timeout)
    }
}
fn wait_for(path: &Path, processes: &mut [Box<dyn SessionHandle>]) -> Result<(), String> {
    let start = Instant::now();
    while !path.exists() {
        for p in processes.iter_mut() {
            while let Some(event) = p.next_event()? {
                if let SessionEvent::Exited(code) = event {
                    return Err(format!(
                        "Android transport exited with {code} while waiting for {}",
                        path.display()
                    ));
                }
            }
        }
        if start.elapsed() > Duration::from_secs(10) {
            return Err(format!(
                "Android transport timed out waiting for {}",
                path.display()
            ));
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    Ok(())
}
struct AndroidSession {
    game: Option<Box<dyn SessionHandle>>,
    support: Vec<Box<dyn SessionHandle>>,
    /// Frame/input/audio relays to the app; stopped before their servers.
    bridges: Option<bridge::Bridges>,
    events: std::collections::VecDeque<SessionEvent>,
    frame: PathBuf,
    /// Serves the guest's files; dropped (and flushed) once the game exits.
    view: Option<Box<dyn TracedView>>,
}
impl SessionHandle for AndroidSession {
    fn next_event(&mut self) -> Result<Option<SessionEvent>, String> {
        if let Some(e) = self.events.pop_front() {
            return Ok(Some(e));
        }
        let mut failed = None;
        for process in &mut self.support {
            while let Some(event) = process.next_event()? {
                if let SessionEvent::Exited(code) = event {
                    failed = Some(code);
                    break;
                }
            }
        }
        if let Some(code) = failed {
            self.cancel()?;
            return Err(format!(
                "Android display/audio transport exited with {code}; inspect the session logs"
            ));
        }
        if let Some(failure) = self.bridges.as_ref().and_then(bridge::Bridges::failure) {
            self.cancel()?;
            return Err(failure);
        }
        self.game
            .as_mut()
            .ok_or("Session has not started")?
            .next_event()
    }
    fn command(&mut self, a: ActionRequest) -> Result<(), String> {
        self.game
            .as_mut()
            .ok_or("Session has not started")?
            .command(a)
    }
    fn cancel(&mut self) -> Result<(), String> {
        if let Some(p) = &mut self.game {
            p.cancel()?;
        }
        for p in &mut self.support {
            p.cancel()?;
        }
        Ok(())
    }
    fn wait(&mut self) -> Result<i32, String> {
        let result = self.game.as_mut().ok_or("Session has not started")?.wait();
        self.bridges.take();
        for p in &mut self.support {
            let _ = p.cancel();
            let _ = p.wait();
        }
        self.view.take();
        result
    }
}
impl Drop for AndroidSession {
    fn drop(&mut self) {
        let _ = self.cancel();
        if let Some(p) = &mut self.game {
            let _ = p.wait();
        }
        self.bridges.take();
        for p in &mut self.support {
            let _ = p.wait();
        }
        self.view.take();
        let _ = std::fs::remove_file(&self.frame);
    }
}
