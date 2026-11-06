mod appearance;
mod aliens_runtime;
mod jurassic_runtime;
mod library_preferences;
mod minecraft_runtime;
mod runtime;
mod supercars_runtime;

use arcade_core::controls::{self, Bindings};
use arcade_core::settings::{self, Renderer, Settings, VSync};
use arcade_core::{CabinetInput, GameReport};
use eframe::egui::{self, Color32, RichText};
use std::{
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver},
};

fn main() {
    if let Err(error) = run_app() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

fn run_app() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.first().is_some_and(|a| a == "--probe-minecraft" || a == "--launch-minecraft" || a == "--setup-minecraft-libs") {
        if args.len() > 2 { return Err("Usage: arcade-desktop --probe-minecraft|--launch-minecraft|--setup-minecraft-libs [game-folder-or-library-root]".into()); }
        let root = args.get(1).map(PathBuf::from).unwrap_or_else(|| "games".into());
        let mode = if args[0] == "--launch-minecraft" { "launch" } else if args[0] == "--setup-minecraft-libs" { "setup" } else { "probe" };
        println!("{}", minecraft_runtime::run(&root, mode)?);
        return Ok(());
    }
    if args
        .first()
        .is_some_and(|a| a == "--probe-aliens" || a == "--launch-aliens")
    {
        if args.len() > 2 {
            return Err("Usage: arcade-desktop --probe-aliens|--launch-aliens [games-root]".into());
        }
        let root = args
            .get(1)
            .map(PathBuf::from)
            .unwrap_or_else(|| "games".into());
        println!(
            "{}",
            aliens_runtime::run(&root, args[0] == "--launch-aliens")?
        );
        return Ok(());
    }
    if args.first().is_some_and(|a| a == "--launch-jurassic") {
        if args.len() > 2 {
            return Err("Usage: arcade-desktop --launch-jurassic [games-root]".into());
        }
        let root = args
            .get(1)
            .map(PathBuf::from)
            .unwrap_or_else(|| "games".into());
        println!(
            "Launching Jurassic Park on your display. Mouse = aim, Esc = exit; keys come from controls.cfg. Audio plays through your desktop sound."
        );
        println!("{}", jurassic_runtime::launch(&root)?);
        return Ok(());
    }
    if args.first().is_some_and(|a| {
        a == "--probe-jurassic" || a == "--probe-jurassic-legacy" || a == "--probe-jurassic-native"
    }) {
        if args.len() > 2 {
            return Err(
                "Usage: arcade-desktop --probe-jurassic[-legacy|-native] [games-root]".into(),
            );
        }
        let build = if args[0] == "--probe-jurassic-native" {
            arcade_core::jurassic::Build::Native
        } else if args[0] == "--probe-jurassic-legacy" {
            arcade_core::jurassic::Build::Legacy
        } else {
            arcade_core::jurassic::Build::Original
        };
        let root = args
            .get(1)
            .map(PathBuf::from)
            .unwrap_or_else(|| "games".into());
        println!("{}", jurassic_runtime::probe(&root, build)?);
        return Ok(());
    }
    if args.first().is_some_and(|a| a == "--launch-super-cars") {
        if args.len() > 2 {
            return Err("Usage: arcade-desktop --launch-super-cars [games-root]".into());
        }
        let root = args
            .get(1)
            .map(PathBuf::from)
            .unwrap_or_else(|| "games".into());
        println!(
            "Launching Super Cars under Wine. Arrows = steer/gas/brake, Enter = start, 5 = coin, Esc = exit; keys come from controls.cfg."
        );
        println!("{}", supercars_runtime::launch(&root)?);
        return Ok(());
    }
    if args.first().is_some_and(|a| a == "--launch-big-buck") {
        if args.len() > 2 {
            return Err("Usage: arcade-desktop --launch-big-buck [games-root]".into());
        }
        let root = args
            .get(1)
            .map(PathBuf::from)
            .unwrap_or_else(|| "games".into());
        println!(
            "Launching Big Buck Hunter on your display. Mouse = aim/fire, right click = pump, Esc = exit; keys come from controls.cfg."
        );
        println!("{}", runtime::launch_big_buck(&root)?);
        return Ok(());
    }
    if args.first().is_some_and(|a| a == "--probe-big-buck") {
        if args.len() > 2 {
            return Err("Usage: arcade-desktop --probe-big-buck [games-root]".into());
        }
        let root = args
            .get(1)
            .map(PathBuf::from)
            .unwrap_or_else(|| "games".into());
        println!(
            "{}",
            runtime::probe_big_buck(&root, &PathBuf::from("runtime/big-buck"))?
        );
        return Ok(());
    }
    if args.first().is_some_and(|a| a == "--scan") {
        if args.len() > 2 {
            return Err("Usage: arcade-desktop --scan [games-root]".into());
        }
        let root = args
            .get(1)
            .map(PathBuf::from)
            .unwrap_or_else(|| "games".into());
        println!(
            "{}",
            serde_json::to_string_pretty(&arcade_core::scan(&root))?
        );
        return Ok(());
    }
    let initial_root = if args.first().is_some_and(|a| a == "--games-root") && args.len() == 2 {
        Some(args[1].to_string_lossy().into_owned())
    } else { None };
    if !args.is_empty() && initial_root.is_none() {
        return Err(
            "Usage: arcade-desktop [--games-root PATH | --scan [PATH] | --probe-minecraft [PATH] | --launch-minecraft [PATH] | --setup-minecraft-libs [PATH] | --probe-big-buck [PATH] | --launch-big-buck [PATH] | --launch-super-cars [PATH]]".into(),
        );
    }
    eframe::run_native(
        "Arcade Foundry",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default().with_inner_size([1160.0, 800.0]).with_min_inner_size([850.0, 620.0]),
            ..Default::default()
        },
        Box::new(move |cc| {
            appearance::install(&cc.egui_ctx);
            let mut app = ArcadeApp::default();
            if let Some(root) = initial_root { app.root = root; app.selected = 4; }
            app.rescan(cc.egui_ctx.clone());
            Ok(Box::new(app))
        }),
    )?;
    Ok(())
}

struct ArcadeApp {
    root: String,
    session_open: bool,
    session_failed: bool,
    folder_message: String,
    folders: library_preferences::Preferences,
    folder_drafts: std::collections::BTreeMap<String, String>,
    game_roots: std::collections::BTreeMap<String, Result<PathBuf, String>>,
    reports: Vec<GameReport>,
    selected: usize,
    pending: Option<Receiver<Vec<GameReport>>>,
    message: String,
    input: CabinetInput,
    probe: Option<Receiver<Result<String, String>>>,
    screenshot: Option<egui::TextureHandle>,
    desktop_running: bool,
    controls: ControlsUi,
    settings: SettingsUi,
}

/// Editable desktop key bindings, saved to controls.cfg in the project root.
struct ControlsUi {
    bindings: Bindings,
    path: PathBuf,
    /// (game, action, add instead of replace) while waiting for a key press.
    capturing: Option<(&'static str, &'static str, bool)>,
    message: String,
}
impl ControlsUi {
    fn load() -> Self {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../controls.cfg");
        let (bindings, message) = match Bindings::load(&path) {
            Ok(bindings) => (bindings, String::new()),
            Err(e) => (
                Bindings::default(),
                format!("{e}. Showing defaults; changing a key will replace the file."),
            ),
        };
        Self {
            bindings,
            path,
            capturing: None,
            message,
        }
    }
    fn save(&mut self, change: String) {
        self.message = match self.bindings.save(&self.path) {
            Ok(()) => format!("{change}. Saved; applies the next time the game starts."),
            Err(e) => e,
        };
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SettingsTab {
    Directory,
    Controls,
    Graphics,
    Audio,
}

/// Per-game settings window state; values are saved to settings.cfg.
struct SettingsUi {
    settings: Settings,
    path: PathBuf,
    /// Game whose settings window is open.
    open: Option<String>,
    tab: SettingsTab,
    message: String,
}
impl SettingsUi {
    fn load() -> Self {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../settings.cfg");
        let (settings, message) = match Settings::load(&path) {
            Ok(settings) => (settings, String::new()),
            Err(e) => (
                Settings::default(),
                format!("{e}. Showing defaults; changing a setting will replace the file."),
            ),
        };
        Self {
            settings,
            path,
            open: None,
            tab: SettingsTab::Directory,
            message,
        }
    }
    fn save(&mut self, change: &str) {
        self.message = match self.settings.save(&self.path) {
            Ok(()) => format!("{change}. Saved; applies the next time the game starts."),
            Err(e) => e,
        };
    }
}

/// "Settings" button plus the current summary, shown on each game page.
fn settings_button(ui: &mut egui::Ui, app: &mut SettingsUi, game_id: &str) {
    if ui.button("Settings").on_hover_text("Game folder, controls, graphics and audio").clicked() {
        app.open = Some(game_id.to_owned());
        app.tab = SettingsTab::Directory;
        app.message.clear();
    }
}

fn settings_window(ctx: &egui::Context, app: &mut ArcadeApp, frame: &eframe::Frame) {
    let Some(id) = app.settings.open.clone() else {
        return;
    };
    let Some(game) = arcade_core::profiles()
        .into_iter()
        .find(|game| game.id == id)
    else {
        app.settings.open = None;
        return;
    };
    let profile = settings::profile(&id.replace('-', "_"));
    let mut open = true;
    egui::Window::new(format!("{} settings", game.title))
        .id(egui::Id::new("game-settings"))
        .open(&mut open)
        .collapsible(false)
        .resizable(true)
        .pivot(egui::Align2::CENTER_CENTER)
        .default_pos(ctx.content_rect().center())
        .default_width(640.0)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                let mut tabs = vec![(SettingsTab::Directory, "Game files")];
                if profile.is_some() {
                    tabs.extend([
                        (SettingsTab::Controls, "Controls"),
                        (SettingsTab::Graphics, "Graphics"),
                        (SettingsTab::Audio, "Audio"),
                    ]);
                }
                for (tab, label) in tabs {
                    if ui
                        .selectable_label(app.settings.tab == tab, label)
                        .clicked()
                    {
                        app.settings.tab = tab;
                        app.controls.capturing = None;
                    }
                }
            });
            ui.separator();
            egui::ScrollArea::vertical()
                .max_height(460.0)
                .show(ui, |ui| match app.settings.tab {
                    SettingsTab::Directory => {
                        app.game_folder_ui(ui, &id, &game.title, frame);
                        ui.add_space(8.0);
                        ui.label(if app.folders.games.contains_key(&id) {
                            "Using a saved folder for this game."
                        } else {
                            "Using the default library folder."
                        });
                        if !app.folder_message.is_empty() {
                            ui.label(&app.folder_message);
                        }
                    }
                    SettingsTab::Controls => {
                        if let Some(profile) = profile {
                            controls_panel(ui, &mut app.controls, profile.id);
                        }
                    }
                    SettingsTab::Graphics => {
                        if let Some(profile) = profile {
                            graphics_tab(ui, &mut app.settings, profile);
                        }
                    }
                    SettingsTab::Audio => {
                        if let Some(profile) = profile {
                            audio_tab(ui, &mut app.settings, profile);
                        }
                    }
                });
            if matches!(app.settings.tab, SettingsTab::Graphics | SettingsTab::Audio)
                && let Some(profile) = profile
            {
                    ui.separator();
                    ui.horizontal(|ui| {
                        if ui.button("Reset graphics & audio").clicked() {
                            app.settings.settings.reset_game(profile.id);
                            app.settings
                                .save(&format!("{} graphics and audio reset", profile.title));
                        }
                        ui.small(app.settings.path.display().to_string());
                    });
                    if !app.settings.message.is_empty() {
                        ui.small(app.settings.message.as_str());
                    }
            }
        });
    if !open {
        app.settings.open = None;
        app.controls.capturing = None;
    }
}

fn graphics_tab(ui: &mut egui::Ui, app: &mut SettingsUi, profile: &'static settings::Profile) {
    let mut change = None;
    let game = app.settings.game_mut(profile.id).expect("profile settings");
    egui::Grid::new(("graphics", profile.id))
        .num_columns(2)
        .spacing([16.0, 10.0])
        .show(ui, |ui| {
            ui.label("Resolution");
            let current = game.resolution;
            egui::ComboBox::from_id_salt(("resolution", profile.id))
                .selected_text(game.resolution_text())
                .show_ui(ui, |ui| {
                    let mut options = settings::RESOLUTIONS.to_vec();
                    if !options.contains(&current) {
                        options.push(current); // Custom size from settings.cfg.
                    }
                    for (w, h) in options {
                        let mut label = format!("{w}x{h}");
                        if (w, h) == profile.default_resolution {
                            label += "  (default)";
                        }
                        if ui
                            .selectable_value(&mut game.resolution, (w, h), label)
                            .clicked()
                        {
                            change = Some(format!("Resolution {w}x{h}"));
                        }
                    }
                });
            ui.end_row();
            ui.label("");
            ui.small(profile.resolution_note);
            ui.end_row();

            ui.label("Display mode");
            ui.horizontal(|ui| {
                if ui
                    .radio_value(&mut game.fullscreen, false, "Windowed")
                    .clicked()
                {
                    change = Some("Windowed".into());
                }
                if ui
                    .radio_value(&mut game.fullscreen, true, "Fullscreen")
                    .clicked()
                {
                    change = Some("Fullscreen".into());
                }
            });
            ui.end_row();

            ui.label("VSync");
            ui.horizontal(|ui| {
                for v in VSync::ALL {
                    if ui.radio_value(&mut game.vsync, v, v.label()).clicked() {
                        change = Some(format!("VSync {}", v.label().to_ascii_lowercase()));
                    }
                }
            });
            ui.end_row();

            ui.label("Renderer");
            ui.vertical(|ui| {
                for r in Renderer::ALL {
                    let mut label = r.label().to_owned();
                    if r == profile.default_renderer {
                        label += "  (default)";
                    }
                    if ui.radio_value(&mut game.renderer, r, label).clicked() {
                        change = Some(format!("Renderer {}", r.id()));
                    }
                }
            });
            ui.end_row();
        });
    ui.add_space(6.0);
    ui.small(
        "Fullscreen fills the monitor the game window opens on; pick that monitor's resolution so the picture fills it. Fullscreen and non-default sizes are experimental. Esc still exits the game.",
    );
    if let Some(change) = change {
        app.save(&change);
    }
}

fn audio_tab(ui: &mut egui::Ui, app: &mut SettingsUi, profile: &'static settings::Profile) {
    let mut change = None;
    let game = app.settings.game_mut(profile.id).expect("profile settings");
    egui::Grid::new(("audio", profile.id))
        .num_columns(2)
        .spacing([16.0, 10.0])
        .show(ui, |ui| {
            ui.label("Sound");
            if ui
                .checkbox(&mut game.audio, "Play through the desktop sound server")
                .changed()
            {
                change = Some(if game.audio { "Sound on" } else { "Sound off" }.to_owned());
            }
            ui.end_row();

            ui.label("Output device");
            ui.add_enabled_ui(game.audio, |ui| {
                let response = ui.add(
                    egui::TextEdit::singleline(&mut game.audio_sink)
                        .hint_text("System default")
                        .desired_width(300.0),
                );
                if response.changed() {
                    game.audio_sink
                        .retain(|c| !c.is_whitespace() && !c.is_control());
                }
                if response.lost_focus() {
                    change = Some(if game.audio_sink.is_empty() {
                        "Output: system default".to_owned()
                    } else {
                        format!("Output: {}", game.audio_sink)
                    });
                }
            });
            ui.end_row();
        });
    ui.add_space(6.0);
    ui.small("Output device is a PulseAudio/PipeWire sink name (see `pactl list short sinks`). Leave empty for the default output. Use the game's own operator menu for in-game volume.");
    if let Some(change) = change {
        app.save(&change);
    }
}

fn controls_panel(ui: &mut egui::Ui, state: &mut ControlsUi, game_id: &str) {
    let Some(game) = controls::game(game_id) else {
        return;
    };
    if let Some((capture_game, action, append)) = state.capturing
        && capture_game == game.id
    {
        let captured = ui.ctx().input(|i| {
            i.events.iter().find_map(|event| match event {
                egui::Event::Key {
                    key,
                    pressed: true,
                    repeat: false,
                    ..
                } => Some(key.name().to_owned()),
                egui::Event::PointerButton {
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    ..
                } => Some("MouseLeft".to_owned()),
                egui::Event::PointerButton {
                    button: egui::PointerButton::Secondary,
                    pressed: true,
                    ..
                } => Some("MouseRight".to_owned()),
                _ => None,
            })
        });
        if let Some(name) = captured {
            state.capturing = None;
            if name == "Escape" || (name.starts_with("Mouse") && !game.mouse_buttons) {
                state.message = "Key change cancelled.".into();
            } else {
                match state.bindings.rebind(game.id, action, &name, append) {
                    Ok(change) => state.save(change),
                    Err(e) => state.message = format!("Not changed: {e}"),
                }
            }
        }
        ui.ctx().request_repaint();
    }
    ui.small(game.note);
    ui.add_space(4.0);
    egui::Grid::new(("controls", game.id))
        .num_columns(3)
        .spacing([16.0, 6.0])
        .striped(true)
        .show(ui, |ui| {
            for action in game.actions {
                ui.label(action.label);
                let waiting = matches!(state.capturing,
                            Some((g, a, _)) if g == game.id && a == action.id);
                let keys = state.bindings.keys(game.id, action.id).join(", ");
                let text = if waiting {
                    "Press a key… (Esc cancels)".to_owned()
                } else if keys.is_empty() {
                    "unbound".to_owned()
                } else {
                    keys
                };
                let button = egui::Button::new(RichText::new(text).monospace())
                    .min_size(egui::vec2(170.0, 0.0));
                if ui
                    .add(button)
                    .on_hover_text("Click, then press the new key")
                    .clicked()
                {
                    state.capturing = Some((game.id, action.id, false));
                    state.message = format!("Press a key for {}.", action.label);
                }
                ui.horizontal(|ui| {
                    if action.single {
                        return;
                    }
                    if ui
                        .small_button("+")
                        .on_hover_text("Add another key")
                        .clicked()
                    {
                        state.capturing = Some((game.id, action.id, true));
                        state.message = format!("Press an extra key for {}.", action.label);
                    }
                    if ui.small_button("×").on_hover_text("Unbind").clicked() {
                        match state.bindings.clear(game.id, action.id) {
                            Ok(()) => state.save(format!("{} unbound", action.label)),
                            Err(e) => state.message = format!("Not changed: {e}"),
                        }
                    }
                });
                ui.end_row();
            }
        });
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        if ui.button("Reset to defaults").clicked() {
            state.capturing = None;
            state.bindings.reset_game(game.id);
            state.save(format!("{} controls reset", game.title));
        }
        ui.small(state.path.display().to_string());
    });
    if !state.message.is_empty() {
        ui.small(state.message.as_str());
    }
}

/// Use the platform's folder chooser, parented to the desktop window.
fn pick_folder(
    frame: &eframe::Frame,
    title: &str,
    current: &str,
) -> Result<Option<String>, String> {
    let mut dialog = rfd::FileDialog::new().set_title(title).set_parent(frame);
    if let Ok(directory) = Path::new(current).canonicalize()
        && directory.is_dir()
    {
        dialog = dialog.set_directory(directory);
    }
    dialog
        .pick_folder()
        .map(|folder| {
            folder
                .into_os_string()
                .into_string()
                .map_err(|_| "The selected folder path must contain valid Unicode.".into())
        })
        .transpose()
}

impl Default for ArcadeApp {
    fn default() -> Self {
        let (folders, folder_message) = match library_preferences::load() {
            Ok(folders) => (folders, String::new()),
            Err(error) => (
                library_preferences::Preferences {
                    root: "games".into(),
                    ..Default::default()
                },
                error,
            ),
        };
        Self {
            root: folders.root.clone(),
            session_open: false,
            session_failed: false,
            folder_drafts: folders.games.clone(),
            folders,
            folder_message,
            game_roots: Default::default(),
            reports: vec![],
            selected: 0,
            pending: None,
            message: "Choose a game to inspect its runtime requirements.".into(),
            input: CabinetInput::default(),
            probe: None,
            screenshot: None,
            desktop_running: false,
            controls: ControlsUi::load(),
            settings: SettingsUi::load(),
        }
    }
}
impl ArcadeApp {
    fn launch_game(&mut self, id: &str, root: PathBuf, ctx: egui::Context, mode: &'static str) {
        let id = id.to_owned();
        let name = arcade_core::profiles().into_iter().find(|profile| profile.id == id)
            .map(|profile| profile.title).unwrap_or_else(|| id.clone());
        self.screenshot = None;
        self.session_failed = false;
        self.desktop_running = true;
        self.message = format!(
            "{name}: {}…",
            if mode == "setup" {
                "preparing libraries"
            } else {
                "starting game"
            }
        );
        let (tx, rx) = mpsc::channel();
        self.probe = Some(rx);
        std::thread::spawn(move || {
            let result = match id.as_str() {
                "aliens" => aliens_runtime::run(&root, true),
                "jurassic" => jurassic_runtime::launch(&root),
                "big-buck" => runtime::launch_big_buck(&root),
                "super-cars" => supercars_runtime::launch(&root),
                "minecraft" => minecraft_runtime::run(&root, mode),
                _ => Err("This game does not have a desktop runtime yet.".into()),
            };
            let _ = tx.send(result);
            ctx.request_repaint();
        });
    }

    fn save_games_folder(&mut self, ctx: egui::Context) {
        let result = library_preferences::validate(&self.root).and_then(|root| {
            let mut folders = self.folders.clone();
            folders.root = root;
            library_preferences::save(&folders)?;
            Ok(folders)
        });
        match result {
            Ok(folders) => {
                self.root = folders.root.clone();
                self.folders = folders;
                self.folder_message =
                    "Default library folder saved. Individual game folders take priority.".into();
                self.rescan(ctx);
            }
            Err(error) => self.folder_message = error,
        }
    }

    fn game_folder_ui(&mut self, ui: &mut egui::Ui, id: &str, title: &str, frame: &eframe::Frame) {
        let mut save = false;
        let mut reset = false;
        let mut browse = false;
        ui.add_enabled_ui(self.probe.is_none(), |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label("Game folder");
                let draft = self.folder_drafts.entry(id.to_owned()).or_default();
                let response = ui.add(
                    egui::TextEdit::singleline(draft)
                        .hint_text("Directory containing this game's executable")
                        .desired_width(380.0),
                );
                browse = ui.button("Browse…").clicked();
                save = ui.button("Save game folder").clicked()
                    || (response.lost_focus()
                        && ui.input(|input| input.key_pressed(egui::Key::Enter)));
                reset = ui
                    .add_enabled(
                        self.folders.games.contains_key(id),
                        egui::Button::new("Use default library"),
                    )
                    .clicked();
            });
        });
        ui.small("Choose this game's own directory. Each game's folder is saved separately.");
        if browse {
            let current = self
                .folder_drafts
                .get(id)
                .filter(|path| !path.is_empty())
                .unwrap_or(&self.root);
            match pick_folder(frame, &format!("Choose {title} folder"), current) {
                Ok(Some(folder)) => {
                    self.folder_drafts.insert(id.into(), folder);
                    save = true;
                }
                Ok(None) => {}
                Err(error) => self.folder_message = error,
            }
        }
        if save || reset {
            let result = (|| {
                let mut folders = self.folders.clone();
                if reset {
                    folders.games.remove(id);
                } else {
                    let folder = library_preferences::validate(&self.folder_drafts[id])?;
                    library_preferences::runtime_root(id, &folder)?;
                    folders.games.insert(id.into(), folder);
                }
                library_preferences::save(&folders)?;
                Ok::<_, String>(folders)
            })();
            match result {
                Ok(folders) => {
                    self.folder_drafts.insert(
                        id.into(),
                        folders.games.get(id).cloned().unwrap_or_default(),
                    );
                    self.folders = folders;
                    self.folder_message = if reset {
                        "Game now uses the default library folder."
                    } else {
                        "Game folder saved for the next launch and future sessions."
                    }
                    .into();
                    self.screenshot = None;
                    self.rescan(ui.ctx().clone());
                }
                Err(error) => self.folder_message = error,
            }
        }
    }

    fn rescan(&mut self, ctx: egui::Context) {
        let root = PathBuf::from(&self.root);
        self.game_roots = self
            .folders
            .games
            .iter()
            .map(|(id, folder)| (id.clone(), library_preferences::runtime_root(id, folder)))
            .collect();
        let game_roots = self.game_roots.clone();
        let (tx, rx) = mpsc::channel();
        self.pending = Some(rx);
        self.message = "Refreshing your library…".into();
        std::thread::spawn(move || {
            let mut reports = arcade_core::scan(&root);
            for report in &mut reports {
                if let Some(custom_root) = game_roots.get(&report.profile.id) {
                    match custom_root {
                        Ok(custom_root) => {
                            if let Some(mut custom) = arcade_core::scan(custom_root)
                                .into_iter()
                                .find(|r| r.profile.id == report.profile.id)
                            {
                                if let Ok(path) = custom.path.canonicalize() {
                                    custom.path = path;
                                }
                                *report = custom;
                            }
                        }
                        Err(error) => {
                            report.binary = None;
                            report.error = Some(error.clone());
                        }
                    }
                }
            }
            let _ = tx.send(reports);
            ctx.request_repaint();
        });
    }
}
impl eframe::App for ArcadeApp {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        if let Some(rx) = &self.pending {
            match rx.try_recv() {
                Ok(reports) => {
                    self.reports = reports;
                    self.pending = None;
                    self.message = "Your library is up to date.".into();
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.pending = None;
                    self.message = "Inspection worker stopped unexpectedly.".into();
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        if let Some(rx) = &self.probe {
            match rx.try_recv() {
                Ok(result) => {
                    if result.is_ok()
                        && !self.desktop_running
                        && let Ok(pixels) =
                            image::open("runtime/big-buck/screen.png").map(|i| i.to_rgba8())
                    {
                        let size = [pixels.width() as usize, pixels.height() as usize];
                        let color = egui::ColorImage::from_rgba_unmultiplied(size, pixels.as_raw());
                        self.screenshot = Some(ui.ctx().load_texture(
                            "big-buck-capture",
                            color,
                            egui::TextureOptions::LINEAR,
                        ));
                    }
                    self.session_failed = result.is_err();
                    self.session_open |= self.session_failed;
                    self.message = result.unwrap_or_else(|e| e);
                    self.probe = None;
                    self.desktop_running = false;
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.message = "Startup worker stopped unexpectedly".into();
                    self.session_failed = true;
                    self.session_open = true;
                    self.probe = None;
                    self.desktop_running = false;
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        let ctx = ui.ctx().clone();
        egui::Panel::top("header")
            .frame(
                egui::Frame::new()
                    .fill(Color32::from_gray(29))
                    .inner_margin(egui::Margin::symmetric(16, 12)),
            )
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("ARCADE FOUNDRY")
                            .size(18.0)
                            .color(appearance::ACCENT),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            RichText::new(env!("CARGO_PKG_VERSION"))
                                .small()
                                .color(appearance::MUTED),
                        );
                    });
                });
                ui.add_space(4.0);
                ui.horizontal_wrapped(|ui| {
                    ui.label("Games folder");
                    let folder = ui.add_enabled(
                        self.probe.is_none(),
                        egui::TextEdit::singleline(&mut self.root)
                            .hint_text("Path to your game files")
                            .desired_width((ui.available_width() - 430.0).clamp(210.0, 520.0)),
                    );
                    if folder.changed() {
                        self.folder_message =
                            "Folder changed. Choose Save folder to remember it.".into();
                    }
                    let mut selected_folder = false;
                    if ui
                        .add_enabled(self.probe.is_none(), egui::Button::new("Browse…"))
                        .clicked()
                    {
                        match pick_folder(frame, "Choose default library folder", &self.root) {
                            Ok(Some(folder)) => {
                                self.root = folder;
                                selected_folder = true;
                            }
                            Ok(None) => {}
                            Err(error) => self.folder_message = error,
                        }
                    }
                    let save = ui
                        .add_enabled(self.probe.is_none(), egui::Button::new("Save folder"))
                        .on_hover_text("Remember this directory and inspect its game files")
                        .clicked();
                    let enter = folder.lost_focus()
                        && ui.input(|input| input.key_pressed(egui::Key::Enter));
                    if save || enter || selected_folder {
                        self.save_games_folder(ctx.clone());
                    }
                    if ui
                        .add_enabled(self.pending.is_none(), egui::Button::new("Refresh library"))
                        .clicked()
                    {
                        self.rescan(ctx.clone());
                    }
                    if ui
                        .add_enabled(
                            !self.reports.is_empty() && self.pending.is_none(),
                            egui::Button::new("Export JSON"),
                        )
                        .clicked()
                    {
                        self.message = match arcade_core::write_report(
                            &PathBuf::from("arcade-report.json"),
                            &self.reports,
                        ) {
                            Ok(()) => {
                                "Saved arcade-report.json in the current working directory.".into()
                            }
                            Err(e) => format!("Export failed: {e}"),
                        };
                    }
                });
                if !self.folder_message.is_empty() {
                    ui.small(&self.folder_message);
                }
            });
        egui::Panel::bottom("status")
            .frame(
                egui::Frame::new()
                    .fill(Color32::from_gray(29))
                    .inner_margin(egui::Margin::symmetric(16, 8)),
            )
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    if self.pending.is_some() || self.probe.is_some() {
                        ui.spinner();
                    }
                    let summary = self
                        .message
                        .lines()
                        .find(|line| !line.trim().is_empty())
                        .unwrap_or_default();
                    ui.add_sized(
                        egui::vec2((ui.available_width() - 80.0).max(100.0), 20.0),
                        egui::Label::new(RichText::new(summary).small().color(
                            if self.session_failed {
                                Color32::LIGHT_RED
                            } else {
                                appearance::MUTED
                            },
                        ))
                        .truncate(),
                    );
                    if ui.small_button("Details").clicked() {
                        self.session_open = true;
                    }
                });
            });
        egui::Panel::left("library")
            .default_size(245.0)
            .min_size(210.0)
            .max_size(360.0)
            .frame(
                egui::Frame::new()
                    .fill(Color32::from_gray(25))
                    .inner_margin(egui::Margin::symmetric(12, 16)),
            )
            .show(ui, |ui| {
                ui.heading("Library");
                ui.label(
                    RichText::new(format!("{} games", arcade_core::profiles().len()))
                        .small()
                        .color(appearance::MUTED),
                );
                ui.add_space(10.0);
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for (i, report) in self.reports.iter().enumerate() {
                        let text = RichText::new(&report.profile.title).size(14.0);
                        if ui
                            .add(
                                egui::Button::new(text)
                                    .selected(self.selected == i)
                                    .frame(self.selected == i)
                                    .wrap()
                                    .min_size(egui::vec2(ui.available_width(), 34.0)),
                            )
                            .clicked()
                        {
                            self.selected = i;
                        }
                        ui.label(
                            RichText::new(if report.binary.is_some() {
                                "Files found"
                            } else {
                                "Folder needs attention"
                            })
                            .small()
                            .color(appearance::MUTED),
                        );
                        ui.add_space(9.0);
                    }
                });
                ui.add_space(8.0);
                ui.separator();
                ui.label(
                    RichText::new(format!(
                        "{} / {}",
                        std::env::consts::OS,
                        std::env::consts::ARCH
                    ))
                    .small()
                    .color(appearance::MUTED),
                );
            });
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(Color32::from_gray(27)).inner_margin(18))
            .show(ui, |ui| {
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    let Some(report) = self.reports.get(self.selected).cloned() else {
                        ui.label("Inspecting your game library…");
                        return;
                    };
                    let id = &report.profile.id;
                    let game_root = self.game_roots.get(id).cloned().unwrap_or_else(|| Ok(PathBuf::from(&self.root)));
                    ui.heading(&report.profile.title);
                    ui.label(RichText::new(match id.as_str() {
                        "minecraft" => "Experimental · Linux startup and licensing",
                        "big-buck" => "Experimental · Desktop gameplay",
                        "super-cars" => "Experimental · Wine desktop gameplay",
                        _ => "Experimental · Linux desktop gameplay",
                    }).small().color(Color32::from_rgb(221, 183, 115)));
                    ui.add_space(8.0);
                    ui.label(RichText::new(report.path.display().to_string()).small().color(appearance::MUTED));
                    if let Some(error) = &report.error { ui.colored_label(Color32::LIGHT_RED, error); }
                    ui.add_space(10.0);
                    ui.horizontal_wrapped(|ui| {
                        let found = report.binary.is_some();
                        let label = if found { "Play game" } else { "Set game folder…" };
                        let enabled = self.probe.is_none() && self.pending.is_none() && (game_root.is_ok() || !found);
                        if ui.add_enabled(enabled, egui::Button::new(RichText::new(label).strong()).min_size(egui::vec2(120.0, 32.0))).clicked() {
                            if found {
                                if let Ok(root) = &game_root { self.launch_game(id, root.clone(), ctx.clone(), "launch"); }
                            } else {
                                self.settings.open = Some(id.clone());
                                self.settings.tab = SettingsTab::Directory;
                            }
                        }
                        settings_button(ui, &mut self.settings, id);
                        if id == "minecraft" && ui.add_enabled(self.probe.is_none() && report.binary.is_some() && game_root.is_ok(), egui::Button::new("Prepare libraries")).clicked()
                            && let Ok(root) = &game_root {
                            self.launch_game(id, root.clone(), ctx.clone(), "setup");
                        }
                    });
                    if let Some(profile) = settings::profile(&id.replace('-', "_")) {
                        ui.label(RichText::new(self.settings.settings.game(profile.id).summary()).small().color(appearance::MUTED));
                    }
                    ui.add_space(6.0);
                    ui.label(match id.as_str() {
                        "super-cars" => "Arrow keys steer, accelerate and brake. Enter starts, 5 inserts a coin, Esc exits. Change bindings in Settings.",
                        "big-buck" => "Mouse aims and fires; right click pumps. Start and coin bindings are under Settings. Esc exits.",
                        "minecraft" => "Cabinet controls and services are still in development. Gameplay is not yet verified.",
                        _ => "Mouse aims and fires. Start, coin and operator controls are under Settings. Esc exits.",
                    });
                    ui.add_space(10.0);
                    ui.separator();
                    ui.add_space(4.0);
                    egui::CollapsingHeader::new("Executable details").id_salt(("technical", id)).show(ui, |ui| {
                        if let Some(binary) = &report.binary {
                            egui::Grid::new(("binary", id)).num_columns(2).spacing([24.0, 8.0]).show(ui, |ui| {
                                ui.label("Format"); ui.monospace(format!("{} / {}-bit / machine {:#x}", binary.format, binary.bits, binary.machine)); ui.end_row();
                                ui.label("Entry address"); ui.monospace(format!("{:#x}", binary.entry)); ui.end_row();
                                ui.label("Interpreter"); ui.monospace(binary.interpreter.as_deref().unwrap_or("Windows runtime")); ui.end_row();
                                ui.label("Sections"); ui.monospace(binary.section_count.to_string()); ui.end_row();
                            });
                            egui::CollapsingHeader::new(format!("Imported libraries ({})", binary.libraries.len())).show(ui, |ui| {
                                for library in &binary.libraries { ui.monospace(library); }
                                for path in &binary.search_paths { ui.label(format!("Search path: {path}")); }
                            });
                        } else { ui.label("Set the game folder in Settings to inspect its executable."); }
                    });
                    egui::CollapsingHeader::new("Compatibility notes").id_salt(("notes", id)).default_open(true).show(ui, |ui| {
                        for blocker in &report.blockers { ui.label(format!("• {blocker}")); }
                    });
                    ui.add_space(14.0);
                    ui.separator();
                    ui.add_space(8.0);
                    ui.label(RichText::new("Cabinet input monitor").size(16.0));
                    ui.label(RichText::new("Local test · Mouse to aim · Left click to fire · Enter to start · 5 for coin").small().color(appearance::MUTED));
                let (rect,response) = ui.allocate_exact_size(egui::vec2(ui.available_width().min(600.0), 190.0), egui::Sense::click_and_drag());
                let painter = ui.painter().with_clip_rect(rect);
                painter.rect_filled(rect, 6.0, Color32::from_rgb(18, 29, 36));
                if let Some(pos) = response.hover_pos() {
                    self.input.set_aim((pos.x-rect.left())/rect.width(), (pos.y-rect.top())/rect.height());
                }
                let accepts_input = response.hovered() && !ctx.egui_wants_keyboard_input() && ctx.input(|i| i.focused);
                self.input.trigger = response.hovered() && ctx.input(|i| i.pointer.primary_down());
                self.input.start = accepts_input && ctx.input(|i| i.key_down(egui::Key::Enter));
                self.input.coin = accepts_input && ctx.input(|i| i.key_down(egui::Key::Num5));
                let p = egui::pos2(rect.left()+self.input.aim[0]*rect.width(),rect.top()+self.input.aim[1]*rect.height());
                let color = if self.input.trigger { Color32::LIGHT_RED } else { Color32::from_rgb(72,213,188) };
                painter.circle_stroke(p, 10.0, egui::Stroke::new(2.0,color));
                painter.line_segment([p-egui::vec2(16.0,0.0),p+egui::vec2(16.0,0.0)],egui::Stroke::new(1.0,color));
                painter.line_segment([p-egui::vec2(0.0,16.0),p+egui::vec2(0.0,16.0)],egui::Stroke::new(1.0,color));
                ui.monospace(format!("X {:.3}   Y {:.3}   TRIGGER {}   START {}   COIN {}",self.input.aim[0],self.input.aim[1],self.input.trigger,self.input.start,self.input.coin));
                });
            });
        if self.session_open {
            let mut open = true;
            egui::Window::new("Session details")
                .open(&mut open)
                .default_width(600.0)
                .show(&ctx, |ui| {
                    egui::ScrollArea::vertical()
                        .max_height(360.0)
                        .show(ui, |ui| {
                            ui.label(&self.message);
                        });
                });
            self.session_open = open;
        }
        settings_window(&ctx, self, frame);
    }
}
