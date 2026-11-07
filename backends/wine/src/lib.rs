//! Wine execution backend for Windows guests on Linux hosts.
use banana_api::{Availability, BackendKind, ExecutionBackend};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::{Command, ExitStatus},
};

/// Private prefix lifecycle and Wine registry operations, independent of titles.
pub struct Prefix {
    pub path: PathBuf,
    pub env: BTreeMap<String, String>,
    wine: PathBuf,
    log: PathBuf,
}
impl Prefix {
    pub fn new(path: &Path, log: &Path, architecture: &str) -> Result<Self, String> {
        fs::File::create(log).map_err(|e| e.to_string())?;
        let wine = std::env::var_os("ARCADE_WINE")
            .map(PathBuf::from)
            .or_else(|| find("wine"))
            .ok_or("Wine is required; install it or set ARCADE_WINE")?;
        let p = Self {
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
            let local = p.wine.with_file_name("wineserver");
            let mut c = Command::new(if local.is_file() {
                local
            } else {
                PathBuf::from("wineserver")
            });
            c.arg("-w").envs(&p.env);
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
    pub fn command(&self) -> Command {
        let mut c = Command::new(&self.wine);
        c.envs(&self.env).env_remove("ARCADE_GUEST_ENV");
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
        if !cfg!(unix) {
            return Availability::Unsupported;
        }
        let found = std::env::var_os("PATH")
            .is_some_and(|path| std::env::split_paths(&path).any(|dir| dir.join("wine").is_file()));
        if found {
            Availability::Available
        } else {
            Availability::Missing("wine is not installed".into())
        }
    }
}
