//! Settings and profile storage under `~/.config/slidewire/`.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use crate::profile::Profile;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    pub start_minimized: bool,
    /// Volume curve exponent: 1.0 = linear in PulseAudio's (already
    /// perceptual) volume scale; higher values give finer control at low
    /// volumes.
    pub curve: f32,
    pub osd: bool,
    /// Resend all LED states every few seconds, for devices that lose them.
    pub led_heartbeat: bool,
    /// Forward incoming MIDI to this output port (substring match).
    pub passthru: String,
    /// Encoder clicks from 0 to 100% when a control doesn't specify `steps`.
    pub default_steps: u32,
    /// Allow volumes above 100% (up to 150%).
    pub allow_boost: bool,
    pub active_profile: Option<String>,
    /// Activate the profile matching a newly connected device.
    pub auto_switch: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            start_minimized: false,
            curve: 1.0,
            osd: true,
            led_heartbeat: false,
            passthru: String::new(),
            default_steps: 50,
            allow_boost: false,
            active_profile: None,
            auto_switch: true,
        }
    }
}

pub struct Store {
    pub dir: PathBuf,
    pub settings: Settings,
    pub profiles: IndexMap<String, Profile>,
}

impl Store {
    pub fn config_dir() -> PathBuf {
        if let Some(dir) = env("CONFIG") {
            return PathBuf::from(dir);
        }
        directories::ProjectDirs::from("dev", "slidewire", "slidewire")
            .map(|d| d.config_dir().to_path_buf())
            .unwrap_or_else(|| PathBuf::from(".slidewire"))
    }

    pub fn load() -> Store {
        let dir = Self::config_dir();
        if env("CONFIG").is_none() {
            migrate_old_config(&dir);
        }
        let _ = std::fs::create_dir_all(dir.join("profiles"));
        let settings = std::fs::read_to_string(dir.join("settings.json"))
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        let mut profiles = IndexMap::new();
        if let Ok(rd) = std::fs::read_dir(dir.join("profiles")) {
            let mut paths: Vec<PathBuf> =
                rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "json")).collect();
            paths.sort();
            for p in paths {
                match Profile::load(&p) {
                    Ok(prof) => {
                        let id = p.file_stem().unwrap().to_string_lossy().to_string();
                        profiles.insert(id, prof);
                    }
                    Err(e) => eprintln!("skipping profile: {e:#}"),
                }
            }
        }
        let mut store = Store { dir, settings, profiles };
        if store.profiles.is_empty() {
            let id = crate::profile::new_id();
            store.profiles.insert(id.clone(), Profile::nanokontrol2());
            store.settings.active_profile = Some(id.clone());
            let _ = store.save_profile(&id);
            let _ = store.save_settings();
        }
        store
    }

    pub fn save_settings(&self) -> Result<()> {
        write_atomic(&self.dir.join("settings.json"), &serde_json::to_string_pretty(&self.settings)?)
    }

    pub fn save_profile(&self, id: &str) -> Result<()> {
        let p = self.profiles.get(id).context("no such profile")?;
        write_atomic(&self.dir.join("profiles").join(format!("{id}.json")), &serde_json::to_string_pretty(p)?)
    }

    pub fn delete_profile(&mut self, id: &str) {
        self.profiles.shift_remove(id);
        let _ = std::fs::remove_file(self.dir.join("profiles").join(format!("{id}.json")));
        if self.settings.active_profile.as_deref() == Some(id) {
            self.settings.active_profile = self.profiles.keys().next().cloned();
        }
    }

    pub fn import(&mut self, path: &Path) -> Result<String> {
        let mut p = Profile::load(path)?;
        p.preset = false;
        let id = crate::profile::new_id();
        self.profiles.insert(id.clone(), p);
        self.save_profile(&id)?;
        Ok(id)
    }
}

/// Reads `SLIDEWIRE_<name>`, falling back to the pre-rename `MIDIMIXER_<name>`.
pub fn env(name: &str) -> Option<String> {
    std::env::var(format!("SLIDEWIRE_{name}")).or_else(|_| std::env::var(format!("MIDIMIXER_{name}"))).ok()
}

/// The app used to be called "midimixer": copy its config over on first start.
fn migrate_old_config(dir: &Path) {
    if dir.exists() {
        return;
    }
    let Some(old) =
        directories::ProjectDirs::from("dev", "midimixer", "midimixer").map(|d| d.config_dir().to_path_buf())
    else {
        return;
    };
    if old.is_dir() {
        match copy_dir(&old, dir) {
            Ok(()) => eprintln!("copied settings from {}", old.display()),
            Err(e) => eprintln!("could not copy settings from {}: {e:#}", old.display()),
        }
    }
    if let Some(autostart) = directories::BaseDirs::new().map(|b| b.config_dir().join("autostart")) {
        let old_entry = autostart.join("midimixer.desktop");
        if old_entry.exists() {
            let _ = std::fs::remove_file(old_entry);
            let _ = set_autostart(true);
        }
    }
}

fn copy_dir(from: &Path, to: &Path) -> Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let dest = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &dest)?;
        } else {
            std::fs::copy(entry.path(), dest)?;
        }
    }
    Ok(())
}

pub fn write_atomic(path: &Path, content: &str) -> Result<()> {
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, content).with_context(|| format!("writing {}", tmp.display()))?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

const DESKTOP_ENTRY: &str = "[Desktop Entry]
Type=Application
Name=Slidewire
Comment=Control application and device volumes with a MIDI controller
Icon=slidewire
Categories=AudioVideo;Audio;Mixer;
";

fn autostart_path() -> Option<PathBuf> {
    directories::BaseDirs::new().map(|b| b.config_dir().join("autostart").join("slidewire.desktop"))
}

pub fn autostart_enabled() -> bool {
    autostart_path().is_some_and(|p| p.exists())
}

pub fn set_autostart(on: bool) -> Result<()> {
    let path = autostart_path().context("no config dir")?;
    if on {
        let exe = std::env::current_exe()?;
        std::fs::create_dir_all(path.parent().unwrap())?;
        std::fs::write(&path, format!("{DESKTOP_ENTRY}Exec=\"{}\" --minimized\n", exe.display()))?;
    } else if path.exists() {
        std::fs::remove_file(path)?;
    }
    Ok(())
}
