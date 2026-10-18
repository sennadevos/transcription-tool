#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod audio;
mod config;
mod icons;
mod install;
mod jobs;
mod local;
mod openrouter;
mod rating;
mod theme;
mod update;

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, channel};

use eframe::egui::{self, Align, Color32, Layout, PopupCloseBehavior, RichText, Sense, Shape, Stroke, UiBuilder};

use config::Settings;
use jobs::{Event, Job};
use openrouter::{Catalog, LlmModel, SttModel};
use rating::Rating;

const AUDIO_EXTS: &[&str] = &[
    "mp3", "m4a", "mp4", "wav", "flac", "ogg", "opus", "aac", "webm", "mkv", "mov", "caf", "aiff", "aif", "wma",
];
const LANGUAGES: &[(&str, &str)] = &[
    ("", "Detect automatically"),
    ("nl", "Dutch"),
    ("en", "English"),
    ("de", "German"),
    ("fr", "French"),
    ("es", "Spanish"),
    ("it", "Italian"),
    ("pt", "Portuguese"),
    ("pl", "Polish"),
    ("tr", "Turkish"),
    ("ar", "Arabic"),
    ("uk", "Ukrainian"),
    ("ru", "Russian"),
    ("zh", "Chinese"),
    ("ja", "Japanese"),
];
const COLUMN_WIDTH: f32 = 680.0;
/// A local model expected to take longer than this on the chosen file triggers a warning before starting.
const SLOW_LOCAL_SECS: f64 = 180.0;

fn main() -> eframe::Result {
    update::cleanup();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Transcription Tool")
            .with_inner_size([760.0, 860.0])
            .with_min_inner_size([520.0, 560.0])
            .with_drag_and_drop(true),
        ..Default::default()
    };
    eframe::run_native("Transcription Tool", options, Box::new(|cc| Ok(Box::new(App::new(&cc.egui_ctx)))))
}

enum CardState {
    Running(String),
    Ok(String),
    Err(String),
}

struct Card {
    id: String,
    label: String,
    state: CardState,
    started: std::time::Instant,
}

#[derive(PartialEq, Clone, Copy)]
enum Screen {
    Main,
    Settings,
}

struct App {
    settings: Settings,
    dirty: bool,
    screen: Screen,
    catalog: Option<Catalog>,
    catalog_error: Option<String>,
    catalog_rx: Option<Receiver<Result<Catalog, String>>>,
    file: Option<PathBuf>,
    file_size: u64,
    /// Length of the chosen file in seconds, when the file says so.
    file_secs: Option<f64>,
    filter: String,
    llm_filter: String,
    cards: Vec<Card>,
    merged: Option<CardState>,
    status: String,
    rx: Option<Receiver<Event>>,
    notice: Option<String>,
    key_input: String,
    key_editing: bool,
    key_checking: bool,
    key_error: Option<String>,
    key_rx: Option<Receiver<(String, Result<(), String>)>>,
    /// Set when the saved key was found to be refused or out of credit, before the user tries anything.
    /// Stops local transcription of the running job.
    cancel: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    key_notice: Option<String>,
    key_notice_rx: Option<Receiver<Result<(), String>>>,
    update: UpdateState,
    update_rx: Option<Receiver<UpdateMsg>>,
}

enum UpdateState {
    None,
    Available(update::Release),
    Installing(String),
    Failed(update::Release, String),
}

enum UpdateMsg {
    Found(Option<update::Release>),
    Failed(update::Release, String),
}

// ---------- small widgets ----------

fn semibold(text: impl Into<String>) -> RichText {
    RichText::new(text).family(theme::semibold())
}

fn muted(ui: &egui::Ui, text: impl Into<String>) -> RichText {
    RichText::new(text).color(theme::palette(ui.ctx()).muted)
}

/// Quiet text button.
fn link(ui: &mut egui::Ui, text: &str) -> bool {
    let p = theme::palette(ui.ctx());
    let resp = ui.add(egui::Label::new(RichText::new(text).color(p.accent)).sense(Sense::click()));
    resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
}

fn primary(ui: &mut egui::Ui, text: &str, width: f32) -> egui::Response {
    let p = theme::palette(ui.ctx());
    let btn = egui::Button::new(RichText::new(text).family(theme::semibold()).color(p.on_accent))
        .fill(p.accent)
        .stroke(Stroke::NONE)
        .min_size(egui::vec2(width, 36.0));
    ui.add(btn)
}

fn field_label(ui: &mut egui::Ui, text: &str) {
    ui.label(muted(ui, text).size(12.5));
}

fn section_title(ui: &mut egui::Ui, text: &str) {
    ui.add_space(14.0);
    ui.label(muted(ui, text.to_uppercase()).family(theme::semibold()).size(11.5));
    ui.add_space(2.0);
}

fn panel() -> egui::Frame {
    egui::Frame::NONE.corner_radius(3).inner_margin(egui::Margin::same(14))
}

fn surface_panel(ui: &egui::Ui) -> egui::Frame {
    let p = theme::palette(ui.ctx());
    panel().fill(p.surface).stroke(Stroke::new(1.0, p.border))
}

fn human_size(bytes: u64) -> String {
    if bytes >= 1 << 20 { format!("{:.1} MB", bytes as f64 / (1 << 20) as f64) } else { format!("{} KB", bytes.div_ceil(1024)) }
}

/// Three-symbol scale, `n` of them filled in. Hover explains it.
fn scale(ui: &mut egui::Ui, symbol: &str, n: u8, note: &str) {
    let p = theme::palette(ui.ctx());
    let mut job = egui::text::LayoutJob::default();
    for i in 0..3 {
        let color = if i < n { p.text } else { p.text.gamma_multiply(0.22) };
        job.append(symbol, 0.0, egui::TextFormat { font_id: egui::FontId::proportional(12.5), color, ..Default::default() });
    }
    ui.add(egui::Label::new(job).sense(Sense::hover())).on_hover_text(note);
}

/// Three little brains, `n` of them filled in. Drawn by hand because no bundled font has the emoji.
fn brains(ui: &mut egui::Ui, n: u8, note: &str) {
    let p = theme::palette(ui.ctx());
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(3.0 * 16.0, 16.0), Sense::hover());
    for i in 0..3 {
        let c = rect.left_center() + egui::vec2(8.0 + i as f32 * 16.0, -0.5);
        let color = if i < n { p.text } else { p.text.gamma_multiply(0.22) };
        let painter = ui.painter();
        for (dx, dy, r) in [(-3.1, -2.4, 3.1), (3.1, -2.4, 3.1), (-3.3, 2.0, 3.0), (3.3, 2.0, 3.0), (0.0, 0.0, 3.6)] {
            painter.circle_filled(c + egui::vec2(dx, dy), r, color);
        }
        painter.line_segment([c + egui::vec2(0.0, -5.5), c + egui::vec2(0.0, 5.0)], Stroke::new(1.0, p.surface));
        painter.line_segment([c + egui::vec2(-5.4, 0.2), c + egui::vec2(-2.4, 0.9)], Stroke::new(0.9, p.surface));
        painter.line_segment([c + egui::vec2(5.4, 0.2), c + egui::vec2(2.4, 0.9)], Stroke::new(0.9, p.surface));
    }
    resp.on_hover_text(note);
}

/// Price and brains, right-aligned. Call inside a right-to-left layout.
fn rating_ui(ui: &mut egui::Ui, r: &Rating) {
    match r.price {
        Some(n) => scale(ui, "$", n, &r.price_note),
        None if !r.price_note.is_empty() => {
            ui.label(muted(ui, &r.price_note).size(12.5));
        }
        None => {}
    }
    if let Some(n) = r.brains {
        ui.add_space(4.0);
        brains(ui, n, &r.brains_note);
    }
}

fn group_label(ui: &mut egui::Ui, text: &str) {
    ui.add_space(2.0);
    ui.label(muted(ui, text).family(theme::semibold()).size(12.0));
}

/// A selectable surface. The chosen one is tinted and outlined in the accent colour; there is no check mark.
fn choice(ui: &mut egui::Ui, size: egui::Vec2, selected: bool, bordered: bool, margin: egui::Vec2, content: impl FnOnce(&mut egui::Ui)) -> egui::Response {
    let p = theme::palette(ui.ctx());
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let (fill, stroke) = if selected {
        (p.surface.lerp_to_gamma(p.accent, 0.18), Stroke::new(1.0, p.accent))
    } else if resp.hovered() {
        (p.bg, Stroke::new(1.0, if bordered { p.muted } else { p.border }))
    } else if bordered {
        (p.surface, Stroke::new(1.0, p.border))
    } else {
        (Color32::TRANSPARENT, Stroke::NONE)
    };
    ui.painter().rect(rect, 3.0, fill, stroke, egui::StrokeKind::Inside);
    // A child that does not claim extra room in the parent, so the card keeps exactly the size it was given.
    let mut inner = ui.new_child(UiBuilder::new().max_rect(rect.shrink2(margin)).layout(Layout::top_down(Align::Min)));
    // Selectable text would swallow the clicks meant for the card.
    inner.style_mut().interaction.selectable_labels = false;
    content(&mut inner);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Recommended model as a card. Returns true when clicked.
fn model_card(ui: &mut egui::Ui, width: f32, id: &str, name: &str, provider: &str, rating: &Rating, selected: bool) -> bool {
    let p = theme::palette(ui.ctx());
    choice(ui, egui::vec2(width, 82.0), selected, true, egui::vec2(11.0, 10.0), |ui| {
        ui.spacing_mut().item_spacing.y = 5.0;
        ui.spacing_mut().interact_size.y = 18.0;
        ui.horizontal(|ui| {
            icons::show(ui, id, 18.0, p.text);
            ui.add(egui::Label::new(semibold(name)).truncate());
        });
        ui.label(muted(ui, provider).size(12.5));
        ui.horizontal(|ui| rating_ui(ui, rating));
    })
    .clicked()
}

/// Any other model as a compact row. `right` fills the right-hand end. Returns true when clicked.
fn model_row(ui: &mut egui::Ui, id: &str, name: &str, provider: &str, selected: bool, right: impl FnOnce(&mut egui::Ui)) -> bool {
    let p = theme::palette(ui.ctx());
    choice(ui, egui::vec2(ui.available_width(), 30.0), selected, false, egui::vec2(8.0, 3.0), |ui| {
        egui::Sides::new().shrink_left().truncate().show(
            ui,
            |ui| {
                icons::show(ui, id, 16.0, p.text);
                ui.label(name);
                if !provider.is_empty() {
                    ui.label(muted(ui, provider).size(12.5));
                }
            },
            right,
        );
    })
    .clicked()
}

/// Lays cards out two to a row.
fn card_grid<T>(ui: &mut egui::Ui, items: &[&T], mut card: impl FnMut(&mut egui::Ui, &T, f32)) {
    let gap = 6.0;
    let width = ((ui.available_width() - gap) / 2.0).floor();
    for pair in items.chunks(2) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            for item in pair {
                card(ui, item, width);
            }
        });
        ui.add_space(4.0);
    }
}

fn toggle(set: &mut std::collections::BTreeSet<String>, id: &str, dirty: &mut bool) {
    if !set.remove(id) {
        set.insert(id.to_string());
    }
    *dirty = true;
}

// ---------- app ----------

impl App {
    fn new(ctx: &egui::Context) -> Self {
        let settings = Settings::load();
        theme::install(ctx, &settings.theme);
        let mut app = Self {
            settings,
            dirty: false,
            screen: Screen::Main,
            catalog: None,
            catalog_error: None,
            catalog_rx: None,
            file: None,
            file_size: 0,
            file_secs: None,
            filter: String::new(),
            llm_filter: String::new(),
            cards: Vec::new(),
            merged: None,
            status: String::new(),
            rx: None,
            notice: None,
            key_input: String::new(),
            key_editing: false,
            key_checking: false,
            key_error: None,
            key_rx: None,
            cancel: None,
            key_notice: None,
            key_notice_rx: None,
            update: UpdateState::None,
            update_rx: None,
        };
        if let Some(p) = std::env::args_os().nth(1).map(PathBuf::from).filter(|p| p.is_file()) {
            app.set_file(p);
        }
        app.load_catalog(ctx);
        app.recheck_saved_key(ctx);
        app.check_for_update(ctx);
        app
    }

    /// Looks for a newer release in the background; staying quiet when offline or when there is none.
    fn check_for_update(&mut self, ctx: &egui::Context) {
        let (tx, rx) = channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            if let Ok(found) = update::check() {
                let _ = tx.send(UpdateMsg::Found(found));
                ctx.request_repaint();
            }
        });
        self.update_rx = Some(rx);
    }

    fn install_update(&mut self, ctx: &egui::Context, rel: update::Release) {
        self.update = UpdateState::Installing(rel.version.clone());
        let (tx, rx) = channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            match update::apply(&rel) {
                Ok(()) => std::process::exit(0),
                Err(e) => {
                    let _ = tx.send(UpdateMsg::Failed(rel, format!("{e:#}")));
                }
            }
            ctx.request_repaint();
        });
        self.update_rx = Some(rx);
    }

    /// Asks OpenRouter (free of charge) whether the saved key still works, so an expired key shows up before the first transcription.
    fn recheck_saved_key(&mut self, ctx: &egui::Context) {
        if !self.has_key() {
            return;
        }
        let key = self.settings.api_key.clone();
        let (tx, rx) = channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(openrouter::check_key(&key).map_err(|e| e.to_string()));
            ctx.request_repaint();
        });
        self.key_notice_rx = Some(rx);
    }

    fn set_file(&mut self, path: PathBuf) {
        self.file_size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        self.file_secs = audio::duration_secs(&path);
        self.file = Some(path);
    }

    fn pick_file(&mut self) {
        if let Some(p) = rfd::FileDialog::new().add_filter("Audio", AUDIO_EXTS).pick_file() {
            self.set_file(p);
        }
    }

    fn load_catalog(&mut self, ctx: &egui::Context) {
        let (tx, rx) = channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(openrouter::catalog().map_err(|e| e.to_string()));
            ctx.request_repaint();
        });
        self.catalog_rx = Some(rx);
        self.catalog_error = None;
    }

    fn check_key(&mut self, ctx: &egui::Context) {
        let key = self.key_input.trim().to_string();
        let (tx, rx) = channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let r = openrouter::verify_key(&key).map_err(|e| e.to_string());
            let _ = tx.send((key, r));
            ctx.request_repaint();
        });
        self.key_rx = Some(rx);
        self.key_checking = true;
        self.key_error = None;
    }

    fn running(&self) -> bool {
        self.rx.is_some()
    }

    fn has_key(&self) -> bool {
        !self.settings.api_key.trim().is_empty()
    }

    fn poll(&mut self) {
        if let Some(rx) = &self.catalog_rx {
            if let Ok(res) = rx.try_recv() {
                match res {
                    Ok(c) => self.catalog = Some(c),
                    Err(e) => self.catalog_error = Some(e),
                }
                self.catalog_rx = None;
            }
        }
        if let Some(rx) = &self.update_rx {
            if let Ok(msg) = rx.try_recv() {
                self.update_rx = None;
                self.update = match msg {
                    UpdateMsg::Found(Some(rel)) => UpdateState::Available(rel),
                    UpdateMsg::Found(None) => UpdateState::None,
                    UpdateMsg::Failed(rel, e) => UpdateState::Failed(rel, e),
                };
            }
        }
        if let Some(rx) = &self.key_notice_rx {
            if let Ok(res) = rx.try_recv() {
                self.key_notice_rx = None;
                // Being offline says nothing about the key, so only a real refusal is shown.
                self.key_notice = res.err().filter(|e| openrouter::is_key_problem(e));
            }
        }
        if let Some(rx) = &self.key_rx {
            if let Ok((key, res)) = rx.try_recv() {
                self.key_rx = None;
                self.key_checking = false;
                match res {
                    Ok(()) => {
                        self.settings.api_key = key;
                        self.key_notice = None;
                        self.dirty = true;
                        self.key_input.clear();
                        self.key_editing = false;
                    }
                    Err(e) => self.key_error = Some(e),
                }
            }
        }
        let Some(rx) = &self.rx else { return };
        let mut done = false;
        while let Ok(ev) = rx.try_recv() {
            match ev {
                Event::Status(s) => self.status = s,
                Event::Progress { id, msg } => {
                    if let Some(c) = self.cards.iter_mut().find(|c| c.id == id) {
                        c.state = CardState::Running(msg);
                    }
                }
                Event::LocalSpeed { model, speed } => {
                    // Blend with earlier runs so one run on a busy machine does not decide everything.
                    let blended = self.settings.local_speed.get(&model).map_or(speed, |old| (old + speed) / 2.0);
                    self.settings.local_speed.insert(model, blended);
                    self.dirty = true;
                }
                Event::Finished { id, result } => {
                    if let Some(c) = self.cards.iter_mut().find(|c| c.id == id) {
                        c.state = match result {
                            Ok(t) => CardState::Ok(t),
                            Err(e) => CardState::Err(e),
                        };
                    }
                }
                Event::Merged(r) => {
                    self.merged = Some(match r {
                        Ok(t) => CardState::Ok(t),
                        Err(e) => CardState::Err(e),
                    })
                }
                Event::Done => done = true,
            }
        }
        if done {
            self.rx = None;
            self.cancel = None;
            self.status.clear();
        }
    }

    fn model_name(&self, id: &str) -> String {
        if let Some(name) = id.strip_prefix("local/") {
            return local::MODELS.iter().find(|m| m.0 == name).map_or(name, |m| m.1).to_string();
        }
        self.catalog
            .iter()
            .flat_map(|c| &c.stt)
            .find(|m| m.id == id)
            .map_or_else(|| id.rsplit('/').next().unwrap_or(id).to_string(), |m| m.name.clone())
    }

    fn selected_ids(&self) -> Vec<String> {
        let mut ids: Vec<String> = self
            .catalog
            .iter()
            .flat_map(|c| &c.stt)
            .filter(|m| self.settings.remote_models.contains(&m.id))
            .map(|m| m.id.clone())
            .collect();
        ids.extend(local::MODELS.iter().filter(|m| self.settings.local_models.contains(m.0)).map(|m| format!("local/{}", m.0)));
        ids
    }

    /// Warns before starting when a local model is known to be slow on this computer for this file.
    fn slow_local_note(&self) -> Option<String> {
        let secs = self.file_secs?;
        let local: Vec<&String> = self.settings.local_models.iter().collect();
        // Several local models share the same processor, so each takes about that many times longer.
        let share = local.len() as f64;
        let (name, est) = local
            .iter()
            .filter_map(|m| Some((m, secs / *self.settings.local_speed.get(*m)? * share)))
            .max_by(|a, b| a.1.total_cmp(&b.1))?;
        (est > SLOW_LOCAL_SECS).then(|| {
            let title = local::MODELS.iter().find(|m| m.0 == name.as_str()).map_or(name.as_str(), |m| m.1);
            format!("{title} will take {} on this computer. Online models are usually much quicker.", jobs::human_duration(est))
        })
    }

    fn model_summary(&self) -> String {
        let ids = self.selected_ids();
        match ids.len() {
            0 => "Choose a model".into(),
            1 => self.model_name(&ids[0]),
            n => format!("{} + {} more", self.model_name(&ids[0]), n - 1),
        }
    }

    fn start(&mut self, ctx: &egui::Context) {
        let Some(file) = self.file.clone() else { return };
        let models: Vec<(String, String)> = self.selected_ids().into_iter().map(|id| (self.model_name(&id), id)).map(|(n, i)| (i, n)).collect();
        self.cards = models.iter().map(|(id, label)| Card { id: id.clone(), label: label.clone(), state: CardState::Running("Waiting…".into()), started: std::time::Instant::now() }).collect();
        self.merged = self.settings.merge.then(|| CardState::Running("Waiting for the transcripts…".into()));
        let job = Job {
            file,
            key: self.settings.api_key.clone(),
            language: self.settings.language.clone(),
            models,
            merge: self.settings.merge,
            llm: self.settings.llm.clone(),
            prompt: self.settings.prompt.clone(),
            cancel: Default::default(),
        };
        self.cancel = Some(job.cancel.clone());
        let (tx, rx) = channel();
        self.rx = Some(rx);
        let ctx = ctx.clone();
        std::thread::spawn(move || jobs::run(job, tx, ctx));
    }

    fn open_key_settings(&mut self) {
        self.screen = Screen::Settings;
        self.key_editing = true;
    }

    // ---------- screens ----------

    fn ui_setup(&mut self, ui: &mut egui::Ui) {
        let p = theme::palette(ui.ctx());
        ui.add_space(70.0);
        ui.label(RichText::new("Transcription Tool").family(theme::semibold()).size(26.0));
        ui.add_space(6.0);
        ui.label(muted(ui, "Paste the key you received to get started. You only need to do this again when the key expires."));
        ui.add_space(22.0);
        self.ui_key_form(ui, "Continue", false);
        if let Some(e) = &self.key_error {
            ui.add_space(4.0);
            ui.label(RichText::new(e).color(p.error));
        }
    }

    /// Key entry shared by first-run setup and settings. Returns true when the user cancelled.
    fn ui_key_form(&mut self, ui: &mut egui::Ui, action: &str, cancellable: bool) -> bool {
        let mut cancelled = false;
        let resp = ui.add(
            egui::TextEdit::singleline(&mut self.key_input)
                .password(true)
                .hint_text("sk-or-…")
                .desired_width(f32::INFINITY)
                .margin(egui::Margin::symmetric(10, 8)),
        );
        let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            let ready = !self.key_input.trim().is_empty() && !self.key_checking;
            if (ui.add_enabled(ready, egui::Button::new(semibold(action)).min_size(egui::vec2(100.0, 32.0))).clicked() || (enter && ready)) && ready {
                self.check_key(ui.ctx());
            }
            if cancellable && ui.add(egui::Button::new("Cancel").frame(false)).clicked() {
                cancelled = true;
            }
            if self.key_checking {
                ui.spinner();
                ui.label(muted(ui, "Checking…"));
            }
        });
        cancelled
    }

    fn ui_main(&mut self, ui: &mut egui::Ui) {
        let p = theme::palette(ui.ctx());
        ui.horizontal(|ui| {
            ui.label(RichText::new("Transcription Tool").family(theme::semibold()).size(17.0));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if link(ui, "Settings") {
                    self.screen = Screen::Settings;
                }
            });
        });
        ui.add_space(18.0);

        if let Some(msg) = &self.notice {
            ui.label(RichText::new(msg).color(p.error));
            ui.add_space(8.0);
        }
        if let Some(msg) = &self.key_notice {
            ui.label(RichText::new(msg).color(p.error));
            if link(ui, "Update your key") {
                self.open_key_settings();
            }
            ui.add_space(8.0);
        }

        self.ui_update_notice(ui);

        self.ui_file(ui);
        ui.add_space(16.0);
        self.ui_options(ui);
        ui.add_space(18.0);

        let ids = self.selected_ids();
        let blocker = if self.file.is_none() {
            Some("Choose an audio file first")
        } else if ids.is_empty() {
            Some("Choose at least one model")
        } else {
            None
        };
        ui.horizontal(|ui| {
            if ui.add_enabled_ui(blocker.is_none() && !self.running(), |ui| primary(ui, "Transcribe", 150.0)).inner.clicked() {
                self.start(ui.ctx());
            }
            if self.running() {
                ui.spinner();
                ui.label(muted(ui, self.status.clone()));
            } else if let Some(b) = blocker {
                ui.label(muted(ui, b));
            }
        });

        ui.add_space(22.0);
        self.ui_results(ui);
    }

    fn ui_file(&mut self, ui: &mut egui::Ui) {
        let p = theme::palette(ui.ctx());
        if let Some(file) = self.file.clone() {
            surface_panel(ui).show(ui, |ui| {
                ui.set_width(ui.available_width());
                let name = file.file_name().unwrap_or_default().to_string_lossy().to_string();
                // A long name is cut short so the size and "Change" always keep their place.
                egui::Sides::new().shrink_left().truncate().show(
                    ui,
                    |ui| {
                        ui.label(semibold(name.clone())).on_hover_text(name);
                    },
                    |ui| {
                        if link(ui, "Change") {
                            self.pick_file();
                        }
                        ui.label(muted(ui, human_size(self.file_size)));
                    },
                );
            });
            return;
        }
        let dragging = ui.ctx().input(|i| !i.raw.hovered_files.is_empty());
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 128.0), Sense::click());
        let active = dragging || resp.hovered();
        let painter = ui.painter();
        painter.rect_filled(rect, 3.0, if active { p.surface } else { p.bg });
        let r = rect.shrink(0.5);
        let outline = [r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom(), r.left_top()];
        painter.extend(Shape::dashed_line(&outline, Stroke::new(1.0, if active { p.accent } else { p.border }), 6.0, 5.0));
        painter.text(
            rect.center() - egui::vec2(0.0, 10.0),
            egui::Align2::CENTER_CENTER,
            "Drop an audio file here",
            egui::FontId::new(16.0, theme::semibold()),
            p.text,
        );
        painter.text(rect.center() + egui::vec2(0.0, 14.0), egui::Align2::CENTER_CENTER, "or click to choose one", egui::FontId::proportional(13.5), p.muted);
        if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
            self.pick_file();
        }
    }

    fn ui_options(&mut self, ui: &mut egui::Ui) {
        let gap = ui.spacing().item_spacing.x;
        let lang_w = 190.0;
        let model_w = (ui.available_width() - lang_w - gap).max(200.0);
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                field_label(ui, "Model");
                self.ui_model_picker(ui, model_w);
            });
            ui.vertical(|ui| {
                field_label(ui, "Language");
                let current = LANGUAGES.iter().find(|l| l.0 == self.settings.language).map_or("Other", |l| l.1);
                egui::ComboBox::from_id_salt("lang").selected_text(current).width(lang_w - 22.0).show_ui(ui, |ui| {
                    for (code, name) in LANGUAGES {
                        self.dirty |= ui.selectable_value(&mut self.settings.language, code.to_string(), *name).changed();
                    }
                });
            });
        });
        if let Some(note) = self.slow_local_note() {
            ui.add_space(6.0);
            ui.label(note);
        }
        ui.add_space(6.0);
        self.dirty |= ui.checkbox(&mut self.settings.merge, "Combine into one final transcript").changed();
    }

    fn ui_model_picker(&mut self, ui: &mut egui::Ui, width: f32) {
        let summary = self.model_summary();
        egui::ComboBox::from_id_salt("models")
            .selected_text(summary)
            .width(width - 22.0)
            .close_behavior(PopupCloseBehavior::CloseOnClickOutside)
            .height(460.0)
            .show_ui(ui, |ui| {
                self.ui_model_list(ui, width);
            });
    }

    fn ui_model_list(&mut self, ui: &mut egui::Ui, width: f32) {
        let p = theme::palette(ui.ctx());
        ui.set_min_width(width - 8.0);
        ui.spacing_mut().item_spacing.y = 2.0;
        ui.spacing_mut().interact_size.y = 26.0;
        ui.add(egui::TextEdit::singleline(&mut self.filter).hint_text("Search").desired_width(f32::INFINITY));
        ui.add_space(4.0);
        egui::ScrollArea::vertical().max_height(400.0).auto_shrink([false, true]).show(ui, |ui| {
            let q = self.filter.to_lowercase();
            match (&self.catalog, &self.catalog_error) {
                (Some(cat), _) => {
                    let shown = |m: &&SttModel| q.is_empty() || format!("{} {}", m.name, m.provider).to_lowercase().contains(&q);
                    let picks: Vec<&SttModel> = rating::RECOMMENDED_STT.iter().filter_map(|id| cat.stt.iter().find(|m| m.id == *id)).filter(shown).collect();
                    if !picks.is_empty() {
                        group_label(ui, "Selection");
                        card_grid(ui, &picks, |ui, m, w| {
                            if model_card(ui, w, &m.id, &m.name, &m.provider, &m.rating, self.settings.remote_models.contains(&m.id)) {
                                toggle(&mut self.settings.remote_models, &m.id, &mut self.dirty);
                            }
                        });
                    }
                    let rest: Vec<&SttModel> = cat.stt.iter().filter(|m| !rating::RECOMMENDED_STT.contains(&m.id.as_str())).filter(shown).collect();
                    if !rest.is_empty() {
                        group_label(ui, "All models");
                        for m in rest {
                            if model_row(ui, &m.id, &m.name, &m.provider, self.settings.remote_models.contains(&m.id), |ui| rating_ui(ui, &m.rating)) {
                                toggle(&mut self.settings.remote_models, &m.id, &mut self.dirty);
                            }
                        }
                    }
                }
                (None, Some(e)) => {
                    ui.label(RichText::new(format!("The model list could not be loaded. {e}")).color(p.error));
                    if link(ui, "Try again") {
                        self.load_catalog(ui.ctx());
                    }
                }
                (None, None) => {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(muted(ui, "Loading models…"));
                    });
                }
            }
            ui.add_space(6.0);
            ui.separator();
            ui.label(muted(ui, "On this computer").size(12.5));
            for (name, title, detail) in local::MODELS {
                if !q.is_empty() && !title.to_lowercase().contains(&q) {
                    continue;
                }
                let note = if local::is_downloaded(name) { format!("{detail} · downloaded") } else { detail.to_string() };
                // Whisper is OpenAI's, so it wears their mark.
                if model_row(ui, "openai/whisper", title, "", self.settings.local_models.contains(*name), |ui| {
                    ui.label(muted(ui, note).size(12.5));
                }) {
                    toggle(&mut self.settings.local_models, name, &mut self.dirty);
                }
            }
        });
    }

    fn ui_results(&mut self, ui: &mut egui::Ui) {
        let mut fix_key = false;
        let file = self.file.clone();
        if let Some(state) = &self.merged {
            fix_key |= Self::ui_result(ui, "Final transcript", None, state, file.as_ref(), None, &mut false);
            ui.add_space(14.0);
        }
        if self.cards.is_empty() {
            return;
        }
        let mut stop = false;
        let draw = |ui: &mut egui::Ui, fix_key: &mut bool, stop: &mut bool| {
            for c in &self.cards {
                let live = (c.id.starts_with("local/") && self.cancel.is_some()).then(|| c.started.elapsed().as_secs());
                *fix_key |= Self::ui_result(ui, &c.label, None, &c.state, file.as_ref(), live, stop);
                ui.add_space(10.0);
            }
        };
        if self.merged.is_some() {
            let failed = matches!(self.merged, Some(CardState::Err(_)));
            egui::CollapsingHeader::new(semibold(format!("Individual results ({})", self.cards.len())))
                .id_salt("individual")
                .default_open(failed)
                .show(ui, |ui| draw(ui, &mut fix_key, &mut stop));
        } else {
            draw(ui, &mut fix_key, &mut stop);
        }
        if stop {
            if let Some(flag) = &self.cancel {
                flag.store(true, std::sync::atomic::Ordering::Relaxed);
            }
        }
        if fix_key {
            self.open_key_settings();
        }
    }

    /// Returns true if the user asked to fix their key.
    /// `live` is the seconds a local run has been going; it adds a clock and a Stop link to the running card.
    fn ui_result(ui: &mut egui::Ui, title: &str, subtitle: Option<&str>, state: &CardState, file: Option<&PathBuf>, live: Option<u64>, stop: &mut bool) -> bool {
        let p = theme::palette(ui.ctx());
        let mut fix = false;
        ui.horizontal(|ui| {
            ui.label(semibold(title));
            if let Some(s) = subtitle {
                ui.label(muted(ui, s));
            }
            if let CardState::Ok(text) = state {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if link(ui, "Save") {
                        let stem = file.and_then(|f| f.file_stem()).map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
                        if let Some(path) = rfd::FileDialog::new().set_file_name(format!("{stem} transcript.txt")).add_filter("Text", &["txt"]).save_file() {
                            let _ = std::fs::write(path, text);
                        }
                    }
                    if link(ui, "Copy") {
                        ui.ctx().copy_text(text.clone());
                    }
                });
            }
        });
        match state {
            CardState::Running(msg) => {
                let (status, warning) = msg.split_once('\n').unwrap_or((msg, ""));
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(muted(ui, status));
                    if let Some(secs) = live {
                        ui.label(muted(ui, format!("{}:{:02}", secs / 60, secs % 60)));
                        if link(ui, "Stop") {
                            *stop = true;
                        }
                    }
                });
                if live.is_some() {
                    ui.ctx().request_repaint_after(std::time::Duration::from_millis(500));
                }
                if !warning.is_empty() {
                    ui.label(warning);
                }
            }
            CardState::Ok(text) => {
                surface_panel(ui).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.add(egui::Label::new(RichText::new(text).size(15.0)).selectable(true));
                });
            }
            CardState::Err(e) => {
                ui.label(RichText::new(e).color(p.error));
                if openrouter::is_key_problem(e) && link(ui, "Update your key") {
                    fix = true;
                }
            }
        }
        fix
    }

    fn ui_settings(&mut self, ui: &mut egui::Ui) {
        let p = theme::palette(ui.ctx());
        ui.horizontal(|ui| {
            if link(ui, "‹ Back") {
                self.screen = Screen::Main;
                self.key_editing = false;
                self.key_error = None;
                self.key_input.clear();
            }
        });
        ui.add_space(6.0);
        ui.label(RichText::new("Settings").family(theme::semibold()).size(22.0));

        section_title(ui, "Appearance");
        ui.horizontal(|ui| {
            for (value, name) in [("system", "System"), ("light", "Light"), ("dark", "Dark")] {
                if ui.selectable_label(self.settings.theme == value, name).clicked() {
                    self.settings.theme = value.into();
                    theme::set_preference(ui.ctx(), value);
                    self.dirty = true;
                }
            }
        });

        section_title(ui, "Combining");
        ui.horizontal(|ui| {
            ui.label("AI model");
            let current = self.catalog.iter().flat_map(|c| &c.llm).find(|m| m.id == self.settings.llm).map_or(self.settings.llm.clone(), |m| m.name.clone());
            egui::ComboBox::from_id_salt("llm").selected_text(current).width(300.0).height(420.0).show_ui(ui, |ui| {
                self.ui_llm_list(ui);
            });
        });
        egui::CollapsingHeader::new("Instructions to the AI").id_salt("prompt").show(ui, |ui| {
            self.dirty |= ui.add(egui::TextEdit::multiline(&mut self.settings.prompt).desired_rows(5).desired_width(f32::INFINITY)).changed();
            if link(ui, "Restore defaults") {
                self.settings.llm = config::DEFAULT_LLM.into();
                self.settings.prompt = config::DEFAULT_PROMPT.into();
                self.dirty = true;
            }
        });

        section_title(ui, "Account");
        if self.key_editing {
            ui.label(muted(ui, "Paste the new key."));
            ui.add_space(2.0);
            if self.ui_key_form(ui, "Save key", true) {
                self.key_editing = false;
                self.key_input.clear();
                self.key_error = None;
            }
            if let Some(e) = &self.key_error {
                ui.label(RichText::new(e).color(p.error));
            }
        } else {
            let key = self.settings.api_key.trim();
            let tail: String = key.chars().rev().take(4).collect::<Vec<_>>().into_iter().rev().collect();
            ui.horizontal(|ui| {
                ui.label("OpenRouter key");
                ui.label(muted(ui, format!("•••• {tail}")));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if link(ui, "Replace") {
                        self.key_editing = true;
                    }
                });
            });
        }

        section_title(ui, "This computer");
        ui.label(muted(ui, format!("Version {}", update::VERSION)));
        if install::is_installed_copy() {
            if link(ui, "Uninstall Transcription Tool…") {
                let yes = rfd::MessageDialog::new()
                    .set_title("Uninstall")
                    .set_description("Remove Transcription Tool, its shortcuts, settings and downloaded models?")
                    .set_buttons(rfd::MessageButtons::YesNo)
                    .show()
                    == rfd::MessageDialogResult::Yes;
                if yes {
                    match install::uninstall() {
                        Ok(()) => std::process::exit(0),
                        Err(e) => self.notice = Some(format!("{e:#}")),
                    }
                }
            }
        } else if link(ui, "Install on this computer") {
            self.run_install();
        }
    }

    fn ui_llm_list(&mut self, ui: &mut egui::Ui) {
        ui.set_min_width(440.0);
        ui.spacing_mut().item_spacing.y = 2.0;
        ui.spacing_mut().interact_size.y = 26.0;
        ui.add(egui::TextEdit::singleline(&mut self.llm_filter).hint_text("Search").desired_width(f32::INFINITY));
        ui.add_space(4.0);
        egui::ScrollArea::vertical().max_height(320.0).auto_shrink([false, true]).show(ui, |ui| {
            let Some(cat) = &self.catalog else {
                if let Some(e) = &self.catalog_error {
                    ui.label(RichText::new(format!("The model list could not be loaded. {e}")).color(theme::palette(ui.ctx()).error));
                    if link(ui, "Try again") {
                        self.load_catalog(ui.ctx());
                    }
                } else {
                    ui.label(muted(ui, "Loading models…"));
                }
                return;
            };
            let q = self.llm_filter.to_lowercase();
            let shown = |m: &&LlmModel| q.is_empty() || format!("{} {}", m.name, m.id).to_lowercase().contains(&q);
            let picks: Vec<&LlmModel> = rating::RECOMMENDED_LLM.iter().filter_map(|id| cat.llm.iter().find(|m| m.id == *id)).filter(shown).collect();
            if !picks.is_empty() {
                group_label(ui, "Selection");
                card_grid(ui, &picks, |ui, m, w| {
                    if model_card(ui, w, &m.id, &m.name, &m.provider, &m.rating, self.settings.llm == m.id) {
                        self.settings.llm = m.id.clone();
                        self.dirty = true;
                        ui.close();
                    }
                });
            }
            let rest: Vec<&LlmModel> = cat.llm.iter().filter(|m| !rating::RECOMMENDED_LLM.contains(&m.id.as_str())).filter(shown).collect();
            if !rest.is_empty() {
                group_label(ui, "All models");
                for m in rest {
                    if model_row(ui, &m.id, &m.name, &m.provider, self.settings.llm == m.id, |ui| rating_ui(ui, &m.rating)) {
                        self.settings.llm = m.id.clone();
                        self.dirty = true;
                        ui.close();
                    }
                }
            }
        });
    }

    fn ui_update_notice(&mut self, ui: &mut egui::Ui) {
        let p = theme::palette(ui.ctx());
        let mut start = None;
        match &self.update {
            UpdateState::None => return,
            UpdateState::Available(rel) | UpdateState::Failed(rel, _) => {
                if let UpdateState::Failed(_, e) = &self.update {
                    ui.label(RichText::new(e).color(p.error));
                }
                ui.horizontal(|ui| {
                    ui.label(format!("Version {} is available.", rel.version));
                    if self.running() {
                        ui.label(muted(ui, "You can update when the transcription is done."));
                    } else if link(ui, "Update now") {
                        start = Some(rel.clone());
                    }
                });
            }
            UpdateState::Installing(v) => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(muted(ui, format!("Downloading version {v}… the program restarts by itself")));
                });
            }
        }
        ui.add_space(8.0);
        if let Some(rel) = start {
            self.install_update(ui.ctx(), rel);
        }
    }

    fn run_install(&mut self) {
        match install::install() {
            Ok(()) => std::process::exit(0),
            Err(e) => self.notice = Some(format!("{e:#}")),
        }
    }

    fn ui_install_bar(&mut self, ui: &mut egui::Ui) {
        if install::is_installed_copy() || self.settings.install_dismissed || self.screen != Screen::Main || !self.has_key() {
            return;
        }
        let p = theme::palette(ui.ctx());
        egui::Panel::bottom("install").show_separator_line(false).frame(egui::Frame::NONE.fill(p.bg).inner_margin(egui::Margin::symmetric(16, 10))).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(muted(ui, "Add Transcription Tool to your start menu and desktop?"));
                if link(ui, "Install") {
                    self.run_install();
                }
                if link(ui, "Not now") {
                    self.settings.install_dismissed = true;
                    self.dirty = true;
                }
            });
        });
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.poll();
        if let Some(p) = ui.ctx().input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).find(|p| p.is_file())) {
            self.set_file(p);
            self.screen = Screen::Main;
        }
        if !self.has_key() {
            self.screen = Screen::Main;
        }

        self.ui_install_bar(ui);
        egui::CentralPanel::default_margins().show(ui, |ui| {
            egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                let full = ui.max_rect();
                let w = full.width().min(COLUMN_WIDTH);
                let column = egui::Rect::from_min_max(egui::pos2(full.center().x - w / 2.0, full.top()), egui::pos2(full.center().x + w / 2.0, f32::INFINITY));
                ui.scope_builder(UiBuilder::new().max_rect(column), |ui| {
                    ui.add_space(8.0);
                    if !self.has_key() {
                        self.ui_setup(ui);
                    } else if self.screen == Screen::Settings {
                        self.ui_settings(ui);
                    } else {
                        self.ui_main(ui);
                    }
                    ui.add_space(24.0);
                });
            });
        });

        if self.dirty {
            self.settings.save();
            self.dirty = false;
        }
    }
}
