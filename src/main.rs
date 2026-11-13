#![windows_subsystem = "windows"]
//! Button Badger — print templates for button badges.

mod images;
mod layout;
mod pdf;
mod preview;
mod print;
mod raster;
mod settings;
mod theme;
mod view;

use images::{Loaded, wrap_degrees};
use layout::{Fit, LineStyle, Orientation, PRESETS, Packing, Paper, Sheet, TestPage, Units, trim_float};
use pdf::{Document, Pages};
use preview::Interaction;
use settings::{Preset, Settings};

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
            icon: window::icon::from_file_data(include_bytes!("../packaging/icon-256.png"), None).ok(),
            ..Default::default()
        })
        .centered()
        .run()
}

/// Every numeric text field. Each keeps its own text so half-typed values ("3.") survive.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum Field {
    Face,
    Bleed,
    Gap,
    Margin,
    PaperW,
    PaperH,
    Weight,
    MeasuredX,
    MeasuredY,
    Copies(usize),
    Scale(usize),
    Rotation(usize),
}

impl Field {
    const FIXED: [Field; 9] = [Field::Face, Field::Bleed, Field::Gap, Field::Margin, Field::PaperW, Field::PaperH, Field::Weight, Field::MeasuredX, Field::MeasuredY];

    fn id(self) -> iced::widget::Id {
        iced::widget::Id::from(format!("field-{self:?}"))
    }

    /// Step size in the field's display unit.
    fn step(self, units: Units) -> f32 {
        match self {
            Field::Weight => 0.05,
            Field::Copies(_) => 1.0,
            Field::Scale(_) => 5.0,
            Field::Rotation(_) => 90.0,
            Field::MeasuredX | Field::MeasuredY => 0.5,
            _ => match units {
                Units::Mm => 0.5,
                Units::Inch => 0.05,
            },
        }
    }

    fn decimals(self, units: Units) -> usize {
        match self {
            Field::Copies(_) | Field::Scale(_) | Field::Rotation(_) => 0,
            Field::Weight => 2,
            _ if units == Units::Inch => 3,
            _ => 1,
        }
    }
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
    /// Up/Down pressed: step whichever field has focus (found via `FocusProbe`).
    ArrowStep(f32),
    FocusProbe(Field, f32, bool),
    // Sheet settings
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
    DismissToast,
    Tick(Instant),
    ToggleDark,
    /// Dev aid: `BUTTON_BADGER_SCREENSHOT=out.png` saves a window capture a few seconds after launch and exits.
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
    screenshot: Option<(PathBuf, Instant)>,
}

const CUSTOM_BADGE: &str = "Custom";
const DEFAULT_CUSTOM_BG: &str = "#F4E7D3";

impl App {
    fn boot() -> (App, Task<Message>) {
        let settings = Settings::load();
        let dark = settings.dark.unwrap_or_else(|| matches!(dark_light::detect(), Ok(dark_light::Mode::Dark)));
        let mut app = App {
            dark,
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
            toast: None,
            loading: 0,
            screenshot: std::env::var_os("BUTTON_BADGER_SCREENSHOT").map(|p| (PathBuf::from(p), Instant::now() + Duration::from_secs(4))),
        };
        app.sync_texts();
        app.reset_measured();
        app.relayout();
        // Files passed on the command line (e.g. "Open with…") load straight away.
        let args: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).filter(|p| p.is_file()).collect();
        let task = Task::batch(args.into_iter().map(|p| app.add_image(p)));
        (app, task)
    }

    fn theme(&self) -> Theme {
        theme::make_theme(self.dark)
    }

    fn subscription(&self) -> Subscription<Message> {
        let events = event::listen_with(|event, status, _| {
            use keyboard::{Key, key::Named};
            match event {
                Event::Window(window::Event::FileDropped(path)) => Some(Message::DroppedFile(path)),
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
        if self.toast.is_some() || self.screenshot.is_some() {
            Subscription::batch([events, time::every(Duration::from_millis(400)).map(Message::Tick)])
        } else {
            events
        }
    }

    // ---- derived state ----

    fn sheet(&self) -> &Sheet {
        &self.settings.sheet
    }

    fn units(&self) -> Units {
        self.sheet().units
    }

    fn text(&self, f: Field) -> &str {
        self.texts.get(&f).map(String::as_str).unwrap_or("")
    }

    /// The sheet changed: re-composite every badge, re-layout and persist.
    fn sheet_changed(&mut self) {
        self.badges = self.images.iter().map(|img| self.compose(img)).collect();
        self.relayout();
        self.settings.save();
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

    /// Reformat every sheet field from the model (after units, presets or sliders change it).
    fn sync_texts(&mut self) {
        let s = self.sheet();
        let u = s.units;
        let values = [
            (Field::Face, u.format(s.face_mm)),
            (Field::Bleed, u.format(s.bleed_mm)),
            (Field::Gap, u.format(s.gap_mm)),
            (Field::Margin, u.format(s.margin_mm)),
            (Field::PaperW, u.format(s.custom_paper_mm.0)),
            (Field::PaperH, u.format(s.custom_paper_mm.1)),
            (Field::Weight, trim_float(s.guides.weight_pt, 2)),
        ];
        self.texts.extend(values);
    }

    fn sync_image_texts(&mut self) {
        self.texts.retain(|f, _| !matches!(f, Field::Copies(_) | Field::Scale(_) | Field::Rotation(_)));
        for (i, img) in self.images.iter().enumerate() {
            self.texts.insert(Field::Copies(i), img.qty.to_string());
            self.texts.insert(Field::Scale(i), trim_float(img.zoom * 100.0, 0));
            self.texts.insert(Field::Rotation(i), trim_float(img.rotation, 0));
        }
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

    /// Apply typed text to the model (copies, scale, rotation or a dimension).
    fn apply_text(&mut self, field: Field, s: String) {
        match field {
            Field::Copies(i) => {
                let digits: String = s.chars().filter(|c| c.is_ascii_digit()).take(4).collect();
                if let Some(img) = self.images.get_mut(i) {
                    img.qty = digits.parse().unwrap_or(0);
                    self.texts.insert(field, digits);
                    self.relayout();
                }
            }
            Field::Scale(i) => {
                let cleaned: String = s.chars().filter(|c| c.is_ascii_digit() || *c == '.').take(6).collect();
                let parsed = cleaned.parse::<f32>();
                self.texts.insert(field, cleaned);
                if let (Ok(v), Some(img)) = (parsed, self.images.get_mut(i))
                    && v.is_finite()
                    && v >= 1.0
                {
                    img.zoom = (v / 100.0).clamp(0.05, 20.0);
                    self.image_changed(i);
                }
            }
            Field::Rotation(i) => {
                let cleaned: String = s.chars().filter(|c| c.is_ascii_digit() || *c == '.' || *c == '-').take(7).collect();
                if let (Ok(v), Some(img)) = (cleaned.parse::<f32>(), self.images.get_mut(i))
                    && v.is_finite()
                {
                    img.rotation = wrap_degrees(v);
                    // Show the wrapped angle once it differs (e.g. 270 → -90), but leave "-" or "4" alone mid-typing.
                    let shown = if (img.rotation - v).abs() > 0.001 { trim_float(img.rotation, 0) } else { cleaned };
                    self.texts.insert(field, shown);
                    self.image_changed(i);
                } else {
                    self.texts.insert(field, cleaned);
                }
            }
            Field::MeasuredX | Field::MeasuredY => {
                // Only read when "Apply" is pressed.
                self.texts.insert(field, s.chars().filter(|c| c.is_ascii_digit() || *c == '.' || *c == ',').take(8).collect());
            }
            _ => {
                let cleaned: String = s.chars().filter(|c| c.is_ascii_digit() || *c == '.' || *c == ',').take(8).collect();
                let parsed = parse_decimal(&cleaned);
                self.texts.insert(field, cleaned);
                if let Some(v) = parsed {
                    let v = if field == Field::Weight { v } else { self.units().to_mm(v) };
                    self.set_dimension(field, v);
                    self.sheet_changed();
                }
            }
        }
    }

    /// Nudge a numeric field by `dir` steps and re-apply it.
    fn step_field(&mut self, field: Field, dir: f32) {
        let current = parse_decimal(self.text(field)).unwrap_or(0.0);
        let step = field.step(self.units());
        let next = (current / step).round() * step + dir * step;
        let next = if matches!(field, Field::Rotation(_)) { next } else { next.max(0.0) };
        self.apply_text(field, trim_float(next, field.decimals(self.units())));
    }

    /// Set a sheet dimension, clamped to a sane range. Lengths in mm, weight in points.
    fn set_dimension(&mut self, field: Field, v: f32) {
        let s = &mut self.settings.sheet;
        match field {
            Field::Face => s.face_mm = v.clamp(5.0, 300.0),
            Field::Bleed => s.bleed_mm = v.clamp(0.0, 50.0),
            Field::Gap => s.gap_mm = v.clamp(0.0, 100.0),
            Field::Margin => s.margin_mm = v.clamp(0.0, 100.0),
            Field::PaperW => s.custom_paper_mm.0 = v.clamp(20.0, 2000.0),
            Field::PaperH => s.custom_paper_mm.1 = v.clamp(20.0, 2000.0),
            Field::Weight => s.guides.weight_pt = v.clamp(0.05, 5.0),
            Field::MeasuredX | Field::MeasuredY | Field::Copies(_) | Field::Scale(_) | Field::Rotation(_) => {}
        }
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
        self.loading += 1;
        Task::perform(async move { images::load(&path) }, Message::Loaded)
    }

    fn default_export_name(&self) -> String {
        let base = self.images.first().map_or("badges", |i| i.name.as_str());
        let s = self.sheet();
        format!("{base} · {}mm badge · {}.pdf", trim_float(s.face_mm, 1), s.paper.to_string().trim_end_matches('…'))
    }

    // ---- update ----

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Open => {
                let dlg = image_dialog("Choose images", self.settings.last_dir.clone());
                return Task::perform(
                    async move { dlg.pick_files().await.map(|fs| fs.iter().map(|f| f.path().to_path_buf()).collect()).unwrap_or_default() },
                    Message::FilesPicked,
                );
            }
            Message::FilesPicked(paths) => return Task::batch(paths.into_iter().map(|p| self.add_image(p))),
            Message::DroppedFile(path) | Message::OpenRecent(path) => return self.add_image(path),
            Message::Loaded(result) => {
                self.loading = self.loading.saturating_sub(1);
                match result {
                    Ok(img) => {
                        self.settings.remember(&img.path);
                        self.badges.push(self.compose(&img));
                        self.images.push(img);
                        self.sync_image_texts();
                        self.relayout();
                    }
                    Err(e) => self.toast = Toast::error(e),
                }
            }
            Message::Replace(i) => {
                let dlg = image_dialog("Replace image", self.settings.last_dir.clone());
                return Task::perform(async move { dlg.pick_file().await.map(|f| f.path().to_path_buf()) }, move |p| Message::ReplaceWith(i, p));
            }
            Message::ReplaceWith(i, Some(path)) => {
                self.loading += 1;
                return Task::perform(async move { images::load(&path) }, move |r| Message::Replaced(i, r));
            }
            Message::ReplaceWith(_, None) => {}
            Message::Replaced(i, result) => {
                self.loading = self.loading.saturating_sub(1);
                match result {
                    Ok(mut img) => {
                        if let Some(old) = self.images.get(i) {
                            // Keep the placement and quantity; only the pixels change.
                            img.qty = old.qty;
                            img.zoom = old.zoom;
                            img.offset_mm = old.offset_mm;
                            img.rotation = old.rotation;
                            self.settings.remember(&img.path);
                            self.images[i] = img;
                            self.image_changed(i);
                        }
                    }
                    Err(e) => self.toast = Toast::error(e),
                }
            }
            Message::Remove(i) => {
                if i < self.images.len() {
                    self.images.remove(i);
                    self.badges.remove(i);
                    self.sync_image_texts();
                    self.relayout();
                }
            }
            Message::ResetPlacement(i) => {
                if let Some(img) = self.images.get_mut(i) {
                    img.reset_placement();
                    self.sync_image_texts();
                    self.image_changed(i);
                }
            }

            Message::Text(field, s) => self.apply_text(field, s),
            Message::Slide(field, v) => {
                self.set_dimension(field, v);
                self.sync_texts();
                self.sheet_changed();
            }
            Message::Step(field, dir) => self.step_field(field, dir),
            Message::ArrowStep(dir) => {
                let fields = Field::FIXED.into_iter().chain((0..self.images.len()).flat_map(|i| [Field::Copies(i), Field::Scale(i), Field::Rotation(i)]));
                return Task::batch(fields.map(|f| iced::widget::operation::is_focused(f.id()).map(move |yes| Message::FocusProbe(f, dir, yes))));
            }
            Message::FocusProbe(field, dir, true) => self.step_field(field, dir),
            Message::FocusProbe(_, _, false) => {}

            Message::BadgePreset(name) => {
                if let Some(p) = PRESETS.iter().find(|p| p.name == name) {
                    self.settings.sheet.face_mm = p.face_mm;
                    self.settings.sheet.bleed_mm = p.bleed_mm;
                    self.sync_texts();
                    self.sheet_changed();
                }
            }
            Message::Paper(p) => {
                self.settings.sheet.paper = p;
                self.settings.calibration_paper = p;
                self.zoom = None;
                self.sheet_changed();
            }
            Message::Orientation(o) => {
                self.settings.sheet.orientation = o;
                self.zoom = None;
                self.sheet_changed();
            }
            Message::Packing(p) => {
                self.settings.sheet.packing = p;
                self.sheet_changed();
            }
            Message::Units(u) => {
                self.settings.sheet.units = u;
                self.sync_texts();
                self.sheet_changed();
            }
            Message::CutStyle(s) => {
                self.settings.sheet.guides.cut = s;
                self.sheet_changed();
            }
            Message::Fit(f) => {
                self.settings.sheet.fit = f;
                self.sheet_changed();
            }
            Message::Toggle(t, v) => {
                let g = &mut self.settings.sheet.guides;
                match t {
                    Toggle::FaceLine => g.face = v,
                    Toggle::CropMarks => g.crop_marks = v,
                    Toggle::Clip => g.clip = v,
                }
                self.sheet_changed();
            }
            Message::Background(bg) => {
                self.bg_mode = bg;
                self.settings.sheet.background = match bg {
                    Bg::None => None,
                    Bg::White => Some([1.0; 3]),
                    Bg::Custom => parse_hex(&self.bg_hex).or(parse_hex(DEFAULT_CUSTOM_BG)),
                };
                self.sheet_changed();
            }
            Message::BgSwatch(rgb) => {
                self.bg_hex = hex_of(rgb);
                self.bg_mode = Bg::Custom;
                self.settings.sheet.background = Some(rgb);
                self.sheet_changed();
            }
            Message::BgHex(s) => {
                let digits: String = s.chars().filter(char::is_ascii_hexdigit).take(6).collect();
                self.bg_hex = format!("#{digits}");
                if let Some(rgb) = parse_hex(&self.bg_hex)
                    && self.bg_mode == Bg::Custom
                {
                    self.settings.sheet.background = Some(rgb);
                    self.sheet_changed();
                }
            }

            Message::Preview(interaction) => match interaction {
                Interaction::Zoom(z, pan) => {
                    self.zoom = Some(z);
                    self.pan = pan;
                    self.invalidate();
                }
                Interaction::Pan(p) => {
                    self.pan = p;
                    self.invalidate();
                }
                Interaction::Hover(cell) => self.hover = cell,
                Interaction::DragEnd => self.dragging = None,
                Interaction::ImageOffset(i, off) => {
                    if let Some(img) = self.images.get_mut(i) {
                        img.offset_mm = off;
                        self.dragging = Some(i);
                        self.image_changed(i);
                    }
                }
                Interaction::ImageZoom(i, z) | Interaction::ScaleDrag(i, z) => {
                    if let Some(img) = self.images.get_mut(i) {
                        img.zoom = z;
                        if matches!(interaction, Interaction::ScaleDrag(..)) {
                            self.dragging = Some(i);
                        }
                        self.sync_image_texts();
                        self.image_changed(i);
                    }
                }
                Interaction::RotateDrag(i, deg) => {
                    if let Some(img) = self.images.get_mut(i) {
                        img.rotation = deg;
                        self.dragging = Some(i);
                        self.texts.insert(Field::Rotation(i), trim_float(deg, 0));
                        self.image_changed(i);
                    }
                }
            },
            Message::ZoomBy(factor) => {
                // About the canvas centre; 3 px/mm stands in for "fit" when no zoom is set yet.
                self.zoom = Some((self.zoom.unwrap_or(3.0) * factor).clamp(0.2, 60.0));
                self.pan *= factor;
                self.invalidate();
            }
            Message::ZoomFit => {
                self.zoom = None;
                self.pan = Vector::ZERO;
                self.invalidate();
            }
            Message::Page(d) => {
                let last = self.pages.len() as isize - 1;
                self.page = (self.page as isize + d).clamp(0, last) as usize;
                self.cache.clear();
            }

            Message::Export => {
                if self.images.is_empty() {
                    self.toast = Toast::error("Add an image first");
                    return Task::none();
                }
                let dir = self.settings.last_dir.clone().or_else(|| std::env::home_dir().map(|h| h.join("Desktop")));
                let dlg = file_dialog("Export PDF", dir).set_file_name(self.default_export_name()).add_filter("PDF", &["pdf"]);
                return Task::perform(async move { dlg.save_file().await.map(|f| f.path().to_path_buf()) }, Message::ExportTo);
            }
            Message::ExportTo(Some(path)) => {
                // Images hold their pixels in `Arc`s, so these clones are cheap.
                let (sheet, images, pages, calibration) = (self.sheet().clone(), self.images.clone(), self.pages.clone(), self.settings.calibration);
                return Task::perform(
                    async move {
                        let doc = Document { sheet: &sheet, images: &images, pages: Pages::Badges(&pages), calibration, title: "Button Badger" };
                        std::fs::write(&path, doc.pdf()).map(|_| path).map_err(|e| e.to_string())
                    },
                    Message::Exported,
                );
            }
            Message::ExportTo(None) => {}
            Message::Exported(Ok(path)) => {
                let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                self.toast = Toast::info(format!("Saved {name}"));
                if let Some(t) = &mut self.toast {
                    t.path = Some(path);
                }
            }
            Message::Exported(Err(e)) => self.toast = Toast::error(format!("Couldn't save: {e}")),
            Message::Print => {
                if self.images.is_empty() {
                    self.toast = Toast::error("Add an image first");
                    return Task::none();
                }
                let doc = Document { sheet: self.sheet(), images: &self.images, pages: Pages::Badges(&self.pages), calibration: self.settings.calibration, title: "Button Badger" };
                self.toast = Toast::result(print::print(&doc));
            }
            Message::PrintTest => {
                let rule = self.reset_measured();
                let sheet = self.calibration_sheet();
                let doc = Document { sheet: &sheet, images: &[], pages: Pages::TestPage, calibration: self.settings.calibration, title: "Button Badger calibration" };
                self.toast = Toast::result(print::print(&doc).map(|msg| format!("{msg}. Measure the two {rule} mm rules and enter them in the calibration card.")));
            }
            Message::Reveal(path) => print::reveal(&path),

            Message::PresetName(s) => self.preset_name = s,
            Message::SavePreset => {
                let name = self.preset_name.trim().to_string();
                if name.is_empty() {
                    return Task::none();
                }
                let presets = &mut self.settings.presets;
                presets.retain(|p| p.name != name);
                presets.push(Preset { name: name.clone(), sheet: self.settings.sheet.clone() });
                presets.sort_by(|a, b| a.name.cmp(&b.name));
                self.settings.save();
                self.toast = Toast::info(format!("Preset “{name}” saved"));
                self.selected_preset = Some(name);
                self.preset_name.clear();
            }
            Message::LoadPreset(name) => {
                if let Some(p) = self.settings.presets.iter().find(|p| p.name == name) {
                    self.set_sheet(p.sheet.clone());
                    self.selected_preset = Some(name);
                }
            }
            Message::DeletePreset => {
                if let Some(name) = &self.selected_preset {
                    return confirm("Delete this setup?", format!("“{name}” will be removed. This can't be undone."), "Delete", Message::DeletePresetConfirmed);
                }
            }
            Message::DeletePresetConfirmed(yes) => {
                if yes && let Some(name) = self.selected_preset.take() {
                    self.settings.presets.retain(|p| p.name != name);
                    self.settings.save();
                    self.toast = Toast::info(format!("Deleted “{name}”"));
                }
            }
            Message::ResetDefaults => {
                return confirm(
                    "Reset to defaults?",
                    "Badge, paper and guide settings go back to their defaults. Your images, saved setups and printer calibration are kept.".into(),
                    "Reset",
                    Message::ResetDefaultsConfirmed,
                );
            }
            Message::ResetDefaultsConfirmed(yes) => {
                if yes {
                    self.set_sheet(Sheet::default());
                    self.settings.calibration_paper = self.sheet().paper;
                    self.reset_measured();
                    self.selected_preset = None;
                    self.pan = Vector::ZERO;
                    self.toast = Toast::info("Settings reset to defaults");
                }
            }
            Message::CalibrationPaper(p) => {
                self.settings.calibration_paper = p;
                self.reset_measured();
                self.settings.save();
            }
            Message::ApplyCalibration => {
                let rule = TestPage::new(&self.calibration_sheet()).rule;
                // Reject typos: a real printer is never off by 50%.
                let measured = |f| parse_decimal(self.text(f)).filter(|v| (rule * 0.5..=rule * 1.5).contains(v));
                match (measured(Field::MeasuredX), measured(Field::MeasuredY)) {
                    (Some(x), Some(y)) => {
                        self.settings.calibration = (rule / x, rule / y);
                        self.settings.save();
                        self.toast = Toast::info(format!("Calibrated: ×{:.4} across, ×{:.4} down", rule / x, rule / y));
                    }
                    _ => self.toast = Toast::error(format!("Enter the measured lengths of the {} mm rules, in mm", trim_float(rule, 0))),
                }
            }
            Message::ResetCalibration => {
                self.settings.calibration = (1.0, 1.0);
                self.reset_measured();
                self.settings.save();
            }

            Message::DismissToast => self.toast = None,
            Message::Tick(now) => {
                if self.toast.as_ref().is_some_and(|t| now >= t.until) {
                    self.toast = None;
                }
                if self.screenshot.as_ref().is_some_and(|(_, at)| now >= *at) {
                    return window::latest().and_then(window::screenshot).map(Message::Screenshot);
                }
            }
            Message::ToggleDark => {
                self.dark = !self.dark;
                self.settings.dark = Some(self.dark);
                self.settings.save();
                self.invalidate();
            }
            Message::Screenshot(shot) => {
                if let Some((path, _)) = self.screenshot.take() {
                    let _ = image::save_buffer(&path, &shot.rgba, shot.size.width, shot.size.height, image::ColorType::Rgba8);
                    return iced::exit();
                }
            }
        }
        Task::none()
    }
}

fn file_dialog(title: &str, dir: Option<PathBuf>) -> rfd::AsyncFileDialog {
    let dlg = rfd::AsyncFileDialog::new().set_title(title);
    match dir {
        Some(d) => dlg.set_directory(d),
        None => dlg,
    }
}

/// Open dialog limited to supported images. Windows and Linux list filters in a dropdown, so "All files" is
/// offered there; macOS (rfd) merges every filter into one allowed-types list, where it would cancel the filtering.
fn image_dialog(title: &str, dir: Option<PathBuf>) -> rfd::AsyncFileDialog {
    let dlg = file_dialog(title, dir).add_filter("Images", images::EXTENSIONS);
    if cfg!(target_os = "macos") { dlg } else { dlg.add_filter("All files", &["*"]) }
}

/// Ask in a native dialog; `then` receives whether `action` (vs Cancel) was chosen.
fn confirm(title: &str, description: String, action: &'static str, then: fn(bool) -> Message) -> Task<Message> {
    let dialog = rfd::AsyncMessageDialog::new()
        .set_level(rfd::MessageLevel::Warning)
        .set_title(title)
        .set_description(description)
        .set_buttons(rfd::MessageButtons::OkCancelCustom(action.into(), "Cancel".into()));
    Task::perform(
        async move {
            match dialog.show().await {
                rfd::MessageDialogResult::Ok => true,
                rfd::MessageDialogResult::Custom(s) => s == action,
                _ => false,
            }
        },
        then,
    )
}

/// Parse a number typed with either decimal separator.
fn parse_decimal(s: &str) -> Option<f32> {
    s.replace(',', ".").parse::<f32>().ok().filter(|v| v.is_finite())
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
    fn hex_round_trip_and_decimals() {
        assert_eq!(parse_hex("#F4E7D3").map(hex_of).as_deref(), Some("#F4E7D3"));
        assert_eq!(parse_hex("#F4E7"), None);
        assert_eq!(parse_decimal("2,5"), Some(2.5));
        assert_eq!(parse_decimal("."), None);
    }
}
