//! Debug builds only: `DICTURS_SCREENSHOTS=<dir>` walks through the pages, saves a PNG of each
//! and quits. Run by `scripts/screenshots.sh` with a demo config; the phrases it simulates
//! are matched in dry-run, so nothing is executed.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

use eframe::egui;

use super::{DictursApp, LogKind, Page};

/// Time given to each page to lay out and load its images before the capture.
const SETTLE: Duration = Duration::from_millis(900);
/// Window size of the screenshots, in points.
const SIZE: [f32; 2] = [1440.0, 900.0];

/// A fixed-size window: tiling compositors (Hyprland, Sway…) float it at that size.
pub(super) fn viewport(viewport: egui::ViewportBuilder) -> egui::ViewportBuilder {
    viewport
        .with_inner_size(SIZE)
        .with_min_inner_size(SIZE)
        .with_max_inner_size(SIZE)
        .with_resizable(false)
}

struct Shot {
    file: &'static str,
    page: Page,
    theme: egui::Theme,
    setup: fn(&mut DictursApp),
}

const SHOTS: &[Shot] = &[
    Shot {
        file: "01-commands.png",
        page: Page::Commands,
        theme: egui::Theme::Dark,
        setup: |_| {},
    },
    Shot {
        file: "02-commands-light.png",
        page: Page::Commands,
        theme: egui::Theme::Light,
        setup: |_| {},
    },
    Shot {
        file: "03-editor.png",
        page: Page::Commands,
        theme: egui::Theme::Dark,
        setup: edit_search_command,
    },
    Shot {
        file: "04-listen.png",
        page: Page::Listen,
        theme: egui::Theme::Dark,
        setup: simulate_phrases,
    },
    Shot {
        file: "05-settings.png",
        page: Page::Settings,
        theme: egui::Theme::Light,
        setup: |_| {},
    },
    Shot {
        file: "06-models.png",
        page: Page::Models,
        theme: egui::Theme::Dark,
        setup: |_| {},
    },
    Shot {
        file: "07-launchers.png",
        page: Page::Launchers,
        theme: egui::Theme::Light,
        setup: |_| {},
    },
];

fn edit_search_command(app: &mut DictursApp) {
    let id = app
        .cfg
        .commands
        .iter()
        .find(|c| c.command.contains("{args_url}"))
        .map(|c| c.id);
    app.open_editor(id);
}

fn simulate_phrases(app: &mut DictursApp) {
    app.dry_run = true;
    for phrase in [
        "open browser",
        "search for rust egui",
        "volume up",
        "hello there",
    ] {
        app.push_log(LogKind::Heard, format!("“{phrase}”  (412 ms)"));
        app.handle_transcription(phrase);
    }
}

pub(super) struct Tour {
    dir: PathBuf,
    step: usize,
    shown_at: Option<Instant>,
    requested: bool,
    saved_tx: Sender<Result<PathBuf, String>>,
    saved_rx: Receiver<Result<PathBuf, String>>,
}

impl Tour {
    pub(super) fn from_env() -> Option<Self> {
        let dir = PathBuf::from(std::env::var_os("DICTURS_SCREENSHOTS")?);
        std::fs::create_dir_all(&dir).ok()?;
        let (saved_tx, saved_rx) = mpsc::channel();
        Some(Self {
            dir,
            step: 0,
            shown_at: None,
            requested: false,
            saved_tx,
            saved_rx,
        })
    }
}

impl DictursApp {
    /// Advances the tour by one frame.
    pub(super) fn screenshot_tour(&mut self, ctx: &egui::Context) {
        let Some(mut tour) = self.tour.take() else {
            return;
        };
        ctx.request_repaint();
        let Some(shot) = SHOTS.get(tour.step) else {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            self.tour = Some(tour);
            return;
        };
        match tour.shown_at {
            None => {
                self.editor = None;
                self.dry_run = false;
                self.fresh = false;
                self.log.clear();
                self.status.clear();
                self.page = shot.page;
                ctx.set_theme(shot.theme);
                (shot.setup)(self);
                tour.shown_at = Some(Instant::now());
            }
            Some(shown) if !tour.requested && shown.elapsed() >= SETTLE => {
                let path = tour.dir.join(shot.file);
                let tx = tour.saved_tx.clone();
                let repaint = ctx.clone();
                ctx.request_screenshot(move |image| {
                    let _ = tx.send(save_png(&path, &image).map(|()| path));
                    repaint.request_repaint();
                });
                tour.requested = true;
            }
            _ => {
                if let Ok(saved) = tour.saved_rx.try_recv() {
                    match saved {
                        Ok(path) => println!("saved {}", path.display()),
                        Err(e) => eprintln!("screenshot {} failed: {e}", shot.file),
                    }
                    tour.step += 1;
                    tour.shown_at = None;
                    tour.requested = false;
                }
            }
        }
        self.tour = Some(tour);
    }
}

fn save_png(path: &Path, image: &egui::ColorImage) -> Result<(), String> {
    let file = std::fs::File::create(path).map_err(|e| e.to_string())?;
    let mut encoder = png::Encoder::new(
        std::io::BufWriter::new(file),
        image.size[0] as u32,
        image.size[1] as u32,
    );
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let rgba: Vec<u8> = image
        .pixels
        .iter()
        .flat_map(|p| p.to_srgba_unmultiplied())
        .collect();
    encoder
        .write_header()
        .and_then(|mut w| w.write_image_data(&rgba))
        .map_err(|e| e.to_string())
}
