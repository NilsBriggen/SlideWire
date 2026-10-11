//! Which window has focus, on compositors other than KWin (which reports
//! it through our KWin script, see `dbus.rs`).

use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;

use crossbeam_channel::Sender;

use crate::engine::{Event, FocusInfo};

/// Start following focus if we're on Hyprland or sway. Returns the
/// compositor's name, or None when neither is running.
pub fn spawn(tx: Sender<Event>) -> Option<&'static str> {
    if let Some(dir) = hyprland_dir() {
        std::thread::Builder::new().name("focus".into()).spawn(move || hyprland(dir, tx)).ok()?;
        return Some("Hyprland");
    }
    if std::env::var_os("SWAYSOCK").is_some() {
        std::thread::Builder::new().name("focus".into()).spawn(move || sway(tx)).ok()?;
        return Some("sway");
    }
    None
}

fn hyprland_dir() -> Option<PathBuf> {
    let sig = std::env::var("HYPRLAND_INSTANCE_SIGNATURE").ok()?;
    let runtime = std::env::var("XDG_RUNTIME_DIR").unwrap_or_default();
    [PathBuf::from(runtime).join("hypr").join(&sig), PathBuf::from("/tmp/hypr").join(&sig)]
        .into_iter()
        .find(|d| d.join(".socket2.sock").exists())
}

/// Hyprland announces focus changes on socket2; the window's pid comes from
/// asking socket1 for the active window as JSON.
fn hyprland(dir: PathBuf, tx: Sender<Event>) {
    let Ok(events) = UnixStream::connect(dir.join(".socket2.sock")) else { return };
    for line in BufReader::new(events).lines() {
        let Ok(line) = line else { return };
        if !line.starts_with("activewindowv2>>") {
            continue;
        }
        let Some(info) = hyprland_active(&dir) else { continue };
        if tx.send(Event::Focus(info)).is_err() {
            return;
        }
    }
}

fn hyprland_active(dir: &std::path::Path) -> Option<FocusInfo> {
    let mut s = UnixStream::connect(dir.join(".socket.sock")).ok()?;
    s.write_all(b"j/activewindow").ok()?;
    let mut reply = String::new();
    s.read_to_string(&mut reply).ok()?;
    parse_hyprland(&reply)
}

fn parse_hyprland(json: &str) -> Option<FocusInfo> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    let pid = v.get("pid")?.as_i64().filter(|p| *p > 0)? as u32;
    let class = v.get("class").and_then(|c| c.as_str()).unwrap_or_default().to_string();
    let caption = v.get("title").and_then(|c| c.as_str()).unwrap_or_default().to_string();
    Some(FocusInfo { pid: Some(pid), caption, class })
}

/// sway: swaymsg prints one JSON object per window event.
fn sway(tx: Sender<Event>) {
    let Ok(mut child) = std::process::Command::new("swaymsg")
        .args(["-t", "subscribe", "-m", r#"["window"]"#])
        .stdout(std::process::Stdio::piped())
        .spawn()
    else {
        return;
    };
    let Some(out) = child.stdout.take() else { return };
    for line in BufReader::new(out).lines() {
        let Ok(line) = line else { break };
        if let Some(info) = parse_sway(&line)
            && tx.send(Event::Focus(info)).is_err()
        {
            break;
        }
    }
    let _ = child.kill();
}

fn parse_sway(json: &str) -> Option<FocusInfo> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    if v.get("change")?.as_str()? != "focus" {
        return None;
    }
    let c = v.get("container")?;
    let pid = c.get("pid")?.as_i64().filter(|p| *p > 0)? as u32;
    // Wayland windows have an app_id; XWayland ones a window class.
    let class = c
        .get("app_id")
        .and_then(|a| a.as_str())
        .or_else(|| c.pointer("/window_properties/class").and_then(|a| a.as_str()))
        .unwrap_or_default()
        .to_string();
    let caption = c.get("name").and_then(|n| n.as_str()).unwrap_or_default().to_string();
    Some(FocusInfo { pid: Some(pid), caption, class })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hyprland_reply() {
        let f = parse_hyprland(r#"{"address":"0x1","pid":4242,"class":"firefox","title":"Docs — Firefox"}"#).unwrap();
        assert_eq!((f.pid, f.class.as_str(), f.caption.as_str()), (Some(4242), "firefox", "Docs — Firefox"));
        assert!(parse_hyprland("{}").is_none());
    }

    #[test]
    fn sway_events() {
        let wayland = r#"{"change":"focus","container":{"pid":7,"app_id":"org.gnome.Nautilus","name":"Home"}}"#;
        assert_eq!(parse_sway(wayland).unwrap().class, "org.gnome.Nautilus");
        let x11 = r#"{"change":"focus","container":{"pid":8,"app_id":null,"window_properties":{"class":"Steam"},"name":"Steam"}}"#;
        assert_eq!(parse_sway(x11).unwrap().class, "Steam");
        assert!(parse_sway(r#"{"change":"title","container":{"pid":7}}"#).is_none());
    }
}
