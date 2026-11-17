//! Persisted settings: last sheet, named presets, calibration, theme.

use crate::layout::{Paper, Sheet};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::{Mutex, Once};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Preset {
    pub name: String,
    pub sheet: Sheet,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub sheet: Sheet,
    pub presets: Vec<Preset>,
    pub recent: Vec<PathBuf>,
    /// Printer scale correction (x, y): 1.0 = perfect.
    pub calibration: (f32, f32),
    /// Paper the calibration test page is printed on (follows the sheet paper until edited directly).
    pub calibration_paper: Paper,
    /// None = follow the system appearance.
    pub dark: Option<bool>,
    pub last_dir: Option<PathBuf>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            sheet: Sheet::default(),
            presets: Vec::new(),
            recent: Vec::new(),
            calibration: (1.0, 1.0),
            calibration_paper: Paper::A4,
            dark: None,
            last_dir: None,
        }
    }
}

/// Settings queued by `save_later`, waiting for `flush`.
static PENDING: Mutex<Option<Settings>> = Mutex::new(None);

fn path() -> PathBuf {
    if cfg!(test) {
        return std::env::temp_dir().join("button-badger-test/settings.json");
    }
    let home = std::env::home_dir().unwrap_or_else(|| PathBuf::from("."));
    let dir = if cfg!(target_os = "macos") {
        home.join("Library/Application Support/Button Badger")
    } else if cfg!(windows) {
        std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or(home).join("Button Badger")
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".config"))
            .join("button-badger")
    };
    dir.join("settings.json")
}

impl Settings {
    pub fn load() -> Settings {
        let mut s: Settings = std::fs::read(path()).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        s.recent.retain(|p| p.exists());
        s
    }

    pub fn save(&self) {
        PENDING.lock().unwrap().take(); // this write supersedes anything queued
        let p = path();
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(json) = serde_json::to_vec_pretty(self) {
            let _ = std::fs::write(p, json);
        }
    }

    /// Queue a save for the next `flush` (the window losing focus) or for exit, whichever comes first.
    /// For edits that arrive on every keystroke or drag tick.
    pub fn save_later(&self) {
        // Quitting (⌘Q on macOS) ends the process without iced hearing of it, so exit is caught by the C runtime.
        static AT_EXIT: Once = Once::new();
        AT_EXIT.call_once(|| unsafe {
            libc::atexit(flush_at_exit);
        });
        *PENDING.lock().unwrap() = Some(self.clone());
    }

    /// Record a successfully opened file in the recent list and save.
    pub fn remember(&mut self, file: &std::path::Path) {
        self.recent.retain(|p| p != file);
        self.recent.insert(0, file.to_path_buf());
        self.recent.truncate(8);
        self.last_dir = file.parent().map(|d| d.to_path_buf());
        self.save();
    }
}

/// Write the settings queued by `save_later`, if any.
pub fn flush() {
    let pending = PENDING.lock().unwrap().take();
    if let Some(s) = pending {
        s.save();
    }
}

extern "C" fn flush_at_exit() {
    flush();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_later_waits_for_flush() {
        let _ = std::fs::remove_file(path());
        Settings { calibration: (1.5, 1.5), ..Settings::default() }.save_later();
        assert!(!path().exists(), "nothing written until flush");
        flush();
        assert_eq!(Settings::load().calibration, (1.5, 1.5));
        std::fs::remove_file(path()).unwrap();
        flush();
        assert!(!path().exists(), "nothing left to write");
    }
}
