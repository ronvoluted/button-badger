//! What each message does to the app.

use crate::fields::{Field, parse_decimal};
use crate::images::{self, Loaded};
use crate::layout::{PRESETS, Sheet, TestPage, trim_float};
use crate::pdf::{Document, Pages};
use crate::preview::Interaction;
use crate::settings::Preset;
use crate::{App, Bg, DEFAULT_CUSTOM_BG, Message, SheetEdit, Toast, Toggle, hex_of, parse_hex, print};
use iced::advanced::widget::{operate, operation::focusable::find_focused};
use iced::{Task, Vector};
use std::path::PathBuf;

impl App {
    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            // Images
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
                if let Some(img) = self.finish_load(result) {
                    self.remember(&img);
                    self.badges.push(self.compose(&img));
                    self.images.push(img);
                    self.sync_image_texts();
                    self.relayout();
                }
            }
            Message::Replace(i) => {
                let dlg = image_dialog("Replace image", self.settings.last_dir.clone());
                return Task::perform(async move { dlg.pick_file().await.map(|f| f.path().to_path_buf()) }, move |p| Message::ReplaceWith(i, p));
            }
            Message::ReplaceWith(i, Some(path)) => return self.load(path, move |r| Message::Replaced(i, r)),
            Message::ReplaceWith(_, None) => {}
            Message::Replaced(i, result) => {
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

            // Numeric fields
            Message::Text(field, s) => self.apply_text(field, s),
            Message::Slide(field, v) => {
                self.set_dimension(field, v);
                self.sync_texts();
                self.sheet_changed();
            }
            Message::Step(field, dir) => self.step_field(field, dir),
            Message::ArrowStep(dir) => {
                // One pass over the widgets; yields nothing unless a widget with an id has focus.
                return operate(find_focused()).map(move |id| Message::StepFocused(id, dir));
            }
            Message::StepFocused(id, dir) => {
                // `texts` has an entry for every field on screen.
                if let Some(&field) = self.texts.keys().find(|f| f.id() == id) {
                    self.step_field(field, dir);
                }
            }

            // Sheet settings
            Message::Sheet(edit) => self.edit_sheet(edit),

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

            // Output
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
                return print_task(&doc, String::new());
            }
            Message::PrintTest => {
                let rule = self.reset_measured();
                let sheet = self.calibration_sheet();
                let doc = Document { sheet: &sheet, images: &[], pages: Pages::TestPage, calibration: self.settings.calibration, title: "Button Badger calibration" };
                return print_task(&doc, format!(". Measure the two {rule} mm rules and enter them in the calibration card."));
            }
            Message::Printed(result) => self.toast = Toast::result(result),
            Message::Reveal(path) => print::reveal(&path),

            // Presets and calibration
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
                self.toast = Toast::info(format!("Setup “{name}” saved"));
                self.selected_preset = Some(name);
                self.preset_name.clear();
                self.save();
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
                    self.toast = Toast::info(format!("Deleted “{name}”"));
                    self.save();
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
                self.save();
            }
            Message::ApplyCalibration => {
                let rule = TestPage::new(&self.calibration_sheet()).rule;
                // Reject typos: a real printer is never off by 50%.
                let measured = |f| parse_decimal(self.text(f)).filter(|v| (rule * 0.5..=rule * 1.5).contains(v));
                match (measured(Field::MeasuredX), measured(Field::MeasuredY)) {
                    (Some(x), Some(y)) => {
                        self.settings.calibration = (rule / x, rule / y);
                        self.toast = Toast::info(format!("Calibrated: ×{:.4} across, ×{:.4} down", rule / x, rule / y));
                        self.save();
                    }
                    _ => self.toast = Toast::error(format!("Enter the measured lengths of the {} mm rules, in mm", trim_float(rule, 0))),
                }
            }
            Message::ResetCalibration => {
                self.settings.calibration = (1.0, 1.0);
                self.reset_measured();
                self.save();
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

    /// A decode ended: count it out, and put a failure in the toast.
    fn finish_load(&mut self, result: Result<Loaded, String>) -> Option<Loaded> {
        self.loading = self.loading.saturating_sub(1);
        result.map_err(|e| self.toast = Toast::error(e)).ok()
    }

    /// Add `img` to the recent files.
    fn remember(&mut self, img: &Loaded) {
        let saved = self.settings.remember(&img.path);
        self.saved(saved);
    }

    fn default_export_name(&self) -> String {
        let base = self.images.first().map_or("badges", |i| i.name.as_str());
        let s = self.sheet();
        format!("{base} · {}mm badge · {}.pdf", trim_float(s.face_mm, 1), s.paper.to_string().trim_end_matches('…'))
    }
}

/// Show the system print dialog for `doc`. The toast is the status line plus `note`.
fn print_task(doc: &Document, note: String) -> Task<Message> {
    let done = move |r: Result<String, String>| Message::Printed(r.map(|msg| msg + &note));
    // The macOS and Windows dialogs are modal and block; they have to run here, on the main thread.
    #[cfg(any(target_os = "macos", windows))]
    return Task::done(done(print::print(doc)));
    #[cfg(not(any(target_os = "macos", windows)))]
    Task::perform(print::print(doc.pdf(), doc.title.to_string()), done)
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
