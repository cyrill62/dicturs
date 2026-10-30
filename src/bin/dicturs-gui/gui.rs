//! Desktop UI (egui): commands/categories, listening, settings, models and launchers.

use std::collections::HashMap;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;

use anyhow::{Result, anyhow};
use dicturs::config::{CATEGORY_SEP, Config, VoiceCommand};
use dicturs::listen::{ListenEvent, Listener};
use dicturs::{internal, launcher, matcher, models};
use eframe::egui::{self, Color32, RichText};

use crate::theme::{Mode, SystemTheme};

#[cfg(debug_assertions)]
mod screenshots;

const LOGO: &[u8] = include_bytes!("../../../assets/logo.svg");
const LOG_LINES: usize = 300;
/// Default UI scale (Ctrl +/- still zooms from there).
const ZOOM: f32 = 1.25;

pub fn run(cfg: Config, fresh: bool) -> Result<()> {
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("Dicturs")
        .with_app_id("dicturs")
        // Window sizes are in screen points, before the zoom: scaled so the layout keeps its room.
        .with_inner_size([1120.0 * ZOOM, 740.0 * ZOOM])
        .with_min_inner_size([760.0 * ZOOM, 480.0 * ZOOM]);
    if let Some(icon) = window_icon() {
        viewport = viewport.with_icon(icon);
    }
    #[cfg(debug_assertions)]
    if std::env::var_os("DICTURS_SCREENSHOTS").is_some() {
        viewport = screenshots::viewport(viewport);
    }
    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    eframe::run_native(
        "Dicturs",
        options,
        Box::new(move |cc| {
            egui_extras::install_image_loaders(&cc.egui_ctx);
            cc.egui_ctx.set_zoom_factor(ZOOM);
            Ok(Box::new(DictursApp::new(cfg, fresh, &cc.egui_ctx)))
        }),
    )
    .map_err(|e| anyhow!("{e}"))
}

/// The logo rasterized for the window/taskbar icon.
fn window_icon() -> Option<egui::IconData> {
    let image = egui_extras::image::load_svg_bytes_with_size(
        LOGO,
        egui::SizeHint::Width(128),
        &Default::default(),
    )
    .ok()?;
    Some(egui::IconData {
        rgba: image
            .pixels
            .iter()
            .flat_map(|p| p.to_srgba_unmultiplied())
            .collect(),
        width: image.size[0] as u32,
        height: image.size[1] as u32,
    })
}

#[derive(Clone, Copy, PartialEq)]
enum Page {
    Commands,
    Listen,
    Settings,
    Models,
    Launchers,
}

impl Page {
    const ALL: [Page; 5] = [
        Page::Commands,
        Page::Listen,
        Page::Settings,
        Page::Models,
        Page::Launchers,
    ];

    fn label(self) -> &'static str {
        match self {
            Page::Commands => "⌨  Commands",
            Page::Listen => "🎤  Listen",
            Page::Settings => "⚙  Settings",
            Page::Models => "📦  Models",
            Page::Launchers => "☰  Launchers",
        }
    }
}

#[derive(Clone, Copy)]
enum LogKind {
    Info,
    Heard,
    Matched,
    DryRun,
    Warn,
    Error,
}

/// New/edit command form.
struct Editor {
    id: Option<u32>,
    keyword: String,
    command: String,
    category: String,
    error: String,
}

/// What a click asked for, applied once the frame's widgets are drawn.
enum Action {
    ToggleCommand(u32),
    RunCommand(u32),
    EditCommand(u32),
    RemoveCommand(u32),
    ToggleCategory(String),
}

struct DictursApp {
    cfg: Config,
    /// First launch: offer to seed the sample commands.
    fresh: bool,
    page: Page,
    /// Selected category, `""` = all.
    category: String,
    search: String,
    editor: Option<Editor>,
    listener: Option<Listener>,
    listen_rx: Option<Receiver<ListenEvent>>,
    listen_state: String,
    level: f32,
    dry_run: bool,
    /// Effect of a built-in, applied once the listener events are processed.
    pending: Option<internal::Effect>,
    log: Vec<(LogKind, String)>,
    phrase: String,
    /// Edit buffers of the Settings page.
    settings: HashMap<&'static str, String>,
    downloading: Option<String>,
    bg_tx: Sender<(String, Result<(), String>)>,
    bg_rx: Receiver<(String, Result<(), String>)>,
    status: String,
    /// System dark/light mode, followed live.
    theme: SystemTheme,
    /// Screenshot tour (debug builds, `DICTURS_SCREENSHOTS=<dir>`).
    #[cfg(debug_assertions)]
    tour: Option<screenshots::Tour>,
}

impl DictursApp {
    fn new(cfg: Config, fresh: bool, ctx: &egui::Context) -> Self {
        let theme = SystemTheme::watch();
        apply_theme(ctx, theme.initial);
        let (bg_tx, bg_rx) = mpsc::channel();
        let listen_on_start = cfg.settings.listen_on_start;
        let mut app = Self {
            settings: HashMap::new(),
            cfg,
            fresh,
            page: Page::Commands,
            category: String::new(),
            search: String::new(),
            editor: None,
            listener: None,
            listen_rx: None,
            listen_state: "not listening".into(),
            level: 0.0,
            dry_run: false,
            pending: None,
            log: Vec::new(),
            phrase: String::new(),
            downloading: None,
            bg_tx,
            bg_rx,
            status: String::new(),
            theme,
            #[cfg(debug_assertions)]
            tour: screenshots::Tour::from_env(),
        };
        app.reset_settings();
        #[cfg(debug_assertions)]
        if app.tour.is_some() {
            // The microphone stays off during the tour.
            return app;
        }
        if listen_on_start {
            app.toggle_listening();
        }
        app
    }

    // ---------- state ----------

    fn save(&mut self) {
        if let Err(e) = self.cfg.save() {
            self.status = format!("✖ cannot save: {e:#}");
        }
    }

    fn push_log(&mut self, kind: LogKind, line: impl Into<String>) {
        self.log.push((kind, line.into()));
        if self.log.len() > LOG_LINES {
            self.log.remove(0);
        }
    }

    /// Processes background events; called every frame.
    fn tick(&mut self, ctx: &egui::Context) {
        if let Some(mode) = self.theme.try_recv() {
            #[cfg(debug_assertions)]
            let touring = self.tour.is_some();
            #[cfg(not(debug_assertions))]
            let touring = false;
            if !touring {
                apply_theme(ctx, mode);
            }
        }
        self.pump_listener();
        while let Ok((name, res)) = self.bg_rx.try_recv() {
            self.downloading = None;
            self.status = match res {
                Ok(()) => format!("model {name} downloaded"),
                Err(e) => format!("✖ download of {name} failed: {e}"),
            };
        }
        match self.pending.take() {
            Some(internal::Effect::StopListening) => self.toggle_listening(),
            Some(internal::Effect::RestartListening) => {
                self.toggle_listening();
                self.toggle_listening();
            }
            _ => {}
        }
        // Events arrive on std channels: poll them (faster while the level meter moves).
        let busy = self.listener.is_some() || self.downloading.is_some();
        ctx.request_repaint_after(Duration::from_millis(if busy { 50 } else { 250 }));
    }

    fn toggle_listening(&mut self) {
        if let Some(mut l) = self.listener.take() {
            l.stop();
            self.listen_rx = None;
            self.listen_state = "not listening".into();
            self.level = 0.0;
            self.push_log(LogKind::Info, "■ stopped");
            return;
        }
        let (tx, rx) = mpsc::channel();
        match Listener::start(&self.cfg, tx) {
            Ok(l) => {
                self.listener = Some(l);
                self.listen_rx = Some(rx);
            }
            Err(e) => {
                // Drain status messages emitted before the failure.
                while let Ok(ListenEvent::Status(s)) = rx.try_recv() {
                    self.push_log(LogKind::Info, s);
                }
                self.listen_state = "error".into();
                self.push_log(LogKind::Error, format!("✖ {e:#}"));
                self.status = format!("✖ cannot listen: {e:#}");
            }
        }
    }

    fn pump_listener(&mut self) {
        let Some(rx) = &self.listen_rx else { return };
        let events: Vec<ListenEvent> = rx.try_iter().collect();
        for ev in events {
            match ev {
                ListenEvent::Status(s) => {
                    self.listen_state = s.clone();
                    self.push_log(LogKind::Info, s);
                }
                ListenEvent::Level(l) => self.level = l,
                ListenEvent::SpeechStarted => self.listen_state = "speech…".into(),
                ListenEvent::Transcribing => self.listen_state = "transcribing…".into(),
                ListenEvent::Error(e) => self.push_log(LogKind::Error, format!("✖ {e}")),
                ListenEvent::Transcribed { text, took } => {
                    self.listen_state = "listening".into();
                    if text.is_empty() {
                        continue;
                    }
                    self.push_log(
                        LogKind::Heard,
                        format!("“{text}”  ({} ms)", took.as_millis()),
                    );
                    self.handle_transcription(&text);
                }
            }
        }
    }

    fn handle_transcription(&mut self, text: &str) {
        let m = match matcher::evaluate(&self.cfg, text) {
            matcher::Outcome::Matched(m) => m,
            other => {
                let reason = other.reason(&self.cfg);
                self.push_log(LogKind::Warn, format!("  {reason}"));
                return;
            }
        };
        let command = matcher::expand(&m.command.command, &m.args);
        let line = format!(
            "  ➡ {} ({:.0}%): {command}",
            m.command.keyword,
            m.score * 100.0
        );
        // Built-ins run even in dry-run mode, so "dry run" can turn it off.
        if self.dry_run && !internal::is_internal(&command) {
            self.push_log(LogKind::DryRun, format!("{line}  [dry-run]"));
            return;
        }
        match self.run_command(&command) {
            Ok(note) => self.push_log(LogKind::Matched, format!("{line}  [{note}]")),
            Err(e) => self.push_log(LogKind::Error, format!("{line}  ✖ {e:#}")),
        }
    }

    /// Runs an (expanded) command; built-ins act on the app. Returns a short note.
    fn run_command(&mut self, command: &str) -> Result<String> {
        if !internal::is_internal(command) {
            let pid = dicturs::executor::spawn_detached(&self.cfg.settings.shell, command)?;
            return Ok(format!("pid {pid}"));
        }
        let (effect, msg) = internal::run(&mut self.cfg, command)?;
        match effect {
            internal::Effect::ToggleDryRun => {
                self.dry_run = !self.dry_run;
                return Ok(format!(
                    "dry-run {}",
                    if self.dry_run { "on" } else { "off" }
                ));
            }
            internal::Effect::StopListening | internal::Effect::RestartListening
                if self.listener.is_some() =>
            {
                self.pending = Some(effect);
            }
            _ => {}
        }
        self.reset_settings();
        Ok(msg)
    }

    fn apply(&mut self, action: Action) {
        match action {
            Action::ToggleCommand(id) => {
                let _ = self.cfg.toggle_command(id);
                self.save();
            }
            Action::RunCommand(id) => {
                let Some(c) = self.cfg.commands.iter().find(|c| c.id == id) else {
                    return;
                };
                let command = matcher::expand(&c.command, "");
                let keyword = c.keyword.clone();
                self.status = match self.run_command(&command) {
                    Ok(note) => format!("ran “{keyword}” ({note})"),
                    Err(e) => format!("✖ “{keyword}” failed: {e:#}"),
                };
            }
            Action::EditCommand(id) => self.open_editor(Some(id)),
            Action::RemoveCommand(id) => {
                if let Ok(c) = self.cfg.remove_command(id) {
                    self.status = format!("removed “{}”", c.keyword);
                }
                self.save();
            }
            Action::ToggleCategory(path) => {
                if let Err(e) = self.cfg.toggle_category(&path) {
                    self.status = format!("✖ {e:#}");
                }
                self.save();
            }
        }
    }

    fn seed(&mut self, lang: &str) {
        let settings = self.cfg.settings.clone();
        self.cfg = Config::seeded((lang != "none").then_some(lang));
        self.cfg.settings = settings;
        self.fresh = false;
        self.category.clear();
        self.save();
        self.status = match lang {
            "none" => "built-in commands only — use “New command” to add yours".into(),
            _ => format!(
                "{} built-in commands (on) and {} samples (off) — tick the samples you want",
                internal::BUILTINS.len(),
                self.cfg.commands.len() - internal::BUILTINS.len()
            ),
        };
    }

    fn open_editor(&mut self, id: Option<u32>) {
        let found = id.and_then(|id| self.cfg.commands.iter().find(|c| c.id == id));
        self.editor = Some(match found {
            Some(c) => Editor {
                id,
                keyword: c.keyword.clone(),
                command: c.command.clone(),
                category: c.category.clone(),
                error: String::new(),
            },
            None => Editor {
                id: None,
                keyword: String::new(),
                command: String::new(),
                category: self.category.clone(),
                error: String::new(),
            },
        });
    }

    fn save_editor(&mut self) {
        let Some(ed) = &mut self.editor else { return };
        let res = match ed.id {
            Some(id) => self
                .cfg
                .update_command(id, Some(&ed.keyword), Some(&ed.command), Some(&ed.category))
                .map(|()| format!("updated “{}”", ed.keyword.trim())),
            None => self
                .cfg
                .add_command(&ed.keyword, &ed.command, &ed.category, true)
                .map(|id| format!("added #{id} “{}”", ed.keyword.trim())),
        };
        match res {
            Ok(msg) => {
                self.editor = None;
                self.save();
                self.status = msg;
            }
            Err(e) => ed.error = format!("{e:#}"),
        }
    }

    fn apply_settings(&mut self) {
        let mut errors = Vec::new();
        let mut changed = 0;
        for key in setting_keys() {
            let value = self.settings.get(key).cloned().unwrap_or_default();
            if self.cfg.get_setting(key).ok().as_deref() == Some(value.as_str()) {
                continue;
            }
            match self.cfg.set_setting(key, &value) {
                Ok(()) => changed += 1,
                Err(e) => errors.push(format!("{key}: {e:#}")),
            }
        }
        self.save();
        self.reset_settings();
        self.status = if errors.is_empty() {
            let restart = if self.listener.is_some() && changed > 0 {
                " — restart listening to apply"
            } else {
                ""
            };
            format!("{changed} setting(s) saved{restart}")
        } else {
            format!("✖ {}", errors.join(" · "))
        };
    }

    /// Puts the current (normalized) values back into the settings edit buffers.
    fn reset_settings(&mut self) {
        self.settings = setting_keys()
            .map(|k| (k, self.cfg.get_setting(k).unwrap_or_default()))
            .collect();
    }

    fn use_setting(&mut self, key: &str, value: &str) {
        self.status = match self.cfg.set_setting(key, value) {
            Ok(()) => {
                self.save();
                self.reset_settings();
                format!("{key} = {value}")
            }
            Err(e) => format!("✖ {e:#}"),
        };
    }

    fn download(&mut self, name: &str) {
        if let Some(d) = &self.downloading {
            self.status = format!("already downloading {d}");
            return;
        }
        self.downloading = Some(name.to_string());
        self.status = format!("downloading {name}…");
        let tx = self.bg_tx.clone();
        let name = name.to_string();
        std::thread::spawn(move || {
            let res = models::download(&name, true)
                .map(|_| ())
                .map_err(|e| format!("{e:#}"));
            let _ = tx.send((name, res));
        });
    }

    fn level_fraction(&self) -> f32 {
        // Speech threshold sits at 20% of the bar.
        let scale = (self.cfg.settings.vad_threshold * 5.0).max(0.01);
        (self.level / scale).min(1.0)
    }

    // ---------- views ----------

    fn sidebar(&mut self, ui: &mut egui::Ui) {
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.add(
                egui::Image::new(egui::include_image!("../../../assets/logo.svg"))
                    .fit_to_exact_size(egui::vec2(30.0, 30.0)),
            );
            ui.label(RichText::new("Dicturs").heading().strong());
        });
        ui.add_space(12.0);
        for page in Page::ALL {
            let label = RichText::new(page.label()).size(15.0);
            if ui
                .add_sized(
                    [ui.available_width(), 30.0],
                    egui::Button::selectable(self.page == page, label),
                )
                .clicked()
            {
                self.page = page;
            }
        }
    }

    fn header(&mut self, ui: &mut egui::Ui) {
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            let listening = self.listener.is_some();
            let (text, fill) = if listening {
                ("■  Stop listening", ui.visuals().error_fg_color)
            } else {
                ("🎤  Start listening", accent(ui))
            };
            if ui
                .add(
                    egui::Button::new(RichText::new(text).color(Color32::WHITE).strong())
                        .fill(fill)
                        .min_size(egui::vec2(150.0, 30.0)),
                )
                .clicked()
            {
                self.toggle_listening();
            }
            let color = if listening {
                success(ui)
            } else {
                ui.visuals().weak_text_color()
            };
            ui.label(
                RichText::new(format!(
                    "{} {}",
                    if listening { "⏺" } else { "○" },
                    self.listen_state
                ))
                .color(color),
            );
            if listening {
                ui.add(egui::ProgressBar::new(self.level_fraction()).desired_width(140.0));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let s = &self.cfg.settings;
                let active = self.cfg.active_commands().count();
                ui.label(
                    RichText::new(format!(
                        "{} · {} · {active} active",
                        s.whisper_model,
                        s.effective_language()
                    ))
                    .color(ui.visuals().weak_text_color()),
                );
                ui.checkbox(&mut self.dry_run, "Dry run")
                    .on_hover_text("Show matches without running them");
            });
        });
        ui.add_space(6.0);
    }

    fn status_bar(&mut self, ui: &mut egui::Ui) {
        let text = match &self.downloading {
            Some(d) => format!("⬇ downloading {d}… {}", self.status),
            None if self.status.is_empty() => "Ready".to_string(),
            None => self.status.clone(),
        };
        let color = if text.starts_with('✖') {
            ui.visuals().error_fg_color
        } else {
            ui.visuals().weak_text_color()
        };
        ui.label(RichText::new(text).small().color(color));
    }

    fn welcome(&mut self, ui: &mut egui::Ui) {
        let system = dicturs::config::system_language().unwrap_or_else(|| "?".into());
        card(ui, |ui| {
            ui.label(
                RichText::new("Welcome! Add the sample commands?")
                    .strong()
                    .size(16.0),
            );
            ui.label(
                RichText::new(
                    "Built-in commands (stop listening, change model…) are always added and on. \
                     Samples (apps, volume, media, …) start turned off: tick the ones you want.",
                )
                .color(ui.visuals().weak_text_color()),
            );
            ui.horizontal(|ui| {
                for (lang, label) in [
                    ("en", "English".to_string()),
                    ("fr", "Français".to_string()),
                    ("system", format!("System language ({system})")),
                    ("none", "No samples".to_string()),
                ] {
                    if ui.button(label).clicked() {
                        self.seed(lang);
                    }
                }
            });
        });
        ui.add_space(8.0);
    }

    fn categories(&mut self, ui: &mut egui::Ui) -> Option<Action> {
        let mut action = None;
        ui.label(
            RichText::new("CATEGORIES")
                .small()
                .color(ui.visuals().weak_text_color()),
        );
        if ui
            .selectable_label(
                self.category.is_empty(),
                format!("All ({})", self.cfg.commands.len()),
            )
            .clicked()
        {
            self.category.clear();
        }
        let categories = self.cfg.categories.clone();
        for c in &categories {
            let depth = c.path.matches(CATEGORY_SEP).count();
            let name = c.path.rsplit(CATEGORY_SEP).next().unwrap_or(&c.path);
            let count = self.cfg.commands_in(&c.path, true).count();
            let active = self.cfg.category_active(&c.path);
            ui.horizontal(|ui| {
                ui.add_space(depth as f32 * 14.0);
                let mut enabled = c.enabled;
                if ui
                    .checkbox(&mut enabled, "")
                    .on_hover_text("Enable or disable the whole category")
                    .changed()
                {
                    action = Some(Action::ToggleCategory(c.path.clone()));
                }
                let mut text = RichText::new(format!("{name} ({count})"));
                if !active {
                    text = text.color(ui.visuals().weak_text_color());
                }
                if ui.selectable_label(self.category == c.path, text).clicked() {
                    self.category = c.path.clone();
                }
            });
        }
        action
    }

    fn command_row(&self, ui: &mut egui::Ui, c: &VoiceCommand) -> Option<Action> {
        let mut action = None;
        let active = c.enabled && self.cfg.category_active(&c.category);
        ui.horizontal(|ui| {
            let mut enabled = c.enabled;
            if ui
                .checkbox(&mut enabled, "")
                .on_hover_text("Turn this command on or off")
                .changed()
            {
                action = Some(Action::ToggleCommand(c.id));
            }
            let text_width = (ui.available_width() - 260.0).max(120.0);
            let texts = ui
                .vertical(|ui| {
                    ui.set_width(text_width);
                    ui.horizontal(|ui| {
                        let mut keyword = RichText::new(&c.keyword).strong();
                        if !active {
                            keyword = keyword.color(ui.visuals().weak_text_color());
                        }
                        ui.label(keyword);
                        if internal::is_internal(&c.command) {
                            ui.label(RichText::new("built-in").small().color(accent(ui)));
                        }
                        if matcher::takes_args(&c.command) {
                            ui.label(RichText::new("args").small().color(success(ui)));
                        }
                    });
                    ui.add(
                        egui::Label::new(
                            RichText::new(&c.command)
                                .monospace()
                                .small()
                                .color(ui.visuals().weak_text_color()),
                        )
                        .truncate(),
                    );
                })
                .response;
            if ui
                .interact(
                    texts.rect,
                    ui.scope_id().with(("row", c.id)),
                    egui::Sense::click(),
                )
                .on_hover_text("Double-click to edit")
                .double_clicked()
            {
                action = Some(Action::EditCommand(c.id));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("🗑").on_hover_text("Remove").clicked() {
                    action = Some(Action::RemoveCommand(c.id));
                }
                if ui.small_button("✏").on_hover_text("Edit").clicked() {
                    action = Some(Action::EditCommand(c.id));
                }
                if ui.small_button("▶").on_hover_text("Run now").clicked() {
                    action = Some(Action::RunCommand(c.id));
                }
                if !c.category.is_empty() {
                    ui.label(
                        RichText::new(&c.category)
                            .small()
                            .color(ui.visuals().weak_text_color()),
                    );
                }
            });
        });
        action
    }

    fn commands_page(&mut self, ui: &mut egui::Ui) {
        let mut action = None;
        egui::Panel::left("categories")
            .resizable(true)
            .default_size(230.0)
            .show(ui, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| action = self.categories(ui));
            });
        egui::CentralPanel::default().show(ui, |ui| {
            if self.fresh {
                self.welcome(ui);
            }
            ui.horizontal(|ui| {
                if ui.add(egui::Button::new(RichText::new("➕  New command").color(Color32::WHITE).strong()).fill(accent(ui))).clicked() {
                    self.open_editor(None);
                }
                ui.add(egui::TextEdit::singleline(&mut self.search).hint_text("🔍 Filter commands…").desired_width(f32::INFINITY));
            });
            ui.add_space(6.0);
            let filter = matcher::normalize(&self.search);
            let commands: Vec<VoiceCommand> = self
                .cfg
                .commands_in(&self.category, true)
                .filter(|c| {
                    filter.is_empty() || matcher::normalize(&c.keyword).contains(&filter) || c.command.to_lowercase().contains(&filter)
                })
                .cloned()
                .collect();
            if commands.is_empty() {
                ui.add_space(24.0);
                ui.vertical_centered(|ui| {
                    ui.label(
                        RichText::new("No commands here. Click “New command”, or run `dicturs seed` to add samples.")
                            .color(ui.visuals().weak_text_color()),
                    );
                });
            }
            egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                for c in &commands {
                    if let Some(a) = self.command_row(ui, c) {
                        action = Some(a);
                    }
                    ui.separator();
                }
            });
        });
        if let Some(a) = action {
            self.apply(a);
        }
        self.editor_modal(ui);
    }

    fn editor_modal(&mut self, ui: &mut egui::Ui) {
        let Some(ed) = &mut self.editor else { return };
        let mut save = false;
        let mut cancel = false;
        let title = match ed.id {
            Some(id) => format!("Edit command #{id}"),
            None => "New command".to_string(),
        };
        let response = egui::Modal::new(egui::Id::unique("dicturs-command-editor")).show(ui.ctx(), |ui| {
            ui.set_width(560.0);
            ui.heading(title);
            ui.add_space(8.0);
            field(ui, "Keyword", "What you say", |ui| {
                let r = ui.add(egui::TextEdit::singleline(&mut ed.keyword).hint_text("open browser").desired_width(f32::INFINITY));
                if ed.keyword.is_empty() && ed.command.is_empty() {
                    r.request_focus();
                }
            });
            field(ui, "Command", "Run by your shell; {args} / {args_url} = words said after the keyword; @… = built-in", |ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut ed.command)
                        .hint_text("xdg-open https://duckduckgo.com")
                        .font(egui::TextStyle::Monospace)
                        .desired_width(f32::INFINITY),
                );
            });
            field(ui, "Category", "Path such as Apps/Web, created if needed", |ui| {
                ui.add(egui::TextEdit::singleline(&mut ed.category).hint_text("Apps/Web").desired_width(f32::INFINITY));
            });
            if !ed.error.is_empty() {
                ui.colored_label(ui.visuals().error_fg_color, format!("✖ {}", ed.error));
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    save = ui.add(egui::Button::new(RichText::new("✔ Save").color(Color32::WHITE).strong()).fill(accent(ui))).clicked();
                    cancel = ui.button("Cancel").clicked();
                });
            });
            save |= ui.input(|i| i.key_pressed(egui::Key::Enter));
        });
        if save {
            self.save_editor();
        } else if cancel || response.should_close() {
            self.editor = None;
        }
    }

    fn listen_page(&mut self, ui: &mut egui::Ui) {
        egui::CentralPanel::default().show(ui, |ui| {
            ui.horizontal(|ui| {
                let send = ui.button("▶ Try phrase").clicked();
                if ui.button("Clear").clicked() {
                    self.log.clear();
                }
                let r = ui.add(
                    egui::TextEdit::singleline(&mut self.phrase)
                        .hint_text("Type a phrase to test, e.g. “volume up”")
                        .desired_width(f32::INFINITY),
                );
                let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if (send || enter) && !self.phrase.trim().is_empty() {
                    let text = std::mem::take(&mut self.phrase);
                    self.push_log(LogKind::Heard, format!("“{text}”  (typed)"));
                    self.handle_transcription(&text);
                }
            });
            ui.add_space(6.0);
            card(ui, |ui| {
                egui::ScrollArea::vertical().auto_shrink(false).stick_to_bottom(true).show(ui, |ui| {
                    if self.log.is_empty() {
                        ui.label(
                            RichText::new("Start listening, then say a keyword. Transcriptions and matches appear here.")
                                .color(ui.visuals().weak_text_color()),
                        );
                    }
                    for (kind, line) in &self.log {
                        let v = ui.visuals();
                        let color = match kind {
                            LogKind::Info => v.weak_text_color(),
                            LogKind::Heard => v.strong_text_color(),
                            LogKind::Matched => success(ui),
                            LogKind::DryRun => accent(ui),
                            LogKind::Warn => v.warn_fg_color,
                            LogKind::Error => v.error_fg_color,
                        };
                        ui.label(RichText::new(line).monospace().color(color));
                    }
                });
            });
        });
    }

    fn settings_page(&mut self, ui: &mut egui::Ui) {
        egui::CentralPanel::default().show(ui, |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink(false)
                .show(ui, |ui| {
                    let mut on = self.cfg.settings.listen_on_start;
                    if ui
                        .checkbox(&mut on, RichText::new("Listen on start").strong())
                        .changed()
                    {
                        self.use_setting("listen_on_start", if on { "on" } else { "off" });
                    }
                    ui.label(
                        RichText::new("Start listening when the app opens")
                            .small()
                            .color(ui.visuals().weak_text_color()),
                    );
                    ui.add_space(8.0);
                    for key in setting_keys() {
                        let help = setting_help(key, &self.cfg);
                        let value = self.settings.entry(key).or_default();
                        field(ui, key, &help, |ui| {
                            ui.add(egui::TextEdit::singleline(value).desired_width(f32::INFINITY));
                        });
                    }
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        let path = Config::path()
                            .map(|p| p.display().to_string())
                            .unwrap_or_default();
                        ui.label(
                            RichText::new(format!("Stored in {path}"))
                                .small()
                                .color(ui.visuals().weak_text_color()),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui
                                .add(
                                    egui::Button::new(
                                        RichText::new("✔ Save").color(Color32::WHITE).strong(),
                                    )
                                    .fill(accent(ui)),
                                )
                                .clicked()
                            {
                                self.apply_settings();
                            }
                            if ui.button("Reset").clicked() {
                                self.reset_settings();
                            }
                        });
                    });
                });
        });
    }

    fn models_page(&mut self, ui: &mut egui::Ui) {
        egui::CentralPanel::default().show(ui, |ui| {
            let dir = models::models_dir().map(|p| p.display().to_string()).unwrap_or_default();
            ui.label(
                RichText::new(format!("whisper.cpp models, stored in {dir}. Changes apply the next time listening starts."))
                    .color(ui.visuals().weak_text_color()),
            );
            ui.add_space(8.0);
            egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                egui::Grid::new("models").num_columns(5).striped(true).spacing([16.0, 10.0]).show(ui, |ui| {
                    for m in models::MODELS {
                        let name = m.name;
                        let installed = models::is_installed(name);
                        let current = self.cfg.settings.whisper_model == name;
                        let downloading = self.downloading.as_deref() == Some(name);
                        ui.label(RichText::new(name).strong());
                        ui.label(format!("{} MB", m.size_mb));
                        ui.label(RichText::new(m.note).color(ui.visuals().weak_text_color()));
                        if downloading {
                            let done = models::partial_path(name).ok().and_then(|p| p.metadata().ok()).map_or(0, |m| m.len());
                            let fraction = done as f32 / (m.size_mb as f32 * 1_048_576.0);
                            ui.add(egui::ProgressBar::new(fraction.min(1.0)).desired_width(120.0).show_percentage());
                        } else if current && !installed {
                            ui.label(RichText::new("in use · not downloaded").color(ui.visuals().warn_fg_color).strong())
                                .on_hover_text("Listening needs this model: download it, or use a downloaded one.");
                        } else if current {
                            ui.label(RichText::new("in use").color(accent(ui)).strong());
                        } else if installed {
                            ui.label(RichText::new("downloaded").color(success(ui)));
                        } else {
                            ui.label("");
                        }
                        ui.horizontal(|ui| {
                            if !installed && !downloading && ui.button("⬇ Download").clicked() {
                                self.download(name);
                            }
                            if installed && !current {
                                if ui.button("Use").clicked() {
                                    self.use_setting("whisper_model", name);
                                }
                                if ui.small_button("🗑").on_hover_text("Remove").clicked() {
                                    self.status = match models::remove(name) {
                                        Ok(()) => format!("removed model {name}"),
                                        Err(e) => format!("✖ {e:#}"),
                                    };
                                }
                            }
                        });
                        ui.end_row();
                    }
                });
            });
        });
    }

    fn launchers_page(&mut self, ui: &mut egui::Ui) {
        egui::CentralPanel::default().show(ui, |ui| {
            ui.label(
                RichText::new("dmenu-style launchers used by `dicturs menu`. Bind `dicturs menu` (run) or `dicturs menu --toggle` in your compositor.")
                    .color(ui.visuals().weak_text_color()),
            );
            ui.add_space(8.0);
            egui::Grid::new("launchers").num_columns(4).striped(true).spacing([16.0, 10.0]).show(ui, |ui| {
                for l in launcher::LAUNCHERS {
                    let name = l.name;
                    let installed = launcher::is_installed(l);
                    let current = self.cfg.settings.launcher == name;
                    let session = match (l.wayland, l.x11) {
                        (true, true) => "Wayland · X11",
                        (true, false) => "Wayland",
                        _ => "X11",
                    };
                    ui.label(RichText::new(name).strong());
                    ui.label(RichText::new(session).color(ui.visuals().weak_text_color()));
                    if current {
                        ui.label(RichText::new("in use").color(accent(ui)).strong());
                    } else if installed {
                        ui.label(RichText::new("installed").color(success(ui)));
                    } else {
                        ui.label("");
                    }
                    ui.horizontal(|ui| {
                        if installed && !current && ui.button("Use").clicked() {
                            self.use_setting("launcher", name);
                        }
                        // No terminal for sudo's password here: hand the command over.
                        if !installed && ui.button("📋 Copy install command").clicked() {
                            self.status = match launcher::install_command(name) {
                                Ok(argv) => {
                                    let line = argv.join(" ");
                                    ui.ctx().copy_text(line.clone());
                                    format!("copied “{line}” — paste it in a terminal")
                                }
                                Err(e) => format!("✖ {e:#}"),
                            };
                        }
                    });
                    ui.end_row();
                }
            });
        });
    }
}

impl eframe::App for DictursApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        #[cfg(debug_assertions)]
        self.screenshot_tour(ui.ctx());
        self.tick(ui.ctx());
        egui::Panel::left("nav")
            .resizable(false)
            .exact_size(190.0)
            .show(ui, |ui| self.sidebar(ui));
        egui::Panel::top("header").show(ui, |ui| self.header(ui));
        egui::Panel::bottom("status").show(ui, |ui| self.status_bar(ui));
        match self.page {
            Page::Commands => self.commands_page(ui),
            Page::Listen => self.listen_page(ui),
            Page::Settings => self.settings_page(ui),
            Page::Models => self.models_page(ui),
            Page::Launchers => self.launchers_page(ui),
        }
    }
}

fn apply_theme(ctx: &egui::Context, mode: Mode) {
    ctx.set_theme(match mode {
        Mode::Dark => egui::ThemePreference::Dark,
        Mode::Light => egui::ThemePreference::Light,
        Mode::Unspecified => egui::ThemePreference::System,
    });
}

/// Brand violet, slightly lighter on dark backgrounds.
fn accent(ui: &egui::Ui) -> Color32 {
    if ui.visuals().dark_mode {
        Color32::from_rgb(139, 92, 246)
    } else {
        Color32::from_rgb(124, 58, 237)
    }
}

fn success(ui: &egui::Ui) -> Color32 {
    if ui.visuals().dark_mode {
        Color32::from_rgb(74, 222, 128)
    } else {
        Color32::from_rgb(22, 163, 74)
    }
}

fn card<R>(ui: &mut egui::Ui, add_contents: impl FnOnce(&mut egui::Ui) -> R) -> R {
    egui::Frame::group(ui.style())
        .inner_margin(12.0)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add_contents(ui)
        })
        .inner
}

/// A labelled form row.
fn field(ui: &mut egui::Ui, label: &str, help: &str, add_input: impl FnOnce(&mut egui::Ui)) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(label).strong());
        ui.label(
            RichText::new(help)
                .small()
                .color(ui.visuals().weak_text_color()),
        );
    });
    add_input(ui);
    ui.add_space(6.0);
}

/// Settings edited as text (listen_on_start is a checkbox).
fn setting_keys() -> impl Iterator<Item = &'static str> {
    Config::SETTING_KEYS
        .iter()
        .copied()
        .filter(|k| *k != "listen_on_start")
}

fn setting_help(key: &str, cfg: &Config) -> String {
    match key {
        "engine" => "whisper (built-in) or external".into(),
        "whisper_model" => "see the Models page".into(),
        "language" => format!(
            "system, auto or a code such as fr, en — now {}",
            cfg.settings.effective_language()
        ),
        "threads" => "CPU threads used by whisper".into(),
        "external_command" => "STT program for the external engine; {wav} = audio file".into(),
        "input_device" => {
            "microphone (dicturs devices); empty = default, e.g. alsa:pulse for PipeWire".into()
        }
        "vad_threshold" => "speech level 0..1".into(),
        "silence_ms" => "silence ending a phrase".into(),
        "max_utterance_ms" => "longest phrase".into(),
        "match_threshold" => "keyword similarity 0..1".into(),
        "wake_word" => "word required in every phrase; empty = none".into(),
        "launcher" => "see the Launchers page".into(),
        "shell" => "auto, sh, bash, zsh, fish, nu, powershell, cmd or a custom prefix".into(),
        _ => String::new(),
    }
}
