//! A native tool window owned by the main desktop app. No separate event loop or theme.
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

#[derive(Default)]
pub struct ZarCompressor {
    open: bool,
    focus_requested: bool,
    source: PathBuf,
    output: PathBuf,
    progress: Arc<Mutex<Progress>>,
    cancel: Arc<AtomicBool>,
    result: Option<mpsc::Receiver<Result<(), String>>>,
    message: Option<Result<String, String>>,
    closing: bool,
    close_app: bool,
}

impl ZarCompressor {
    pub fn open(&mut self) {
        self.open = true;
        self.focus_requested = true;
    }

    fn start(&mut self, context: egui::Context) {
        self.message = None;
        self.closing = false;
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

    fn poll(&mut self, ctx: &egui::Context) {
        let finished = self.result.as_ref().and_then(|rx| match rx.try_recv() {
            Ok(result) => Some(result),
            Err(mpsc::TryRecvError::Disconnected) => {
                Some(Err("Conversion worker stopped unexpectedly.".into()))
            }
            Err(mpsc::TryRecvError::Empty) => None,
        });
        if let Some(result) = finished {
            self.message = Some(result.map(|()| {
                format!(
                    "Created {}\nReady to select in Banana pepper.{}",
                    self.output.display(),
                    self.progress.lock().unwrap().skipped_links_message()
                )
            }));
            self.result = None;
            if self.closing {
                self.open = false;
                self.closing = false;
            }
            if self.close_app {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }

    pub fn show(&mut self, ctx: &egui::Context) {
        // Let the existing converter finish cancellation and remove partial files
        // before exiting the parent application.
        if self.result.is_some() && ctx.input(|input| input.viewport().close_requested()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.cancel.store(true, Ordering::Relaxed);
            self.close_app = true;
            self.open = true;
        }
        self.poll(ctx);
        if !self.open {
            return;
        }
        if self.result.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
        ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("zar-compressor"),
            egui::ViewportBuilder::default()
                .with_title("ZAR compressor")
                .with_inner_size([700.0, 560.0])
                .with_min_inner_size([560.0, 440.0])
                .with_decorations(true)
                .with_resizable(true)
                .with_taskbar(true),
            |viewport_ui, _class| {
                if self.focus_requested {
                    viewport_ui
                        .ctx()
                        .send_viewport_cmd(egui::ViewportCommand::Focus);
                    self.focus_requested = false;
                }
                if viewport_ui
                    .ctx()
                    .input(|input| input.viewport().close_requested())
                {
                    if self.result.is_some() {
                        viewport_ui
                            .ctx()
                            .send_viewport_cmd(egui::ViewportCommand::CancelClose);
                        self.cancel.store(true, Ordering::Relaxed);
                        self.closing = true;
                    } else {
                        self.open = false;
                        ctx.request_repaint();
                        return;
                    }
                }
                egui::CentralPanel::default()
                    .frame(egui::Frame::central_panel(viewport_ui.style()).inner_margin(16))
                    .show(viewport_ui, |ui| {
                        egui::ScrollArea::vertical()
                            .auto_shrink([false, false])
                            .show(ui, |ui| self.contents(ui));
                    });
            },
        );
    }

    fn contents(&mut self, ui: &mut egui::Ui) {
        let busy = self.result.is_some();
        ui.add_space(14.0);
        ui.heading("ZAR compressor");
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
                    ui.colored_label(ui.visuals().selection.stroke.color, text);
                }
                Err(text) => {
                    ui.colored_label(ui.visuals().error_fg_color, text);
                }
            }
        }
    }
}

impl Drop for ZarCompressor {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn background_job_creates_a_loadable_archive_and_keeps_source_files() {
        let temp = tempfile::tempdir_in(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target"),
        )
        .unwrap();
        let source = temp.path().join("game folder");
        std::fs::create_dir_all(source.join("data/empty")).unwrap();
        std::fs::write(source.join("game.exe"), b"game bytes").unwrap();
        std::fs::write(source.join("data/asset"), b"asset bytes").unwrap();
        let ctx = egui::Context::default();
        let mut tool = ZarCompressor::default();
        tool.source = source.clone();
        tool.output = temp.path().join("game.zar");
        tool.start(ctx.clone());
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while tool.result.is_some() {
            tool.poll(&ctx);
            assert!(
                std::time::Instant::now() < deadline,
                "Compression worker timed out"
            );
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(tool.message.as_ref().unwrap().is_ok(), "{:?}", tool.message);
        let provider = banana_content::open(&tool.output).unwrap();
        assert_eq!(
            banana_content::read_all(provider.as_ref(), "game.exe", 1024 * 1024).unwrap(),
            b"game bytes"
        );
        assert!(provider.metadata("data/empty").unwrap().directory);
        assert_eq!(
            std::fs::read(source.join("data/asset")).unwrap(),
            b"asset bytes"
        );
    }
}
