//! Native execution: the host runs the guest executable directly. Neither
//! host mounts the view: on Linux, ELF guests run in `banana-sandbox`, which
//! mounts the served view inside the session's own namespaces (see
//! `namespace`); on Windows, PE images are staged and the view shim serves
//! every other file (see `staging`).
use banana_api::{Availability, BackendKind, ExecutionBackend};
#[cfg(target_os = "linux")]
pub mod namespace;
pub mod pe;
pub mod sandbox;
#[cfg(windows)]
mod staging;
pub use sandbox::Sandbox;

pub struct Native;
pub static BACKEND: Native = Native;

impl ExecutionBackend for Native {
    fn kind(&self) -> BackendKind {
        BackendKind::Native
    }
    fn name(&self) -> &'static str {
        "Native"
    }
    fn formats(&self) -> &'static [&'static str] {
        if cfg!(windows) { &["PE"] } else { &["ELF"] }
    }
    fn availability(&self) -> Availability {
        if cfg!(windows) {
            Availability::Available
        } else if cfg!(target_os = "linux") {
            if std::path::Path::new("/dev/fuse").exists() {
                Availability::Available
            } else {
                Availability::Missing("FUSE (/dev/fuse) is unavailable".into())
            }
        } else {
            Availability::Unsupported
        }
    }
    fn check(&self, p: &banana_api::LaunchPlan) -> Result<(), banana_api::CapabilityError> {
        banana_api::check_host(self, p)?;
        if cfg!(windows) && p.binary.bits != 32 {
            return Err(banana_api::CapabilityError {
                backend: self.name().into(),
                missing: vec!["The Windows view shim is built for 32-bit titles only".into()],
            });
        }
        Ok(())
    }
    fn needs_mount(&self, _plan: &banana_api::LaunchPlan) -> bool {
        false
    }
    fn adapters(&self, _plan: &banana_api::LaunchPlan) -> Vec<banana_api::Adapter> {
        #[cfg(windows)]
        return vec![banana_api::Adapter {
            artifact: "banana_view".into(),
            destination: staging::SHIM.into(),
            preload: false,
        }];
        #[cfg(not(windows))]
        Vec::new()
    }
    fn launch(
        &self,
        s: banana_api::PreparedSession,
    ) -> Result<Box<dyn banana_api::SessionHandle>, String> {
        self.check(&s.plan).map_err(|e| e.to_string())?;
        #[cfg(windows)]
        return launch_windows(s);
        #[cfg(not(windows))]
        launch_unix(s)
    }
}

#[cfg(not(windows))]
fn launch_unix(
    s: banana_api::PreparedSession,
) -> Result<Box<dyn banana_api::SessionHandle>, String> {
    #[cfg(target_os = "linux")]
    let connection = namespace::ViewConnection::new().map_err(|e| e.to_string())?;
    #[cfg(target_os = "linux")]
    let input = s
        .plan
        .input
        .clone()
        .map(|config| input::Bridge::new(&s.state, config))
        .transpose()?;
    #[cfg(not(windows))]
    let mut command = {
        #[cfg(target_os = "linux")]
        let mut box_ = Sandbox::new(&namespace::helper(&s.resources));
        #[cfg(not(target_os = "linux"))]
        let mut box_ = Sandbox::new(std::path::Path::new("banana-sandbox"));
        #[cfg(target_os = "linux")]
        box_.command.args(connection.args());
        box_.view(&s.plan.guest_root)
            .bind(&s.state, "/work", false)
            .chdir(&format!("{}/{}", s.plan.guest_root, s.plan.cwd));
        for m in &s.plan.guest_mounts {
            banana_content::path(&m.source).map_err(|e| e.to_string())?;
            box_.view_bind(&m.source, &m.target, m.readonly);
        }
        for device in &s.devices {
            box_.device(device);
        }
        for name in &s.plan.inert_tools {
            if let (Some(source), Some(destination)) = (find_program("true"), find_program(name)) {
                box_.bind(source, destination, true);
            }
        }
        if let Some(input) = &input {
            for device in input.paths() {
                box_.device(device);
            }
        }
        box_.desktop()?.gpu();
        box_.audio(
            s.plan.env.get("ARCADE_AUDIO").is_none_or(|v| v != "0"),
            s.plan.env.get("PULSE_SINK").map_or("", String::as_str),
            "pulse",
        );
        for (k, v) in &s.plan.env {
            box_.env(k, v);
        }
        box_.exec(
            format!("{}/{}", s.plan.guest_root, s.plan.executable),
            &s.plan.args,
        );
        box_.command
    };
    let log = std::fs::File::create(s.state.join("game.log")).map_err(|e| e.to_string())?;
    command
        .stdout(log.try_clone().map_err(|e| e.to_string())?)
        .stderr(log);
    let process = banana_api::process::ProcessSession::spawn_controlled(
        &mut command,
        s.plan.timeout_ms,
        s.plan.command_channel.as_ref().map(|p| s.state.join(p)),
    )?;
    #[cfg(target_os = "linux")]
    connection.serve(s.view.clone())?;
    #[cfg(target_os = "linux")]
    return Ok(Box::new(NativeSession {
        process,
        _input: input,
    }));
    #[cfg(not(target_os = "linux"))]
    Ok(process)
}

#[cfg(windows)]
fn launch_windows(
    s: banana_api::PreparedSession,
) -> Result<Box<dyn banana_api::SessionHandle>, String> {
    let root = s.state.join("root");
    let executable = staging::stage(s.view.as_ref(), &s.plan, &root)?;
    let mut command = std::process::Command::new(&executable);
    command
        .current_dir(staging::host_path(&root, &s.plan.cwd))
        .envs(&s.plan.env)
        .env("BANANA_VIEW_ROOT", &root)
        .args(&s.plan.args);
    let log = std::fs::File::create(s.state.join("game.log")).map_err(|e| e.to_string())?;
    command
        .stdout(log.try_clone().map_err(|e| e.to_string())?)
        .stderr(log);
    let pipes = banana_api::process::ViewPipes::new().map_err(|e| e.to_string())?;
    command.env("BANANA_VIEW", pipes.child_spec());
    pipes
        .spawn(s.view.clone(), || {
            banana_api::process::ProcessSession::spawn_controlled(
                &mut command,
                s.plan.timeout_ms,
                s.plan.command_channel.as_ref().map(|p| s.state.join(p)),
            )
        })
        .map_err(|e| e.to_string())?
}

/// First `name` on `PATH`.
pub fn find_program(name: &str) -> Option<std::path::PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|dir| dir.join(name))
        .find(|path| path.is_file())
}

#[cfg(target_os = "linux")]
mod input;
#[cfg(target_os = "linux")]
struct NativeSession {
    process: Box<dyn banana_api::SessionHandle>,
    _input: Option<input::Bridge>,
}
#[cfg(target_os = "linux")]
impl banana_api::SessionHandle for NativeSession {
    fn next_event(&mut self) -> Result<Option<banana_api::SessionEvent>, String> {
        self.process.next_event()
    }
    fn command(&mut self, a: banana_api::ActionRequest) -> Result<(), String> {
        self.process.command(a)
    }
    fn cancel(&mut self) -> Result<(), String> {
        self.process.cancel()
    }
    fn wait(&mut self) -> Result<i32, String> {
        self.process.wait()
    }
}
#[cfg(target_os = "linux")]
impl Drop for NativeSession {
    fn drop(&mut self) {
        let _ = self.process.cancel();
        let _ = self.process.wait();
    }
}
