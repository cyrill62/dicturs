//! Terminal UI: commands/categories, listening, settings, launchers and models.

use std::collections::VecDeque;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

use anyhow::Result;
use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyEventKind,
    KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::DefaultTerminal;
use ratatui::layout::{Constraint, Flex, Layout, Position, Rect};
use ratatui::style::{Color, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, BorderType, Cell, Clear, Gauge, List, ListItem, ListState, Paragraph, Row, Table,
    TableState, Tabs, Wrap,
};

use dicturs::config::{CATEGORY_SEP, Config};
use dicturs::listen::{ListenEvent, Listener};
use dicturs::{executor, internal, launcher, matcher, models};

const TABS: &[&str] = &["Commands", "Listen", "Settings", "Launchers", "Models"];
const LANGUAGES: &[&str] = &[
    "system", "auto", "en", "fr", "de", "es", "it", "pt", "nl", "pl", "ru", "uk", "ja", "zh", "ko",
    "ar",
];
const MAX_LOG: usize = 200;
const DOUBLE_CLICK: Duration = Duration::from_millis(400);

type KeyHelp = (&'static str, &'static str);

/// Help sections; `Some(tab)` marks a tab-specific section (highlighted when on that tab).
const HELP: &[(&str, Option<Tab>, &[KeyHelp])] = &[
    (
        "General",
        None,
        &[
            ("1-5 · Tab", "switch tab"),
            ("L", "start / stop listening"),
            ("?", "this help"),
            ("q · Ctrl-C", "quit"),
        ],
    ),
    (
        "Commands",
        Some(Tab::Commands),
        &[
            ("a · n", "new command in selected category"),
            ("e · Enter", "edit the selected command"),
            ("space · t", "turn command / category on-off"),
            ("d · Del", "delete command or category"),
            ("x", "run the command now"),
            ("c", "new category (a/b = subcategory)"),
            ("r", "rename the selected category"),
            ("← → · h l", "focus categories / commands"),
            ("↑ ↓ · j k", "move"),
        ],
    ),
    (
        "Listen",
        Some(Tab::Listen),
        &[
            ("space · Enter", "start / stop listening"),
            ("d", "dry-run: show matches only"),
            ("c", "clear the log"),
        ],
    ),
    (
        "Settings",
        Some(Tab::Settings),
        &[("Enter · e", "edit the selected value")],
    ),
    (
        "Launchers",
        Some(Tab::Launchers),
        &[
            ("Enter · u", "use for `dicturs menu`"),
            ("i", "install via package manager"),
            ("t", "test the launcher"),
        ],
    ),
    (
        "Models",
        Some(Tab::Models),
        &[
            ("Enter · u", "use (downloads it if needed)"),
            ("g", "download"),
            ("d", "delete"),
        ],
    ),
    (
        "In commands",
        None,
        &[
            ("{args}", "words said after the keyword"),
            ("{args_url}", "same, URL-encoded"),
            ("@stop-listening", "built-in: stop listening"),
            ("@model {args}", "built-in: switch whisper model"),
            ("@language {args}", "built-in: switch language"),
            ("@dry-run", "built-in: toggle dry-run"),
        ],
    ),
    (
        "Mouse",
        None,
        &[
            ("click", "select · switch tab · press hint"),
            ("double-click", "edit · use · toggle a category"),
            ("right-click", "turn on / off"),
            ("wheel", "scroll"),
            ("click ○ / ●", "start / stop listening"),
        ],
    ),
    (
        "Legend",
        None,
        &[
            ("✔", "active"),
            ("◌", "on, but a parent category is off"),
            ("✘", "turned off"),
        ],
    ),
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Commands,
    Listen,
    Settings,
    Launchers,
    Models,
}

impl Tab {
    const ALL: [Tab; 5] = [
        Tab::Commands,
        Tab::Listen,
        Tab::Settings,
        Tab::Launchers,
        Tab::Models,
    ];
    fn index(self) -> usize {
        Self::ALL.iter().position(|t| *t == self).expect("tab")
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Focus {
    Tree,
    Commands,
}

/// Single-line text input.
#[derive(Clone, Default)]
struct Input {
    chars: Vec<char>,
    cursor: usize,
}

impl Input {
    fn new(s: &str) -> Self {
        let chars: Vec<char> = s.chars().collect();
        Self {
            cursor: chars.len(),
            chars,
        }
    }
    fn value(&self) -> String {
        self.chars.iter().collect()
    }
    fn handle(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.chars.drain(..self.cursor);
                self.cursor = 0;
            }
            KeyCode::Char(c) => {
                self.chars.insert(self.cursor, c);
                self.cursor += 1;
            }
            KeyCode::Backspace if self.cursor > 0 => {
                self.cursor -= 1;
                self.chars.remove(self.cursor);
            }
            KeyCode::Delete if self.cursor < self.chars.len() => {
                self.chars.remove(self.cursor);
            }
            KeyCode::Left => self.cursor = self.cursor.saturating_sub(1),
            KeyCode::Right => self.cursor = (self.cursor + 1).min(self.chars.len()),
            KeyCode::Home => self.cursor = 0,
            KeyCode::End => self.cursor = self.chars.len(),
            _ => {}
        }
    }
}

enum FormKind {
    AddCommand,
    EditCommand(u32),
    AddCategory,
    RenameCategory(String),
    Setting(String),
}

struct Form {
    title: String,
    kind: FormKind,
    labels: Vec<&'static str>,
    inputs: Vec<Input>,
    field: usize,
}

enum ChoiceKind {
    Setting(String),
    /// First launch: language of the sample commands.
    SeedSamples,
}

struct Choice {
    title: String,
    kind: ChoiceKind,
    /// (value, label)
    items: Vec<(String, String)>,
    state: ListState,
}

enum Popup {
    Form(Form),
    Choice(Choice),
    Confirm {
        message: String,
        action: ConfirmAction,
    },
    Help {
        scroll: u16,
    },
}

#[allow(clippy::enum_variant_names)]
enum ConfirmAction {
    DeleteCommand(u32),
    DeleteCategory(String),
    DeleteModel(String),
}

/// Work that needs the real terminal (sudo prompts, terminal launchers).
enum Suspend {
    InstallLauncher(String),
    TestLauncher(String),
}

/// Listener changes requested while handling its own events, applied after them.
enum Pending {
    StopListening,
    RestartListening,
}

enum Background {
    Downloaded(String, Result<(), String>),
}

/// Screen areas recorded while drawing, used to route mouse events.
#[derive(Default)]
struct Hitboxes {
    tabs: Vec<(Rect, Tab)>,
    mic: Rect,
    tree: Rect,
    commands: Rect,
    listen: Rect,
    settings: Rect,
    launchers: Rect,
    models: Rect,
    popup: Rect,
    /// Form fields with their horizontal scroll offset.
    popup_fields: Vec<(Rect, usize)>,
    hints: Vec<(Rect, KeyCode)>,
}

#[derive(Clone)]
struct LogLine {
    style: Style,
    text: String,
}

struct App {
    cfg: Config,
    tab: Tab,
    focus: Focus,
    tree: ListState,
    table: TableState,
    settings: ListState,
    launchers: TableState,
    models: TableState,
    popup: Option<Popup>,
    status: String,
    quit: bool,
    // listening
    listener: Option<Listener>,
    listen_rx: Option<Receiver<ListenEvent>>,
    level: f32,
    listen_state: String,
    dry_run: bool,
    log: VecDeque<LogLine>,
    // background tasks
    bg_tx: Sender<Background>,
    bg_rx: Receiver<Background>,
    downloading: Option<String>,
    suspend: Option<Suspend>,
    pending: Option<Pending>,
    // mouse
    hit: Hitboxes,
    last_click: Option<(Instant, Position)>,
}

pub fn run(cfg: Config, fresh: bool) -> Result<()> {
    let mut terminal = init_terminal();
    let mut app = App::new(cfg);
    if fresh {
        app.popup = Some(Popup::Choice(welcome_choice()));
    }
    let result = app.run(&mut terminal);
    restore_terminal();
    result
}

fn init_terminal() -> DefaultTerminal {
    let terminal = ratatui::init();
    let _ = crossterm::execute!(std::io::stdout(), EnableMouseCapture);
    terminal
}

fn restore_terminal() {
    let _ = crossterm::execute!(std::io::stdout(), DisableMouseCapture);
    ratatui::restore();
}

impl App {
    fn new(cfg: Config) -> Self {
        let (bg_tx, bg_rx) = mpsc::channel();
        let mut app = Self {
            cfg,
            tab: Tab::Commands,
            focus: Focus::Commands,
            tree: ListState::default().with_selected(Some(0)),
            table: TableState::default().with_selected(Some(0)),
            settings: ListState::default().with_selected(Some(0)),
            launchers: TableState::default().with_selected(Some(0)),
            models: TableState::default().with_selected(Some(0)),
            popup: None,
            status: String::new(),
            quit: false,
            listener: None,
            listen_rx: None,
            level: 0.0,
            listen_state: "stopped".into(),
            dry_run: false,
            log: VecDeque::new(),
            bg_tx,
            bg_rx,
            downloading: None,
            suspend: None,
            pending: None,
            hit: Hitboxes::default(),
            last_click: None,
        };
        app.clamp_selections();
        if app.cfg.settings.listen_on_start {
            app.toggle_listening();
        }
        app
    }

    fn run(&mut self, terminal: &mut DefaultTerminal) -> Result<()> {
        while !self.quit {
            terminal.draw(|f| self.draw(f))?;
            if event::poll(Duration::from_millis(50))? {
                match event::read()? {
                    Event::Key(key) if key.kind == KeyEventKind::Press => self.on_key(key),
                    Event::Mouse(m) => self.on_mouse(m),
                    _ => {}
                }
            }
            self.pump_listener();
            match self.pending.take() {
                Some(Pending::StopListening) => self.toggle_listening(),
                Some(Pending::RestartListening) => {
                    self.toggle_listening();
                    self.toggle_listening();
                }
                None => {}
            }
            self.pump_background();
            if let Some(s) = self.suspend.take() {
                restore_terminal();
                self.run_suspended(s);
                *terminal = init_terminal();
                terminal.clear()?;
            }
        }
        if let Some(mut l) = self.listener.take() {
            l.stop();
        }
        Ok(())
    }

    fn run_suspended(&mut self, s: Suspend) {
        let result = match &s {
            Suspend::InstallLauncher(name) => {
                launcher::install(name).map(|_| format!("{name} installed"))
            }
            Suspend::TestLauncher(name) => {
                let entries: Vec<String> = self
                    .cfg
                    .active_commands()
                    .map(|c| launcher::menu_entry(&self.cfg, c, false))
                    .collect();
                launcher::pick(name, "voice cmd", &entries).map(|i| {
                    format!(
                        "launcher returned: {}",
                        i.map(|i| entries[i].clone()).unwrap_or("nothing".into())
                    )
                })
            }
        };
        match result {
            Ok(msg) => self.status = msg,
            Err(e) => {
                self.status = format!("error: {e:#}");
                if matches!(s, Suspend::InstallLauncher(_)) {
                    eprintln!("{}", self.status);
                }
            }
        }
        if matches!(s, Suspend::InstallLauncher(_)) {
            println!("\nPress Enter to return to dicturs…");
            let _ = std::io::stdin().read_line(&mut String::new());
        }
    }

    fn save(&mut self) {
        if let Err(e) = self.cfg.save() {
            self.status = format!("save failed: {e:#}");
        }
    }

    fn push_log(&mut self, style: Style, text: impl Into<String>) {
        self.log.push_back(LogLine {
            style,
            text: text.into(),
        });
        while self.log.len() > MAX_LOG {
            self.log.pop_front();
        }
    }

    // ---------- data views ----------

    /// Category tree rows: (path, depth). Row 0 is "All".
    fn tree_rows(&self) -> Vec<(String, usize)> {
        let mut rows = vec![(String::new(), 0)];
        rows.extend(
            self.cfg
                .categories
                .iter()
                .map(|c| (c.path.clone(), c.path.matches(CATEGORY_SEP).count() + 1)),
        );
        rows
    }

    fn selected_category(&self) -> String {
        self.tree_rows()
            .get(self.tree.selected().unwrap_or(0))
            .map(|r| r.0.clone())
            .unwrap_or_default()
    }

    fn visible_command_ids(&self) -> Vec<u32> {
        let cat = self.selected_category();
        self.cfg.commands_in(&cat, true).map(|c| c.id).collect()
    }

    fn selected_command_id(&self) -> Option<u32> {
        self.visible_command_ids()
            .get(self.table.selected()?)
            .copied()
    }

    fn clamp_selections(&mut self) {
        let clamp = |sel: Option<usize>, len: usize| {
            if len == 0 {
                None
            } else {
                Some(sel.unwrap_or(0).min(len - 1))
            }
        };
        self.tree
            .select(clamp(self.tree.selected(), self.tree_rows().len()));
        self.table.select(clamp(
            self.table.selected(),
            self.visible_command_ids().len(),
        ));
        self.settings
            .select(clamp(self.settings.selected(), Config::SETTING_KEYS.len()));
        self.launchers
            .select(clamp(self.launchers.selected(), launcher::LAUNCHERS.len()));
        self.models
            .select(clamp(self.models.selected(), models::MODELS.len()));
    }

    // ---------- listening ----------

    fn toggle_listening(&mut self) {
        if let Some(mut l) = self.listener.take() {
            l.stop();
            self.listen_rx = None;
            self.listen_state = "stopped".into();
            self.level = 0.0;
            self.push_log(Style::new().dim(), "■ stopped");
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
                    self.push_log(Style::new().dim(), s);
                }
                self.listen_state = "error".into();
                self.push_log(Style::new().red(), format!("✘ {e:#}"));
                self.status = format!("cannot listen: {e:#}");
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
                    self.push_log(Style::new().dim(), s);
                }
                ListenEvent::Level(l) => self.level = l,
                ListenEvent::SpeechStarted => self.listen_state = "speech…".into(),
                ListenEvent::Transcribing => self.listen_state = "transcribing…".into(),
                ListenEvent::Error(e) => self.push_log(Style::new().red(), format!("✘ {e}")),
                ListenEvent::Transcribed { text, took } => {
                    self.listen_state = "listening".into();
                    if text.is_empty() {
                        continue;
                    }
                    self.push_log(Style::new(), format!("“{text}”  ({} ms)", took.as_millis()));
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
                self.push_log(Style::new().yellow(), format!("  {reason}"));
                return;
            }
        };
        let command = matcher::expand(&m.command.command, &m.args);
        let line = format!(
            "  ⇒ {} ({:.0}%): {command}",
            m.command.keyword,
            m.score * 100.0
        );
        // Built-ins run even in dry-run mode, so "dry run" can turn it off.
        if self.dry_run && !internal::is_internal(&command) {
            self.push_log(Style::new().cyan(), format!("{line}  [dry-run]"));
            return;
        }
        match self.run_command(&command) {
            Ok(note) => self.push_log(Style::new().green(), format!("{line}  [{note}]")),
            Err(e) => self.push_log(Style::new().red(), format!("{line}  ✘ {e:#}")),
        }
    }

    /// Runs an (expanded) command; built-ins act on the app. Returns a short note.
    fn run_command(&mut self, command: &str) -> Result<String> {
        if !internal::is_internal(command) {
            let pid = executor::spawn_detached(&self.cfg.settings.shell, command)?;
            return Ok(format!("pid {pid}"));
        }
        let (effect, msg) = internal::run(&mut self.cfg, command)?;
        match effect {
            internal::Effect::None => {}
            internal::Effect::StopListening => {
                if self.listener.is_some() {
                    self.pending = Some(Pending::StopListening);
                }
            }
            internal::Effect::ToggleDryRun => {
                self.dry_run = !self.dry_run;
                return Ok(format!(
                    "dry-run {}",
                    if self.dry_run { "on" } else { "off" }
                ));
            }
            internal::Effect::RestartListening => {
                if self.listener.is_some() {
                    self.pending = Some(Pending::RestartListening);
                }
            }
        }
        Ok(msg)
    }

    fn pump_background(&mut self) {
        while let Ok(ev) = self.bg_rx.try_recv() {
            match ev {
                Background::Downloaded(name, res) => {
                    self.downloading = None;
                    self.status = match res {
                        Ok(()) => format!("model {name} downloaded"),
                        Err(e) => format!("download of {name} failed: {e}"),
                    };
                }
            }
        }
    }

    // ---------- keys ----------

    // ---------- mouse ----------

    fn on_mouse(&mut self, m: MouseEvent) {
        let pos = Position::new(m.column, m.row);
        match m.kind {
            MouseEventKind::Down(button) => {
                let double = button == MouseButton::Left
                    && self
                        .last_click
                        .is_some_and(|(t, p)| p == pos && t.elapsed() < DOUBLE_CLICK);
                self.last_click = if double {
                    None
                } else {
                    Some((Instant::now(), pos))
                };
                if self.popup.is_some() {
                    self.on_popup_click(pos);
                } else {
                    self.on_click(pos, button, double);
                }
            }
            MouseEventKind::ScrollDown => self.on_scroll(pos, KeyCode::Down),
            MouseEventKind::ScrollUp => self.on_scroll(pos, KeyCode::Up),
            _ => {}
        }
        self.clamp_selections();
    }

    fn on_click(&mut self, pos: Position, button: MouseButton, double: bool) {
        let key = |code| KeyEvent::new(code, KeyModifiers::NONE);
        if let Some(&(_, code)) = self.hit.hints.iter().find(|(r, _)| r.contains(pos)) {
            self.on_key(key(code));
            return;
        }
        if let Some(&(_, tab)) = self.hit.tabs.iter().find(|(r, _)| r.contains(pos)) {
            self.tab = tab;
            return;
        }
        if self.hit.mic.contains(pos) {
            self.toggle_listening();
            return;
        }
        // Left: select (double: activate). Right: select + toggle.
        let action = match (button, double) {
            (MouseButton::Right, _) => Some(KeyCode::Char(' ')),
            (MouseButton::Left, true) => Some(KeyCode::Enter),
            (MouseButton::Left, false) => None,
            _ => return,
        };
        match self.tab {
            Tab::Commands => {
                if self.hit.tree.contains(pos) {
                    self.focus = Focus::Tree;
                    if let Some(row) = list_row(self.hit.tree, self.tree.offset(), pos.y) {
                        if row < self.tree_rows().len() && self.tree.selected() != Some(row) {
                            self.tree.select(Some(row));
                            self.table.select(Some(0));
                        }
                        if action.is_some() {
                            // Double-click on a category toggles it, like right-click.
                            self.on_commands_key(key(KeyCode::Char(' ')));
                        }
                    }
                } else if self.hit.commands.contains(pos) {
                    self.focus = Focus::Commands;
                    if let Some(row) = table_row(self.hit.commands, self.table.offset(), pos.y)
                        && row < self.visible_command_ids().len()
                    {
                        self.table.select(Some(row));
                        if let Some(code) = action {
                            self.on_commands_key(key(code));
                        }
                    }
                }
            }
            Tab::Listen => {
                if self.hit.listen.contains(pos) && button == MouseButton::Left {
                    self.toggle_listening();
                }
            }
            Tab::Settings => {
                if let Some(row) = list_row(self.hit.settings, self.settings.offset(), pos.y)
                    && self.hit.settings.contains(pos)
                    && row < Config::SETTING_KEYS.len()
                {
                    self.settings.select(Some(row));
                    if action.is_some() {
                        self.on_settings_key(key(KeyCode::Enter));
                    }
                }
            }
            Tab::Launchers => {
                if let Some(row) = table_row(self.hit.launchers, self.launchers.offset(), pos.y)
                    && self.hit.launchers.contains(pos)
                    && row < launcher::LAUNCHERS.len()
                {
                    self.launchers.select(Some(row));
                    if double {
                        self.on_launchers_key(key(KeyCode::Enter));
                    }
                }
            }
            Tab::Models => {
                if let Some(row) = table_row(self.hit.models, self.models.offset(), pos.y)
                    && self.hit.models.contains(pos)
                    && row < models::MODELS.len()
                {
                    self.models.select(Some(row));
                    if double {
                        self.on_models_key(key(KeyCode::Enter));
                    }
                }
            }
        }
    }

    fn on_popup_click(&mut self, pos: Position) {
        let key = |code| KeyEvent::new(code, KeyModifiers::NONE);
        let r = self.hit.popup;
        if !r.contains(pos) {
            self.popup = None; // click outside = cancel
            return;
        }
        match &mut self.popup {
            Some(Popup::Help { .. }) => self.popup = None,
            Some(Popup::Choice(choice)) => {
                if let Some(row) = list_row(r, choice.state.offset(), pos.y)
                    && row < choice.items.len()
                {
                    choice.state.select(Some(row));
                    self.on_popup_key(key(KeyCode::Enter));
                }
            }
            Some(Popup::Form(form)) => {
                if let Some(i) = self
                    .hit
                    .popup_fields
                    .iter()
                    .position(|(f, _)| f.contains(pos))
                {
                    let (field, offset) = self.hit.popup_fields[i];
                    form.field = i;
                    let input = &mut form.inputs[i];
                    let col = pos.x.saturating_sub(field.x + 1) as usize + offset;
                    input.cursor = col.min(input.chars.len());
                }
            }
            // Last inner line reads "[y] yes   [n] no".
            Some(Popup::Confirm { .. }) if pos.y == r.y + r.height.saturating_sub(2) => {
                match pos.x.saturating_sub(r.x + 1) {
                    0..=6 => self.on_popup_key(key(KeyCode::Char('y'))),
                    10..=15 => self.on_popup_key(key(KeyCode::Char('n'))),
                    _ => {}
                }
            }
            Some(Popup::Confirm { .. }) => {}
            None => {}
        }
    }

    fn on_scroll(&mut self, pos: Position, code: KeyCode) {
        let key = KeyEvent::new(code, KeyModifiers::NONE);
        if let Some(popup) = &self.popup {
            if matches!(popup, Popup::Choice(_) | Popup::Help { .. }) {
                self.on_popup_key(key);
            }
            return;
        }
        let delta = if code == KeyCode::Down { 1 } else { -1 };
        match self.tab {
            Tab::Commands if self.hit.tree.contains(pos) => {
                self.focus = Focus::Tree;
                self.on_commands_key(key);
            }
            Tab::Commands if self.hit.commands.contains(pos) => {
                let len = self.visible_command_ids().len();
                move_table(&mut self.table, len, delta);
            }
            Tab::Settings => self.on_settings_key(key),
            Tab::Launchers => self.on_launchers_key(key),
            Tab::Models => self.on_models_key(key),
            _ => {}
        }
    }

    fn on_key(&mut self, key: KeyEvent) {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.quit = true;
            return;
        }
        if self.popup.is_some() {
            self.on_popup_key(key);
            return;
        }
        match key.code {
            KeyCode::Char('q') => self.quit = true,
            KeyCode::Char('?') => self.popup = Some(Popup::Help { scroll: 0 }),
            KeyCode::Char(c @ '1'..='5') => self.tab = Tab::ALL[c as usize - '1' as usize],
            KeyCode::Tab => self.tab = Tab::ALL[(self.tab.index() + 1) % Tab::ALL.len()],
            KeyCode::BackTab => {
                self.tab = Tab::ALL[(self.tab.index() + Tab::ALL.len() - 1) % Tab::ALL.len()]
            }
            KeyCode::Char('L') => self.toggle_listening(),
            _ => match self.tab {
                Tab::Commands => self.on_commands_key(key),
                Tab::Listen => self.on_listen_key(key),
                Tab::Settings => self.on_settings_key(key),
                Tab::Launchers => self.on_launchers_key(key),
                Tab::Models => self.on_models_key(key),
            },
        }
        self.clamp_selections();
    }

    fn on_commands_key(&mut self, key: KeyEvent) {
        let tree_len = self.tree_rows().len();
        let table_len = self.visible_command_ids().len();
        match key.code {
            KeyCode::Left | KeyCode::Char('h') => self.focus = Focus::Tree,
            KeyCode::Right | KeyCode::Char('l') => self.focus = Focus::Commands,
            KeyCode::Down | KeyCode::Char('j') => match self.focus {
                Focus::Tree => {
                    move_sel(&mut self.tree, tree_len, 1);
                    self.table.select(Some(0));
                }
                Focus::Commands => move_table(&mut self.table, table_len, 1),
            },
            KeyCode::Up | KeyCode::Char('k') => match self.focus {
                Focus::Tree => {
                    move_sel(&mut self.tree, tree_len, -1);
                    self.table.select(Some(0));
                }
                Focus::Commands => move_table(&mut self.table, table_len, -1),
            },
            KeyCode::Char(' ') | KeyCode::Char('t') => match self.focus {
                Focus::Tree => {
                    let cat = self.selected_category();
                    if !cat.is_empty()
                        && let Ok(on) = self.cfg.toggle_category(&cat)
                    {
                        self.status =
                            format!("category {cat} {}", if on { "enabled" } else { "disabled" });
                        self.save();
                    }
                }
                Focus::Commands => {
                    if let Some(id) = self.selected_command_id()
                        && let Ok(on) = self.cfg.toggle_command(id)
                    {
                        self.status = format!("#{id} {}", if on { "enabled" } else { "disabled" });
                        self.save();
                    }
                }
            },
            KeyCode::Char('a') | KeyCode::Char('n') => {
                let cat = self.selected_category();
                self.popup = Some(Popup::Form(Form {
                    title: "New voice command".into(),
                    kind: FormKind::AddCommand,
                    labels: vec!["Keyword", "Command", "Category"],
                    inputs: vec![Input::default(), Input::default(), Input::new(&cat)],
                    field: 0,
                }));
            }
            KeyCode::Char('c') => {
                let cat = self.selected_category();
                let prefix = if cat.is_empty() {
                    cat
                } else {
                    format!("{cat}{CATEGORY_SEP}")
                };
                self.popup = Some(Popup::Form(Form {
                    title: "New category (use / for subcategories)".into(),
                    kind: FormKind::AddCategory,
                    labels: vec!["Path"],
                    inputs: vec![Input::new(&prefix)],
                    field: 0,
                }));
            }
            KeyCode::Char('r') if self.focus == Focus::Tree => {
                let cat = self.selected_category();
                if !cat.is_empty() {
                    self.popup = Some(Popup::Form(Form {
                        title: format!("Rename {cat}"),
                        kind: FormKind::RenameCategory(cat.clone()),
                        labels: vec!["New path"],
                        inputs: vec![Input::new(&cat)],
                        field: 0,
                    }));
                }
            }
            KeyCode::Enter | KeyCode::Char('e') if self.focus == Focus::Commands => {
                if let Some(c) = self
                    .selected_command_id()
                    .and_then(|id| self.cfg.commands.iter().find(|c| c.id == id))
                {
                    self.popup = Some(Popup::Form(Form {
                        title: format!("Edit #{}", c.id),
                        kind: FormKind::EditCommand(c.id),
                        labels: vec!["Keyword", "Command", "Category"],
                        inputs: vec![
                            Input::new(&c.keyword),
                            Input::new(&c.command),
                            Input::new(&c.category),
                        ],
                        field: 0,
                    }));
                }
            }
            KeyCode::Enter if self.focus == Focus::Tree => self.focus = Focus::Commands,
            KeyCode::Char('d') | KeyCode::Delete => match self.focus {
                Focus::Tree => {
                    let cat = self.selected_category();
                    if !cat.is_empty() {
                        self.popup = Some(Popup::Confirm {
                            message: format!(
                                "Delete category {cat} and its subcategories?\nCommands move to the parent."
                            ),
                            action: ConfirmAction::DeleteCategory(cat),
                        });
                    }
                }
                Focus::Commands => {
                    if let Some(c) = self
                        .selected_command_id()
                        .and_then(|id| self.cfg.commands.iter().find(|c| c.id == id))
                    {
                        self.popup = Some(Popup::Confirm {
                            message: format!("Delete “{}”?", c.keyword),
                            action: ConfirmAction::DeleteCommand(c.id),
                        });
                    }
                }
            },
            KeyCode::Char('x') if self.focus == Focus::Commands => {
                if let Some(c) = self
                    .selected_command_id()
                    .and_then(|id| self.cfg.commands.iter().find(|c| c.id == id))
                {
                    let command = matcher::expand(&c.command, "");
                    self.status = match self.run_command(&command) {
                        Ok(note) => format!("ran `{command}` ({note})"),
                        Err(e) => format!("error: {e:#}"),
                    };
                }
            }
            _ => {}
        }
    }

    fn on_listen_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char(' ') | KeyCode::Enter | KeyCode::Char('s') => self.toggle_listening(),
            KeyCode::Char('d') => {
                self.dry_run = !self.dry_run;
                self.status = format!("dry-run {}", if self.dry_run { "on" } else { "off" });
            }
            KeyCode::Char('c') => self.log.clear(),
            _ => {}
        }
    }

    fn on_settings_key(&mut self, key: KeyEvent) {
        let len = Config::SETTING_KEYS.len();
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => move_sel(&mut self.settings, len, 1),
            KeyCode::Up | KeyCode::Char('k') => move_sel(&mut self.settings, len, -1),
            KeyCode::Enter | KeyCode::Char('e') => {
                let key = Config::SETTING_KEYS[self.settings.selected().unwrap_or(0)].to_string();
                self.edit_setting(key);
            }
            _ => {}
        }
    }

    fn edit_setting(&mut self, key: String) {
        let current = self.cfg.get_setting(&key).unwrap_or_default();
        let items: Option<Vec<(String, String)>> = match key.as_str() {
            "engine" => Some(vec![
                ("whisper".into(), "whisper — built-in whisper.cpp".into()),
                (
                    "external".into(),
                    "external — any STT program (see external_command)".into(),
                ),
            ]),
            "whisper_model" => Some(
                models::MODELS
                    .iter()
                    .map(|m| {
                        let dl = if models::is_installed(m.name) {
                            "✔"
                        } else {
                            " "
                        };
                        (
                            m.name.to_string(),
                            format!("{dl} {:<22} {:>5} MB  {}", m.name, m.size_mb, m.note),
                        )
                    })
                    .collect(),
            ),
            "language" => Some(
                LANGUAGES
                    .iter()
                    .map(|l| {
                        let label = match *l {
                            "system" => format!(
                                "system — from your locale (→ {})",
                                dicturs::config::system_language().unwrap_or_else(|| "auto".into())
                            ),
                            "auto" => "auto — whisper detects the language".into(),
                            other => other.to_string(),
                        };
                        (l.to_string(), label)
                    })
                    .collect(),
            ),
            "launcher" => Some(
                launcher::LAUNCHERS
                    .iter()
                    .map(|l| {
                        let inst = if launcher::is_installed(l) {
                            "✔"
                        } else {
                            " "
                        };
                        (l.name.to_string(), format!("{inst} {}", l.name))
                    })
                    .collect(),
            ),
            "listen_on_start" => Some(vec![
                (
                    "on".into(),
                    "on — start listening when the TUI opens".into(),
                ),
                ("off".into(), "off — start manually with L".into()),
            ]),
            "shell" => Some(
                executor::KNOWN_SHELLS
                    .iter()
                    .map(|s| {
                        (
                            s.to_string(),
                            format!("{s:<5} → {}", executor::shell_argv(s).join(" ")),
                        )
                    })
                    .collect(),
            ),
            "input_device" => {
                let mut v = vec![(String::new(), "(system default)".to_string())];
                if let Ok(devs) = dicturs::audio::list_input_devices() {
                    v.extend(devs.into_iter().map(|d| {
                        let label = format!("{}  {}", d.id, d.description);
                        (d.id, label)
                    }));
                }
                Some(v)
            }
            _ => None,
        };
        match items {
            Some(mut items) => {
                if key != "engine" && key != "listen_on_start" {
                    items.push(("\u{0}custom".into(), "custom value…".into()));
                }
                let sel = items.iter().position(|(v, _)| *v == current).unwrap_or(0);
                self.popup = Some(Popup::Choice(Choice {
                    title: key.to_string(),
                    kind: ChoiceKind::Setting(key),
                    items,
                    state: ListState::default().with_selected(Some(sel)),
                }));
            }
            None => self.open_setting_form(key, &current),
        }
    }

    fn open_setting_form(&mut self, key: String, current: &str) {
        self.popup = Some(Popup::Form(Form {
            title: format!("Set {key}"),
            kind: FormKind::Setting(key),
            labels: vec!["Value"],
            inputs: vec![Input::new(current)],
            field: 0,
        }));
    }

    fn apply_setting(&mut self, key: &str, value: &str) {
        match self.cfg.set_setting(key, value) {
            Ok(()) => {
                self.save();
                self.status = format!("{key} = {value}");
                if self.listener.is_some() {
                    self.status
                        .push_str("  (restart listening with L to apply)");
                }
            }
            Err(e) => self.status = format!("error: {e:#}"),
        }
    }

    fn on_launchers_key(&mut self, key: KeyEvent) {
        let len = launcher::LAUNCHERS.len();
        let l = &launcher::LAUNCHERS[self.launchers.selected().unwrap_or(0).min(len - 1)];
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => move_table(&mut self.launchers, len, 1),
            KeyCode::Up | KeyCode::Char('k') => move_table(&mut self.launchers, len, -1),
            KeyCode::Char('i') => {
                if launcher::is_installed(l) {
                    self.status = format!("{} is already installed", l.name);
                } else {
                    match launcher::install_command(l.name) {
                        Ok(_) => self.suspend = Some(Suspend::InstallLauncher(l.name.to_string())),
                        Err(e) => self.status = format!("error: {e:#}"),
                    }
                }
            }
            KeyCode::Enter | KeyCode::Char('u') => self.apply_setting("launcher", l.name),
            KeyCode::Char('t') => self.suspend = Some(Suspend::TestLauncher(l.name.to_string())),
            _ => {}
        }
    }

    fn on_models_key(&mut self, key: KeyEvent) {
        let len = models::MODELS.len();
        let m = &models::MODELS[self.models.selected().unwrap_or(0).min(len - 1)];
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => move_table(&mut self.models, len, 1),
            KeyCode::Up | KeyCode::Char('k') => move_table(&mut self.models, len, -1),
            KeyCode::Char('D') | KeyCode::Char('g') => self.start_download(m.name),
            KeyCode::Enter | KeyCode::Char('u') => {
                self.apply_setting("whisper_model", m.name);
                if !models::is_installed(m.name) {
                    self.start_download(m.name);
                }
            }
            KeyCode::Char('d') | KeyCode::Delete if models::is_installed(m.name) => {
                self.popup = Some(Popup::Confirm {
                    message: format!("Delete model {}?", m.name),
                    action: ConfirmAction::DeleteModel(m.name.to_string()),
                });
            }
            _ => {}
        }
    }

    fn start_download(&mut self, name: &str) {
        if let Some(d) = &self.downloading {
            self.status = format!("already downloading {d}");
            return;
        }
        if models::is_installed(name) {
            self.status = format!("{name} is already downloaded");
            return;
        }
        self.downloading = Some(name.to_string());
        let tx = self.bg_tx.clone();
        let name = name.to_string();
        std::thread::spawn(move || {
            let res = models::download(&name, true)
                .map(|_| ())
                .map_err(|e| format!("{e:#}"));
            let _ = tx.send(Background::Downloaded(name, res));
        });
    }

    // ---------- popups ----------

    fn on_popup_key(&mut self, key: KeyEvent) {
        let Some(popup) = self.popup.take() else {
            return;
        };
        match popup {
            Popup::Help { scroll } => match key.code {
                KeyCode::Down | KeyCode::Char('j') => {
                    self.popup = Some(Popup::Help { scroll: scroll + 1 })
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    self.popup = Some(Popup::Help {
                        scroll: scroll.saturating_sub(1),
                    })
                }
                _ => {}
            },
            Popup::Confirm { message, action } => match key.code {
                KeyCode::Char('y') | KeyCode::Enter => self.confirm(action),
                KeyCode::Char('n') | KeyCode::Esc => {}
                _ => self.popup = Some(Popup::Confirm { message, action }),
            },
            Popup::Choice(mut choice) => match key.code {
                KeyCode::Esc => {}
                KeyCode::Down | KeyCode::Char('j') => {
                    move_sel(&mut choice.state, choice.items.len(), 1);
                    self.popup = Some(Popup::Choice(choice));
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    move_sel(&mut choice.state, choice.items.len(), -1);
                    self.popup = Some(Popup::Choice(choice));
                }
                KeyCode::Enter => {
                    let (value, _) = choice.items[choice.state.selected().unwrap_or(0)].clone();
                    let key = match choice.kind {
                        ChoiceKind::Setting(key) => key,
                        ChoiceKind::SeedSamples => {
                            self.seed_samples(&value);
                            return;
                        }
                    };
                    if value == "\u{0}custom" {
                        let current = self.cfg.get_setting(&key).unwrap_or_default();
                        self.open_setting_form(key, &current);
                    } else {
                        self.apply_setting(&key, &value);
                    }
                }
                _ => self.popup = Some(Popup::Choice(choice)),
            },
            Popup::Form(mut form) => match key.code {
                KeyCode::Esc => {}
                KeyCode::Tab | KeyCode::Down => {
                    form.field = (form.field + 1) % form.inputs.len();
                    self.popup = Some(Popup::Form(form));
                }
                KeyCode::BackTab | KeyCode::Up => {
                    form.field = (form.field + form.inputs.len() - 1) % form.inputs.len();
                    self.popup = Some(Popup::Form(form));
                }
                KeyCode::Enter => {
                    if let Err(e) = self.submit(&form) {
                        self.status = format!("error: {e:#}");
                        self.popup = Some(Popup::Form(form));
                    }
                }
                _ => {
                    form.inputs[form.field].handle(key);
                    self.popup = Some(Popup::Form(form));
                }
            },
        }
        self.clamp_selections();
    }

    /// First launch: replaces the default commands by samples in `lang` ("none" = empty).
    fn seed_samples(&mut self, lang: &str) {
        let settings = self.cfg.settings.clone();
        self.cfg = Config::seeded((lang != "none").then_some(lang));
        self.cfg.settings = settings;
        self.save();
        self.status = match lang {
            "none" => "built-in commands only — press a to add your own".into(),
            _ => format!(
                "{} built-in commands (on) and {} samples (off) — select a sample and press space to turn it on",
                dicturs::internal::BUILTINS.len(),
                self.cfg.commands.len() - dicturs::internal::BUILTINS.len()
            ),
        };
        self.tree.select(Some(0));
        self.table.select(Some(0));
        self.clamp_selections();
    }

    fn submit(&mut self, form: &Form) -> Result<()> {
        let v: Vec<String> = form.inputs.iter().map(Input::value).collect();
        match &form.kind {
            FormKind::AddCommand => {
                let id = self.cfg.add_command(&v[0], &v[1], &v[2], true)?;
                self.status = format!("added #{id}");
            }
            FormKind::EditCommand(id) => {
                self.cfg
                    .update_command(*id, Some(&v[0]), Some(&v[1]), Some(&v[2]))?;
                self.status = format!("updated #{id}");
            }
            FormKind::AddCategory => {
                let path = self.cfg.ensure_category(&v[0])?;
                self.status = format!("category {path} created");
            }
            FormKind::RenameCategory(from) => {
                self.cfg.rename_category(from, &v[0])?;
                self.status = format!("renamed {from} → {}", v[0]);
            }
            FormKind::Setting(key) => {
                self.cfg.set_setting(key, &v[0])?;
                self.status = format!("{key} = {}", v[0]);
            }
        }
        self.save();
        Ok(())
    }

    fn confirm(&mut self, action: ConfirmAction) {
        let res = match action {
            ConfirmAction::DeleteCommand(id) => self
                .cfg
                .remove_command(id)
                .map(|c| format!("deleted “{}”", c.keyword)),
            ConfirmAction::DeleteCategory(p) => {
                self.cfg.remove_category(&p).map(|_| format!("deleted {p}"))
            }
            ConfirmAction::DeleteModel(m) => {
                models::remove(&m).map(|_| format!("deleted model {m}"))
            }
        };
        match res {
            Ok(msg) => {
                self.status = msg;
                self.save();
            }
            Err(e) => self.status = format!("error: {e:#}"),
        }
    }

    // ---------- drawing ----------

    fn draw(&mut self, f: &mut ratatui::Frame) {
        let [header, body, hints, footer] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .areas(f.area());

        let listening = self.listener.is_some();
        let [tabs_area, state_area] =
            Layout::horizontal([Constraint::Min(0), Constraint::Length(28)]).areas(header);
        let titles: Vec<String> = TABS
            .iter()
            .enumerate()
            .map(|(i, t)| format!("{} {t}", i + 1))
            .collect();
        // Tabs renders " title " separated by a 1-column divider.
        self.hit.tabs.clear();
        let mut x = tabs_area.x;
        for (title, tab) in titles.iter().zip(Tab::ALL) {
            let w = title.chars().count() as u16 + 2;
            self.hit
                .tabs
                .push((Rect::new(x, tabs_area.y, w, 1).intersection(tabs_area), tab));
            x += w + 1;
        }
        self.hit.mic = state_area;
        f.render_widget(
            Tabs::new(titles)
                .select(self.tab.index())
                .highlight_style(Style::new().bold().reversed())
                .divider("│"),
            tabs_area,
        );
        let mic = if listening {
            Span::styled(
                format!("● {} ", self.listen_state),
                Style::new().green().bold(),
            )
        } else {
            Span::styled("○ not listening (L) ", Style::new().dim())
        };
        f.render_widget(Line::from(mic).right_aligned(), state_area);

        match self.tab {
            Tab::Commands => self.draw_commands(f, body),
            Tab::Listen => self.draw_listen(f, body),
            Tab::Settings => self.draw_settings(f, body),
            Tab::Launchers => self.draw_launchers(f, body),
            Tab::Models => self.draw_models(f, body),
        }

        let mut status = self.status.clone();
        if let Some(d) = &self.downloading {
            status = format!("⬇ downloading {d}… · {status}");
        }
        let version = format!("v{}", dicturs::VERSION);
        let [status_area, version_area] =
            Layout::horizontal([Constraint::Min(0), Constraint::Length(version.len() as u16 + 1)])
                .areas(footer);
        f.render_widget(
            Paragraph::new(status).style(Style::new().fg(Color::Gray)),
            status_area,
        );
        f.render_widget(
            Line::from(version).right_aligned().style(Style::new().fg(Color::DarkGray)),
            version_area,
        );
        self.draw_hints(f, hints);

        if let Some(popup) = &mut self.popup {
            draw_popup(f, popup, &mut self.hit, self.tab);
        }
    }

    /// Keys of the current tab: (key label, action label, key sent when clicked).
    fn hints(&self) -> Vec<(&'static str, &'static str, KeyCode)> {
        use KeyCode::{Char, Enter, Left, Right};
        let mut v = match (self.tab, self.focus) {
            (Tab::Commands, Focus::Tree) => vec![
                ("a", "new command", Char('a')),
                ("c", "new category", Char('c')),
                ("r", "rename", Char('r')),
                ("space", "on/off", Char(' ')),
                ("d", "delete", Char('d')),
                ("→", "commands", Right),
            ],
            (Tab::Commands, Focus::Commands) => vec![
                ("a", "new", Char('a')),
                ("e", "edit", Char('e')),
                ("space", "on/off", Char(' ')),
                ("x", "run", Char('x')),
                ("d", "delete", Char('d')),
                ("c", "new category", Char('c')),
                ("←", "categories", Left),
            ],
            (Tab::Listen, _) => vec![
                (
                    "space",
                    if self.listener.is_some() {
                        "stop"
                    } else {
                        "start"
                    },
                    Char(' '),
                ),
                (
                    "d",
                    if self.dry_run {
                        "dry-run: on"
                    } else {
                        "dry-run: off"
                    },
                    Char('d'),
                ),
                ("c", "clear log", Char('c')),
            ],
            (Tab::Settings, _) => vec![("enter", "edit", Enter)],
            (Tab::Launchers, _) => vec![
                ("enter", "use", Enter),
                ("i", "install", Char('i')),
                ("t", "test", Char('t')),
            ],
            (Tab::Models, _) => vec![
                ("enter", "use", Enter),
                ("g", "download", Char('g')),
                ("d", "delete", Char('d')),
            ],
        };
        if self.tab != Tab::Listen {
            v.push((
                "L",
                if self.listener.is_some() {
                    "stop listening"
                } else {
                    "listen"
                },
                Char('L'),
            ));
        }
        v.push(("?", "help", Char('?')));
        v.push(("q", "quit", Char('q')));
        v
    }

    fn draw_hints(&mut self, f: &mut ratatui::Frame, area: Rect) {
        self.hit.hints.clear();
        let key_style = Style::new().fg(Color::Black).bg(Color::Cyan).bold();
        let mut spans = Vec::new();
        let mut x = area.x;
        for (k, label, code) in self.hints() {
            let key = format!(" {k} ");
            let text = format!(" {label}  ");
            let w = (key.chars().count() + text.chars().count()) as u16;
            if x + w > area.x + area.width {
                break;
            }
            self.hit.hints.push((Rect::new(x, area.y, w - 2, 1), code));
            x += w;
            spans.push(Span::styled(key, key_style));
            spans.push(Span::raw(text));
        }
        f.render_widget(Line::from(spans), area);
    }

    fn draw_commands(&mut self, f: &mut ratatui::Frame, area: Rect) {
        let [left, right] =
            Layout::horizontal([Constraint::Percentage(28), Constraint::Min(0)]).areas(area);

        let rows = self.tree_rows();
        let items: Vec<ListItem> = rows
            .iter()
            .map(|(path, depth)| {
                let total = self.cfg.commands_in(path, true).count();
                if path.is_empty() {
                    return ListItem::new(format!("◆ All  ({total})"));
                }
                let name = path.rsplit(CATEGORY_SEP).next().unwrap_or(path);
                let own = self.cfg.category(path).is_some_and(|c| c.enabled);
                let active = self.cfg.category_active(path);
                let (mark, style) = match (own, active) {
                    (true, true) => ("✔", Style::new()),
                    (true, false) => ("◌", Style::new().dim()),
                    _ => ("✘", Style::new().dim().crossed_out()),
                };
                ListItem::new(Line::from(vec![
                    Span::raw("  ".repeat(depth.saturating_sub(1))),
                    Span::raw(format!("{mark} ")),
                    Span::styled(format!("{name}  ({total})"), style),
                ]))
            })
            .collect();
        let tree = List::new(items)
            .block(panel("Categories", self.focus == Focus::Tree))
            .highlight_style(Style::new().reversed())
            .highlight_symbol("› ");
        self.hit.tree = left;
        f.render_stateful_widget(tree, left, &mut self.tree);

        let cat = self.selected_category();
        let rows: Vec<Row> = self
            .cfg
            .commands_in(&cat, true)
            .map(|c| {
                let active = c.enabled && self.cfg.category_active(&c.category);
                let (mark, style) = match (c.enabled, active) {
                    (true, true) => (Span::styled("✔", Style::new().green()), Style::new()),
                    (true, false) => (Span::styled("◌", Style::new().yellow()), Style::new().dim()),
                    _ => (Span::styled("✘", Style::new().red()), Style::new().dim()),
                };
                Row::new(vec![
                    Cell::from(mark),
                    Cell::from(c.keyword.clone()).style(style.bold()),
                    Cell::from(c.command.clone()).style(style),
                    Cell::from(c.category.clone()).style(Style::new().dim()),
                ])
            })
            .collect();
        let title = if cat.is_empty() {
            "Commands".to_string()
        } else {
            format!("Commands — {cat}")
        };
        let table = Table::new(
            rows,
            [
                Constraint::Length(2),
                Constraint::Percentage(28),
                Constraint::Percentage(47),
                Constraint::Min(8),
            ],
        )
        .header(
            Row::new(["", "Keyword", "Command", "Category"])
                .style(Style::new().bold().underlined()),
        )
        .block(panel(&title, self.focus == Focus::Commands))
        .row_highlight_style(Style::new().reversed());
        self.hit.commands = right;
        let empty = self.visible_command_ids().is_empty();
        f.render_stateful_widget(table, right, &mut self.table);
        if empty {
            let where_ = if cat.is_empty() {
                String::new()
            } else {
                format!(" in {cat}")
            };
            let msg = vec![
                Line::from(format!("No commands{where_} yet.")),
                Line::from(""),
                Line::from(vec![
                    "Press ".into(),
                    Span::styled(" a ", Style::new().fg(Color::Black).bg(Color::Cyan).bold()),
                    " or click ".into(),
                    "a new".cyan().bold(),
                    " below to add one.".into(),
                ]),
            ];
            let inner = Rect {
                y: right.y + 3,
                height: right.height.saturating_sub(4),
                ..right
            };
            f.render_widget(
                Paragraph::new(msg).centered().style(Style::new().dim()),
                inner,
            );
        }
    }

    fn draw_listen(&mut self, f: &mut ratatui::Frame, area: Rect) {
        let [info, gauge, log] = Layout::vertical([
            Constraint::Length(4),
            Constraint::Length(3),
            Constraint::Min(0),
        ])
        .areas(area);
        self.hit.listen = info.union(gauge);
        let s = &self.cfg.settings;
        let engine = match s.engine {
            dicturs::config::EngineKind::Whisper => format!("whisper ({})", s.whisper_model),
            dicturs::config::EngineKind::External => format!("external: {}", s.external_command),
        };
        let wake = if s.wake_word.is_empty() {
            "-".into()
        } else {
            s.wake_word.clone()
        };
        let lines = vec![
            Line::from(vec![
                "Engine ".dim(),
                engine.into(),
                "   Language ".dim(),
                s.effective_language().into(),
                "   Wake word ".dim(),
                wake.into(),
            ]),
            Line::from(vec![
                "Active commands ".dim(),
                self.cfg.active_commands().count().to_string().into(),
                "   Dry-run ".dim(),
                if self.dry_run {
                    "on".yellow()
                } else {
                    "off".into()
                },
                "   Shell ".dim(),
                executor::shell_argv(&s.shell)[0].clone().into(),
            ]),
        ];
        f.render_widget(
            Paragraph::new(lines).block(panel(
                "Listen  [space/click] start/stop · [d] dry-run · [c] clear",
                true,
            )),
            info,
        );

        let threshold = s.vad_threshold;
        let ratio = (self.level / (threshold * 4.0)).clamp(0.0, 1.0) as f64;
        let color = if self.level >= threshold {
            Color::Green
        } else {
            Color::DarkGray
        };
        f.render_widget(
            Gauge::default()
                .block(panel("Input level", false))
                .gauge_style(Style::new().fg(color))
                .label(format!("{:.3} (threshold {threshold})", self.level))
                .ratio(ratio),
            gauge,
        );

        let height = log.height.saturating_sub(2) as usize;
        let lines: Vec<Line> = self
            .log
            .iter()
            .skip(self.log.len().saturating_sub(height))
            .map(|l| Line::styled(l.text.clone(), l.style))
            .collect();
        f.render_widget(Paragraph::new(lines).block(panel("Log", false)), log);
    }

    fn draw_settings(&mut self, f: &mut ratatui::Frame, area: Rect) {
        let [list_area, help_area] =
            Layout::vertical([Constraint::Min(0), Constraint::Length(5)]).areas(area);
        self.hit.settings = list_area;
        let items: Vec<ListItem> = Config::SETTING_KEYS
            .iter()
            .map(|k| {
                let v = self.cfg.get_setting(k).unwrap_or_default();
                let extra = match *k {
                    "language" if v == "system" => {
                        format!("  → {}", self.cfg.settings.effective_language())
                    }
                    "shell" => format!("  → {}", executor::shell_argv(&v).join(" ")),
                    "whisper_model" if !models::is_installed(&v) => "  (not downloaded)".into(),
                    "input_device" if v.is_empty() => "(system default)".into(),
                    _ => String::new(),
                };
                ListItem::new(Line::from(vec![
                    Span::styled(format!("{k:<18}"), Style::new().bold()),
                    Span::raw(v),
                    Span::styled(extra, Style::new().dim()),
                ]))
            })
            .collect();
        f.render_stateful_widget(
            List::new(items)
                .block(panel("Settings  [enter] edit", true))
                .highlight_style(Style::new().reversed())
                .highlight_symbol("› "),
            list_area,
            &mut self.settings,
        );
        let key = Config::SETTING_KEYS[self.settings.selected().unwrap_or(0)];
        f.render_widget(
            Paragraph::new(setting_help(key))
                .wrap(Wrap { trim: true })
                .block(panel("Help", false)),
            help_area,
        );
    }

    fn draw_launchers(&mut self, f: &mut ratatui::Frame, area: Rect) {
        let pm = launcher::detect_package_manager();
        let rows: Vec<Row> = launcher::LAUNCHERS
            .iter()
            .map(|l| {
                let current = if self.cfg.settings.launcher == l.name {
                    "★"
                } else {
                    ""
                };
                let installed = if launcher::is_installed(l) {
                    Span::styled("installed", Style::new().green())
                } else {
                    Span::styled("missing", Style::new().dim())
                };
                let session = match (l.wayland, l.x11) {
                    (true, true) => "wayland + x11",
                    (true, false) => "wayland",
                    _ => "x11",
                };
                let pkg = pm
                    .and_then(|pm| launcher::package_name(l.name, pm.name))
                    .unwrap_or("(not packaged)");
                Row::new(vec![
                    Cell::from(current),
                    Cell::from(l.name),
                    Cell::from(installed),
                    Cell::from(session),
                    Cell::from(pkg),
                ])
            })
            .collect();
        let title = format!(
            "Launchers ({})  [enter] use · [i] install · [t] test",
            pm.map(|p| p.name).unwrap_or("no package manager")
        );
        let table = Table::new(
            rows,
            [
                Constraint::Length(2),
                Constraint::Length(10),
                Constraint::Length(11),
                Constraint::Length(15),
                Constraint::Min(10),
            ],
        )
        .header(
            Row::new(["", "Name", "State", "Session", "Package"])
                .style(Style::new().bold().underlined()),
        )
        .block(panel(&title, true))
        .row_highlight_style(Style::new().reversed());
        let [table_area, hint] =
            Layout::vertical([Constraint::Min(0), Constraint::Length(4)]).areas(area);
        self.hit.launchers = table_area;
        f.render_stateful_widget(table, table_area, &mut self.launchers);
        f.render_widget(
            Paragraph::new(vec![
                Line::from("Bind `dicturs menu` to a key in your compositor to pick a command with the launcher,"),
                Line::from("`dicturs menu --toggle` to enable/disable commands. Custom launcher: settings › launcher › custom."),
            ])
            .block(panel("Integration", false))
            .style(Style::new().dim()),
            hint,
        );
    }

    fn draw_models(&mut self, f: &mut ratatui::Frame, area: Rect) {
        let rows: Vec<Row> = models::MODELS
            .iter()
            .map(|m| {
                let current = if self.cfg.settings.whisper_model == m.name {
                    "★"
                } else {
                    ""
                };
                let state = if self.downloading.as_deref() == Some(m.name) {
                    let done = models::partial_path(m.name)
                        .ok()
                        .and_then(|p| std::fs::metadata(p).ok())
                        .map(|md| md.len() as f64 / (m.size_mb as f64 * 1_048_576.0) * 100.0)
                        .unwrap_or(0.0);
                    Span::styled(format!("⬇ {done:.0}%"), Style::new().yellow())
                } else if models::is_installed(m.name) {
                    Span::styled("downloaded", Style::new().green())
                } else {
                    Span::styled("-", Style::new().dim())
                };
                Row::new(vec![
                    Cell::from(current),
                    Cell::from(m.name),
                    Cell::from(format!("{:>5} MB", m.size_mb)),
                    Cell::from(state),
                    Cell::from(m.note).style(Style::new().dim()),
                ])
            })
            .collect();
        let table = Table::new(
            rows,
            [
                Constraint::Length(2),
                Constraint::Length(22),
                Constraint::Length(9),
                Constraint::Length(11),
                Constraint::Min(10),
            ],
        )
        .header(
            Row::new(["", "Model", "Size", "State", ""]).style(Style::new().bold().underlined()),
        )
        .block(panel(
            "Whisper models  [enter] use · [g] download · [d] delete",
            true,
        ))
        .row_highlight_style(Style::new().reversed());
        self.hit.models = area;
        f.render_stateful_widget(table, area, &mut self.models);
    }
}

fn welcome_choice() -> Choice {
    let system = dicturs::config::system_language().unwrap_or_else(|| "?".into());
    let resolved = dicturs::config::sample_language("system");
    let system_label = if system == resolved {
        format!("System language (→ {system})")
    } else {
        format!("System language (→ {system}, samples in {resolved})")
    };
    Choice {
        title: "Welcome! Sample commands language (all start turned off)".into(),
        kind: ChoiceKind::SeedSamples,
        items: vec![
            ("en".into(), "English".into()),
            ("fr".into(), "Français".into()),
            ("system".into(), system_label),
            (
                "none".into(),
                "No samples (built-in commands only, English)".into(),
            ),
        ],
        state: ListState::default().with_selected(Some(0)),
    }
}

fn setting_help(key: &str) -> &'static str {
    match key {
        "engine" => {
            "whisper: built-in whisper.cpp (offline). external: run any speech-to-text program on a WAV file."
        }
        "whisper_model" => {
            "Bigger models are more accurate but slower. 'base' or 'small' work well for short commands."
        }
        "language" => {
            "system: taken from your locale (LC_ALL / LC_MESSAGES / LANG). auto: whisper guesses. Or a code: fr, en, de…"
        }
        "threads" => "CPU threads used by whisper.",
        "external_command" => {
            "Command printing the transcription on stdout. {wav} is replaced by a 16 kHz mono WAV file."
        }
        "input_device" => {
            "Microphone used for listening (empty = system default). See also `dicturs devices`."
        }
        "vad_threshold" => {
            "Input level considered as speech. Watch the level gauge in the Listen tab to tune it."
        }
        "silence_ms" => "Silence that ends an utterance, in milliseconds.",
        "max_utterance_ms" => "Utterances are cut after this duration.",
        "match_threshold" => {
            "Fuzzy matching tolerance between the transcription and the keyword (1.0 = exact)."
        }
        "wake_word" => {
            "If set, commands only fire when the phrase contains this word first (e.g. 'jarvis volume up')."
        }
        "launcher" => {
            "Launcher used by `dicturs menu`: wofi, rofi, fuzzel… or a custom dmenu-like command ({prompt} is replaced)."
        }
        "listen_on_start" => {
            "Start listening as soon as the TUI opens (otherwise press L or click the ○ indicator)."
        }
        "shell" => {
            "Shell running commands, so your aliases work. auto = $SHELL. nu loads config.nu, env.nu and autoload/*.nu."
        }
        _ => "",
    }
}

fn panel(title: &str, focused: bool) -> Block<'static> {
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .title(format!(" {title} "));
    if focused {
        block.border_style(Style::new().cyan())
    } else {
        block.border_style(Style::new().dark_gray())
    }
}

fn move_sel(state: &mut ListState, len: usize, delta: i32) {
    if len == 0 {
        return;
    }
    let cur = state.selected().unwrap_or(0) as i32;
    state.select(Some((cur + delta).rem_euclid(len as i32) as usize));
}

fn move_table(state: &mut TableState, len: usize, delta: i32) {
    if len == 0 {
        return;
    }
    let cur = state.selected().unwrap_or(0) as i32;
    state.select(Some((cur + delta).rem_euclid(len as i32) as usize));
}

fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let [v] = Layout::vertical([Constraint::Length(height)])
        .flex(Flex::Center)
        .areas(area);
    let [h] = Layout::horizontal([Constraint::Length(width)])
        .flex(Flex::Center)
        .areas(v);
    h
}

/// Row index under `y` in a bordered list, `None` on borders.
fn list_row(area: Rect, offset: usize, y: u16) -> Option<usize> {
    let top = area.y + 1;
    (y >= top && y + 1 < area.y + area.height).then(|| offset + (y - top) as usize)
}

/// Row index under `y` in a bordered table with a one-line header.
fn table_row(area: Rect, offset: usize, y: u16) -> Option<usize> {
    let top = area.y + 2;
    (y >= top && y + 1 < area.y + area.height).then(|| offset + (y - top) as usize)
}

fn draw_popup(f: &mut ratatui::Frame, popup: &mut Popup, hit: &mut Hitboxes, current_tab: Tab) {
    let area = f.area();
    hit.popup_fields.clear();
    match popup {
        Popup::Help { scroll } => {
            // Each section: title line, key lines, blank separator.
            let section_lines = |(title, tab, keys): &(&str, Option<Tab>, &[KeyHelp])| {
                let current = tab.is_some_and(|t| t == current_tab);
                let title_style = if current {
                    Style::new().fg(Color::Black).bg(Color::Cyan).bold()
                } else {
                    Style::new().cyan().bold()
                };
                let suffix = if current { "  ← this tab" } else { "" };
                let mut lines = vec![Line::from(vec![
                    Span::styled(format!(" {title} "), title_style),
                    Span::styled(suffix, Style::new().dim()),
                ])];
                lines.extend(keys.iter().map(|(k, d)| {
                    Line::from(vec![
                        Span::styled(format!("  {k:<15}"), Style::new().yellow().bold()),
                        Span::raw(*d),
                    ])
                }));
                lines.push(Line::from(""));
                lines
            };
            // The current tab's section comes right after "General".
            let mut sections: Vec<_> = HELP.iter().collect();
            if let Some(i) = sections.iter().position(|s| s.1 == Some(current_tab)) {
                let s = sections.remove(i);
                sections.insert(1, s);
            }
            let blocks: Vec<Vec<Line>> = sections.into_iter().map(section_lines).collect();
            let total: usize = blocks.iter().map(Vec::len).sum();

            let two_columns = area.width >= 110;
            let columns: Vec<Vec<Line>> = if two_columns {
                // Fill the left column up to half the lines, keeping sections whole.
                let (mut left, mut right) = (Vec::new(), Vec::new());
                for b in blocks {
                    if left.len() + b.len() / 2 <= total / 2 {
                        left.extend(b)
                    } else {
                        right.extend(b)
                    }
                }
                vec![left, right]
            } else {
                vec![blocks.into_iter().flatten().collect()]
            };
            let content_h = columns.iter().map(Vec::len).max().unwrap_or(0) as u16;
            let width = if two_columns { 108 } else { 64.min(area.width) };
            let r = centered(
                area,
                width,
                (content_h + 2).min(area.height.saturating_sub(2)),
            );
            hit.popup = r;
            let max_scroll = (content_h + 2).saturating_sub(r.height);
            *scroll = (*scroll).min(max_scroll);
            let footer = if max_scroll > 0 {
                " ↑↓ scroll · any key closes "
            } else {
                " any key closes "
            };
            let block = panel("Dicturs — keys", true)
                .title_bottom(Line::from(footer).dim().right_aligned());
            let inner = block.inner(r);
            f.render_widget(Clear, r);
            f.render_widget(block, r);
            let n = columns.len() as u32;
            let areas = Layout::horizontal(vec![Constraint::Ratio(1, n); n as usize])
                .spacing(2)
                .split(inner);
            for (col, a) in columns.into_iter().zip(areas.iter()) {
                let pad = Rect {
                    x: a.x + 1,
                    width: a.width.saturating_sub(1),
                    ..*a
                };
                f.render_widget(Paragraph::new(col).scroll((*scroll, 0)), pad);
            }
        }
        Popup::Confirm { message, .. } => {
            let r = centered(area, 60.min(area.width), message.lines().count() as u16 + 4);
            hit.popup = r;
            f.render_widget(Clear, r);
            let mut lines: Vec<Line> = message.lines().map(Line::from).collect();
            lines.push(Line::from(""));
            lines.push(Line::from("[y] yes   [n] no").dim());
            f.render_widget(Paragraph::new(lines).block(panel("Confirm", true)), r);
        }
        Popup::Choice(choice) => {
            let h = (choice.items.len() as u16 + 2).min(area.height.saturating_sub(2));
            let r = centered(area, 72.min(area.width), h);
            hit.popup = r;
            f.render_widget(Clear, r);
            let items: Vec<ListItem> = choice
                .items
                .iter()
                .map(|(_, l)| ListItem::new(l.clone()))
                .collect();
            f.render_stateful_widget(
                List::new(items)
                    .block(panel(&choice.title, true))
                    .highlight_style(Style::new().reversed())
                    .highlight_symbol("› "),
                r,
                &mut choice.state,
            );
        }
        Popup::Form(form) => {
            let h = form.inputs.len() as u16 * 3 + 3;
            let r = centered(area, 76.min(area.width), h);
            hit.popup = r;
            f.render_widget(Clear, r);
            let block = panel(&form.title, true)
                .title_bottom(Line::from(" enter save · tab next · esc cancel ").dim());
            let inner = block.inner(r);
            f.render_widget(block, r);
            let constraints = vec![Constraint::Length(3); form.inputs.len()];
            let areas = Layout::vertical(constraints).split(inner);
            for (i, (input, label)) in form.inputs.iter().zip(&form.labels).enumerate() {
                let focused = i == form.field;
                let field = areas[i];
                let inner_w = field.width.saturating_sub(2) as usize;
                // Horizontal scroll so the cursor stays visible.
                let offset = input.cursor.saturating_sub(inner_w.saturating_sub(1));
                let text: String = input.chars.iter().skip(offset).collect();
                hit.popup_fields.push((field, offset));
                f.render_widget(Paragraph::new(text).block(panel(label, focused)), field);
                if focused {
                    f.set_cursor_position((
                        field.x + 1 + (input.cursor - offset) as u16,
                        field.y + 1,
                    ));
                }
            }
        }
    }
}
