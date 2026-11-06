mod aliens_runtime;
mod jurassic_runtime;
mod minecraft_runtime;
mod runtime;
mod supercars_runtime;
mod ford_runtime;
mod big_buck_runtime;
mod wsl_runtime;

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
    if args.first().is_some_and(|a|a=="--launch-ford"){
        if args.len()>2{return Err("Usage: arcade-desktop --launch-ford [game-folder-or-library-root]".into());}
        let root=args.get(1).map(PathBuf::from).unwrap_or_else(||"games".into());
        println!("{}",ford_runtime::launch(&root)?);return Ok(());
    }
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
        println!("{}", big_buck_runtime::run(&root,false)?);
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
            big_buck_runtime::run(&root,true)?
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
    } else {
        None
    };
    if !args.is_empty() && initial_root.is_none() {
        return Err(
            "Usage: arcade-desktop [--games-root PATH | --scan [PATH] | --probe-minecraft [PATH] | --launch-minecraft [PATH] | --setup-minecraft-libs [PATH] | --probe-big-buck [PATH] | --launch-big-buck [PATH] | --launch-super-cars [PATH]]".into(),
        );
    }
    eframe::run_native(
        "Arcade Foundry",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default().with_inner_size([1160.0, 800.0]),
            ..Default::default()
        },
        Box::new(move |cc| {
            cc.egui_ctx.set_visuals(egui::Visuals::dark());
            let mut app = ArcadeApp::default();
            if let Some(root) = initial_root {
                app.root = root;
                app.selected = 4;
            }
            app.rescan(cc.egui_ctx.clone());
            Ok(Box::new(app))
        }),
    )?;
    Ok(())
}

struct ArcadeApp {
    root: String,
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
    Controls,
    Graphics,
    Audio,
}

/// Per-game settings window state; values are saved to settings.cfg.
struct SettingsUi {
    settings: Settings,
    path: PathBuf,
    /// Game whose settings window is open.
    open: Option<&'static str>,
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
            tab: SettingsTab::Controls,
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
    let Some(profile) = settings::profile(game_id) else {
        return;
    };
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        if ui
            .button(RichText::new("⚙ Settings").strong())
            .on_hover_text("Controls, graphics and audio for this game")
            .clicked()
        {
            app.open = Some(profile.id);
            app.message.clear();
        }
        ui.small(app.settings.game(profile.id).summary());
    });
    ui.add_space(8.0);
}

fn settings_window(ctx: &egui::Context, app: &mut SettingsUi, controls: &mut ControlsUi) {
    let Some(id) = app.open else {
        return;
    };
    let Some(profile) = settings::profile(id) else {
        app.open = None;
        return;
    };
    let mut open = true;
    egui::Window::new(format!("{} settings", profile.title))
        .id(egui::Id::new("game-settings"))
        .open(&mut open)
        .collapsible(false)
        .resizable(true)
        .pivot(egui::Align2::CENTER_CENTER)
        .default_pos(ctx.content_rect().center())
        .default_width(560.0)
        .show(ctx, |ui| {
            if id=="big_buck" && cfg!(windows){
                let game=app.settings.game_mut(id).unwrap();let previous=game.backend;
                ui.horizontal(|ui|{ui.label("Execution backend");egui::ComboBox::from_id_salt("big-buck-backend").selected_text(game.backend.label()).show_ui(ui,|ui|{for backend in settings::Backend::ALL{ui.selectable_value(&mut game.backend,backend,backend.label());}});});
                if game.backend!=previous{app.save("Big Buck backend changed");}
                ui.small("Automatic uses the Water runtime. WSL2 remains available with Linux dependencies installed.");
            }
            ui.horizontal(|ui| {
                for (tab, label) in [
                    (SettingsTab::Controls, "🎮 Controls"),
                    (SettingsTab::Graphics, "🖥 Graphics"),
                    (SettingsTab::Audio, "🔊 Audio"),
                ] {
                    if ui.selectable_label(app.tab == tab, label).clicked() {
                        app.tab = tab;
                        controls.capturing = None;
                    }
                }
            });
            ui.separator();
            egui::ScrollArea::vertical()
                .max_height(460.0)
                .show(ui, |ui| match app.tab {
                    SettingsTab::Controls => controls_panel(ui, controls, profile.id),
                    SettingsTab::Graphics => graphics_tab(ui, app, profile),
                    SettingsTab::Audio => audio_tab(ui, app, profile),
                });
            if app.tab != SettingsTab::Controls {
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("Reset graphics & audio").clicked() {
                        app.settings.reset_game(profile.id);
                        app.save(&format!("{} graphics and audio reset", profile.title));
                    }
                    ui.small(app.path.display().to_string());
                });
                if !app.message.is_empty() {
                    ui.small(app.message.as_str());
                }
            }
        });
    if !open {
        app.open = None;
        controls.capturing = None;
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
                    let mut options = if profile.id=="ford"{vec![(640,480),(800,600),(1024,768),(1280,1024),(1600,1200)]}else{settings::RESOLUTIONS.to_vec()};
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
            ui.add_enabled_ui(!(profile.id=="ford"&&cfg!(windows)),|ui| {ui.horizontal(|ui| {
                for v in VSync::ALL {
                    if ui.radio_value(&mut game.vsync, v, v.label()).clicked() {
                        change = Some(format!("VSync {}", v.label().to_ascii_lowercase()));
                    }
                }
            });});
            ui.end_row();

            ui.label("Renderer");
            ui.vertical(|ui| {
                for r in Renderer::ALL {
                    let mut label = if cfg!(windows)&&profile.id=="big_buck"&&game.backend!=settings::Backend::Wsl&&r==Renderer::Software{"Software (Windows OpenGL)".to_owned()}else{r.label().to_owned()};
                    if r == profile.default_renderer {
                        label += "  (default)";
                    }
                    if ui.add_enabled(!(profile.id=="ford"&&cfg!(windows)&&r==Renderer::Software),egui::RadioButton::new(game.renderer==r,label)).clicked() {
                        game.renderer=r;
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
    let windows_audio=cfg!(windows)&&(profile.id=="ford"||(profile.id=="big_buck"&&game.backend!=settings::Backend::Wsl));
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
            ui.add_enabled_ui(game.audio&&!windows_audio, |ui| {
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
    ui.small(if windows_audio{"Uses the default Windows sound output. Select its device and volume in Windows sound settings."}else{"Output device is a PulseAudio/PipeWire sink name. Leave empty for the default output."});
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

impl Default for ArcadeApp {
    fn default() -> Self {
        Self {
            root: "games".into(),
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
    fn rescan(&mut self, ctx: egui::Context) {
        let root = PathBuf::from(&self.root);
        let (tx, rx) = mpsc::channel();
        self.pending = Some(rx);
        self.message = "Inspecting executable metadata…".into();
        std::thread::spawn(move || {
            let _ = tx.send(arcade_core::scan(&root));
            ctx.request_repaint();
        });
    }
}
impl eframe::App for ArcadeApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if let Some(rx) = &self.pending {
            match rx.try_recv() {
                Ok(reports) => {
                    self.reports = reports;
                    if self.reports.get(self.selected).is_none_or(|r|r.binary.is_none()) {self.selected=self.reports.iter().position(|r|r.binary.is_some()).unwrap_or(0);}
                    self.pending = None;
                    self.message =
                        "Inspection complete. Choose a game for launch options and runtime requirements."
                            .into();
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
                            image::open(if cfg!(windows){"runtime/big-buck-native-probe/screen.png"}else{"runtime/big-buck/screen.png"}).map(|i| i.to_rgba8())
                    {
                        let size = [pixels.width() as usize, pixels.height() as usize];
                        let color = egui::ColorImage::from_rgba_unmultiplied(size, pixels.as_raw());
                        self.screenshot = Some(ui.ctx().load_texture(
                            "big-buck-capture",
                            color,
                            egui::TextureOptions::LINEAR,
                        ));
                    }
                    self.message = result.unwrap_or_else(|e| e);
                    self.probe = None;
                    self.desktop_running = false;
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.message = "Startup worker stopped unexpectedly".into();
                    self.probe = None;
                    self.desktop_running = false;
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        let ctx = ui.ctx().clone();
        egui::Panel::top("header").show(ui, |ui| {
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                ui.heading(RichText::new("ARCADE FOUNDRY").color(Color32::from_rgb(72, 213, 188)));
                ui.label(" /  compatibility workbench  /  0.1");
            });
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.label("Games folder");
                ui.add(egui::TextEdit::singleline(&mut self.root).desired_width(520.0));
                if ui
                    .add_enabled(self.pending.is_none(), egui::Button::new("Inspect library"))
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
            ui.add_space(10.0);
        });
        egui::Panel::bottom("status").show(ui, |ui| {
            egui::ScrollArea::vertical()
                .max_height(140.0)
                .show(ui, |ui| {
                    ui.label(&self.message);
                });
        });
        egui::Panel::left("library")
            .default_size(250.0)
            .show(ui, |ui| {
                ui.add_space(12.0);
                ui.heading("Library");
                ui.label(format!(
                    "{} cabinet profiles",
                    arcade_core::profiles().len()
                ));
                ui.add_space(14.0);
                for (i, report) in self.reports.iter().enumerate() {
                    ui.vertical(|ui| {
                        if ui
                            .selectable_label(self.selected == i, &report.profile.title)
                            .clicked()
                        {
                            self.selected = i;
                        }
                        ui.small(if report.binary.is_some() {
                            "Detected · adapter needed"
                        } else {
                            "Inspection unavailable"
                        });
                        ui.add_space(12.0);
                    });
                }
                if self.pending.is_some() {
                    ui.spinner();
                }
                ui.separator();
                ui.small(format!(
                    "Host: {} / {}",
                    std::env::consts::OS,
                    std::env::consts::ARCH
                ));
            });
        egui::CentralPanel::default().show(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                let Some(report) = self.reports.get(self.selected) else { ui.label("Waiting for library inspection…"); return; };
                ui.add_space(12.0);
                ui.heading(&report.profile.title);
                ui.label(RichText::new(match report.profile.id.as_str() {
                    "minecraft" => "EXPERIMENTAL — diagnose Linux startup and licensing",
                    "big-buck" => "EXPERIMENTAL — virtual JAMMA inputs; gameplay incomplete",
                    "ford" => "EXPERIMENTAL — desktop cabinet adapter",
                    "aliens" | "jurassic" if cfg!(target_os = "linux") => "EXPERIMENTAL — Linux desktop gameplay",
                    _ => "Runtime adapter unavailable on this host",
                }).color(Color32::from_rgb(244, 188, 95)));
                ui.add_space(10.0);
                ui.label(report.path.display().to_string());
                if let Some(error) = &report.error { ui.colored_label(Color32::LIGHT_RED, error); }
                if let Some(binary) = &report.binary {
                    ui.add_space(12.0);
                    egui::Grid::new("binary").num_columns(2).spacing([24.0, 8.0]).show(ui, |ui| {
                        ui.label("Executable"); ui.monospace(format!("{} / {}-bit / machine {:#x}", binary.format, binary.bits, binary.machine)); ui.end_row();
                        ui.label("Entry address"); ui.monospace(format!("{:#x}", binary.entry)); ui.end_row();
                        ui.label("Interpreter"); ui.monospace(binary.interpreter.as_deref().unwrap_or("Windows runtime")); ui.end_row();
                        ui.label("Sections"); ui.monospace(binary.section_count.to_string()); ui.end_row();
                    });
                    ui.add_space(12.0);
                    egui::CollapsingHeader::new(format!("Imported libraries ({})", binary.libraries.len())).show(ui, |ui| {
                        for library in &binary.libraries { ui.monospace(library); }
                        for path in &binary.search_paths { ui.label(format!("Search path: {path}")); }
                    });
                }
                ui.add_space(12.0);
                if report.profile.id == "minecraft" {
                    ui.label(if cfg!(windows) { "Windows backend: WSL2 / Ubuntu with WSLg." } else { "Linux backend: native x86-64 guest inside Bubblewrap." });
                    ui.label("Original game files stay read-only; saves and logs use runtime/minecraft.");
                    ui.label("Cabinet controls are not mapped yet. Startup diagnostics identify missing libraries and the next failure.");
                    for (label, mode) in [("Diagnose Minecraft startup", "probe"), ("Prepare Linux libraries", "setup"), ("Launch Minecraft guest", "launch")] {
                        if ui.add_enabled(self.probe.is_none() && report.binary.is_some(), egui::Button::new(label)).clicked() {
                            self.screenshot = None;
                            self.desktop_running = true;
                            let root = PathBuf::from(&self.root);
                            let (tx, rx) = mpsc::channel();
                            self.probe = Some(rx);
                            self.message = format!("Minecraft: {mode} in progress…");
                            let repaint = ctx.clone();
                            std::thread::spawn(move || {
                                let _ = tx.send(minecraft_runtime::run(&root, mode));
                                repaint.request_repaint();
                            });
                        }
                    }
                }
                if report.profile.id == "big-buck" {
                    ui.label("Mouse aims and fires, right click pumps, Esc exits. Keys, graphics and audio are under Settings.");
                    if ui.add_enabled(self.probe.is_none(),egui::Button::new("Play on my display")).clicked() {
                        self.desktop_running=true;
                        let root=PathBuf::from(&self.root);
                        let (tx,rx)=mpsc::channel(); self.probe=Some(rx);
                        self.screenshot=None;
                        self.message="Big Buck Hunter running on your display. Close the game or press Esc to return.".into();
                        let repaint=ctx.clone();
                        std::thread::spawn(move || {let _=tx.send(big_buck_runtime::run(&root,false));repaint.request_repaint();});
                    }
                    ui.label(if cfg!(windows){"Windows: Water runtime by default; choose WSL2 under Settings. The progression probe opens a visible window."}else{"Linux progression probe: private display with scripted inputs."});
                    if ui.add_enabled(self.probe.is_none(), egui::Button::new("Test Big Buck Hunter progression")).clicked() {
                        self.screenshot = None;
                        self.desktop_running=false;
                        let root = PathBuf::from(&self.root);
                        let (tx, rx) = mpsc::channel();
                        self.probe = Some(rx);
                        self.message = "Testing Big Buck Hunter on a private display…".into();
                        let repaint = ctx.clone();
                        std::thread::spawn(move || {
                            let result = big_buck_runtime::run(&root,true);
                            let _ = tx.send(result);
                            repaint.request_repaint();
                        });
                    }
                }
                if report.profile.id == "big-buck"
                    && let Some(texture) = &self.screenshot {
                        ui.label("Captured progression-test frame");
                        let size = texture.size_vec2();
                        ui.image((texture.id(), size * (ui.available_width().min(640.0) / size.x)));
                }
                if report.profile.id == "aliens" {
                    ui.label("Mouse aims and fires. Keys (including T = Test), graphics and audio are under Settings. Audio plays through your desktop sound.");
                    if ui.add_enabled(self.probe.is_none(), egui::Button::new("Open Aliens on my display")).clicked() {
                        self.screenshot = None;
                        self.desktop_running = true;
                        let root = PathBuf::from(&self.root);
                        let (tx, rx) = mpsc::channel();
                        self.probe = Some(rx);
                        self.message = "Aliens Armageddon is running on your display…".into();
                        let repaint = ctx.clone();
                        std::thread::spawn(move || {
                            let _ = tx.send(aliens_runtime::run(&root, true));
                            repaint.request_repaint();
                        });
                    }
                    if ui.add_enabled(self.probe.is_none(), egui::Button::new("Diagnose Aliens startup")).clicked() {
                        self.screenshot = None;
                        self.desktop_running = false;
                        let root = PathBuf::from(&self.root);
                        let (tx, rx) = mpsc::channel();
                        self.probe = Some(rx);
                        self.message = "Diagnosing Aliens startup…".into();
                        let repaint = ctx.clone();
                        std::thread::spawn(move || {
                            let _ = tx.send(aliens_runtime::run(&root, false));
                            repaint.request_repaint();
                        });
                    }
                }
                if report.profile.id == "jurassic" {
                    ui.label("Mouse aims for player one, Esc exits. Keys, graphics and audio are under Settings. Audio plays through your desktop sound.");
                    if ui.add_enabled(self.probe.is_none(),egui::Button::new("Open Jurassic Park on my display")).clicked() {
                        self.screenshot=None;
                        self.desktop_running=true;
                        let root=PathBuf::from(&self.root);
                        let (tx,rx)=mpsc::channel();self.probe=Some(rx);
                        self.message="Jurassic Park is running on your display…".into();
                        let repaint=ctx.clone();
                        std::thread::spawn(move || {
                            let _=tx.send(jurassic_runtime::launch(&root));
                            repaint.request_repaint();
                        });
                    }
                    if ui.add_enabled(self.probe.is_none(),egui::Button::new("Diagnose repaired Jurassic Park startup")).clicked() {
                        self.screenshot=None;
                        self.desktop_running=true; // Suppress the Big Buck Hunter frame preview.
                        let root=PathBuf::from(&self.root);
                        let (tx,rx)=mpsc::channel();self.probe=Some(rx);
                        self.message="Tracing Jurassic Park native startup…".into();
                        let repaint=ctx.clone();
                        std::thread::spawn(move || {
                            let _=tx.send(jurassic_runtime::probe(&root,arcade_core::jurassic::Build::Native));
                            repaint.request_repaint();
                        });
                    }
                }
                if report.profile.id == "super-cars" {
                    ui.label("Runs under Wine. Left/Right steer, Up = gas, Down = brake, Enter = start, 5 = coin, Esc exits. Keys, graphics and audio are under Settings.");
                    if ui.add_enabled(self.probe.is_none(),egui::Button::new("Open Super Cars on my display")).clicked() {
                        self.screenshot=None;
                        self.desktop_running=true;
                        let root=PathBuf::from(&self.root);
                        let (tx,rx)=mpsc::channel();self.probe=Some(rx);
                        self.message="Super Cars is running on your display…".into();
                        let repaint=ctx.clone();
                        std::thread::spawn(move || {
                            let _=tx.send(supercars_runtime::launch(&root));
                            repaint.request_repaint();
                        });
                    }
                }
                if report.profile.id=="ford"{
                    ui.label("Native Windows or Linux/Wine. Arrows steer, accelerate and brake; Enter/1 starts; V changes view; Space/X shift; 5 adds credit; Esc exits.");
                    if ui.add_enabled(self.probe.is_none()&&report.binary.is_some(),egui::Button::new("Play Ford Racing")).clicked(){
                        self.screenshot=None;self.desktop_running=true;let root=PathBuf::from(&self.root);let(tx,rx)=mpsc::channel();self.probe=Some(rx);self.message="Ford Racing is running…".into();let repaint=ctx.clone();
                        std::thread::spawn(move||{let _=tx.send(ford_runtime::launch(&root));repaint.request_repaint();});
                    }
                }
                settings_button(ui, &mut self.settings, &report.profile.id.replace('-', "_"));
                ui.heading("Runtime work remaining");
                for blocker in &report.blockers { ui.label(format!("• {blocker}")); }
                ui.add_space(18.0);
                ui.heading("Cabinet input monitor");
                ui.label("Local test only. Pointer = aim / left click = trigger / Enter = start / 5 = coin.");
                let (rect,response) = ui.allocate_exact_size(egui::vec2(ui.available_width().min(600.0), 190.0), egui::Sense::click_and_drag());
                ui.painter().rect_filled(rect, 6.0, Color32::from_rgb(18, 29, 36));
                if let Some(pos) = response.hover_pos() {
                    self.input.set_aim((pos.x-rect.left())/rect.width(), (pos.y-rect.top())/rect.height());
                }
                let accepts_input = response.hovered() && !ctx.egui_wants_keyboard_input() && ctx.input(|i| i.focused);
                self.input.trigger = response.hovered() && ctx.input(|i| i.pointer.primary_down());
                self.input.start = accepts_input && ctx.input(|i| i.key_down(egui::Key::Enter));
                self.input.coin = accepts_input && ctx.input(|i| i.key_down(egui::Key::Num5));
                let p = egui::pos2(rect.left()+self.input.aim[0]*rect.width(),rect.top()+self.input.aim[1]*rect.height());
                let color = if self.input.trigger { Color32::LIGHT_RED } else { Color32::from_rgb(72,213,188) };
                ui.painter().circle_stroke(p, 10.0, egui::Stroke::new(2.0,color));
                ui.painter().line_segment([p-egui::vec2(16.0,0.0),p+egui::vec2(16.0,0.0)],egui::Stroke::new(1.0,color));
                ui.painter().line_segment([p-egui::vec2(0.0,16.0),p+egui::vec2(0.0,16.0)],egui::Stroke::new(1.0,color));
                ui.monospace(format!("X {:.3}   Y {:.3}   TRIGGER {}   START {}   COIN {}",self.input.aim[0],self.input.aim[1],self.input.trigger,self.input.start,self.input.coin));
            });
        });
        settings_window(&ctx, &mut self.settings, &mut self.controls);
    }
}
