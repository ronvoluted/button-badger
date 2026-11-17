#![windows_subsystem = "windows"]
//! Button Badger — print templates for button badges.
//!
//! This file is the app's state and wiring. `update` handles messages, `view` draws, and `fields` owns the
//! numeric text fields; each adds its own `impl App`.

mod fields;
mod images;
mod layout;
mod pdf;
mod preview;
mod print;
mod raster;
mod settings;
mod theme;
mod update;
mod view;

use fields::Field;
use images::Loaded;
use layout::{Fit, LineStyle, Orientation, Packing, Paper, Sheet, TestPage, Units, trim_float};
use preview::Interaction;
use settings::Settings;

use iced::widget::canvas::Cache;
use iced::widget::image::Handle;
use iced::{Event, Font, Size, Subscription, Task, Theme, Vector, event, keyboard, time, window};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

fn main() -> iced::Result {
    iced::application(App::boot, App::update, App::view)
        .title("Button Badger")
        .theme(App::theme)
        .subscription(App::subscription)
        .default_font(if cfg!(target_os = "macos") { Font::with_name("Helvetica Neue") } else { Font::DEFAULT })
        .antialiasing(true)
        .window(window::Settings {
            size: Size::new(1280.0, 840.0),
            min_size: Some(Size::new(520.0, 400.0)),
            icon: image::load_from_memory(include_bytes!("../packaging/icon-256.png"))
                .ok()
                .and_then(|i| window::icon::from_rgba(i.to_rgba8().into_raw(), i.width(), i.height()).ok()),
            ..Default::default()
        })
        .centered()
        .run()
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Toggle {
    FaceLine,
    CropMarks,
    Clip,
}

/// Background mode shown in the UI; the sheet itself only stores the resulting colour.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Bg {
    None,
    White,
    Custom,
}

/// One change to the sheet's settings. `App::edit_sheet` applies it, then refreshes everything derived from the sheet.
#[derive(Debug, Clone)]
enum SheetEdit {
    BadgePreset(String),
    Paper(Paper),
    Orientation(Orientation),
    Packing(Packing),
    Units(Units),
    CutStyle(LineStyle),
    Fit(Fit),
    Toggle(Toggle, bool),
    Background(Bg),
    BgHex(String),
    BgSwatch([f32; 3]),
}

#[derive(Debug, Clone)]
enum Message {
    // Images
    Open,
    FilesPicked(Vec<PathBuf>),
    Loaded(Result<Loaded, String>),
    DroppedFile(PathBuf),
    OpenRecent(PathBuf),
    Remove(usize),
    Replace(usize),
    ReplaceWith(usize, Option<PathBuf>),
    Replaced(usize, Result<Loaded, String>),
    ResetPlacement(usize),
    // Numeric fields
    Text(Field, String),
    Slide(Field, f32),
    Step(Field, f32),
    /// Up/Down pressed: step whichever field has focus (found via `StepFocused`).
    ArrowStep(f32),
    StepFocused(iced::widget::Id, f32),
    // Sheet settings
    Sheet(SheetEdit),
    // Preview
    Preview(Interaction),
    ZoomBy(f32),
    ZoomFit,
    Page(isize),
    // Output
    Export,
    ExportTo(Option<PathBuf>),
    Exported(Result<PathBuf, String>),
    Print,
    PrintTest,
    Printed(Result<String, String>),
    Reveal(PathBuf),
    // Presets and calibration
    PresetName(String),
    SavePreset,
    LoadPreset(String),
    DeletePreset,
    DeletePresetConfirmed(bool),
    ResetDefaults,
    ResetDefaultsConfirmed(bool),
    CalibrationPaper(Paper),
    ApplyCalibration,
    ResetCalibration,
    // Misc
    /// The window lost focus: write any queued settings.
    FocusLost,
    DismissToast,
    Tick(Instant),
    ToggleDark,
    /// The system's light/dark setting, followed until the user picks one.
    SystemTheme(iced::theme::Mode),
    /// Dev aid, debug builds only: `BUTTON_BADGER_SCREENSHOT=out.png` saves a window capture a few seconds
    /// after launch and exits.
    #[cfg(debug_assertions)]
    Screenshot(window::Screenshot),
}

#[derive(Clone, Debug)]
struct Toast {
    text: String,
    /// A saved file, offered as "Reveal in Finder".
    path: Option<PathBuf>,
    error: bool,
    until: Instant,
}

impl Toast {
    fn info(text: impl Into<String>) -> Option<Toast> {
        Some(Toast { text: text.into(), path: None, error: false, until: Instant::now() + Duration::from_secs(5) })
    }

    fn error(text: impl Into<String>) -> Option<Toast> {
        Some(Toast { text: text.into(), path: None, error: true, until: Instant::now() + Duration::from_secs(8) })
    }

    fn result(r: Result<String, String>) -> Option<Toast> {
        r.map_or_else(Toast::error, Toast::info)
    }
}

struct App {
    settings: Settings,
    dark: bool,
    images: Vec<Loaded>,
    /// Preview bitmap per image (same order as `images`).
    badges: Vec<Handle>,
    /// Badge centres on a page; cached `sheet.cells()`.
    cells: Vec<(f32, f32)>,
    /// Image index per cell, per page.
    pages: Vec<Vec<Option<usize>>>,
    page: usize,
    zoom: Option<f32>,
    pan: Vector,
    /// Paper and images.
    cache: Cache,
    /// Guide lines, drawn above the images.
    guide_cache: Cache,
    hover: Option<usize>,
    dragging: Option<usize>,
    texts: HashMap<Field, String>,
    bg_hex: String,
    bg_mode: Bg,
    preset_name: String,
    selected_preset: Option<String>,
    toast: Option<Toast>,
    loading: usize,
    #[cfg(debug_assertions)]
    screenshot: Option<(PathBuf, Instant)>,
}

const CUSTOM_BADGE: &str = "Custom";
const DEFAULT_CUSTOM_BG: &str = "#F4E7D3";

impl App {
    fn boot() -> (App, Task<Message>) {
        let (settings, unreadable) = Settings::load();
        let mut app = App {
            dark: settings.dark.unwrap_or(false),
            bg_hex: settings.sheet.background.filter(|b| *b != [1.0; 3]).map(hex_of).unwrap_or_else(|| DEFAULT_CUSTOM_BG.into()),
            bg_mode: bg_mode_of(settings.sheet.background),
            settings,
            images: Vec::new(),
            badges: Vec::new(),
            cells: Vec::new(),
            pages: Vec::new(),
            page: 0,
            zoom: None,
            pan: Vector::ZERO,
            cache: Cache::new(),
            guide_cache: Cache::new(),
            hover: None,
            dragging: None,
            texts: HashMap::new(),
            preset_name: String::new(),
            selected_preset: None,
            // Long enough to be read: this is the only notice that a settings file was set aside.
            toast: unreadable.map(|(text, path)| Toast { text, path: Some(path), error: true, until: Instant::now() + Duration::from_secs(30) }),
            loading: 0,
            #[cfg(debug_assertions)]
            screenshot: std::env::var_os("BUTTON_BADGER_SCREENSHOT").map(|p| (PathBuf::from(p), Instant::now() + Duration::from_secs(4))),
        };
        app.sync_texts();
        app.reset_measured();
        app.relayout();
        // Files passed on the command line (e.g. "Open with…") load straight away.
        let args: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).filter(|p| p.is_file()).collect();
        let mut tasks: Vec<_> = args.into_iter().map(|p| app.add_image(p)).collect();
        if app.settings.dark.is_none() {
            tasks.push(iced::system::theme().map(Message::SystemTheme));
        }
        (app, Task::batch(tasks))
    }

    fn theme(&self) -> Theme {
        theme::make_theme(self.dark)
    }

    fn subscription(&self) -> Subscription<Message> {
        let events = event::listen_with(|event, status, _| {
            use keyboard::{Key, key::Named};
            match event {
                Event::Window(window::Event::FileDropped(path)) => Some(Message::DroppedFile(path)),
                Event::Window(window::Event::Unfocused) => Some(Message::FocusLost),
                Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. }) => {
                    let cmd = modifiers.command();
                    let unfocused = status == event::Status::Ignored;
                    let step = if modifiers.shift() { 10.0 } else { 1.0 };
                    match key.as_ref() {
                        Key::Character("o") if cmd => Some(Message::Open),
                        Key::Character("e") if cmd => Some(Message::Export),
                        Key::Character("p") if cmd => Some(Message::Print),
                        Key::Character("0") if cmd => Some(Message::ZoomFit),
                        Key::Character("=" | "+") if cmd => Some(Message::ZoomBy(1.25)),
                        Key::Character("-") if cmd => Some(Message::ZoomBy(0.8)),
                        Key::Named(Named::ArrowLeft) if unfocused => Some(Message::Page(-1)),
                        Key::Named(Named::ArrowRight) if unfocused => Some(Message::Page(1)),
                        Key::Named(Named::ArrowUp) => Some(Message::ArrowStep(step)),
                        Key::Named(Named::ArrowDown) => Some(Message::ArrowStep(-step)),
                        Key::Named(Named::Escape) => Some(Message::DismissToast),
                        _ => None,
                    }
                }
                _ => None,
            }
        });
        let mut subs = vec![events];
        if self.settings.dark.is_none() {
            subs.push(iced::system::theme_changes().map(Message::SystemTheme));
        }
        let ticking = self.toast.is_some() || self.settings.queued();
        #[cfg(debug_assertions)]
        let ticking = ticking || self.screenshot.is_some();
        if ticking {
            subs.push(time::every(Duration::from_millis(400)).map(Message::Tick));
        }
        Subscription::batch(subs)
    }

    // ---- derived state ----

    fn sheet(&self) -> &Sheet {
        &self.settings.sheet
    }

    fn units(&self) -> Units {
        self.sheet().units
    }

    /// Show a failed settings write in the toast, over whatever it was saying.
    fn saved(&mut self, result: Result<(), String>) {
        if let Err(e) = result {
            self.toast = Toast::error(e);
        }
    }

    /// Write the settings now.
    fn save(&mut self) {
        let result = self.settings.save();
        self.saved(result);
    }

    /// Write queued settings once they are due at `now`; `None` writes them whatever the time.
    fn flush(&mut self, now: Option<Instant>) {
        let result = self.settings.flush(now);
        self.saved(result);
    }

    /// The sheet changed: re-composite every badge, re-layout and queue a save. This runs on every keystroke
    /// and drag tick, so the file is only written once the edits pause or the window loses focus.
    fn sheet_changed(&mut self) {
        self.badges = self.images.iter().map(|img| self.compose(img)).collect();
        self.relayout();
        self.settings.save_later();
    }

    /// One image's placement or pixels changed: re-composite just its badge.
    fn image_changed(&mut self, i: usize) {
        if let Some(img) = self.images.get(i) {
            self.badges[i] = self.compose(img);
            self.cache.clear();
        }
    }

    /// Recompute cells and pages (sheet, image list or quantities changed) and redraw.
    fn relayout(&mut self) {
        self.cells = self.sheet().cells();
        let qty: Vec<u32> = self.images.iter().map(|i| i.qty).collect();
        self.pages = layout::paginate(&self.cells, &qty);
        self.page = self.page.min(self.pages.len() - 1);
        self.invalidate();
    }

    /// Preview bitmap for one image, about 10 px per mm.
    fn compose(&self, img: &Loaded) -> Handle {
        let px = ((self.sheet().cut_mm() * 10.0).round() as u32).clamp(96, 1024);
        let bmp = images::compose(self.sheet(), img, &img.preview, px);
        Handle::from_rgba(bmp.width(), bmp.height(), bmp.into_raw())
    }

    fn invalidate(&mut self) {
        self.cache.clear();
        self.guide_cache.clear();
    }

    /// Sheet used for the calibration test page: the calibration paper with the current orientation.
    fn calibration_sheet(&self) -> Sheet {
        Sheet { paper: self.settings.calibration_paper, ..self.sheet().clone() }
    }

    /// Pre-fill both measured lengths with the nominal rule length, which is returned.
    fn reset_measured(&mut self) -> String {
        let rule = trim_float(TestPage::new(&self.calibration_sheet()).rule, 0);
        self.texts.insert(Field::MeasuredX, rule.clone());
        self.texts.insert(Field::MeasuredY, rule.clone());
        rule
    }

    /// Replace the whole sheet (a preset or the defaults) and refresh everything that mirrors it.
    fn set_sheet(&mut self, sheet: Sheet) {
        self.bg_mode = bg_mode_of(sheet.background);
        if let (Bg::Custom, Some(rgb)) = (self.bg_mode, sheet.background) {
            self.bg_hex = hex_of(rgb);
        }
        self.settings.sheet = sheet;
        self.zoom = None;
        self.sync_texts();
        self.sheet_changed();
    }

    fn add_image(&mut self, path: PathBuf) -> Task<Message> {
        self.load(path, Message::Loaded)
    }

    /// Decode `path` off the UI thread. `loading` counts the decodes in flight; `finish_load` counts them out.
    fn load(&mut self, path: PathBuf, then: impl FnOnce(Result<Loaded, String>) -> Message + Send + 'static) -> Task<Message> {
        self.loading += 1;
        Task::perform(async move { images::load(&path) }, then)
    }
}

fn bg_mode_of(bg: Option<[f32; 3]>) -> Bg {
    match bg {
        None => Bg::None,
        Some([1.0, 1.0, 1.0]) => Bg::White,
        Some(_) => Bg::Custom,
    }
}

fn hex_of(rgb: [f32; 3]) -> String {
    let [r, g, b] = rgb.map(|v| (v * 255.0).round() as u8);
    format!("#{r:02X}{g:02X}{b:02X}")
}

fn parse_hex(s: &str) -> Option<[f32; 3]> {
    let h = s.trim().trim_start_matches('#');
    if h.len() != 6 {
        return None;
    }
    let v = u32::from_str_radix(h, 16).ok()?;
    Some([16, 8, 0].map(|shift| ((v >> shift) & 0xFF) as f32 / 255.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_round_trip() {
        assert_eq!(parse_hex("#F4E7D3").map(hex_of).as_deref(), Some("#F4E7D3"));
        assert_eq!(parse_hex("#F4E7"), None);
    }
}
