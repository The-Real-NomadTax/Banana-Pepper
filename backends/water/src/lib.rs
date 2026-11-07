//! Water Adapts The ELF Runtime: runs Linux programs on Windows, as Wine runs
//! Windows programs on Linux. Guest code runs directly in a dedicated worker
//! process; Water supplies the Linux libraries and system calls it uses.
//!
//! - `water-x86`: ELF32 programs, their imports bound to Water's own Win32
//!   implementations of the Linux libraries (C library, files, GLUT/OpenGL,
//!   OSS audio). An optional preload adapter ([`extension`]) may override
//!   any import, like `LD_PRELOAD`.
//! - `water-x64`: static ELF64 programs with their system calls trapped.
//!
//! Each missing capability is reported, never silently skipped. This is a
//! compatibility runtime, not a security sandbox for untrusted executables.
use banana_api::{Availability, BackendKind, ExecutionBackend};

pub mod elf;
pub mod stack;
pub mod vfs;

#[cfg(all(windows, target_arch = "x86_64"))]
mod windows;

pub fn run_worker(image: &[u8], argv: &[String]) -> Result<(), String> {
    let plan = elf::Image::parse(image)?;
    plan.require_native_support()?;
    #[cfg(all(windows, target_arch = "x86_64"))]
    return windows::run(image, &plan, argv);
    #[cfg(not(all(windows, target_arch = "x86_64")))]
    {
        let _ = argv;
        Err("Native Linux ABI execution requires Windows x86-64".into())
    }
}

/// Where a plan places Water's optional preload adapter in the view.
pub const ADAPTER: &str = ".banana/water-adapter.dll";

/// Water as a registered execution backend.
pub struct Water;
pub static BACKEND: Water = Water;

impl ExecutionBackend for Water {
    fn kind(&self) -> BackendKind {
        BackendKind::Water
    }
    fn name(&self) -> &'static str {
        "Water (Linux to Windows)"
    }
    fn formats(&self) -> &'static [&'static str] {
        &["ELF"]
    }
    fn availability(&self) -> Availability {
        if cfg!(windows) {
            Availability::Available
        } else {
            Availability::Unsupported
        }
    }
    fn check(&self, p: &banana_api::LaunchPlan) -> Result<(), banana_api::CapabilityError> {
        banana_api::check_host(self, p)?;
        let supported = if p.binary.bits == 32 {
            &["elf32-fixed-imports"][..]
        } else {
            &[][..]
        };
        let mut missing = p
            .requirements
            .iter()
            .filter(|r| !supported.contains(&r.as_str()))
            .cloned()
            .collect::<Vec<_>>();
        if p.binary.bits == 64 {
            if let Some(bytes) = p.generated.get(&p.executable) {
                match elf::Image::parse(bytes) {
                    Ok(image) => missing.extend(image.native_blockers),
                    Err(e) => missing.push(e),
                }
            } else if p.binary.interpreter.is_some() || !p.binary.libraries.is_empty() {
                missing.push(
                    "ELF64 dynamic linking, PIE relocation, and TLS are not implemented".into(),
                );
            }
        }
        if missing.is_empty() {
            Ok(())
        } else {
            Err(banana_api::CapabilityError {
                backend: self.name().into(),
                missing,
            })
        }
    }
    fn launch(
        &self,
        s: banana_api::PreparedSession,
    ) -> Result<Box<dyn banana_api::SessionHandle>, String> {
        self.check(&s.plan).map_err(|e| e.to_string())?;
        let name = if s.plan.binary.bits == 32 {
            "water-x86.exe"
        } else {
            "water-x64.exe"
        };
        let worker = s.resources.join("helpers").join(name);
        if !worker.is_file() {
            return Err(format!(
                "Missing Water worker {}; rebuild with just build",
                worker.display()
            ));
        }
        let mut c = std::process::Command::new(worker);
        c.arg("--image")
            .arg(&s.plan.executable)
            .arg("--state")
            .arg(&s.state);
        if s.plan.binary.bits == 32 && s.plan.adapters.iter().any(|a| a.destination == ADAPTER) {
            // An optional preload adapter. Windows loads DLLs from files:
            // stage it (our own build artifact, not game content) in the
            // session state.
            let adapter = s.state.join("water-adapter.dll");
            let bytes = s
                .view
                .read(ADAPTER, 64 * 1024 * 1024)
                .map_err(|e| format!("Missing Water adapter: {e}"))?;
            std::fs::write(&adapter, bytes).map_err(|e| e.to_string())?;
            c.arg("--adapter").arg(adapter);
        }
        for name in [
            "ARCADE_NATIVE_PROBE",
            "ARCADE_NATIVE_FIRE_PROBE",
            "ARCADE_NATIVE_CAPTURE_ALL",
            "ARCADE_NATIVE_TRACE",
        ] {
            c.env_remove(name);
        }
        c.envs(&s.plan.env).current_dir(&s.state);
        let log = std::fs::File::create(s.state.join("game.log")).map_err(|e| e.to_string())?;
        c.stdout(log.try_clone().map_err(|e| e.to_string())?)
            .stderr(log);
        serve_and_spawn(&mut c, &s)
    }
}

/// Start the worker connected to the session view over private pipes.
#[cfg(windows)]
fn serve_and_spawn(
    c: &mut std::process::Command,
    s: &banana_api::PreparedSession,
) -> Result<Box<dyn banana_api::SessionHandle>, String> {
    let pipes = banana_api::process::ViewPipes::new().map_err(|e| e.to_string())?;
    c.arg("--view").arg(pipes.child_spec());
    c.arg("--").args(&s.plan.args);
    pipes
        .spawn(s.view.clone(), || {
            banana_api::process::ProcessSession::spawn(c, s.plan.timeout_ms)
        })
        .map_err(|e| e.to_string())?
}
#[cfg(not(windows))]
fn serve_and_spawn(
    _: &mut std::process::Command,
    _: &banana_api::PreparedSession,
) -> Result<Box<dyn banana_api::SessionHandle>, String> {
    Err("Water runs on Windows hosts".into())
}

pub mod extension;
#[cfg(all(windows, target_arch = "x86"))]
pub mod win32;
