//! Native execution: the host runs the guest executable directly. On Linux,
//! ELF guests run inside a bubblewrap sandbox with a 32-bit userspace.
use banana_api::{Availability, BackendKind, ExecutionBackend};
pub mod sandbox;
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
            match find_program("bwrap") {
                Some(_) => Availability::Available,
                None => Availability::Missing("bubblewrap (bwrap) is not installed".into()),
            }
        } else {
            Availability::Unsupported
        }
    }
    fn check(&self, p: &banana_api::LaunchPlan) -> Result<(), banana_api::CapabilityError> {
        banana_api::check_host(self, p)
    }
    fn launch(
        &self,
        s: banana_api::PreparedSession,
    ) -> Result<Box<dyn banana_api::SessionHandle>, String> {
        self.check(&s.plan).map_err(|e| e.to_string())?;
        #[cfg(target_os = "linux")]
        let input = s
            .plan
            .input
            .clone()
            .map(|config| input::Bridge::new(&s.state, config))
            .transpose()?;
        #[cfg(windows)]
        let mut command = {
            let mut c = std::process::Command::new(s.mount.join(&s.plan.executable));
            c.current_dir(s.mount.join(&s.plan.cwd))
                .envs(&s.plan.env)
                .args(&s.plan.args);
            c
        };
        #[cfg(not(windows))]
        let mut command = {
            let mut box_ = Sandbox::new();
            box_.bind(&s.mount, &s.plan.guest_root, false)
                .bind(&s.state, "/work", false)
                .chdir(&format!("{}/{}", s.plan.guest_root, s.plan.cwd));
            for m in &s.plan.guest_mounts {
                banana_content::path(&m.source).map_err(|e| e.to_string())?;
                box_.bind(s.mount.join(&m.source), &m.target, m.readonly);
            }
            for device in &s.devices {
                box_.device(device);
            }
            for name in &s.plan.inert_tools {
                if let (Some(source), Some(destination)) =
                    (find_program("true"), find_program(name))
                {
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
            box_.command
                .arg(format!("{}/{}", s.plan.guest_root, s.plan.executable))
                .args(&s.plan.args);
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
        return Ok(Box::new(NativeSession {
            process,
            _input: input,
        }));
        #[cfg(not(target_os = "linux"))]
        Ok(process)
    }
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
