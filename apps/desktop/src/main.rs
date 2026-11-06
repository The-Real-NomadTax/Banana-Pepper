//! Banana Pepper desktop frontend. Games are reached only through the
//! `banana-api` registry; this crate names no individual game.
mod zar_compressor;
mod appearance;
mod library_preferences;

use banana_core::GameReport;
use banana_input::controls::{self, Bindings};
use banana_settings::{self as settings, Renderer, Settings, VSync};
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
    banana_compat::install();
    // Per-game command-line entry points, declared by each game crate.
    if let Some(flag) = args.first().and_then(|a| a.to_str()) {
        for game in banana_api::games() {
            if let Some(command) = game.cli_commands().iter().find(|c| c.flag == flag) {
                if args.len() > 2 {
                    return Err(format!("Usage: arcade-desktop {flag} [games-root]").into());
                }
                let root = args.get(1).map(PathBuf::from).unwrap_or_else(|| "games".into());
                if !command.banner.is_empty() {
                    println!("{}", command.banner);
                }
                println!("{}", game.run(&root, command.action)?);
                return Ok(());
            }
        }
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
            serde_json::to_string_pretty(&banana_runtime::content::scan(&root)?)?
        );
        return Ok(());
    }
    let initial_root = if args.first().is_some_and(|a| a == "--games-root") && args.len() == 2 {
        Some(args[1].to_string_lossy().into_owned())
    } else {
        None
    };
    if !args.is_empty() && initial_root.is_none() {
        let flags: Vec<_> = banana_api::games()
            .iter()
            .flat_map(|g| g.cli_commands())
            .map(|c| format!(" | {} [PATH]", c.flag))
            .collect();
        return Err(format!(
            "Usage: arcade-desktop [--games-root PATH | --scan [PATH]{}]",
            flags.concat()
        )
        .into());
    }
    eframe::run_native(
        "Banana pepper",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default().with_inner_size([1160.0, 800.0]).with_min_inner_size([850.0, 620.0]),
            ..Default::default()
        },
        Box::new(move |cc| {
            let mut app = ArcadeApp::default();
            appearance::install(&cc.egui_ctx, app.folders.theme);
            cc.egui_ctx.set_embed_viewports(false);
            let logo = image::load_from_memory(include_bytes!("../assets/banana-pepper.png"))?
                .to_rgba8();
            app.logo = Some(cc.egui_ctx.load_texture(
                "banana-pepper-logo",
                egui::ColorImage::from_rgba_unmultiplied(
                    [logo.width() as usize, logo.height() as usize],
                    logo.as_raw(),
                ),
                egui::TextureOptions::LINEAR,
            ));
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
    zar: zar_compressor::ZarCompressor,
    logo: Option<egui::TextureHandle>,
    root: String,
    session_open: bool,
    session_failed: bool,
    folder_message: String,
    folders: library_preferences::Preferences,
    folder_drafts: std::collections::BTreeMap<String, String>,
    game_roots: std::collections::BTreeMap<String, Result<PathBuf, String>>,
    reports: Vec<GameReport>,
    selected: usize,
    global_library: bool,
    pending: Option<Receiver<Result<library_preferences::LibraryScan, String>>>,
    message: String,
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
    let Some(game) = banana_core::profiles()
        .into_iter()
        .find(|game| game.id == id)
    else {
        app.settings.open = None;
        return;
    };
    let profile = banana_api::game(&id).and_then(|g| g.settings_profile());
    let mut close_requested = false;
    ctx.show_viewport_immediate(
        egui::ViewportId::from_hash_of("game-settings"),
        egui::ViewportBuilder::default()
            .with_title(format!("{} settings", game.title))
            .with_inner_size([700.0, 560.0])
            .with_min_inner_size([560.0, 380.0])
            .with_decorations(true)
            .with_resizable(true)
            .with_taskbar(true),
        |viewport_ui, _class| {
            if viewport_ui
                .ctx()
                .input(|input| input.viewport().close_requested())
            {
                close_requested = true;
                return;
            }
            egui::CentralPanel::default()
                .frame(egui::Frame::central_panel(viewport_ui.style()).inner_margin(16))
                .show(viewport_ui, |ui| {
            // Backend selector, for games that declare a choice on this host.
            if let Some(profile) = profile.filter(|p| p.backend_choice()) {
                let game = app.settings.settings.game_mut(profile.id).expect("profile settings");
                let previous = game.backend;
                ui.horizontal(|ui| {
                    ui.label("Execution backend");
                    egui::ComboBox::from_id_salt((profile.id, "backend"))
                        .selected_text(game.backend.label())
                        .show_ui(ui, |ui| {
                            for &backend in profile.backends {
                                ui.selectable_value(&mut game.backend, backend, backend.label());
                            }
                        });
                });
                if game.backend != previous {
                    app.settings.save("Execution backend changed");
                }
                ui.small("Automatic uses the Water runtime. WSL2 remains available with Linux dependencies installed.");
            }
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
                    let footer_height =
                        if matches!(app.settings.tab, SettingsTab::Graphics | SettingsTab::Audio) {
                            96.0
                        } else {
                            0.0
                        };
                    egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .max_height((ui.available_height() - footer_height).max(80.0))
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
                                    players_row(ui, &mut app.settings, profile);
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
                            ui.label(RichText::new("Changes save automatically").small())
                            .on_hover_text(app.settings.path.display().to_string());
                        });
                        if !app.settings.message.is_empty() {
                            ui.small(app.settings.message.as_str());
                        }
                    }
                });
        },
    );
    if close_requested {
        app.settings.open = None;
        app.controls.capturing = None;
    }
}

/// One or two players, for two-player games (settings.cfg `players`).
fn players_row(ui: &mut egui::Ui, app: &mut SettingsUi, profile: &'static settings::Profile) {
    if profile.max_players < 2 {
        return;
    }
    let game = app.settings.game_mut(profile.id).expect("profile settings");
    let mut change = None;
    ui.horizontal(|ui| {
        ui.label("Players");
        for (players, label) in [(1, "1 player"), (2, "2 players")] {
            if ui.radio_value(&mut game.players, players, label).clicked() {
                change = Some(format!("{} {label}", profile.title));
            }
        }
    });
    ui.small(if game.players > 1 {
        "Controller 1 plays player one, controller 2 plays player two with the same button layout. Mouse and keyboard stay player one. Connect both controllers before starting the game."
    } else {
        "Every controller plays player one."
    });
    if let Some(change) = change {
        app.save(&change);
    }
    if app.message.contains(" player") {
        ui.small(app.message.as_str());
    }
    ui.separator();
}

fn graphics_tab(ui: &mut egui::Ui, app: &mut SettingsUi, profile: &'static settings::Profile) {
    let mut change = None;
    let game = app.settings.game_mut(profile.id).expect("profile settings");
    let max_column_width = (ui.available_width() - 160.0).max(260.0);
    egui::Grid::new(("graphics", profile.id))
        .max_col_width(max_column_width)
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
            ui.add(egui::Label::new(RichText::new(profile.resolution_note).small()).wrap());
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

            if profile.crosshair.is_some() {
                ui.label("Crosshair");
                ui.horizontal_wrapped(|ui| {
                    for style in settings::Crosshair::all() {
                        ui.vertical_centered(|ui| {
                            ui.set_width(84.0);
                            match style.0.and_then(|id| crosshair_texture(ui.ctx(), id)) {
                                Some(texture) => {
                                    ui.add(egui::Image::new((texture.id(), egui::vec2(44.0, 44.0))));
                                }
                                None => {
                                    ui.add_space(44.0);
                                }
                            }
                            if ui.radio(game.crosshair == style, style.label()).clicked() {
                                change = Some(format!("Crosshair {}", style.label().to_ascii_lowercase()));
                                game.crosshair = style;
                            }
                        });
                    }
                });
                ui.end_row();

                ui.label("Crosshair size");
                ui.vertical(|ui| {
                    let on = game.crosshair.0.is_some();
                    let slider = egui::Slider::new(
                        &mut game.crosshair_size,
                        banana_input::crosshair::SIZE_MIN..=banana_input::crosshair::SIZE_MAX,
                    )
                    .suffix("%")
                    .step_by(5.0);
                    let response = ui
                        .add_enabled(on, slider)
                        .on_disabled_hover_text("Choose a crosshair to set its size");
                    // Save once a drag ends (or on a click/keyboard change).
                    if response.drag_stopped() || (response.changed() && !response.dragged()) {
                        change = Some(format!("Crosshair size {}%", game.crosshair_size));
                    }
                    if let Some(texture) = game.crosshair.0.and_then(|id| crosshair_texture(ui.ctx(), id)) {
                        // Actual size in a 640x480 window; larger windows scale it up.
                        let side = 110.0 * game.crosshair_size as f32 / 100.0;
                        ui.add(egui::Image::new((texture.id(), egui::vec2(side, side))));
                        ui.small("Shown at its size in a 640x480 window.");
                    } else {
                        ui.small("Crosshair is off: the game window shows no crosshair.");
                    }
                });
                ui.end_row();
            }
        });
    ui.add_space(6.0);
    ui.small(
        "Fullscreen fills the monitor the game window opens on; pick that monitor's resolution so the picture fills it. Fullscreen and non-default sizes are experimental. Esc still exits the game.",
    );
    if let Some(change) = change {
        app.save(&change);
    }
}

/// Preview texture for a crosshair style, decoded once per style.
fn crosshair_texture(ctx: &egui::Context, id: &'static str) -> Option<egui::TextureHandle> {
    let key = egui::Id::new(("crosshair-preview", id));
    if let Some(texture) = ctx.data(|d| d.get_temp::<egui::TextureHandle>(key)) {
        return Some(texture);
    }
    let image = image::load_from_memory(banana_input::crosshair::png(id)?).ok()?.to_rgba8();
    let texture = ctx.load_texture(
        format!("crosshair-{id}"),
        egui::ColorImage::from_rgba_unmultiplied(
            [image.width() as usize, image.height() as usize],
            image.as_raw(),
        ),
        egui::TextureOptions::LINEAR,
    );
    ctx.data_mut(|d| d.insert_temp(key, texture.clone()));
    Some(texture)
}

fn audio_tab(ui: &mut egui::Ui, app: &mut SettingsUi, profile: &'static settings::Profile) {
    let mut change = None;
    let game = app.settings.game_mut(profile.id).expect("profile settings");
    let windows_audio = profile.plays_windows_audio(game);
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
                if action.pad_only {
                    ui.add_enabled(false, button)
                        .on_disabled_hover_text("Controller only: choose buttons with 🎮");
                } else if ui
                    .add(button)
                    .on_hover_text("Click, then press the new key")
                    .clicked()
                {
                    state.capturing = Some((game.id, action.id, false));
                    state.message = format!("Press a key for {}.", action.label);
                }
                ui.horizontal(|ui| {
                    ui.menu_button("🎮", |ui| {
                        ui.label(RichText::new("Controller buttons (Xbox layout)").small());
                        for (name, _, label) in controls::PAD_BUTTONS {
                            let mut bound = state
                                .bindings
                                .keys(game.id, action.id)
                                .iter()
                                .any(|k| k == name);
                            if ui.checkbox(&mut bound, *label).changed() {
                                match state.bindings.toggle_pad(game.id, action.id, name) {
                                    Ok(change) => state.save(change),
                                    Err(e) => state.message = format!("Not changed: {e}"),
                                }
                            }
                        }
                    })
                    .response
                    .on_hover_text("Add or remove controller buttons");
                    if action.single || action.pad_only {
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

#[derive(Clone, Copy)]
enum SourceKind {
    Folder,
    Archive,
}

fn source_browse_menu(ui: &mut egui::Ui) -> Option<SourceKind> {
    let mut selected = None;
    ui.menu_button("Browse…", |ui| {
        for (label, kind) in [
            ("Folder…", SourceKind::Folder),
            ("Archive file…", SourceKind::Archive),
        ] {
            if ui.button(label).clicked() {
                selected = Some(kind);
                ui.close();
            }
        }
    });
    selected
}

/// Use a native picker for either a directory or one supported archive file.
fn pick_source(
    frame: &eframe::Frame,
    title: &str,
    current: &str,
    kind: SourceKind,
) -> Result<Option<String>, String> {
    let mut dialog = rfd::FileDialog::new().set_title(title).set_parent(frame);
    if let Ok(path) = Path::new(current).canonicalize() {
        let directory = if path.is_dir() {
            Some(path.as_path())
        } else {
            path.parent()
        };
        if let Some(directory) = directory {
            dialog = dialog.set_directory(directory);
        }
    }
    let selected = match kind {
        SourceKind::Folder => dialog.pick_folder(),
        SourceKind::Archive => dialog
            .add_filter("ZArchive", &["zar", "zarchive", "wua"])
            .pick_file(),
    };
    selected
        .map(|path| {
            path.into_os_string()
                .into_string()
                .map_err(|_| "The selected path must contain valid Unicode.".into())
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
            logo: None,
            zar: Default::default(),
            session_open: false,
            session_failed: false,
            folder_drafts: folders.games.clone(),
            folders,
            folder_message,
            game_roots: Default::default(),
            reports: banana_core::profiles().into_iter().map(|profile| GameReport {
                path: PathBuf::from("games").join(&profile.executable),
                profile, binary: None, error: None, blockers: vec![],
            }).collect(),
            selected: 0,
            global_library: false,
            pending: None,
            message: "Choose a game to inspect its runtime requirements.".into(),
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
        let game = banana_api::game(&id);
        let name = game.map_or_else(|| id.clone(), |g| g.title().to_owned());
        let extra = game
            .and_then(|g| g.extra_actions().iter().find(|a| a.id == mode))
            .map(|a| a.label.to_lowercase());
        self.screenshot = None;
        self.session_failed = false;
        self.desktop_running = true;
        self.message = format!("{name}: {}…", extra.as_deref().unwrap_or("starting game"));
        let (tx, rx) = mpsc::channel();
        self.probe = Some(rx);
        std::thread::spawn(move || {
            let result = banana_runtime::run(&id, &root, mode);
            let _ = tx.send(result);
            ctx.request_repaint();
        });
    }

    fn set_library_membership(&mut self, id: &str, added: bool) {
        let mut folders = self.folders.clone();
        if added {
            folders.added.insert(id.to_owned());
        } else {
            folders.added.remove(id);
        }
        match library_preferences::save(&folders) {
            Ok(()) => {
                self.folders = folders;
                self.message = if added {
                    "Added to My library. Set the game folder in Settings to play."
                } else {
                    "Removed from My library. Game files and settings are preserved."
                }.into();
            }
            Err(error) => self.folder_message = error,
        }
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
        let mut browse = None;
        ui.add_enabled_ui(self.probe.is_none(), |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label("Game folder or ZArchive");
                let draft = self.folder_drafts.entry(id.to_owned()).or_default();
                let response = ui.add(
                    egui::TextEdit::singleline(draft)
                        .hint_text("Game directory or game.zar")
                        .desired_width(380.0),
                );
                browse = source_browse_menu(ui);
                save = ui.button("Save game source").clicked()
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
        ui.small("Choose this game's directory or a ZArchive of it. Each game source is saved separately.");
        if let Some(kind) = browse {
            let current = self
                .folder_drafts
                .get(id)
                .filter(|path| !path.is_empty())
                .unwrap_or(&self.root);
            match pick_source(frame, &format!("Choose {title} source"), current, kind) {
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
        let games = self.folders.games.clone();
        let (tx, rx) = mpsc::channel();
        self.pending = Some(rx);
        self.message = "Preparing game source and inspecting executables... Large ZArchives are verified and cached on first load.".into();
        std::thread::spawn(move || {
            let _ = tx.send(library_preferences::scan(&root, &games));
            ctx.request_repaint();
        });
    }
}
impl eframe::App for ArcadeApp {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        if let Some(rx) = &self.pending {
            match rx.try_recv() {
                Ok(Ok(scan)) => {
                    self.reports = scan.reports;
                    self.game_roots = scan.game_roots;
                    if self.reports.get(self.selected).is_none() { self.selected = 0; }
                    self.pending = None;
                    self.message = "Your library is up to date.".into();
                }
                Ok(Err(error)) => {
                    for report in &mut self.reports {
                        report.binary = None;
                        report.error = Some(error.clone());
                    }
                    self.game_roots.clear();
                    self.pending = None;
                    self.message = error;
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
        let palette = self.folders.theme.palette();
        egui::Panel::top("header")
            .frame(
                egui::Frame::new()
                    .fill(palette.raised)
                    .inner_margin(egui::Margin::symmetric(16, 12)),
            )
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    if let Some(logo) = &self.logo {
                        ui.add(
                            egui::Image::new((logo.id(), egui::vec2(32.0, 32.0)))
                                .tint(palette.accent),
                        )
                        .on_hover_text("Banana pepper");
                    }
                    ui.label(
                        RichText::new("Banana pepper")
                            .size(18.0)
                            .color(palette.accent),
                    );
                    ui.menu_button("Tools", |ui| {
                        if ui.button("ZAR compressor…").clicked() {
                            self.zar.open();
                            ui.close();
                        }
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let previous = self.folders.theme;
                        egui::ComboBox::from_id_salt("theme-selector")
                            .selected_text(self.folders.theme.name())
                            .width(130.0)
                            .show_ui(ui, |ui| {
                                for theme in appearance::Theme::ALL {
                                    ui.selectable_value(&mut self.folders.theme, theme, theme.name());
                                }
                            });
                        ui.label("Theme");
                        if self.folders.theme != previous {
                            appearance::install(&ctx, self.folders.theme);
                            if let Err(error) = library_preferences::save(&self.folders) {
                                self.folder_message = format!("Theme applied, but could not save it: {error}");
                            }
                            ctx.request_repaint();
                        }
                        ui.label(
                            RichText::new(env!("CARGO_PKG_VERSION"))
                                .small()
                                .color(palette.muted),
                        );
                    });
                });
                ui.add_space(4.0);
                ui.horizontal_wrapped(|ui| {
                    ui.label("Game folder or ZArchive");
                    let folder = ui.add_enabled(
                        self.probe.is_none(),
                        egui::TextEdit::singleline(&mut self.root)
                            .hint_text("Folder path or games.zar")
                            .desired_width((ui.available_width() - 430.0).clamp(210.0, 520.0)),
                    );
                    if folder.changed() {
                        self.folder_message =
                            "Source changed. Choose Save source to remember it.".into();
                    }
                    let mut selected_folder = false;
                    let browse = ui.add_enabled_ui(self.probe.is_none(), source_browse_menu).inner;
                    if let Some(kind) = browse {
                        match pick_source(frame, "Choose default library source", &self.root, kind) {
                            Ok(Some(folder)) => {
                                self.root = folder;
                                selected_folder = true;
                            }
                            Ok(None) => {}
                            Err(error) => self.folder_message = error,
                        }
                    }
                    let save = ui
                        .add_enabled(self.probe.is_none(), egui::Button::new("Save source"))
                        .on_hover_text("Remember this folder or archive and inspect its game files")
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
                        self.message = match banana_core::write_report(
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
                    .fill(palette.raised)
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
                                palette.muted
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
                    .fill(palette.sidebar)
                    .inner_margin(egui::Margin::symmetric(12, 16)),
            )
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.global_library, false, "My library");
                    ui.selectable_value(&mut self.global_library, true, "Global library");
                });
                let count = self.reports.iter().filter(|report|
                    self.global_library || self.folders.added.contains(&report.profile.id)
                ).count();
                ui.label(RichText::new(format!("{count} games")).small().color(palette.muted));
                if !self.global_library && count == 0 {
                    ui.label("Your library is empty.");
                    if ui.button("Browse global library").clicked() {
                        self.global_library = true;
                    }
                }
                if !self.global_library && self.reports.get(self.selected)
                    .is_none_or(|report| !self.folders.added.contains(&report.profile.id)) {
                    self.selected = self.reports.iter().position(|report|
                        self.folders.added.contains(&report.profile.id)
                    ).unwrap_or(0);
                }
                ui.add_space(10.0);
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for (i, report) in self.reports.iter().enumerate() {
                        if !self.global_library && !self.folders.added.contains(&report.profile.id) {
                            continue;
                        }
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
                            RichText::new(if self.global_library && !self.folders.added.contains(&report.profile.id) {
                                "Available to add"
                            } else if self.global_library {
                                "In My library"
                            } else if report.binary.is_some() {
                                "Files found"
                            } else {
                                "Folder needs attention"
                            })
                            .small()
                            .color(palette.muted),
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
                    .color(palette.muted),
                );
            });
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(palette.panel).inner_margin(18))
            .show(ui, |ui| {
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    if !self.global_library && !self.reports.iter().any(|report|
                        self.folders.added.contains(&report.profile.id)) {
                        ui.heading("My library");
                        ui.label("Add games from the global library to build your collection.");
                        if ui.button("Browse global library").clicked() {
                            self.global_library = true;
                        }
                        return;
                    }
                    let Some(report) = self.reports.get(self.selected).cloned() else {
                        ui.label("Inspecting your game library…");
                        return;
                    };
                    let id = &report.profile.id;
                    let game_root = self.game_roots.get(id).cloned().unwrap_or_else(|| Ok(PathBuf::from(&self.root)));
                    ui.heading(&report.profile.title);
                    let experimental = Color32::from_rgb(221, 183, 115);
                    let compat = banana_api::game(id);
                    let (status, status_color) = match compat.map(|g| g.status()) {
                        Some(banana_api::Status { level: banana_api::SupportLevel::Verified, label }) => (format!("Verified · {label}"), Color32::from_rgb(130, 200, 120)),
                        Some(banana_api::Status { label, .. }) => (format!("Experimental · {label}"), experimental),
                        None => ("Experimental".to_owned(), experimental),
                    };
                    ui.label(RichText::new(status).small().color(status_color));
                    ui.add_space(8.0);
                    if self.global_library {
                        ui.label("Global library · Supported game profile");
                        ui.label("Add this game to My library, then choose your own game folder or ZArchive in Settings.");
                        ui.small("Game files are not included. Compatibility and platform support vary by game.");
                        ui.add_space(12.0);
                        if self.folders.added.contains(id) {
                            ui.label("Already in My library");
                            if ui.button("Open in My library").clicked() {
                                self.global_library = false;
                            }
                        } else if ui.button("Add to My library").clicked() {
                            self.set_library_membership(id, true);
                        }
                        ui.add_space(12.0);
                        egui::CollapsingHeader::new("Compatibility notes").default_open(true).show(ui, |ui| {
                            for blocker in &report.blockers { ui.label(format!("• {blocker}")); }
                        });
                        return;
                    }
                    if ui.add_enabled(self.probe.is_none(), egui::Button::new("Remove from My library")).clicked() {
                        self.set_library_membership(id, false);
                        return;
                    }
                    ui.label(RichText::new(report.path.display().to_string()).small().color(palette.muted));
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
                        for action in compat.map(|g| g.extra_actions()).unwrap_or_default() {
                            if ui.add_enabled(self.probe.is_none() && report.binary.is_some() && game_root.is_ok(), egui::Button::new(action.label)).clicked()
                                && let Ok(root) = &game_root {
                                self.launch_game(id, root.clone(), ctx.clone(), action.id);
                            }
                        }
                    });
                    if let Some(profile) = compat.and_then(|g| g.settings_profile()) {
                        ui.label(RichText::new(self.settings.settings.game(profile.id).summary()).small().color(palette.muted));
                    }
                    ui.add_space(6.0);
                    ui.label(compat.map(|g| g.play_hint()).unwrap_or_default());
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
        self.zar.show(&ctx);
    }
}
