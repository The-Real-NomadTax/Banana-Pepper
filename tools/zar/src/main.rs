use banana_content::pack::{Progress, convert};
use eframe::egui;
use std::{
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
};

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if !args.is_empty() {
        if args[0] == "--help" || args[0] == "-h" {
            println!(
                "Arcade ZAR Converter\n\nNo arguments: open desktop app\narcade-zar --convert <game-folder> <output.zar>\n\nArchives folder contents, including empty directories. Source files are kept.\nOutput must be outside the source folder and must not already exist."
            );
            return Ok(());
        }
        if args.len() != 3 || args[0] != "--convert" {
            return Err("Usage: arcade-zar --convert <game-folder> <output.zar> (or no arguments for the desktop app)".into());
        }
        let mut last_phase = "";
        convert(
            &PathBuf::from(&args[1]),
            &PathBuf::from(&args[2]),
            &AtomicBool::new(false),
            |p| {
                if p.phase == "Complete" && !p.skipped_links.is_empty() {
                    eprintln!("{}", p.skipped_links_message());
                }
                if p.phase != last_phase {
                    eprintln!(
                        "{} ({} files, {} bytes)",
                        p.phase, p.files_total, p.bytes_total
                    );
                    last_phase = p.phase;
                }
            },
        )?;
        println!("Created {}", PathBuf::from(&args[2]).display());
        return Ok(());
    }
    eframe::run_native(
        "Arcade ZAR Converter",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([680.0, 470.0])
                .with_min_inner_size([540.0, 420.0]),
            ..Default::default()
        },
        Box::new(|_| Ok(Box::new(Converter::default()))),
    )?;
    Ok(())
}

#[derive(Default)]
struct Converter {
    source: PathBuf,
    output: PathBuf,
    progress: Arc<Mutex<Progress>>,
    cancel: Arc<AtomicBool>,
    result: Option<mpsc::Receiver<Result<(), String>>>,
    message: Option<Result<String, String>>,
    closing: bool,
}

impl Converter {
    fn start(&mut self, context: egui::Context) {
        self.message = None;
        self.cancel = Arc::new(AtomicBool::new(false));
        self.progress = Arc::new(Mutex::new(Progress {
            phase: "Starting",
            ..Default::default()
        }));
        let (tx, rx) = mpsc::channel();
        self.result = Some(rx);
        let (source, output) = (self.source.clone(), self.output.clone());
        let cancel = self.cancel.clone();
        let progress = self.progress.clone();
        std::thread::spawn(move || {
            let result = convert(&source, &output, &cancel, |p| {
                *progress.lock().unwrap() = p;
            });
            let _ = tx.send(result);
            context.request_repaint();
        });
    }
}

impl Drop for Converter {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

impl eframe::App for Converter {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if let Some(rx) = &self.result {
            let finished = match rx.try_recv() {
                Ok(result) => Some(result),
                Err(mpsc::TryRecvError::Disconnected) => {
                    Some(Err("Conversion worker stopped unexpectedly.".into()))
                }
                Err(mpsc::TryRecvError::Empty) => None,
            };
            if let Some(result) = finished {
                self.message = Some(result.map(|()| {
                    format!(
                        "Created {}\nReady to select in Arcade Foundry.{}",
                        self.output.display(),
                        self.progress.lock().unwrap().skipped_links_message()
                    )
                }));
                self.result = None;
                if self.closing {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
        }
        let busy = self.result.is_some();
        if busy && ui.ctx().input(|input| input.viewport().close_requested()) {
            // Keep the process alive until the worker can remove its partial file.
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.cancel.store(true, Ordering::Relaxed);
            self.closing = true;
        }
        ui.add_space(14.0);
        ui.heading("Arcade ZAR Converter");
        ui.label("Pack a game folder or your entire games library into a .zar file.");
        ui.add_space(16.0);
        ui.add_enabled_ui(!busy, |ui| {
            ui.strong("1. Choose the source folder");
            ui.horizontal(|ui| {
                if ui.button("Choose folder…").clicked()
                    && let Some(path) = rfd::FileDialog::new()
                        .set_title("Choose game folder or games library")
                        .pick_folder()
                {
                    self.output = path.parent().unwrap_or(&path).join(format!(
                        "{}.zar",
                        path.file_name().unwrap_or_default().to_string_lossy()
                    ));
                    self.source = path;
                    self.message = None;
                }
                ui.label(if self.source.as_os_str().is_empty() {
                    "No folder selected".into()
                } else {
                    self.source.display().to_string()
                });
            });
            ui.add_space(12.0);
            ui.strong("2. Choose where to save the archive");
            ui.horizontal(|ui| {
                if ui.button("Save as…").clicked() {
                    let mut dialog = rfd::FileDialog::new()
                        .set_title("Save ZArchive outside the source folder")
                        .add_filter("ZArchive", &["zar"]);
                    if let Some(parent) = self.output.parent() {
                        dialog = dialog.set_directory(parent);
                    }
                    if let Some(name) = self.output.file_name() {
                        dialog = dialog.set_file_name(name.to_string_lossy());
                    }
                    if let Some(mut path) = dialog.save_file() {
                        if path.extension().is_none() {
                            path.set_extension("zar");
                        }
                        self.output = path;
                        self.message = None;
                    }
                }
                ui.label(if self.output.as_os_str().is_empty() {
                    "No destination selected".into()
                } else {
                    self.output.display().to_string()
                });
            });
            ui.add_space(12.0);
            ui.label("Source files are kept. Existing archives are never replaced.");
            ui.label("Close the game before converting. Linked files and folders are copied, including external targets.");
            ui.label("Broken links are skipped and reported. Link cycles and special files are unsupported.");
            ui.add_space(12.0);
            if ui
                .add_enabled(
                    !self.source.as_os_str().is_empty() && !self.output.as_os_str().is_empty(),
                    egui::Button::new("Create .zar archive"),
                )
                .clicked()
            {
                self.start(ui.ctx().clone());
            }
        });
        if busy {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(100));
            let p = self.progress.lock().unwrap().clone();
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(p.phase);
                if ui
                    .add_enabled(
                        !self.cancel.load(Ordering::Relaxed),
                        egui::Button::new("Cancel"),
                    )
                    .clicked()
                {
                    self.cancel.store(true, Ordering::Relaxed);
                }
            });
            let fraction = if p.bytes_total == 0 {
                0.0
            } else {
                p.bytes_done as f32 / p.bytes_total as f32
            };
            ui.add(egui::ProgressBar::new(fraction).text(format!(
                "{} / {} files · {:.1} / {:.1} MiB",
                p.files_done,
                p.files_total,
                p.bytes_done as f64 / 1048576.0,
                p.bytes_total as f64 / 1048576.0
            )));
            ui.label(&p.path);
            if self.cancel.load(Ordering::Relaxed) {
                ui.label("Cancelling after the current operation…");
            }
        }
        if let Some(message) = &self.message {
            ui.add_space(12.0);
            match message {
                Ok(text) => {
                    ui.colored_label(egui::Color32::from_rgb(80, 190, 130), text);
                }
                Err(text) => {
                    ui.colored_label(egui::Color32::from_rgb(240, 110, 100), text);
                }
            }
        }
    }
}
