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

/// On-screen crosshair for gun games: off, or a style from
/// [`banana_input::crosshair::STYLES`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Crosshair(pub Option<&'static str>);

impl Crosshair {
    pub fn id(&self) -> &'static str {
        self.0.unwrap_or("off")
    }
    pub fn label(&self) -> &'static str {
        banana_input::crosshair::STYLES
            .iter()
            .find(|s| Some(s.0) == self.0)
            .map_or("Off", |s| s.1)
    }
    /// Off, then every style.
    pub fn all() -> Vec<Self> {
        std::iter::once(Self(None))
            .chain(banana_input::crosshair::STYLES.iter().map(|s| Self(Some(s.0))))
            .collect()
    }
    fn parse(value: &str) -> Result<Self, String> {
        if value.eq_ignore_ascii_case("off") {
            return Ok(Self(None));
        }
        banana_input::crosshair::STYLES
            .iter()
            .find(|s| s.0.eq_ignore_ascii_case(value))
            .map(|s| Self(Some(s.0)))
            .ok_or_else(|| {
                format!("crosshair must be off, p1, p1_large, p2 or p2_large, not \"{value}\"")
            })
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
    /// Default crosshair, or `None` if the game cannot draw one.
    pub crosshair: Option<Crosshair>,
    /// Most simultaneous players the game's adapter supports (1 or 2).
    pub max_players: u8,
    /// Input methods a player may pick (empty: no choice, e.g. Minecraft).
    pub input_methods: &'static [InputMethod],
    /// Execution backends the user may pick between on Windows hosts.
    /// Empty or a single entry hides the backend selector.
    pub backends: &'static [Backend],
    /// Whether audio is played by a Windows-native backend (which ignores
    /// PulseAudio sink selection).
    pub windows_audio: WindowsAudio,
}

/// Which Windows-host sessions play audio directly through Windows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowsAudio {
    Never,
    Always,
    /// Every backend except WSL2 (which routes through WSLg/PulseAudio).
    UnlessWsl,
}

impl Profile {
    /// Whether this session plays audio through Windows rather than a
    /// PulseAudio-compatible server.
    pub fn plays_windows_audio(&self, settings: &GameSettings) -> bool {
        cfg!(windows)
            && match self.windows_audio {
                WindowsAudio::Never => false,
                WindowsAudio::Always => true,
                WindowsAudio::UnlessWsl => settings.backend != Backend::Wsl,
            }
    }
    /// Whether to show an execution backend selector on this host.
    pub fn backend_choice(&self) -> bool {
        cfg!(windows) && self.backends.len() > 1
    }
}

static PROFILES: std::sync::OnceLock<Vec<&'static Profile>> = std::sync::OnceLock::new();

/// Register the settings profiles declared by game compatibility crates.
/// Called once by `banana-compat`; later calls are ignored.
pub fn install(profiles: Vec<&'static Profile>) {
    let _ = PROFILES.set(profiles);
}

/// Registered profiles, in library order (empty before [`install`]).
pub fn profiles() -> &'static [&'static Profile] {
    PROFILES.get().map(Vec::as_slice).unwrap_or(&[])
}

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
    profiles().iter().copied().find(|p| p.id == id)
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
    /// Only for games whose profile supports one (Big Buck).
    pub crosshair: Crosshair,
    /// Crosshair size, percent of the image (crosshair::SIZE_MIN..=SIZE_MAX).
    pub crosshair_size: u32,
    /// 1, or 2 in two-player games: each controller then drives its own
    /// player (controller 1 = player one, controller 2 = player two).
    pub players: u8,
    /// Each player's input method (`p1_input`, `p2_input`). At most one
    /// player has the keyboard and mouse ([`GameSettings::set_input`] keeps it
    /// that way); player two's applies with two players.
    pub inputs: [InputMethod; 2],
}

/// What a player plays with: keyboard & mouse, XInput, DirectInput or Raw
/// Input (see [`banana_input::pad::Method`]).
pub use banana_input::pad::Method as InputMethod;

/// Input methods for single-seat racers: the keyboard, or an XInput or
/// DirectInput controller or wheel (no aim, so no Raw Input).
pub const RACING_INPUTS: &[InputMethod] = &[
    InputMethod::KeyboardMouse,
    InputMethod::XInput,
    InputMethod::DirectInput,
];

impl GameSettings {
    /// Set what a player (0 or 1) uses. Giving one player the keyboard and
    /// mouse moves the other off it (to XInput).
    pub fn set_input(&mut self, player: usize, input: InputMethod) {
        let player = player.min(1);
        self.inputs[player] = input;
        if input == InputMethod::KeyboardMouse && self.inputs[1 - player] == input {
            self.inputs[1 - player] = InputMethod::XInput;
        }
    }

    /// Who plays with which device, as the adapters will see it.
    pub fn input_assignment(&self) -> banana_input::pad::Inputs {
        banana_input::pad::Inputs {
            players: usize::from(self.players.clamp(1, 2)),
            methods: self.inputs,
        }
    }

    /// `ARCADE_INPUTS` for the adapters: each player's method, comma-separated.
    pub fn inputs_env(&self) -> String {
        format!("{},{}", self.inputs[0].id(), self.inputs[1].id())
    }

    /// The input methods actually in play (player two's only with two players).
    fn methods_label(&self) -> String {
        if self.players > 1 {
            format!("P1 {}, P2 {}", self.inputs[0].label(), self.inputs[1].label())
        } else {
            self.inputs[0].label().to_owned()
        }
    }

    pub fn defaults(profile: &Profile) -> Self {
        Self {
            backend: Backend::Auto,
            resolution: profile.default_resolution,
            fullscreen: false,
            vsync: VSync::Default,
            renderer: profile.default_renderer,
            audio: true,
            audio_sink: String::new(),
            crosshair: profile.crosshair.clone().unwrap_or(Crosshair(None)),
            crosshair_size: banana_input::crosshair::SIZE_DEFAULT,
            players: 1,
            inputs: [InputMethod::XInput; 2],
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
        ) + &match (self.players > 1, self.inputs == [InputMethod::XInput; 2]) {
            (false, true) => String::new(),
            (false, false) => format!(" · {}", self.methods_label()),
            (true, _) => format!(" · 2 players ({})", self.methods_label()),
        }
    }

    /// Environment variables for the sandboxed game process. Read by the
    /// guest shim (ARCADE_*), Mesa (`vblank_mode`, `LIBGL_ALWAYS_SOFTWARE`)
    /// and NVIDIA (`__GL_SYNC_TO_VBLANK`).
    pub fn guest_env(&self) -> Vec<(&'static str, String)> {
        let mut env = vec![("ARCADE_RESOLUTION", self.resolution_text())];
        if let Some(style) = self.crosshair.0 {
            env.push(("ARCADE_CROSSHAIR", style.into()));
            env.push(("ARCADE_CROSSHAIR_SIZE", self.crosshair_size.to_string()));
        }
        if self.fullscreen {
            env.push(("ARCADE_FULLSCREEN", "1".into()));
        }
        if self.players > 1 {
            env.push(("ARCADE_PLAYERS", self.players.to_string()));
        }
        if self.players > 1 || self.inputs[0] != InputMethod::XInput {
            env.push(("ARCADE_INPUTS", self.inputs_env()));
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
            "crosshair" => self.crosshair = Crosshair::parse(value)?,
            "crosshair_size" => {
                use banana_input::crosshair::{SIZE_MAX, SIZE_MIN};
                let size: u32 = value.trim_end_matches('%').trim().parse().map_err(|_| {
                    format!("crosshair_size must be a percentage, not \"{value}\"")
                })?;
                if !(SIZE_MIN..=SIZE_MAX).contains(&size) {
                    return Err(format!(
                        "crosshair_size must be {SIZE_MIN} to {SIZE_MAX} (percent), not {size}"
                    ));
                }
                self.crosshair_size = size;
            }
            "players" => {
                self.players = match value {
                    "1" => 1,
                    "2" => 2,
                    _ => return Err(format!("players must be 1 or 2, not \"{value}\"")),
                }
            }
            "p1_input" | "p2_input" => {
                let input = InputMethod::parse(value.to_ascii_lowercase().as_bytes()).ok_or_else(|| {
                    format!(
                        "{key} must be keyboard_mouse, xinput, dinput or rawinput, not \"{value}\""
                    )
                })?;
                self.set_input(usize::from(key == "p2_input"), input);
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
            games: profiles()
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
            if key.trim().starts_with("crosshair")
                && profile(id).is_some_and(|p| p.crosshair.is_none())
            {
                return Err(at(format!("[{id}] has no crosshair setting")));
            }
            if matches!(key.trim(), "players" | "p2_input")
                && value.trim() != "1"
                && profile(id).is_some_and(|p| p.max_players < 2)
            {
                return Err(at(format!("[{id}] is a one-player game")));
            }
            if matches!(key.trim(), "p1_input" | "p2_input")
                && let Some(p) = profile(id)
                && let Some(method) = InputMethod::parse(value.trim().to_ascii_lowercase().as_bytes())
                && !p.input_methods.contains(&method)
            {
                return Err(at(format!("[{id}] cannot use {}", method.label())));
            }
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
            "# Banana pepper per-game settings. Edit here or in each game's Settings window.\n\
             # Key bindings live in controls.cfg. Changes apply the next time a game starts.\n",
        );
        for p in profiles() {
            let g = self.game(p.id);
            out += &format!("\n[{}]  # {}\n", p.id, p.title);
            out += &format!("resolution = {}\n", g.resolution_text());
            out += &format!("backend = {}  # auto | water | wsl (games with a backend choice on Windows)\n",g.backend.id());
            out += &format!("fullscreen = {}\n", g.fullscreen);
            out += &format!("vsync = {}  # default | on | off\n", g.vsync.id());
            out += &format!("renderer = {}  # gpu | software\n", g.renderer.id());
            out += &format!("audio = {}\n", g.audio);
            out += &format!(
                "audio_sink = {}  # PulseAudio/PipeWire sink; empty = default output\n",
                g.audio_sink
            );
            if p.crosshair.is_some() {
                out += &format!(
                    "crosshair = {}  # off | p1 | p1_large | p2 | p2_large\n",
                    g.crosshair.id()
                );
                out += &format!(
                    "crosshair_size = {}  # percent, {} to {}\n",
                    g.crosshair_size,
                    banana_input::crosshair::SIZE_MIN,
                    banana_input::crosshair::SIZE_MAX
                );
            }
            if p.max_players > 1 {
                out += &format!(
                    "players = {}  # 1 | 2 (controller 2 drives player two)\n",
                    g.players
                );
            }
            if !p.input_methods.is_empty() {
                let choices = p.input_methods.iter().map(|m| m.id()).collect::<Vec<_>>().join(" | ");
                for (player, input) in g.inputs.iter().enumerate().take(usize::from(p.max_players)) {
                    out += &format!("p{}_input = {}  # {choices}\n", player + 1, input.id());
                }
            }
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

