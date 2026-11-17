//! Export and print: the sheet's PDF goes to a file or to the system print dialog.

use crate::layout::trim_float;
use crate::pdf::{Document, Pages};
use crate::update::file_dialog;
use crate::{App, Toast, print};
use iced::Task;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub enum OutputMsg {
    Export,
    ExportTo(Option<PathBuf>),
    Exported(Result<PathBuf, String>),
    Print,
    PrintTest,
    Printed(Result<String, String>),
    Reveal(PathBuf),
}

impl App {
    pub fn output(&mut self, msg: OutputMsg) -> Task<OutputMsg> {
        match msg {
            OutputMsg::Export => {
                if self.images.is_empty() {
                    self.toast = Some(Toast::error("Add an image first"));
                    return Task::none();
                }
                let dir = self.settings.last_dir.clone().or_else(|| std::env::home_dir().map(|h| h.join("Desktop")));
                let dlg = file_dialog("Export PDF", dir).set_file_name(self.default_export_name()).add_filter("PDF", &["pdf"]);
                return Task::perform(async move { dlg.save_file().await.map(|f| f.path().to_path_buf()) }, OutputMsg::ExportTo);
            }
            OutputMsg::ExportTo(Some(path)) => {
                // Images hold their pixels in `Arc`s, so these clones are cheap.
                let (sheet, images, pages, calibration) = (self.sheet().clone(), self.images.clone(), self.pages.clone(), self.settings.calibration);
                return Task::perform(
                    async move {
                        let doc = Document { sheet: &sheet, images: &images, pages: Pages::Badges(&pages), calibration, title: "Button Badger" };
                        std::fs::write(&path, doc.pdf()).map(|_| path).map_err(|e| e.to_string())
                    },
                    OutputMsg::Exported,
                );
            }
            OutputMsg::ExportTo(None) => {}
            OutputMsg::Exported(Ok(path)) => {
                let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                self.toast = Some(Toast { path: Some(path), ..Toast::info(format!("Saved {name}")) });
            }
            OutputMsg::Exported(Err(e)) => self.toast = Some(Toast::error(format!("Couldn't save: {e}"))),
            OutputMsg::Print => {
                if self.images.is_empty() {
                    self.toast = Some(Toast::error("Add an image first"));
                    return Task::none();
                }
                let doc = Document { sheet: self.sheet(), images: &self.images, pages: Pages::Badges(&self.pages), calibration: self.settings.calibration, title: "Button Badger" };
                return print_task(&doc, String::new());
            }
            OutputMsg::PrintTest => {
                let rule = self.reset_measured();
                let sheet = self.calibration_sheet();
                let doc = Document { sheet: &sheet, images: &[], pages: Pages::TestPage, calibration: self.settings.calibration, title: "Button Badger calibration" };
                return print_task(&doc, format!(". Measure the two {rule} mm rules and enter them in the calibration card."));
            }
            OutputMsg::Printed(result) => self.toast = Some(Toast::result(result)),
            OutputMsg::Reveal(path) => print::reveal(&path),
        }
        Task::none()
    }

    fn default_export_name(&self) -> String {
        let base = self.images.first().map_or("badges", |i| i.name.as_str());
        let s = self.sheet();
        format!("{base} · {}mm badge · {}.pdf", trim_float(s.face_mm, 1), s.paper.to_string().trim_end_matches('…'))
    }
}

/// Show the system print dialog for `doc`. The toast is the status line plus `note`.
fn print_task(doc: &Document, note: String) -> Task<OutputMsg> {
    let done = move |r: Result<String, String>| OutputMsg::Printed(r.map(|msg| msg + &note));
    // The macOS and Windows dialogs are modal and block; they have to run here, on the main thread.
    #[cfg(any(target_os = "macos", windows))]
    return Task::done(done(print::print(doc)));
    #[cfg(not(any(target_os = "macos", windows)))]
    Task::perform(print::print(doc.pdf(), doc.title.to_string()), done)
}
