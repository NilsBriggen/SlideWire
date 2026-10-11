//! App and device icons from the freedesktop icon theme.

use std::collections::HashMap;
use std::path::PathBuf;

use eframe::egui;

pub struct Icons {
    theme: String,
    cache: HashMap<String, Option<PathBuf>>,
    /// Lowercased binary / WM class / desktop file name -> `Icon=` value.
    desktop: HashMap<String, String>,
}

impl Icons {
    pub fn new() -> Icons {
        let mut desktop = HashMap::new();
        let mut dirs: Vec<PathBuf> =
            vec!["/usr/share/applications".into(), "/var/lib/flatpak/exports/share/applications".into()];
        if let Some(b) = directories::BaseDirs::new() {
            dirs.push(b.data_dir().join("applications"));
            dirs.push(b.data_dir().join("flatpak/exports/share/applications"));
        }
        for dir in dirs {
            let Ok(rd) = std::fs::read_dir(&dir) else { continue };
            for e in rd.flatten() {
                let path = e.path();
                if path.extension().is_none_or(|x| x != "desktop") {
                    continue;
                }
                let Ok(text) = std::fs::read_to_string(&path) else { continue };
                let stem = path.file_stem().unwrap_or_default().to_string_lossy().to_lowercase();
                if let Some(entry) = parse_desktop(&text) {
                    for key in entry.keys(&stem) {
                        desktop.entry(key).or_insert_with(|| entry.icon.clone());
                    }
                }
            }
        }
        Icons { theme: icon_theme(), cache: HashMap::new(), desktop }
    }

    fn lookup(&mut self, name: &str) -> Option<PathBuf> {
        if name.is_empty() {
            return None;
        }
        if let Some(p) = self.cache.get(name) {
            return p.clone();
        }
        let found = if name.starts_with('/') {
            Some(PathBuf::from(name)).filter(|p| p.exists())
        } else {
            freedesktop_icons::lookup(name)
                .with_size(48)
                .with_theme(&self.theme)
                .with_cache()
                .find()
                .or_else(|| freedesktop_icons::lookup(name).with_size(48).find())
        };
        // Only formats our image loaders handle.
        let found = found.filter(|p| p.extension().is_some_and(|e| e == "svg" || e == "png"));
        self.cache.insert(name.to_string(), found.clone());
        found
    }

    /// First icon found among `hints` (icon names, binaries, app names).
    pub fn find(&mut self, hints: &[String]) -> Option<PathBuf> {
        for h in hints {
            if let Some(p) = self.lookup(h) {
                return Some(p);
            }
            let key = h.to_lowercase();
            if let Some(p) = self.lookup(&key) {
                return Some(p);
            }
            if let Some(icon) = self.desktop.get(&key).cloned()
                && let Some(p) = self.lookup(&icon)
            {
                return Some(p);
            }
        }
        None
    }

    pub fn image(&mut self, hints: &[String]) -> Option<egui::Image<'static>> {
        self.find(hints).map(|p| egui::Image::new(format!("file://{}", p.display())))
    }
}

struct DesktopEntry {
    icon: String,
    wm_class: Option<String>,
    exec: Option<String>,
}

impl DesktopEntry {
    fn keys(&self, stem: &str) -> Vec<String> {
        let mut keys = vec![stem.to_string()];
        // org.mozilla.firefox -> firefox
        if let Some(last) = stem.rsplit('.').next() {
            keys.push(last.to_string());
        }
        keys.extend(self.wm_class.iter().map(|s| s.to_lowercase()));
        keys.extend(self.exec.iter().map(|s| s.to_lowercase()));
        keys
    }
}

fn parse_desktop(text: &str) -> Option<DesktopEntry> {
    let mut in_entry = false;
    let (mut icon, mut wm_class, mut exec) = (None, None, None);
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
            continue;
        }
        if !in_entry {
            continue;
        }
        if let Some(v) = line.strip_prefix("Icon=") {
            icon = Some(v.trim().to_string());
        } else if let Some(v) = line.strip_prefix("StartupWMClass=") {
            wm_class = Some(v.trim().to_string());
        } else if let Some(v) = line.strip_prefix("Exec=") {
            exec = exec_binary(v);
        }
    }
    Some(DesktopEntry { icon: icon?, wm_class, exec })
}

/// `env FOO=1 /usr/bin/firefox %u` -> `firefox`; `flatpak run org.x.App` -> `app`.
fn exec_binary(exec: &str) -> Option<String> {
    let mut parts = exec.split_whitespace().filter(|p| !p.starts_with('%'));
    let mut first = parts.next()?;
    if first == "env" {
        first = parts.find(|p| !p.contains('='))?;
    }
    if first.ends_with("flatpak") {
        let app = parts.rfind(|p| !p.starts_with('-'))?;
        return app.rsplit('.').next().map(str::to_string);
    }
    first.rsplit('/').next().map(str::to_string)
}

fn icon_theme() -> String {
    let kdeglobals = directories::BaseDirs::new().map(|b| b.config_dir().join("kdeglobals"));
    if let Some(text) = kdeglobals.and_then(|p| std::fs::read_to_string(p).ok()) {
        let mut in_icons = false;
        for line in text.lines() {
            let line = line.trim();
            if line.starts_with('[') {
                in_icons = line == "[Icons]";
            } else if in_icons && let Some(v) = line.strip_prefix("Theme=") {
                return v.trim().to_string();
            }
        }
    }
    // Our UI is dark, so prefer the dark Breeze variant.
    "breeze-dark".into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_parsing() {
        let e = parse_desktop(
            "[Desktop Entry]\nName=Firefox\nExec=env MOZ=1 /usr/lib/firefox/firefox %u\nIcon=firefox\nStartupWMClass=Firefox\n[Desktop Action new]\nIcon=other\n",
        )
        .unwrap();
        assert_eq!(e.icon, "firefox");
        assert_eq!(e.exec.as_deref(), Some("firefox"));
        assert!(e.keys("org.mozilla.firefox").contains(&"firefox".to_string()));
        assert!(e.keys("x").contains(&"firefox".to_string()));
        assert!(parse_desktop("[Desktop Entry]\nName=No icon\n").is_none());
    }

    #[test]
    fn exec_variants() {
        assert_eq!(exec_binary("/usr/bin/vesktop --flag %U").as_deref(), Some("vesktop"));
        assert_eq!(exec_binary("/usr/bin/flatpak run --branch=stable com.spotify.Client").as_deref(), Some("Client"));
        assert_eq!(exec_binary("env A=b spotify").as_deref(), Some("spotify"));
    }
}
