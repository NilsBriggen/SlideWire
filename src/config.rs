//! Settings and profile storage under `~/.config/slidewire/`.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use crate::profile::Profile;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    /// Format version, for migrations ([`SETTINGS_VERSION`]).
    pub version: u32,
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
    /// Soft takeover: a fader only changes the volume once it reaches it.
    pub pickup: bool,
    pub theme: ThemeMode,
    pub accent: Accent,
    /// Skip UI animations.
    pub reduce_motion: bool,
    /// obs-websocket address and password, for OBS button actions.
    pub obs_url: String,
    pub obs_password: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ThemeMode {
    /// Follow the desktop's light/dark preference.
    #[default]
    System,
    Dark,
    Light,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Accent {
    #[default]
    Cyan,
    Violet,
    Green,
    Amber,
    Pink,
    /// The desktop's accent colour.
    System,
}

impl Accent {
    pub const ALL: [Accent; 6] =
        [Accent::Cyan, Accent::Violet, Accent::Green, Accent::Amber, Accent::Pink, Accent::System];

    pub fn label(self) -> &'static str {
        match self {
            Accent::Cyan => "Cyan",
            Accent::Violet => "Violet",
            Accent::Green => "Green",
            Accent::Amber => "Amber",
            Accent::Pink => "Pink",
            Accent::System => "Desktop",
        }
    }
}

/// The current settings format. Files without a version are from before 1.0
/// and need nothing changed.
pub const SETTINGS_VERSION: u32 = 1;

impl Default for Settings {
    fn default() -> Self {
        Self {
            version: SETTINGS_VERSION,
            start_minimized: false,
            curve: 1.0,
            osd: true,
            led_heartbeat: false,
            passthru: String::new(),
            default_steps: 50,
            allow_boost: false,
            active_profile: None,
            auto_switch: true,
            pickup: true,
            theme: ThemeMode::System,
            accent: Accent::Cyan,
            reduce_motion: false,
            obs_url: "ws://localhost:4455".into(),
            obs_password: String::new(),
        }
    }
}

pub struct Store {
    pub dir: PathBuf,
    pub settings: Settings,
    pub profiles: IndexMap<String, Profile>,
    /// Problems found while loading, for the log.
    pub warnings: Vec<String>,
    /// There were no settings yet: show the welcome.
    pub first_run: bool,
    /// Changed since the last save: profile ids, settings, and when.
    dirty_profiles: std::collections::HashSet<String>,
    dirty_settings: bool,
    touched: Option<std::time::Instant>,
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
        let mut warnings = Vec::new();
        let settings_path = dir.join("settings.json");
        let first_run = !settings_path.exists();
        let mut settings = match std::fs::read_to_string(&settings_path) {
            Err(_) => Settings::default(),
            Ok(text) => match serde_json::from_str::<Settings>(&text) {
                Ok(s) => s,
                Err(e) => {
                    // Keep the broken file so nothing is lost when defaults are saved.
                    let keep = settings_path.with_extension("json.broken");
                    let _ = std::fs::copy(&settings_path, &keep);
                    let backup = std::fs::read_to_string(settings_path.with_extension("json.bak"))
                        .ok()
                        .and_then(|t| serde_json::from_str::<Settings>(&t).ok());
                    let used = if backup.is_some() { "the previous version" } else { "defaults" };
                    warnings.push(format!(
                        "settings.json is invalid ({e}); using {used}. The broken file was kept as {}",
                        keep.display()
                    ));
                    backup.unwrap_or_default()
                }
            },
        };
        settings.version = SETTINGS_VERSION;
        // Files from before they were written owner-only.
        for f in [settings_path.clone(), settings_path.with_extension("json.bak")] {
            if f.exists() {
                let _ = restrict(&f);
            }
        }
        let mut profiles = IndexMap::new();
        if let Ok(rd) = std::fs::read_dir(dir.join("profiles")) {
            let mut paths: Vec<PathBuf> =
                rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "json")).collect();
            paths.sort();
            for p in paths {
                let id = p.file_stem().unwrap().to_string_lossy().to_string();
                match Profile::load(&p) {
                    Ok(prof) => {
                        profiles.insert(id, prof);
                    }
                    Err(e) => match Profile::load(&p.with_extension("json.bak")) {
                        Ok(prof) => {
                            warnings.push(format!(
                                "Profile {} couldn't be read ({e:#}); using its previous version",
                                p.display()
                            ));
                            let _ = std::fs::copy(&p, p.with_extension("json.broken"));
                            profiles.insert(id, prof);
                        }
                        Err(_) => warnings.push(format!("Skipping profile {}: {e:#}", p.display())),
                    },
                }
            }
        }
        let mut store = Store {
            dir,
            settings,
            profiles,
            warnings,
            first_run,
            dirty_profiles: Default::default(),
            dirty_settings: false,
            touched: None,
        };
        if store.profiles.is_empty() {
            let id = crate::profile::new_id();
            let mut p = Profile::nanokontrol2();
            // Test runs point the default profile at a fake device, so they
            // never talk to a real controller.
            if let Some(port) = env("DEFAULT_PORT") {
                p.input = port.clone();
                p.output = port;
            }
            store.profiles.insert(id.clone(), p);
            store.settings.active_profile = Some(id.clone());
            let _ = store.save_profile(&id);
            let _ = store.save_settings();
        }
        store
    }

    /// Mark a profile as changed; it is saved shortly (see [`Store::save_due`]).
    pub fn touch(&mut self, id: &str) {
        self.dirty_profiles.insert(id.to_string());
        self.touched = Some(std::time::Instant::now());
    }

    pub fn touch_settings(&mut self) {
        self.dirty_settings = true;
        self.touched = Some(std::time::Instant::now());
    }

    /// Unsaved changes, and nothing changed for a moment: time to save.
    /// Saving waits for a pause so dragging a slider doesn't write the
    /// files on every frame.
    pub fn save_due(&self) -> bool {
        self.touched.is_some_and(|t| t.elapsed() >= std::time::Duration::from_millis(500))
    }

    pub fn has_unsaved(&self) -> bool {
        self.touched.is_some()
    }

    /// Write everything changed. Returns what couldn't be saved.
    pub fn save_now(&mut self) -> Vec<String> {
        let mut errors = Vec::new();
        if std::mem::take(&mut self.dirty_settings)
            && let Err(e) = self.save_settings()
        {
            errors.push(format!("Couldn't save settings: {e:#}"));
        }
        for id in std::mem::take(&mut self.dirty_profiles) {
            // Deleted in the meantime: nothing to save.
            if self.profiles.contains_key(&id)
                && let Err(e) = self.save_profile(&id)
            {
                errors.push(format!("Couldn't save profile \"{}\": {e:#}", self.profiles[&id].name));
            }
        }
        self.touched = None;
        errors
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

    /// Add the profile at `path`. Buttons that run commands or press keys
    /// are kept only when `trusted`.
    pub fn import(&mut self, path: &Path, trusted: bool) -> Result<String> {
        let mut p = Profile::load(path)?;
        p.preset = false;
        if !trusted {
            p.disarm();
        }
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

/// Write via a temporary file and rename, keeping the previous version as
/// `.bak`. Only the user can read the files: settings hold the OBS password.
pub fn write_atomic(path: &Path, content: &str) -> Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let tmp = path.with_extension("tmp");
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&tmp)
        .with_context(|| format!("writing {}", tmp.display()))?;
    f.write_all(content.as_bytes()).and_then(|_| f.sync_all()).with_context(|| format!("writing {}", tmp.display()))?;
    if path.exists() {
        let bak = path.with_extension("json.bak");
        let _ = std::fs::copy(path, &bak);
        let _ = restrict(&bak);
    }
    std::fs::rename(&tmp, path)?;
    Ok(())
}

fn restrict(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
}

/// The packaged desktop entry.
const DESKTOP_FILE: &str = include_str!("../packaging/dev.briggen.Slidewire.desktop");

/// The desktop entry, starting this executable with `args`.
fn desktop_entry(args: &str) -> Result<String> {
    let exe = std::env::current_exe()?;
    let lines: Vec<String> = DESKTOP_FILE
        .lines()
        .map(|l| if l.starts_with("Exec=") { format!("Exec=\"{}\"{args}", exe.display()) } else { l.to_string() })
        .collect();
    Ok(lines.join("\n") + "\n")
}

fn data_dir() -> Result<PathBuf> {
    Ok(directories::BaseDirs::new().context("no home directory")?.data_dir().to_path_buf())
}

/// Add a menu entry and the icon for this user (for builds without a package).
pub fn install_desktop_files() -> Result<PathBuf> {
    let data = data_dir()?;
    let icon_dir = data.join("icons/hicolor/scalable/apps");
    std::fs::create_dir_all(&icon_dir)?;
    std::fs::write(icon_dir.join(format!("{}.svg", crate::APP_ID)), crate::icon::svg())?;
    let apps = data.join("applications");
    std::fs::create_dir_all(&apps)?;
    let entry = apps.join(format!("{}.desktop", crate::APP_ID));
    std::fs::write(&entry, desktop_entry("")?)?;
    Ok(entry)
}

/// Undo `--install-icon` and autostart. Returns what was removed. The
/// settings stay; their folder is named so the user can delete it.
pub fn uninstall() -> Result<Vec<String>> {
    let data = data_dir()?;
    let mut removed = Vec::new();
    let mut files = vec![
        data.join(format!("applications/{}.desktop", crate::APP_ID)),
        data.join(format!("icons/hicolor/scalable/apps/{}.svg", crate::APP_ID)),
        data.join("applications/slidewire.desktop"),
        data.join("icons/hicolor/scalable/apps/slidewire.svg"),
    ];
    if let Some(p) = autostart_path() {
        files.push(p.with_file_name("slidewire.desktop"));
        files.push(p);
    }
    for f in files {
        if f.exists() {
            std::fs::remove_file(&f)?;
            removed.push(f.display().to_string());
        }
    }
    if systemd_unit() && autostart_enabled_now() {
        set_autostart(false)?;
        removed.push("systemd autostart (slidewire.service)".into());
    }
    Ok(removed)
}

/// Before 1.0 the desktop entry, icon and autostart file were named
/// `slidewire`; rename them to the app id.
pub fn migrate_desktop_files() {
    let Ok(data) = data_dir() else { return };
    let old_entry = data.join("applications/slidewire.desktop");
    if old_entry.exists() {
        let _ = std::fs::remove_file(&old_entry);
        let _ = std::fs::remove_file(data.join("icons/hicolor/scalable/apps/slidewire.svg"));
        let _ = install_desktop_files();
    }
    if let Some(old) = autostart_path().map(|p| p.with_file_name("slidewire.desktop"))
        && old.exists()
    {
        let _ = std::fs::remove_file(&old);
        let _ = set_autostart(true);
    }
}

fn autostart_path() -> Option<PathBuf> {
    directories::BaseDirs::new().map(|b| b.config_dir().join("autostart").join(format!("{}.desktop", crate::APP_ID)))
}

/// The systemd user unit the package installs; when present, autostart
/// goes through systemd (restarts on failure, starts with the session).
const SYSTEMD_UNIT: &str = "/usr/lib/systemd/user/slidewire.service";

fn systemd_unit() -> bool {
    // Test instances leave the user's real service alone.
    std::path::Path::new(SYSTEMD_UNIT).exists() && env("INSTANCE").is_none()
}

/// `systemctl --user is-enabled` is a process spawn, so it runs once in the
/// background and its answer is kept.
static SYSTEMD_ENABLED: std::sync::Mutex<Option<bool>> = std::sync::Mutex::new(None);
static SYSTEMD_ASKED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Whether we start with the session. Never blocks: the first call asks
/// systemd in the background (and says no until it answered).
pub fn autostart_enabled() -> bool {
    if systemd_unit() {
        if let Some(on) = *SYSTEMD_ENABLED.lock().unwrap() {
            return on;
        }
        if !SYSTEMD_ASKED.swap(true, std::sync::atomic::Ordering::SeqCst) {
            std::thread::spawn(|| {
                *SYSTEMD_ENABLED.lock().unwrap() = Some(autostart_enabled_now());
            });
        }
        return false;
    }
    autostart_path().is_some_and(|p| p.exists())
}

/// Ask systemd now (blocking; for the command line).
fn autostart_enabled_now() -> bool {
    std::process::Command::new("systemctl")
        .args(["--user", "is-enabled", "--quiet", "slidewire.service"])
        .status()
        .is_ok_and(|s| s.success())
}

/// [`set_autostart`] on a worker thread; `report` gets the error, if any.
pub fn set_autostart_in_background(on: bool, report: impl FnOnce(String) + Send + 'static) {
    if systemd_unit() {
        *SYSTEMD_ENABLED.lock().unwrap() = Some(on);
    }
    std::thread::spawn(move || {
        if let Err(e) = set_autostart(on) {
            if systemd_unit() {
                *SYSTEMD_ENABLED.lock().unwrap() = Some(!on);
            }
            report(format!("{e:#}"));
        }
    });
}

pub fn set_autostart(on: bool) -> Result<()> {
    let path = autostart_path().context("no config dir")?;
    if systemd_unit() {
        // Don't start twice: drop the desktop-file autostart, use the unit.
        if path.exists() {
            std::fs::remove_file(&path)?;
        }
        let status = std::process::Command::new("systemctl")
            .args(["--user", if on { "enable" } else { "disable" }, "slidewire.service"])
            .status()
            .context("systemctl")?;
        anyhow::ensure!(status.success(), "systemctl --user {} failed", if on { "enable" } else { "disable" });
        return Ok(());
    }
    if on {
        std::fs::create_dir_all(path.parent().unwrap())?;
        std::fs::write(&path, desktop_entry(" --minimized")?)?;
    } else if path.exists() {
        std::fs::remove_file(path)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn broken_settings_are_kept() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("slidewire-cfg-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("settings.json"), "{ \"curve\": 2.0, oops").unwrap();
        // SAFETY: no other test reads or writes the environment.
        unsafe { std::env::set_var("SLIDEWIRE_CONFIG", &dir) };
        let store = Store::load();
        assert_eq!(store.warnings.len(), 1, "{:?}", store.warnings);
        assert!(dir.join("settings.json.broken").exists());
        // The default profile was written, and saving settings left a backup.
        assert_eq!(store.profiles.len(), 1);
        assert!(dir.join("settings.json.bak").exists());
        // Settings hold a password: only the user may read them.
        let mode = std::fs::metadata(dir.join("settings.json")).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);

        // A broken file with a good backup: the backup is used.
        let mut good = Settings { curve: 2.5, ..Default::default() };
        good.obs_url = "ws://example:1".into();
        std::fs::write(dir.join("settings.json.bak"), serde_json::to_string(&good).unwrap()).unwrap();
        std::fs::write(dir.join("settings.json"), "nope").unwrap();
        // So is a profile's.
        let (id, _) = store.profiles.first().unwrap();
        let profile = dir.join("profiles").join(format!("{id}.json"));
        std::fs::copy(&profile, profile.with_extension("json.bak")).unwrap();
        std::fs::write(&profile, "{ broken").unwrap();
        let store = Store::load();
        unsafe { std::env::remove_var("SLIDEWIRE_CONFIG") };
        assert_eq!(store.settings.curve, 2.5);
        assert_eq!(store.settings.obs_url, "ws://example:1");
        assert_eq!(store.profiles.len(), 1, "{:?}", store.warnings);
        assert_eq!(store.warnings.len(), 2, "{:?}", store.warnings);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
