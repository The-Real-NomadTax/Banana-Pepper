//! Linux filesystem/display/audio isolation. Game policy belongs to callers.
use std::{
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

pub struct Sandbox {
    pub command: Command,
}

impl Default for Sandbox {
    fn default() -> Self {
        Self::new()
    }
}
impl Sandbox {
    pub fn new() -> Self {
        let mut s = Self {
            command: Command::new("bwrap"),
        };
        s.command.args([
            "--unshare-all",
            "--die-with-parent",
            "--new-session",
            "--clearenv",
            "--ro-bind",
            "/usr",
            "/usr",
        ]);
        for name in ["bin", "sbin", "lib", "lib32", "lib64"] {
            let p = Path::new("/").join(name);
            if let Ok(link) = fs::read_link(&p) {
                s.command.arg("--symlink").arg(link).arg(p);
            } else if p.exists() {
                s.bind(&p, &p, true);
            }
        }
        s.command
            .args(["--dev", "/dev", "--proc", "/proc", "--tmpfs", "/tmp"]);
        for p in [
            "/etc/ld.so.cache",
            "/etc/ld.so.conf",
            "/etc/fonts",
            "/etc/alternatives",
            "/etc/localtime",
        ] {
            if Path::new(p).exists() {
                s.bind(p, p, true);
            }
        }
        s.env("PATH", "/usr/bin:/bin")
            .env("HOME", "/tmp")
            .env("LANG", "C.UTF-8");
        s
    }
    pub fn bind(
        &mut self,
        source: impl AsRef<OsStr>,
        destination: impl AsRef<OsStr>,
        readonly: bool,
    ) -> &mut Self {
        self.command
            .arg(if readonly { "--ro-bind" } else { "--bind" })
            .arg(source)
            .arg(destination);
        self
    }
    pub fn device(&mut self, p: impl AsRef<OsStr>) -> &mut Self {
        self.command.arg("--dev-bind").arg(&p).arg(&p);
        self
    }
    pub fn env(&mut self, key: impl AsRef<OsStr>, value: impl AsRef<OsStr>) -> &mut Self {
        self.command.arg("--setenv").arg(key).arg(value);
        self
    }
    pub fn chdir(&mut self, p: &str) -> &mut Self {
        self.command.args(["--chdir", p]);
        self
    }
    pub fn desktop(&mut self) -> Result<&mut Self, String> {
        let display = std::env::var("DISPLAY")
            .map_err(|_| "No X11 display; a desktop/XWayland session or WSLg is required")?;
        let sockets = Path::new("/tmp/.X11-unix")
            .canonicalize()
            .map_err(|e| format!("X11 sockets unavailable: {e}"))?;
        self.bind(sockets, "/tmp/.X11-unix", true)
            .env("DISPLAY", display);
        let auth = std::env::var_os("XAUTHORITY")
            .map(PathBuf::from)
            .or_else(|| home().map(|h| h.join(".Xauthority")));
        if let Some(p) = auth.filter(|p| p.is_file()) {
            self.bind(p, "/tmp/game.Xauthority", true)
                .env("XAUTHORITY", "/tmp/game.Xauthority");
        }
        for p in ["/mnt/wslg", "/usr/lib/wsl"] {
            if Path::new(p).exists() {
                self.bind(p, p, true);
            }
        }
        for key in ["PULSE_SERVER", "XDG_RUNTIME_DIR"] {
            if let Some(v) = std::env::var_os(key) {
                self.env(key, v);
            }
        }
        Ok(self)
    }
    pub fn gpu(&mut self) -> &mut Self {
        for p in [
            "/dev/dxg",
            "/dev/dri",
            "/dev/nvidia0",
            "/dev/nvidiactl",
            "/dev/nvidia-modeset",
            "/dev/nvidia-uvm",
        ] {
            if Path::new(p).exists() {
                self.device(p);
            }
        }
        for p in ["/sys", "/proc/driver/nvidia", "/run/udev"] {
            if Path::new(p).exists() {
                self.bind(p, p, true);
            }
        }
        self
    }
    pub fn audio(&mut self, enabled: bool, sink: &str, driver: &str) -> String {
        let server = std::env::var("PULSE_SERVER").unwrap_or_default();
        let socket = server
            .strip_prefix("unix:")
            .or_else(|| server.starts_with('/').then_some(server.as_str()))
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                std::env::var_os("XDG_RUNTIME_DIR")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| {
                        #[cfg(target_os = "linux")]
                        {
                            PathBuf::from(format!("/run/user/{}", unsafe { libc::getuid() }))
                        }
                        #[cfg(not(target_os = "linux"))]
                        {
                            PathBuf::from("/run/user/1000")
                        }
                    })
                    .join("pulse/native")
            });
        if !enabled || !socket.exists() {
            self.env("SDL_AUDIODRIVER", "dummy");
            return "dummy".into();
        }
        self.bind(socket, "/tmp/arcade-pulse/native", true)
            .env("PULSE_SERVER", "unix:/tmp/arcade-pulse/native")
            .env("SDL_AUDIODRIVER", driver);
        if !sink.is_empty() {
            self.env("PULSE_SINK", sink);
        }
        let cookie = std::env::var_os("PULSE_COOKIE")
            .map(PathBuf::from)
            .or_else(|| home().map(|h| h.join(".config/pulse/cookie")));
        if let Some(p) = cookie.filter(|p| p.is_file()) {
            self.bind(p, "/tmp/arcade-pulse/cookie", true)
                .env("PULSE_COOKIE", "/tmp/arcade-pulse/cookie");
        }
        driver.into()
    }
    /// Cabinet display/host-management commands become inert ELF executables,
    /// not generated shell scripts. The change is private to the sandbox.
    pub fn inert_tools(&mut self, output: &Path, names: &[&str]) -> Result<(), String> {
        fs::create_dir_all(output.join("bin")).map_err(|e| e.to_string())?;
        let true_bin =
            crate::find_program("true").ok_or("The standard 'true' executable is required")?;
        for name in names {
            let target = output.join("bin").join(name);
            fs::copy(&true_bin, &target).map_err(|e| e.to_string())?;
            if let Some(original) = crate::find_program(name) {
                self.bind(&target, original, true);
            }
        }
        self.env("PATH", "/work/bin:/usr/bin:/bin");
        Ok(())
    }
}
fn home() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}
