//! Per-game desktop graphics and audio settings, stored in `settings.cfg`.
//!
//! Same plain-text layout as `controls.cfg`, one section per game:
//!
//! ```text
//! [aliens]
//! resolution = 1360x768
//! fullscreen = false
//! vsync = default      # default | on | off
//! renderer = gpu       # gpu | software
//! audio = true
//! audio_sink =         # PulseAudio/PipeWire sink name; empty = system default
//! ```
//!
//! Missing games/keys keep their defaults, which match the original fixed
//! launcher behaviour. Key bindings stay in `controls.cfg`.
use std::{collections::BTreeMap, fs, io, path::Path};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VSync {
    /// Leave the game's and driver's own swap interval alone.
    Default,
    On,
    Off,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Renderer {
    /// The host's OpenGL driver (GPU devices are exposed to the sandbox).
    Gpu,
    /// Mesa's llvmpipe software rasterizer: slower, but works without a GPU.
    Software,
}

#[derive(Debug,Clone,Copy,PartialEq,Eq)]
pub enum Backend { Auto, Water, Wsl }
impl Backend {
    pub const ALL:[Self;3]=[Self::Auto,Self::Water,Self::Wsl];
    pub fn id(self)->&'static str{match self{Self::Auto=>"auto",Self::Water=>"water",Self::Wsl=>"wsl"}}
    pub fn label(self)->&'static str{match self{Self::Auto=>"Automatic",Self::Water=>"Water (Linux to Windows)",Self::Wsl=>"WSL2 / WSLg"}}
}

impl VSync {
    pub const ALL: [Self; 3] = [Self::Default, Self::On, Self::Off];
    pub fn id(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::On => "on",
            Self::Off => "off",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Default => "Game default",
            Self::On => "On",
            Self::Off => "Off",
        }
    }
}

impl Renderer {
    pub const ALL: [Self; 2] = [Self::Gpu, Self::Software];
    pub fn id(self) -> &'static str {
        match self {
            Self::Gpu => "gpu",
            Self::Software => "software",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Gpu => "GPU (hardware OpenGL)",
            Self::Software => "Software (Mesa llvmpipe)",
        }
    }
}

/// What a game's runtime can honour.
#[derive(Debug)]
pub struct Profile {
    pub id: &'static str,
    pub title: &'static str,
    pub default_resolution: (u32, u32),
    pub default_renderer: Renderer,
    /// Shown under the resolution picker.
    pub resolution_note: &'static str,
}

pub const PROFILES: &[Profile] = &[
    Profile {
        id: "aliens",
        title: "Aliens Armageddon",
        default_resolution: (1360, 768),
        default_renderer: Renderer::Gpu,
        resolution_note: "Passed to the game's own -f option. 1360x768 is the cabinet mode and the tested size.",
    },
    Profile {
        id: "jurassic",
        title: "Jurassic Park 2015",
        default_resolution: (1360, 768),
        default_renderer: Renderer::Gpu,
        resolution_note: "Passed to the game's own -f option. 1360x768 is the cabinet mode and the tested size.",
    },
    Profile {
        id: "big_buck",
        title: "Big Buck Hunter Pro",
        default_resolution: (640, 480),
        default_renderer: if cfg!(windows){Renderer::Gpu}else{Renderer::Software},
        resolution_note: "Window size. The game still renders at 640x480; aiming is rescaled to the window.",
    },
    Profile {
        id: "super_cars",
        title: "Fast and Furious Super Cars",
        default_resolution: (1920, 1080),
        default_renderer: Renderer::Gpu,
        resolution_note: "Passed to the game's own -res option: 640x480, 800x600, 1024x768, 1280x1024, 1360x768, 1600x1200 or 1920x1080. Also the Wine window size. Renderer: WineD3D on OpenGL (GPU) or Mesa llvmpipe (software).",
    },
    Profile {id:"ford",title:"Ford Racing Full Blown",default_resolution:(640,480),default_renderer:Renderer::Gpu,resolution_note:"Native Direct3D 9 on Windows; Wine on Linux. Cabinet resolutions: 640x480, 800x600, 1024x768, 1280x1024, 1600x1200. Default is windowed."},
];

pub const RESOLUTIONS: &[(u32, u32)] = &[
    (640, 480),
    (800, 600),
    (1024, 768),
    (1280, 720),
    (1280, 1024),
    (1360, 768),
    (1366, 768),
    (1600, 900),
    (1600, 1200),
    (1920, 1080),
    (2560, 1440),
    (3840, 2160),
];

pub fn profile(id: &str) -> Option<&'static Profile> {
    PROFILES.iter().find(|p| p.id == id)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameSettings {
    pub backend: Backend,
    pub resolution: (u32, u32),
    pub fullscreen: bool,
    pub vsync: VSync,
    pub renderer: Renderer,
    pub audio: bool,
    /// Empty = the desktop's default output.
    pub audio_sink: String,
}

impl GameSettings {
    pub fn defaults(profile: &Profile) -> Self {
        Self {
            backend: Backend::Auto,
            resolution: profile.default_resolution,
            fullscreen: false,
            vsync: VSync::Default,
            renderer: profile.default_renderer,
            audio: true,
            audio_sink: String::new(),
        }
    }

    pub fn resolution_text(&self) -> String {
        format!("{}x{}", self.resolution.0, self.resolution.1)
    }

    /// One-line summary for the game page.
    pub fn summary(&self) -> String {
        format!(
            "{} {} · {} · VSync {} · audio {}",
            self.resolution_text(),
            if self.fullscreen {
                "fullscreen"
            } else {
                "windowed"
            },
            match self.renderer {
                Renderer::Gpu => "GPU",
                Renderer::Software => "software",
            },
            self.vsync.label().to_ascii_lowercase(),
            if self.audio { "on" } else { "off" },
        )
    }

    /// Environment variables for the sandboxed game process. Read by the
    /// guest shim (ARCADE_*), Mesa (`vblank_mode`, `LIBGL_ALWAYS_SOFTWARE`)
    /// and NVIDIA (`__GL_SYNC_TO_VBLANK`).
    pub fn guest_env(&self) -> Vec<(&'static str, String)> {
        let mut env = vec![("ARCADE_RESOLUTION", self.resolution_text())];
        if self.fullscreen {
            env.push(("ARCADE_FULLSCREEN", "1".into()));
        }
        match self.vsync {
            VSync::Default => {}
            VSync::On => {
                env.push(("ARCADE_VSYNC", "1".into()));
                env.push(("vblank_mode", "3".into()));
                env.push(("__GL_SYNC_TO_VBLANK", "1".into()));
            }
            VSync::Off => {
                env.push(("ARCADE_VSYNC", "0".into()));
                env.push(("vblank_mode", "0".into()));
                env.push(("__GL_SYNC_TO_VBLANK", "0".into()));
            }
        }
        if self.renderer == Renderer::Software {
            env.push(("LIBGL_ALWAYS_SOFTWARE", "1".into()));
            env.push(("__GLX_VENDOR_LIBRARY_NAME", "mesa".into()));
        }
        env
    }

    /// Newline-separated `KEY=VALUE` form handed to the Python launchers.
    pub fn guest_env_value(&self) -> String {
        self.guest_env()
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn set(&mut self, key: &str, value: &str) -> Result<(), String> {
        let flag = |v: &str| match v.to_ascii_lowercase().as_str() {
            "true" | "yes" | "on" | "1" => Ok(true),
            "false" | "no" | "off" | "0" => Ok(false),
            _ => Err(format!("expected true or false, got \"{v}\"")),
        };
        match key {
            "backend"=>self.backend=Backend::ALL.into_iter().find(|b|b.id()==value || (value=="native" && *b==Backend::Water)).ok_or("backend must be auto, water or wsl")?,
            "resolution" => self.resolution = parse_resolution(value)?,
            "fullscreen" => self.fullscreen = flag(value)?,
            "vsync" => {
                self.vsync = VSync::ALL
                    .into_iter()
                    .find(|v| v.id().eq_ignore_ascii_case(value))
                    .ok_or_else(|| format!("vsync must be default, on or off, not \"{value}\""))?
            }
            "renderer" => {
                self.renderer = Renderer::ALL
                    .into_iter()
                    .find(|r| r.id().eq_ignore_ascii_case(value))
                    .ok_or_else(|| format!("renderer must be gpu or software, not \"{value}\""))?
            }
            "audio" => self.audio = flag(value)?,
            "audio_sink" => {
                if value.chars().any(|c| c.is_whitespace() || c.is_control()) {
                    return Err("audio_sink cannot contain spaces".into());
                }
                self.audio_sink = value.to_owned();
            }
            _ => return Err(format!("unknown setting \"{key}\"")),
        }
        Ok(())
    }
}

pub fn parse_resolution(value: &str) -> Result<(u32, u32), String> {
    let bad = || format!("resolution must look like 1360x768, not \"{value}\"");
    let (w, h) = value.trim().split_once(['x', 'X', '×']).ok_or_else(bad)?;
    let w: u32 = w.trim().parse().map_err(|_| bad())?;
    let h: u32 = h.trim().parse().map_err(|_| bad())?;
    if !(320..=7680).contains(&w) || !(240..=4320).contains(&h) {
        return Err(format!("resolution {w}x{h} is out of range"));
    }
    Ok((w, h))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    games: BTreeMap<&'static str, GameSettings>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            games: PROFILES
                .iter()
                .map(|p| (p.id, GameSettings::defaults(p)))
                .collect(),
        }
    }
}

impl Settings {
    /// Panics only for an id outside [`PROFILES`].
    pub fn game(&self, id: &str) -> &GameSettings {
        &self.games[id]
    }
    pub fn game_mut(&mut self, id: &str) -> Option<&mut GameSettings> {
        self.games.get_mut(id)
    }
    pub fn reset_game(&mut self, id: &str) {
        if let Some(p) = profile(id) {
            self.games.insert(p.id, GameSettings::defaults(p));
        }
    }

    pub fn parse(text: &str) -> Result<Self, String> {
        let mut settings = Self::default();
        let mut section: Option<&'static str> = None;
        for (number, raw) in text.lines().enumerate() {
            let line = raw.split('#').next().unwrap_or("").trim();
            let at = |e: String| format!("settings.cfg line {}: {e}", number + 1);
            if line.is_empty() {
                continue;
            }
            if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
                section = Some(
                    profile(name.trim())
                        .ok_or_else(|| at(format!("unknown game [{name}]")))?
                        .id,
                );
                continue;
            }
            let id = section.ok_or_else(|| at("setting before any [game] section".into()))?;
            let (key, value) = line
                .split_once('=')
                .ok_or_else(|| at("expected setting = value".into()))?;
            settings
                .games
                .get_mut(id)
                .expect("profile section")
                .set(key.trim(), value.trim())
                .map_err(at)?;
        }
        Ok(settings)
    }

    pub fn to_text(&self) -> String {
        let mut out = String::from(
            "# Arcade Foundry per-game settings. Edit here or in each game's Settings window.\n\
             # Key bindings live in controls.cfg. Changes apply the next time a game starts.\n",
        );
        for p in PROFILES {
            let g = self.game(p.id);
            out += &format!("\n[{}]  # {}\n", p.id, p.title);
            out += &format!("resolution = {}\n", g.resolution_text());
            out += &format!("backend = {}  # auto | water | wsl (Big Buck on Windows)\n",g.backend.id());
            out += &format!("fullscreen = {}\n", g.fullscreen);
            out += &format!("vsync = {}  # default | on | off\n", g.vsync.id());
            out += &format!("renderer = {}  # gpu | software\n", g.renderer.id());
            out += &format!("audio = {}\n", g.audio);
            out += &format!(
                "audio_sink = {}  # PulseAudio/PipeWire sink; empty = default output\n",
                g.audio_sink
            );
        }
        out
    }

    /// Missing file → defaults.
    pub fn load(path: &Path) -> Result<Self, String> {
        match fs::read_to_string(path) {
            Ok(text) => Self::parse(&text),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(format!("Cannot read {}: {e}", path.display())),
        }
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        let temp = path.with_extension("cfg.saving");
        fs::write(&temp, self.to_text())
            .and_then(|_| fs::rename(&temp, path))
            .map_err(|e| format!("Cannot save {}: {e}", path.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_previous_fixed_launch_behaviour() {
        let s = Settings::default();
        assert_eq!(s.game("aliens").resolution_text(), "1360x768");
        assert_eq!(
            s.game("jurassic").guest_env(),
            [("ARCADE_RESOLUTION", "1360x768".into())]
        );
        let bbh = s.game("big_buck");
        assert_eq!(bbh.resolution, (640, 480));
        assert_eq!(bbh.renderer, if cfg!(windows){Renderer::Gpu}else{Renderer::Software});
        assert!(!bbh.fullscreen && bbh.audio);
    }

    #[test]
    fn round_trip_and_partial_files() {
        let mut s = Settings::default();
        let g = s.game_mut("jurassic").unwrap();
        g.resolution = (1920, 1080);
        g.fullscreen = true;
        g.vsync = VSync::Off;
        g.renderer = Renderer::Software;
        g.audio = false;
        g.audio_sink = "alsa_output.usb".into();
        assert_eq!(Settings::parse(&s.to_text()).unwrap(), s);
        let p = Settings::parse("[aliens]\nresolution = 1280X720 # c\nvsync = ON\n").unwrap();
        assert_eq!(p.game("aliens").resolution, (1280, 720));
        assert_eq!(p.game("aliens").vsync, VSync::On);
        assert_eq!(p.game("big_buck"), Settings::default().game("big_buck"));
        let env = p.game("aliens").guest_env_value();
        assert!(env.contains("vblank_mode=3") && env.contains("ARCADE_VSYNC=1"));
    }

    #[test]
    fn rejects_bad_settings() {
        assert!(Settings::parse("[aliens]\nresolution = big\n").is_err());
        assert!(Settings::parse("[aliens]\nresolution = 10x10\n").is_err());
        assert!(Settings::parse("[aliens]\nvsync = sometimes\n").is_err());
        assert!(Settings::parse("[aliens]\nrenderer = vulkan\n").is_err());
        assert!(Settings::parse("[aliens]\nfullscreen = maybe\n").is_err());
        assert!(Settings::parse("[aliens]\naudio_sink = a b\n").is_err());
        assert!(Settings::parse("[aliens]\nbogus = 1\n").is_err());
        assert!(Settings::parse("[nope]\n").is_err());
        assert!(Settings::parse("audio = true\n").is_err());
    }
}
