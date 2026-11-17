//! Saved setups (sheet presets), reset to defaults, and printer calibration.

use crate::fields::{Field, parse_decimal};
use crate::layout::{Paper, Sheet, TestPage, trim_float};
use crate::settings::Preset;
use crate::{App, Toast};
use iced::{Task, Vector};

#[derive(Debug, Clone)]
pub enum SetupMsg {
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
}

impl App {
    pub fn setup(&mut self, msg: SetupMsg) -> Task<SetupMsg> {
        match msg {
            SetupMsg::PresetName(s) => self.preset_name = s,
            SetupMsg::SavePreset => {
                let name = self.preset_name.trim().to_string();
                if name.is_empty() {
                    return Task::none();
                }
                let presets = &mut self.settings.presets;
                presets.retain(|p| p.name != name);
                presets.push(Preset { name: name.clone(), sheet: self.settings.sheet.clone() });
                presets.sort_by(|a, b| a.name.cmp(&b.name));
                self.toast = Some(Toast::info(format!("Setup “{name}” saved")));
                self.selected_preset = Some(name);
                self.preset_name.clear();
                self.save();
            }
            SetupMsg::LoadPreset(name) => {
                if let Some(p) = self.settings.presets.iter().find(|p| p.name == name) {
                    self.set_sheet(p.sheet.clone());
                    self.selected_preset = Some(name);
                }
            }
            SetupMsg::DeletePreset => {
                if let Some(name) = &self.selected_preset {
                    return confirm("Delete this setup?", format!("“{name}” will be removed. This can't be undone."), "Delete", SetupMsg::DeletePresetConfirmed);
                }
            }
            SetupMsg::DeletePresetConfirmed(yes) => {
                if yes && let Some(name) = self.selected_preset.take() {
                    self.settings.presets.retain(|p| p.name != name);
                    self.toast = Some(Toast::info(format!("Deleted “{name}”")));
                    self.save();
                }
            }
            SetupMsg::ResetDefaults => {
                return confirm(
                    "Reset to defaults?",
                    "Badge, paper and guide settings go back to their defaults. Your images, saved setups and printer calibration are kept.".into(),
                    "Reset",
                    SetupMsg::ResetDefaultsConfirmed,
                );
            }
            SetupMsg::ResetDefaultsConfirmed(yes) => {
                if yes {
                    self.set_sheet(Sheet::default());
                    self.settings.calibration_paper = self.sheet().paper;
                    self.reset_measured();
                    self.selected_preset = None;
                    self.pan = Vector::ZERO;
                    self.toast = Some(Toast::info("Settings reset to defaults"));
                }
            }
            SetupMsg::CalibrationPaper(p) => {
                self.settings.calibration_paper = p;
                self.reset_measured();
                self.save();
            }
            SetupMsg::ApplyCalibration => {
                let rule = TestPage::new(&self.calibration_sheet()).rule;
                // Reject typos: a real printer is never off by 50%.
                let measured = |f| parse_decimal(self.text(f)).filter(|v| (rule * 0.5..=rule * 1.5).contains(v));
                match (measured(Field::MeasuredX), measured(Field::MeasuredY)) {
                    (Some(x), Some(y)) => {
                        self.settings.calibration = (rule / x, rule / y);
                        self.toast = Some(Toast::info(format!("Calibrated: ×{:.4} across, ×{:.4} down", rule / x, rule / y)));
                        self.save();
                    }
                    _ => self.toast = Some(Toast::error(format!("Enter the measured lengths of the {} mm rules, in mm", trim_float(rule, 0)))),
                }
            }
            SetupMsg::ResetCalibration => {
                self.settings.calibration = (1.0, 1.0);
                self.reset_measured();
                self.save();
            }
        }
        Task::none()
    }
}

/// Ask in a native dialog; `then` receives whether `action` (vs Cancel) was chosen.
fn confirm(title: &str, description: String, action: &'static str, then: fn(bool) -> SetupMsg) -> Task<SetupMsg> {
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
