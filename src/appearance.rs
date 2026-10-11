//! The desktop's light/dark preference and accent colour, read from the XDG
//! desktop portal (works on KDE, GNOME and most others) and kept up to date.

use std::sync::Mutex;

use zbus::blocking::{Connection, Proxy};
use zbus::zvariant::{OwnedValue, Value};

const NS: &str = "org.freedesktop.appearance";

#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct System {
    /// None when the desktop states no preference.
    pub dark: Option<bool>,
    pub accent: Option<[u8; 3]>,
}

static SYSTEM: Mutex<System> = Mutex::new(System { dark: None, accent: None });

pub fn system() -> System {
    *SYSTEM.lock().unwrap()
}

/// Read the current values, then follow changes on a background thread,
/// calling `on_change` after each one.
pub fn watch(on_change: impl Fn() + Send + 'static) {
    std::thread::spawn(move || {
        let Ok(conn) = Connection::session() else { return };
        let Ok(proxy) = Proxy::new(
            &conn,
            "org.freedesktop.portal.Desktop",
            "/org/freedesktop/portal/desktop",
            "org.freedesktop.portal.Settings",
        ) else {
            return;
        };
        let mut s = System {
            dark: read(&proxy, "color-scheme").and_then(|v| color_scheme(&v)),
            accent: read(&proxy, "accent-color").and_then(|v| accent(&v)).or_else(kde_accent),
        };
        *SYSTEM.lock().unwrap() = s;
        on_change();
        let Ok(signals) = proxy.receive_signal("SettingChanged") else { return };
        for msg in signals {
            let Ok((ns, key, value)) = msg.body().deserialize::<(String, String, OwnedValue)>() else { continue };
            if ns != NS {
                continue;
            }
            match key.as_str() {
                "color-scheme" => s.dark = color_scheme(&value),
                "accent-color" => s.accent = accent(&value).or_else(kde_accent),
                _ => continue,
            }
            *SYSTEM.lock().unwrap() = s;
            on_change();
        }
    });
}

fn read(proxy: &Proxy, key: &str) -> Option<OwnedValue> {
    if let Ok(v) = proxy.call::<_, _, OwnedValue>("ReadOne", &(NS, key)) {
        return Some(v);
    }
    // Older portals only have Read, which wraps the value in one more variant.
    let v: OwnedValue = proxy.call("Read", &(NS, key)).ok()?;
    match &*v {
        Value::Value(inner) => OwnedValue::try_from(&**inner).ok(),
        _ => Some(v),
    }
}

/// 0 = no preference, 1 = dark, 2 = light.
fn color_scheme(v: &Value) -> Option<bool> {
    match u32::try_from(v).ok()? {
        1 => Some(true),
        2 => Some(false),
        _ => None,
    }
}

/// Three doubles in 0..=1; anything outside means "not set".
fn accent(v: &Value) -> Option<[u8; 3]> {
    let (r, g, b) = <(f64, f64, f64)>::try_from(v.try_clone().ok()?).ok()?;
    let ok = |c: f64| (0.0..=1.0).contains(&c);
    (ok(r) && ok(g) && ok(b)).then(|| [r, g, b].map(|c| (c * 255.0).round() as u8))
}

/// Plasma also stores the accent in kdeglobals.
fn kde_accent() -> Option<[u8; 3]> {
    let path = directories::BaseDirs::new()?.config_dir().join("kdeglobals");
    parse_kde_accent(&std::fs::read_to_string(path).ok()?)
}

fn parse_kde_accent(ini: &str) -> Option<[u8; 3]> {
    let mut general = false;
    for line in ini.lines().map(str::trim) {
        if line.starts_with('[') {
            general = line == "[General]";
        } else if general && let Some(v) = line.strip_prefix("AccentColor=") {
            let parts: Vec<u8> = v.split(',').filter_map(|p| p.trim().parse().ok()).collect();
            return (parts.len() >= 3).then(|| [parts[0], parts[1], parts[2]]);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kde_accent_parsing() {
        let ini = "[Colors:View]\nAccentColor=1,2,3\n\n[General]\nColorScheme=BreezeDark\nAccentColor=61,174,233\n";
        assert_eq!(parse_kde_accent(ini), Some([61, 174, 233]));
        assert_eq!(parse_kde_accent("[General]\nColorScheme=Breeze\n"), None);
    }

    #[test]
    fn portal_values() {
        assert_eq!(color_scheme(&Value::U32(1)), Some(true));
        assert_eq!(color_scheme(&Value::U32(0)), None);
        let v = Value::from((0.2f64, 0.5f64, 1.0f64));
        assert_eq!(accent(&v), Some([51, 128, 255]));
        assert_eq!(accent(&Value::from((-1.0f64, 0.0f64, 0.0f64))), None);
    }
}
