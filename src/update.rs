//! What each message does to the app. `update` hands every group of messages to its own handler: images, the
//! sheet and the preview here, numeric fields in `fields`, export and print in `output`, setups in `setups`.

use crate::fields::Field;
use crate::images::{self, Loaded};
use crate::layout::{Fit, LineStyle, Orientation, PRESETS, Packing, Paper, Units, trim_float};
use crate::preview::Interaction;
use crate::{App, Bg, DEFAULT_CUSTOM_BG, Message, Toast, hex_of, parse_hex};
use iced::{Task, Vector};
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub enum ImageMsg {
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
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Toggle {
    FaceLine,
    CropMarks,
    Clip,
}

/// One change to the sheet's settings. `App::edit_sheet` applies it, then refreshes everything derived from the sheet.
#[derive(Debug, Clone)]
pub enum SheetEdit {
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

impl App {
    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Image(msg) => return self.image(msg).map(Message::Image),
            Message::Field(edit) => return self.edit_field(edit).map(Message::Field),
            Message::Sheet(edit) => self.edit_sheet(edit),
            Message::Output(msg) => return self.output(msg).map(Message::Output),
            Message::Setup(msg) => return self.setup(msg).map(Message::Setup),

            // Preview
            Message::Preview(interaction) => self.interact(interaction),
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

            // Misc
            Message::FocusLost => self.flush(None),
            Message::DismissToast => self.toast = None,
            Message::Tick(now) => {
                if self.toast.as_ref().is_some_and(|t| now >= t.until) {
                    self.toast = None;
                }
                self.flush(Some(now));
                #[cfg(debug_assertions)]
                {
                    if self.screenshot.as_ref().is_some_and(|(_, at)| now >= *at) {
                        return iced::window::latest().and_then(iced::window::screenshot).map(Message::Screenshot);
                    }
                }
            }
            Message::SystemTheme(mode) => {
                if self.settings.dark.is_none() {
                    self.dark = mode == iced::theme::Mode::Dark;
                    self.invalidate();
                }
            }
            Message::ToggleDark => {
                self.dark = !self.dark;
                self.settings.dark = Some(self.dark);
                self.save();
                self.invalidate();
            }
            #[cfg(debug_assertions)]
            Message::Screenshot(shot) => {
                if let Some((path, _)) = self.screenshot.take() {
                    let _ = image::save_buffer(&path, &shot.rgba, shot.size.width, shot.size.height, image::ColorType::Rgba8);
                    return iced::exit();
                }
            }
        }
        Task::none()
    }

    /// Add, replace, remove or reset an image.
    fn image(&mut self, msg: ImageMsg) -> Task<ImageMsg> {
        match msg {
            ImageMsg::Open => {
                let dlg = image_dialog("Choose images", self.settings.last_dir.clone());
                return Task::perform(
                    async move { dlg.pick_files().await.map(|fs| fs.iter().map(|f| f.path().to_path_buf()).collect()).unwrap_or_default() },
                    ImageMsg::FilesPicked,
                );
            }
            ImageMsg::FilesPicked(paths) => return Task::batch(paths.into_iter().map(|p| self.add_image(p))),
            ImageMsg::DroppedFile(path) | ImageMsg::OpenRecent(path) => return self.add_image(path),
            ImageMsg::Loaded(result) => {
                if let Some(img) = self.finish_load(result) {
                    self.remember(&img);
                    self.badges.push(self.compose(&img));
                    self.images.push(img);
                    self.sync_image_texts();
                    self.relayout();
                }
            }
            ImageMsg::Replace(i) => {
                let dlg = image_dialog("Replace image", self.settings.last_dir.clone());
                return Task::perform(async move { dlg.pick_file().await.map(|f| f.path().to_path_buf()) }, move |p| ImageMsg::ReplaceWith(i, p));
            }
            ImageMsg::ReplaceWith(i, Some(path)) => return self.load(path, move |r| ImageMsg::Replaced(i, r)),
            ImageMsg::ReplaceWith(_, None) => {}
            ImageMsg::Replaced(i, result) => {
                if let (Some(mut img), Some(old)) = (self.finish_load(result), self.images.get(i)) {
                    // Keep the placement and quantity; only the pixels change.
                    img.qty = old.qty;
                    img.zoom = old.zoom;
                    img.offset_mm = old.offset_mm;
                    img.rotation = old.rotation;
                    self.remember(&img);
                    self.images[i] = img;
                    self.image_changed(i);
                }
            }
            ImageMsg::Remove(i) => {
                if i < self.images.len() {
                    self.images.remove(i);
                    self.badges.remove(i);
                    self.sync_image_texts();
                    self.relayout();
                }
            }
            ImageMsg::ResetPlacement(i) => {
                if let Some(img) = self.images.get_mut(i) {
                    img.reset_placement();
                    self.sync_image_texts();
                    self.image_changed(i);
                }
            }
        }
        Task::none()
    }

    /// Apply one sheet setting, then refresh what's derived from the sheet. An arm that changes nothing returns early.
    fn edit_sheet(&mut self, edit: SheetEdit) {
        let sheet = &mut self.settings.sheet;
        match edit {
            SheetEdit::BadgePreset(name) => {
                let Some(p) = PRESETS.iter().find(|p| p.name == name) else { return };
                sheet.face_mm = p.face_mm;
                sheet.bleed_mm = p.bleed_mm;
                self.sync_texts();
            }
            SheetEdit::Paper(p) => {
                sheet.paper = p;
                self.settings.calibration_paper = p;
                self.zoom = None;
            }
            SheetEdit::Orientation(o) => {
                sheet.orientation = o;
                self.zoom = None;
            }
            SheetEdit::Packing(p) => sheet.packing = p,
            SheetEdit::Units(u) => {
                sheet.units = u;
                self.sync_texts();
            }
            SheetEdit::CutStyle(s) => sheet.guides.cut = s,
            SheetEdit::Fit(f) => sheet.fit = f,
            SheetEdit::Toggle(t, v) => match t {
                Toggle::FaceLine => sheet.guides.face = v,
                Toggle::CropMarks => sheet.guides.crop_marks = v,
                Toggle::Clip => sheet.guides.clip = v,
            },
            SheetEdit::Background(bg) => {
                self.bg_mode = bg;
                sheet.background = match bg {
                    Bg::None => None,
                    Bg::White => Some([1.0; 3]),
                    Bg::Custom => parse_hex(&self.bg_hex).or(parse_hex(DEFAULT_CUSTOM_BG)),
                };
            }
            SheetEdit::BgSwatch(rgb) => {
                self.bg_hex = hex_of(rgb);
                self.bg_mode = Bg::Custom;
                sheet.background = Some(rgb);
            }
            SheetEdit::BgHex(s) => {
                let digits: String = s.chars().filter(char::is_ascii_hexdigit).take(6).collect();
                self.bg_hex = format!("#{digits}");
                match parse_hex(&self.bg_hex) {
                    Some(rgb) if self.bg_mode == Bg::Custom => sheet.background = Some(rgb),
                    _ => return,
                }
            }
        }
        self.sheet_changed();
    }

    /// The mouse did something on the preview canvas.
    fn interact(&mut self, interaction: Interaction) {
        match interaction {
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
        }
    }

    pub fn add_image(&mut self, path: PathBuf) -> Task<ImageMsg> {
        self.load(path, ImageMsg::Loaded)
    }

    /// Decode `path` off the UI thread. `loading` counts the decodes in flight; `finish_load` counts them out.
    fn load(&mut self, path: PathBuf, then: impl FnOnce(Result<Loaded, String>) -> ImageMsg + Send + 'static) -> Task<ImageMsg> {
        self.loading += 1;
        Task::perform(async move { images::load(&path) }, then)
    }

    /// A decode ended: count it out, and put a failure in the toast.
    fn finish_load(&mut self, result: Result<Loaded, String>) -> Option<Loaded> {
        self.loading = self.loading.saturating_sub(1);
        result.map_err(|e| self.toast = Some(Toast::error(e))).ok()
    }

    /// Add `img` to the recent files.
    fn remember(&mut self, img: &Loaded) {
        let saved = self.settings.remember(&img.path);
        self.saved(saved);
    }
}

pub fn file_dialog(title: &str, dir: Option<PathBuf>) -> rfd::AsyncFileDialog {
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
