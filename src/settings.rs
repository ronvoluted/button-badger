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
    /// The saved settings, or the defaults when nothing has been saved yet. A file that can't be read is moved
    /// aside rather than left for the next save to overwrite; the second value is then a message for the user
    /// and the file it is about.
    pub fn load() -> (Settings, Option<(String, PathBuf)>) {
        let p = path();
        match std::fs::read(&p).map(|bytes| serde_json::from_slice::<Settings>(&bytes)) {
            Ok(Ok(mut s)) => {
                s.recent.retain(|p| p.exists());
                return (s, None);
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return (Settings::default(), None),
            _ => {}
        }
        let kept = p.with_extension("corrupt.json");
        let warning = match std::fs::rename(&p, &kept) {
            Ok(()) => ("Your settings couldn't be read, so the defaults are loaded. The old file is kept as settings.corrupt.json.".into(), kept),
            Err(_) => ("Your settings couldn't be read, so the defaults are loaded. Copy the old file now if you need it: the next change overwrites it.".into(), p),
        };
        (Settings::default(), Some(warning))
    }

    pub fn save(&self) -> Result<(), String> {
        PENDING.lock().unwrap().take(); // this write supersedes anything queued
        let p = path();
        let write = || -> std::io::Result<()> {
            if let Some(dir) = p.parent() {
                std::fs::create_dir_all(dir)?;
            }
            // Written beside the file, then renamed over it, so a crash mid-write can't leave half a file.
            let tmp = p.with_extension("json.tmp");
            std::fs::write(&tmp, serde_json::to_vec_pretty(self)?)?;
            std::fs::rename(tmp, &p)
        };
        write().map_err(|e| format!("Couldn't save your settings: {e}"))
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
    pub fn remember(&mut self, file: &std::path::Path) -> Result<(), String> {
        self.recent.retain(|p| p != file);
        self.recent.insert(0, file.to_path_buf());
        self.recent.truncate(8);
        self.last_dir = file.parent().map(|d| d.to_path_buf());
        self.save()
    }
}

/// Write the settings queued by `save_later`, if any.
pub fn flush() -> Result<(), String> {
    let pending = PENDING.lock().unwrap().take();
    pending.map_or(Ok(()), |s| s.save())
}

extern "C" fn flush_at_exit() {
    // Too late for a toast.
    if let Err(e) = flush() {
        eprintln!("{e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saves_on_flush_and_never_loses_a_file() {
        // One test, because every step shares the settings file.
        let _ = std::fs::remove_file(path());
        Settings { calibration: (1.5, 1.5), ..Settings::default() }.save_later();
        assert!(!path().exists(), "nothing written until flush");
        flush().unwrap();
        assert_eq!(Settings::load().0.calibration, (1.5, 1.5));
        std::fs::remove_file(path()).unwrap();
        flush().unwrap();
        assert!(!path().exists(), "nothing left to write");
        assert!(Settings::load().1.is_none(), "no file yet is not an error");

        // A file that doesn't parse is set aside, where the next save can't overwrite it.
        std::fs::write(path(), "{ not json").unwrap();
        let (settings, warning) = Settings::load();
        let kept = warning.expect("the user is told").1;
        assert_eq!(settings.calibration, (1.0, 1.0));
        settings.save().unwrap();
        assert_eq!(std::fs::read_to_string(&kept).unwrap(), "{ not json");
        std::fs::remove_file(kept).unwrap();

        // A write that fails is reported and leaves the saved file as it was.
        let tmp = path().with_extension("json.tmp");
        std::fs::create_dir(&tmp).unwrap();
        assert!(Settings { calibration: (2.0, 2.0), ..Settings::default() }.save().is_err());
        std::fs::remove_dir(tmp).unwrap();
        assert_eq!(Settings::load().0.calibration, (1.0, 1.0));
    }
}
