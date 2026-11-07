//! Banana Pepper desktop frontend. Games are reached only through the
//! `banana-api` registry; this crate names no individual game.
mod capture;
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
    let runtime_root = banana_runtime::project_root()?;
    if args.first().is_some_and(|a| a == "--runtime-info") && args.len() == 1 {
        println!("{}", serde_json::to_string_pretty(&serde_json::json!({
            "root": runtime_root,
            "distribution": banana_runtime::is_distribution(&runtime_root),
            "settings": runtime_root.join("settings.cfg"),
            "controls": runtime_root.join("controls.cfg"),
            "default_archive_cache": runtime_root.join("runtime/archive-cache"),
        }))?);
        return Ok(());
    }
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
            "Usage: arcade-desktop [--runtime-info | --games-root PATH | --scan [PATH]{}]",
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
    /// (game, action, add instead of replace, player) while waiting for a key press.
    capturing: Option<(&'static str, &'static str, bool, usize)>,
    message: String,
    /// Press-to-bind for controller buttons and movement axes.
    pad_capture: Option<capture::Capture>,
    devices: capture::Devices,
}
impl ControlsUi {
    fn load() -> Self {
        let path = banana_runtime::project_root().expect("Runtime root validated at startup").join("controls.cfg");
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
            pad_capture: None,
            devices: capture::Devices::default(),
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
        let path = banana_runtime::project_root().expect("Runtime root validated at startup").join("settings.cfg");
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
                                .add(egui::Button::selectable(app.settings.tab == tab, label)
                                    .frame_when_inactive(true))
                                .clicked()
                            {
                                app.settings.tab = tab;
                                app.controls.capturing = None;
                                app.controls.pad_capture = None;
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
                                    controls_tab(ui, &mut app.settings, &mut app.controls, profile);
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
        app.controls.pad_capture = None;
    }
}

/// Framed choices keep both selected and available settings visible.
fn settings_choice<T: PartialEq>(
    ui: &mut egui::Ui,
    current: &mut T,
    value: T,
    label: impl Into<egui::WidgetText>,
) -> egui::Response {
    let mut response = ui.add(
        egui::Button::selectable(*current == value, label.into())
            .frame_when_inactive(true),
    );
    if response.clicked() && *current != value {
        *current = value;
        response.mark_changed();
    }
    response
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
                if settings_choice(ui, &mut game.fullscreen, false, "Windowed")
                    .clicked()
                {
                    change = Some("Windowed".into());
                }
                if settings_choice(ui, &mut game.fullscreen, true, "Fullscreen")
                    .clicked()
                {
                    change = Some("Fullscreen".into());
                }
            });
            ui.end_row();

            ui.label("VSync");
            ui.horizontal(|ui| {
                for v in VSync::ALL {
                    if settings_choice(ui, &mut game.vsync, v, v.label()).clicked() {
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
                    if settings_choice(ui, &mut game.renderer, r, label).clicked() {
                        change = Some(format!("Renderer {}", r.id()));
                    }
                }
            });
            ui.end_row();

        });
    if profile.crosshair.is_some() {
        ui.add_space(12.0);
        ui.separator();
        ui.label(RichText::new("Crosshair").strong());
        ui.small("Choose a style. Click a preview to select it.");
        let styles = settings::Crosshair::all();
        let gap = ui.spacing().item_spacing.x;
        let columns = ((ui.available_width() + gap) / (112.0 + gap))
            .floor().max(1.0) as usize;
        let columns = columns.min(styles.len());
        let card_width = ((ui.available_width() - gap * (columns - 1) as f32)
            / columns as f32).min(140.0);
        for row in styles.chunks(columns) {
            ui.horizontal(|ui| {
                for style in row {
                    let selected = game.crosshair == *style;
                    let (rect, response) = ui.allocate_exact_size(
                        egui::vec2(card_width, 112.0), egui::Sense::click(),
                    );
                    response.widget_info(|| egui::WidgetInfo::selected(
                        egui::WidgetType::Button, ui.is_enabled(), selected, style.label(),
                    ));
                    let visuals = ui.style().interact_selectable(&response, selected);
                    let stroke = if selected {
                        ui.visuals().selection.stroke
                    } else {
                        visuals.bg_stroke
                    };
                    ui.painter().rect(
                        rect, 6.0, visuals.weak_bg_fill, stroke, egui::StrokeKind::Inside,
                    );
                    let center = egui::pos2(rect.center().x, rect.top() + 34.0);
                    if let Some(texture) = style.0.and_then(|id| crosshair_texture(ui.ctx(), id)) {
                        ui.painter().image(
                            texture.id(),
                            egui::Rect::from_center_size(center, egui::vec2(48.0, 48.0)),
                            egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                            Color32::WHITE,
                        );
                    } else {
                        ui.painter().text(
                            center, egui::Align2::CENTER_CENTER, "—",
                            egui::FontId::proportional(28.0), ui.visuals().text_color(),
                        );
                    }
                    // Keep the large variants readable without widening the cards.
                    let label = style.label().replace(", large", "\nLarge");
                    ui.painter().text(
                        egui::pos2(rect.center().x, rect.top() + 81.0),
                        egui::Align2::CENTER_CENTER, label,
                        egui::FontId::proportional(13.0), ui.visuals().text_color(),
                    );
                    if response.clicked() {
                        game.crosshair = style.clone();
                        change = Some(format!("Crosshair {}", style.label().to_ascii_lowercase()));
                    }
                    response.on_hover_text(style.label());
                }
            });
        }
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.label("Size");
            let slider = egui::Slider::new(
                &mut game.crosshair_size,
                banana_input::crosshair::SIZE_MIN..=banana_input::crosshair::SIZE_MAX,
            ).suffix("%").step_by(5.0);
            let response = ui.add_enabled(game.crosshair.0.is_some(), slider)
                .on_disabled_hover_text("Choose a crosshair to set its size");
            if response.drag_stopped() || (response.changed() && !response.dragged()) {
                change = Some(format!("Crosshair size {}%", game.crosshair_size));
            }
        });
        if game.crosshair.0.is_none() {
            ui.small("Off: the normal mouse pointer is shown.");
        } else {
            ui.small("Size applies in game. Previews above show the available styles.");
        }
    }
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

/// All player bindings in one scrolling controls setup, without player tabs.
fn controls_tab(
    ui: &mut egui::Ui,
    settings_ui: &mut SettingsUi,
    state: &mut ControlsUi,
    profile: &'static settings::Profile,
) {
    let Some(game) = controls::game(profile.id) else {
        return;
    };
    let two_player = profile.max_players > 1 && !game.player_two.is_empty();
    let mut change = None;
    let settings = settings_ui
        .settings
        .game_mut(profile.id)
        .expect("profile settings");
    let player_count = if two_player { 2 } else { 1 };
    if settings.players != player_count as u8 {
        settings.players = player_count as u8;
        change = Some(format!("{} controls updated", profile.title));
    }
    capture_key(ui, state, game);
    if let Some(capture) = state.pad_capture.as_ref() {
        if capture.player < player_count {
            let operator = match capture.want {
                capture::Want::Button(id) => game.actions.iter()
                    .any(|action| action.id == id && is_operator_control(action)),
                _ => false,
            };
            let method = if operator { settings.operator_input } else { settings.inputs[capture.player] };
            poll_pad_capture(ui, state, game, method);
        } else {
            state.pad_capture = None;
        }
    }
    if player_count == 2 && ui.available_width() >= 1380.0 {
        ui.columns(2, |columns| {
            for (player, column) in columns.iter_mut().enumerate() {
                control_player_card(
                    column,
                    state,
                    settings,
                    profile,
                    game,
                    player,
                    two_player,
                    &mut change,
                );
            }
        });
    } else {
        for player in 0..player_count {
            control_player_card(
                ui,
                state,
                settings,
                profile,
                game,
                player,
                two_player,
                &mut change,
            );
            ui.add_space(12.0);
        }
    }
    if game
        .actions
        .iter()
        .any(|action| is_operator_control(action))
    {
        ui.add_space(12.0);
        egui::Frame::group(ui.style())
            .fill(ui.visuals().faint_bg_color)
            .inner_margin(16)
            .corner_radius(8)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.heading("Operator controls");
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.add_sized([132.0, 24.0], egui::Label::new("Input device"));
                    let previous = settings.operator_input;
                    egui::ComboBox::from_id_salt((game.id, "operator-input"))
                        .width(230.0)
                        .selected_text(method_label(profile, previous))
                        .show_ui(ui, |ui| {
                            for &method in profile.input_methods {
                                ui.selectable_value(&mut settings.operator_input, method, method_label(profile, method));
                            }
                        });
                    if settings.operator_input != previous {
                        state.capturing = None;
                        state.pad_capture = None;
                        change = Some(format!("{} operator input changed", profile.title));
                    }
                });
                ui.add_space(10.0);
                ui.separator();
                ui.add_space(6.0);
                player_controls(ui, state, game, two_player, settings.operator_input, true);
            });
    }
    if let Some(change) = change {
        settings_ui.save(&change);
    }
    ui.add_space(8.0);
    // Keep errors and active capture prompts visible; successful saves stay compact.
    let message = if !state.message.is_empty() {
        &state.message
    } else {
        &settings_ui.message
    };
    if message.contains("Saved;") {
        ui.weak("Saved");
    } else if !message.is_empty() {
        ui.small(message);
    }
}

fn control_player_card(
    ui: &mut egui::Ui,
    state: &mut ControlsUi,
    settings: &mut settings::GameSettings,
    profile: &'static settings::Profile,
    game: &'static controls::Game,
    player: usize,
    two_player: bool,
    change: &mut Option<String>,
) {
    egui::Frame::group(ui.style())
        .fill(ui.visuals().faint_bg_color)
        .inner_margin(16)
        .corner_radius(8)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.heading(format!("Player {}", player + 1));
            ui.add_space(8.0);
            egui::Grid::new(("input-settings", game.id, player))
                .num_columns(2)
                .spacing([16.0, 8.0])
                .show(ui, |ui| {
                    if !profile.input_methods.is_empty() {
                        ui.add_sized([132.0, 24.0], egui::Label::new("Input device"));
                        let previous = settings.inputs[player];
                        let mut method = previous;
                        egui::ComboBox::from_id_salt(("input-method", player))
                            .width(230.0)
                            .selected_text(method_label(profile, method))
                            .show_ui(ui, |ui| {
                                for &input in profile.input_methods {
                                    ui.selectable_value(
                                        &mut method,
                                        input,
                                        method_label(profile, input),
                                    );
                                }
                            });
                        if method != previous {
                            state.capturing = None;
                            state.pad_capture = None;
                            settings.set_input(player, method);
                            *change = Some(format!(
                                "{} player {} input changed",
                                profile.title,
                                player + 1
                            ));
                        }
                        ui.end_row();
                    }
                    if profile
                        .input_methods
                        .contains(&settings::InputMethod::RawInput)
                        && matches!(
                            settings.inputs[player],
                            settings::InputMethod::XInput | settings::InputMethod::DirectInput
                        )
                    {
                        ui.label("Stick aim");
                        let previous = settings.aim_modes[player];
                        egui::ComboBox::from_id_salt(("aim-mode", player))
                            .width(230.0)
                            .selected_text(previous.label())
                            .show_ui(ui, |ui| {
                                for mode in [settings::AimMode::Cursor, settings::AimMode::Centered]
                                {
                                    ui.selectable_value(
                                        &mut settings.aim_modes[player],
                                        mode,
                                        mode.label(),
                                    );
                                }
                            });
                        if settings.aim_modes[player] != previous {
                            *change = Some(format!(
                                "{} player {} aim changed",
                                profile.title,
                                player + 1
                            ));
                        }
                        ui.end_row();
                    }
                });
            ui.add_space(10.0);
            ui.separator();
            ui.add_space(6.0);
            let method = settings.inputs[player];
            if player == 0 {
                player_controls(ui, state, game, two_player, method, false);
            } else {
                player_two_controls(ui, state, game, method, false);
            }
        });
}

fn method_label(profile: &settings::Profile, method: settings::InputMethod) -> &'static str {
    // Racers have no aim: their keyboard-and-mouse choice is the keyboard.
    if method == settings::InputMethod::KeyboardMouse
        && !profile.input_methods.contains(&settings::InputMethod::RawInput)
    {
        "Keyboard"
    } else {
        method.label()
    }
}

/// Key capture for the open menu: the key or mouse button pressed next.
fn capture_key(ui: &mut egui::Ui, state: &mut ControlsUi, game: &'static controls::Game) {
    let Some((capture_game, action, append, player)) = state.capturing else {
        return;
    };
    if capture_game != game.id {
        return;
    }
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
        if name == "Escape" || (name.starts_with("Mouse") && (!game.mouse_buttons || player == 1)) {
            state.message = "Key change cancelled.".into();
        } else {
            let result = if player == 1 {
                state.bindings.rebind_p2(game.id, action, &name, append)
            } else {
                state.bindings.rebind(game.id, action, &name, append)
            };
            match result {
                Ok(change) => state.save(change),
                Err(e) => state.message = format!("Not changed: {e}"),
            }
        }
    }
    ui.ctx().request_repaint();
}

/// A key button: shows `keys`, and starts capturing on click.
fn key_button(
    ui: &mut egui::Ui,
    state: &mut ControlsUi,
    game: &'static controls::Game,
    action: &'static controls::Action,
    player: usize,
    keys: &[String],
) {
    let waiting = matches!(state.capturing,
        Some((g, a, _, p)) if g == game.id && a == action.id && p == player);
    let text = if waiting {
        "Press a key… (Esc cancels)".to_owned()
    } else if keys.is_empty() {
        "unbound".to_owned()
    } else {
        keys.join(", ")
    };
    let button = egui::Button::new(RichText::new(text).monospace()).min_size(egui::vec2(170.0, 0.0));
    if ui.add(button).on_hover_text("Click, then press the new key").clicked() {
        state.capturing = Some((game.id, action.id, false, player));
        state.pad_capture = None;
        state.message = format!("Press a key for {}.", action.label);
    }
}

/// Press-to-bind: reads the player's devices while a capture is waiting,
/// and binds the first new button press or axis movement.
fn poll_pad_capture(
    ui: &mut egui::Ui,
    state: &mut ControlsUi,
    game: &'static controls::Game,
    method: settings::InputMethod,
) {
    let Some(capture) = state.pad_capture.as_mut() else {
        return;
    };
    if capture.game != game.id || capture.method != method {
        state.pad_capture = None;
        return;
    }
    let escape = ui.ctx().input(|i| {
        i.key_pressed(egui::Key::Escape)
            || i.events.iter().any(|e| matches!(e, egui::Event::Key { key: egui::Key::Escape, pressed: true, .. }))
    });
    if escape {
        state.pad_capture = None;
        state.message = "Binding cancelled.".into();
        return;
    }
    if capture.timed_out() {
        state.pad_capture = None;
        state.message = "Nothing was pressed; binding cancelled.".into();
        return;
    }
    let Some(now) = state.devices.read(method) else {
        state.pad_capture = None;
        state.message = format!(
            "{} devices cannot be read here; choose from the List menu instead.",
            method.label()
        );
        return;
    };
    ui.ctx().request_repaint();
    let Some(found) = capture.update(now) else {
        return;
    };
    let (player, want) = (capture.player, capture.want);
    state.pad_capture = None;
    let result = match (found, want) {
        (capture::Captured::Button(name), capture::Want::Button(action)) => {
            let bound = if player == 1 {
                state.bindings.p2_pads(game.id, action)
            } else {
                state.bindings.keys(game.id, action).to_vec()
            };
            if bound.iter().any(|b| b == name) {
                Ok(format!("{} is already bound there", controls::pad_label(method, name).unwrap_or(name)))
            } else if player == 1 {
                state.bindings.toggle_pad_p2(game.id, action, name)
            } else {
                state.bindings.toggle_pad(game.id, action, name)
            }
        }
        (capture::Captured::Axis(name), capture::Want::Axis { which, append }) => {
            let mut axes = if append {
                state.bindings.movement(game.id, player, which)
            } else {
                Vec::new()
            };
            axes.retain(|a| a != &name);
            axes.truncate(1);
            axes.push(name);
            state.bindings.set_movement(game.id, player, which, &axes)
        }
        _ => return,
    };
    match result {
        Ok(change) => state.save(change),
        Err(e) => state.message = format!("Not changed: {e}"),
    }
}

/// Controller buttons for one action, named for the player's input method:
/// click, then press the button on the device (List shows them all).
fn pad_menu(
    ui: &mut egui::Ui,
    state: &mut ControlsUi,
    game: &'static controls::Game,
    action: &'static controls::Action,
    player: usize,
    method: settings::InputMethod,
) {
    let bound: Vec<String> = if player == 1 {
        state.bindings.p2_pads(game.id, action.id)
    } else {
        state
            .bindings
            .keys(game.id, action.id)
            .iter()
            .filter(|k| controls::is_pad(k))
            .cloned()
            .collect()
    };
    let named: Vec<&str> = bound
        .iter()
        .filter_map(|b| controls::pad_label(method, b))
        .collect();
    let waiting = state.pad_capture.as_ref().is_some_and(|c| {
        c.game == game.id && c.player == player && c.want == capture::Want::Button(action.id)
    });
    let text = if waiting {
        "🎮 Press a button…".to_owned()
    } else if named.is_empty() {
        "🎮 none".to_owned()
    } else {
        format!("🎮 {}", named.join(", "))
    };
    ui.horizontal(|ui| {
        let button = egui::Button::new(RichText::new(text).monospace()).min_size(egui::vec2(150.0, 0.0));
        let clicked = ui
            .add(button)
            .on_hover_text("Click, then press the button on the device (adds it). Click again or press Esc to cancel.")
            .clicked();
        if clicked && waiting {
            state.pad_capture = None;
            state.message = "Binding cancelled.".into();
        } else if clicked {
            let start = capture::Capture::new(game.id, player, method, capture::Want::Button(action.id));
            state.message = start.prompt(["", ""]);
            state.capturing = None;
            state.pad_capture = Some(start);
        }
        ui.menu_button("List", |ui| {
            ui.label(RichText::new(format!("{} buttons", method.label())).small());
            for (name, _, _) in controls::PAD_BUTTONS {
                let Some(label) = controls::pad_label(method, name) else {
                    continue;
                };
                let mut on = bound.iter().any(|k| k == name);
                if ui.checkbox(&mut on, label).changed() {
                    let result = if player == 1 {
                        state.bindings.toggle_pad_p2(game.id, action.id, name)
                    } else {
                        state.bindings.toggle_pad(game.id, action.id, name)
                    };
                    match result {
                        Ok(change) => state.save(change),
                        Err(e) => state.message = format!("Not changed: {e}"),
                    }
                }
            }
        })
        .response
        .on_hover_text("Choose or remove buttons from a list");
    });
}

/// Movement X and Y (aim or steering) for a player who uses a controller,
/// joystick, mouse or light gun: click, then move the axis.
fn movement_rows(
    ui: &mut egui::Ui,
    state: &mut ControlsUi,
    game: &'static controls::Game,
    player: usize,
    method: settings::InputMethod,
) {
    let Some(spec) = game.movement else {
        return;
    };
    if method == settings::InputMethod::KeyboardMouse {
        return;
    }
    ui.add_space(6.0);
    ui.label(RichText::new("Movement").strong());
    egui::Grid::new(("movement", game.id, player))
        .num_columns(2)
        .spacing([16.0, 6.0])
        .striped(true)
        .show(ui, |ui| {
            for which in 0..2 {
                ui.add_sized([132.0, 28.0], egui::Label::new(spec.labels[which]).halign(egui::Align::LEFT));
                let axes = state.bindings.movement(game.id, player, which);
                let named: Vec<String> = axes
                    .iter()
                    .filter_map(|a| controls::axis_label(method, a))
                    .collect();
                let waiting = state.pad_capture.as_ref().is_some_and(|c| {
                    c.game == game.id
                        && c.player == player
                        && matches!(c.want, capture::Want::Axis { which: w, .. } if w == which)
                });
                let text = if waiting {
                    "Move it…".to_owned()
                } else if named.is_empty() {
                    if axes.is_empty() { "unbound".to_owned() } else { format!("not a {} axis", method.label()) }
                } else {
                    named.join(", ")
                };
                ui.horizontal(|ui| {
                    let button = egui::Button::new(RichText::new(text).monospace()).min_size(egui::vec2(170.0, 0.0));
                    let mut start = None;
                    if ui
                        .add(button)
                        .on_hover_text("Click, then move the stick, axis or pointer (replaces). Click again or press Esc to cancel.")
                        .clicked()
                    {
                        if waiting {
                            state.pad_capture = None;
                            state.message = "Binding cancelled.".into();
                        } else {
                            start = Some(false);
                        }
                    }
                    if ui.small_button("+").on_hover_text("Add a second axis").clicked() {
                        start = Some(true);
                    }
                    if let Some(append) = start {
                        let capture = capture::Capture::new(game.id, player, method, capture::Want::Axis { which, append });
                        state.message = capture.prompt(spec.labels);
                        state.capturing = None;
                        state.pad_capture = Some(capture);
                    }
                    if ui.small_button("×").on_hover_text("Unbind").clicked() {
                        match state.bindings.set_movement(game.id, player, which, &[]) {
                            Ok(change) => state.save(change),
                            Err(e) => state.message = format!("Not changed: {e}"),
                        }
                    }
                    ui.menu_button("List", |ui| {
                        ui.label(RichText::new(format!("{} axes (at most two)", method.label())).small());
                        for axis in banana_input::pad::Axis::ALL {
                            for name in [axis.name().to_owned(), format!("{}-", axis.name())] {
                                let Some(label) = controls::axis_label(method, &name) else {
                                    continue;
                                };
                                let mut on = axes.contains(&name);
                                if ui.checkbox(&mut on, label).changed() {
                                    let mut next = axes.clone();
                                    if on {
                                        next.retain(|a| a.trim_end_matches('-') != axis.name());
                                        next.truncate(1);
                                        next.push(name.clone());
                                    } else {
                                        next.retain(|a| a != &name);
                                    }
                                    match state.bindings.set_movement(game.id, player, which, &next) {
                                        Ok(change) => state.save(change),
                                        Err(e) => state.message = format!("Not changed: {e}"),
                                    }
                                }
                            }
                        }
                    })
                    .response
                    .on_hover_text("Choose axes from a list");
                });
                ui.end_row();
            }
        });
}

fn is_operator_control(action: &controls::Action) -> bool {
    matches!(action.id, "test" | "service" | "volume_up" | "volume_down")
        || action.id.starts_with("menu_")
}

/// Player one's menu (every player's menu in one-player games).
fn player_controls(
    ui: &mut egui::Ui,
    state: &mut ControlsUi,
    game: &'static controls::Game,
    two_player: bool,
    method: settings::InputMethod,
    operator_only: bool,
) {
    let buttons = method != settings::InputMethod::KeyboardMouse;
    egui::Grid::new(("controls", game.id, 0, operator_only))
        .num_columns(2)
        .spacing([16.0, 6.0])
        .striped(true)
        .show(ui, |ui| {
            ui.weak("Control");
            ui.weak(method.label());
            ui.end_row();
            for action in game.actions {
                if is_operator_control(action) != operator_only {
                    continue;
                }
                if !buttons && action.pad_only {
                    continue;
                }
                // Start 2 and Coin 2 are player two's, in player two's menu.
                if two_player
                    && matches!(
                        controls::player_two_role(game, action),
                        controls::PlayerTwo::Hidden
                    )
                {
                    continue;
                }
                ui.add_sized(
                    [132.0, 28.0],
                    egui::Label::new(action.label).halign(egui::Align::LEFT),
                );
                if !buttons {
                    ui.horizontal(|ui| {
                        if action.pad_only {
                            ui.add_enabled(false, egui::Button::new("controller only"));
                        } else {
                            let keys: Vec<String> = state
                                .bindings
                                .keys(game.id, action.id)
                                .iter()
                                .filter(|k| !controls::is_pad(k))
                                .cloned()
                                .collect();
                            key_button(ui, state, game, action, 0, &keys);
                            if !action.single {
                                if ui
                                    .small_button("+")
                                    .on_hover_text("Add another key")
                                    .clicked()
                                {
                                    state.capturing = Some((game.id, action.id, true, 0));
                                    state.message =
                                        format!("Press an extra key for {}.", action.label);
                                }
                                if ui.small_button("×").on_hover_text("Unbind").clicked() {
                                    let pads: Vec<String> = state
                                        .bindings
                                        .keys(game.id, action.id)
                                        .iter()
                                        .filter(|k| controls::is_pad(k))
                                        .cloned()
                                        .collect();
                                    let refs: Vec<&str> = pads.iter().map(String::as_str).collect();
                                    let mut next = state.bindings.clone();
                                    match next
                                        .set(game.id, action.id, &refs)
                                        .and_then(|()| next.validate())
                                    {
                                        Ok(()) => {
                                            state.bindings = next;
                                            state.save(format!("{} keys unbound", action.label));
                                        }
                                        Err(e) => state.message = format!("Not changed: {e}"),
                                    }
                                }
                            }
                        }
                    });
                }
                if buttons {
                    pad_menu(ui, state, game, action, 0, method);
                }
                ui.end_row();
            }
        });
    if operator_only {
        return;
    }
    movement_rows(ui, state, game, 0, method);
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        if ui.button("Reset to defaults").clicked() {
            state.capturing = None;
            state.bindings.reset_player_one(game.id);
            state.save(format!("{} player 1 controls reset", game.title));
        }
    });
}

/// Player two's menu: its Start and Coin keys, its own keys for the gun
/// actions, shared keys for the rest, and its own controller buttons.
fn player_two_controls(
    ui: &mut egui::Ui,
    state: &mut ControlsUi,
    game: &'static controls::Game,
    method: settings::InputMethod,
    operator_only: bool,
) {
    let buttons = method != settings::InputMethod::KeyboardMouse;
    egui::Grid::new(("controls", game.id, 1, operator_only))
        .num_columns(2)
        .spacing([16.0, 6.0])
        .striped(true)
        .show(ui, |ui| {
            ui.weak("Control");
            ui.weak(method.label());
            ui.end_row();
            for action in game.actions {
                if is_operator_control(action) != operator_only {
                    continue;
                }
                if !buttons && action.pad_only {
                    continue;
                }
                let role = controls::player_two_role(game, action);
                if matches!(role, controls::PlayerTwo::Hidden) {
                    continue;
                }
                let label = match role {
                    controls::PlayerTwo::Linked(linked) => linked.label.to_owned(),
                    _ => action
                        .label
                        .replace("player one", "player two")
                        .replace("Player 1", "Player 2"),
                };
                ui.add_sized(
                    [132.0, 28.0],
                    egui::Label::new(label).halign(egui::Align::LEFT),
                );
                if !buttons {
                    ui.horizontal(|ui| match role {
                        controls::PlayerTwo::Linked(linked) => {
                            let keys: Vec<String> = state
                                .bindings
                                .keys(game.id, linked.id)
                                .iter()
                                .filter(|k| !controls::is_pad(k))
                                .cloned()
                                .collect();
                            key_button(ui, state, game, linked, 0, &keys);
                        }
                        controls::PlayerTwo::Own(_) if !action.pad_only => {
                            let keys = state.bindings.p2_keys(game.id, action.id);
                            key_button(ui, state, game, action, 1, &keys);
                            if ui.small_button("×").on_hover_text("Unbind").clicked() {
                                state.bindings.clear_p2_keys(game.id, action.id);
                                state.save(format!(
                                    "{} player 2 {} keys unbound",
                                    game.title, action.id
                                ));
                            }
                        }
                        controls::PlayerTwo::Own(_) => {
                            ui.add_enabled(false, egui::Button::new("controller only"));
                        }
                        _ if operator_only => {
                            let keys: Vec<String> = state
                                .bindings
                                .keys(game.id, action.id)
                                .iter()
                                .filter(|key| !controls::is_pad(key))
                                .cloned()
                                .collect();
                            key_button(ui, state, game, action, 0, &keys);
                        }
                        _ => {
                            let keys = state
                                .bindings
                                .keys(game.id, action.id)
                                .iter()
                                .filter(|k| !controls::is_pad(k))
                                .cloned()
                                .collect::<Vec<_>>()
                                .join(", ");
                            ui.add_enabled(
                                false,
                                egui::Button::new(
                                    RichText::new(format!("shared: {keys}")).monospace(),
                                ),
                            )
                            .on_disabled_hover_text("Set in the Player 1 controls above");
                        }
                    });
                }
                if buttons {
                    pad_menu(ui, state, game, action, 1, method);
                }
                ui.end_row();
            }
        });
    if operator_only {
        return;
    }
    movement_rows(ui, state, game, 1, method);
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        if state.bindings.p2_custom(game.id) {
            if ui.button("Reset controller buttons").clicked() {
                state.capturing = None;
                state.bindings.reset_p2(game.id);
                state.save(format!("{} player 2 uses player 1's buttons", game.title));
            }
        }
    });
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
                    let previous = self.folders.theme;
                    ui.menu_button("Theme", |ui| {
                        egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                            for theme in appearance::Theme::ALL {
                                let label = RichText::new(format!("●  {}", theme.name()))
                                    .color(theme.palette().accent);
                                if ui.selectable_label(self.folders.theme == theme, label).clicked() {
                                    self.folders.theme = theme;
                                    ui.close();
                                }
                            }
                        });
                    }).response.on_hover_text(format!("Current theme: {}", self.folders.theme.name()));
                    if self.folders.theme != previous {
                        appearance::install(&ctx, self.folders.theme);
                        if let Err(error) = library_preferences::save(&self.folders) {
                            self.folder_message = format!("Theme applied, but could not save it: {error}");
                        }
                        ctx.request_repaint();
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            RichText::new(env!("CARGO_PKG_VERSION"))
                                .small()
                                .color(palette.muted),
                        );
                    });
                });
                ui.add_space(4.0);
                ui.horizontal_wrapped(|ui| {
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
