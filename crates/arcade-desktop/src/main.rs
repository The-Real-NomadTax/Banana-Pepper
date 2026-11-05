mod aliens_runtime;
mod jurassic_runtime;
mod runtime;
mod minecraft_runtime;

use arcade_core::{CabinetInput, GameReport};
use eframe::egui::{self, Color32, RichText};
use std::{
    path::PathBuf,
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
            "Launching Jurassic Park on your display. 1 = start, mouse = aim, left click = fire, right click = secondary, T = test, Esc = exit. Audio remains disabled."
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
    if args.first().is_some_and(|a| a == "--launch-big-buck") {
        if args.len() > 2 {
            return Err("Usage: arcade-desktop --launch-big-buck [games-root]".into());
        }
        let root = args
            .get(1)
            .map(PathBuf::from)
            .unwrap_or_else(|| "games".into());
        println!(
            "Launching Big Buck Hunter on your display. T = test, 1 = start, mouse = aim/fire, right click = pump, Esc = exit."
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
            "Usage: arcade-desktop [--games-root PATH | --scan [PATH] | --probe-minecraft [PATH] | --launch-minecraft [PATH] | --setup-minecraft-libs [PATH] | --probe-big-buck [PATH] | --launch-big-buck [PATH]]".into(),
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
            if let Some(root) = initial_root { app.root = root; app.selected = 4; }
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
            egui::ScrollArea::vertical().max_height(140.0).show(ui, |ui| {
                ui.label(&self.message);
            });
        });
        egui::Panel::left("library")
            .default_size(250.0)
            .show(ui, |ui| {
                ui.add_space(12.0);
                ui.heading("Library");
                ui.label(format!("{} cabinet profiles", arcade_core::profiles().len()));
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
                    ui.label("Desktop controls: T = test, 1 = start, mouse = aim/fire, right click = pump, Esc = exit.");
                    if ui.add_enabled(self.probe.is_none(),egui::Button::new("Play on my display")).clicked() {
                        self.desktop_running=true;
                        let root=PathBuf::from(&self.root);
                        let (tx,rx)=mpsc::channel(); self.probe=Some(rx);
                        self.screenshot=None;
                        self.message="Big Buck Hunter running on your display. Close the game or press Esc to return.".into();
                        let repaint=ctx.clone();
                        std::thread::spawn(move || {let _=tx.send(runtime::launch_big_buck(&root));repaint.request_repaint();});
                    }
                    ui.label("Experimental Linux backend: private display with scripted switch and mouse inputs.");
                    if ui.add_enabled(self.probe.is_none(), egui::Button::new("Test Big Buck Hunter progression")).clicked() {
                        self.screenshot = None;
                        self.desktop_running=false;
                        let root = PathBuf::from(&self.root);
                        let (tx, rx) = mpsc::channel();
                        self.probe = Some(rx);
                        self.message = "Testing Big Buck Hunter on a private display…".into();
                        let repaint = ctx.clone();
                        std::thread::spawn(move || {
                            let result = runtime::probe_big_buck(&root, &PathBuf::from("runtime/big-buck"));
                            let _ = tx.send(result);
                            repaint.request_repaint();
                        });
                    }
                }
                if report.profile.id == "big-buck"
                    && let Some(texture) = &self.screenshot {
                        ui.label("Captured private-display frame (not an interactive game view)");
                        let size = texture.size_vec2();
                        ui.image((texture.id(), size * (ui.available_width().min(640.0) / size.x)));
                }
                if report.profile.id == "aliens" {
                    ui.label("Aliens desktop controls: 1 = Start, mouse = aim, left click = fire, 5 = coin. Audio disabled.");
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
                    ui.label("Desktop controls: 1 = start, mouse = aim, left click = fire, right click = secondary, T = test, Esc = exit.");
                    ui.label("5/6 = coin, 2 = player-two start, 9 = service; arrows/Enter/Backspace = menus. Mouse controls player one. Audio disabled.");
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
    }
}
