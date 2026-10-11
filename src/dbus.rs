//! Session-bus integration: single instance, focused-window tracking through
//! a KWin script, MPRIS media control and the Plasma OSD.

use std::time::Duration;

use anyhow::{Context, Result};
use crossbeam_channel::Sender;
use zbus::blocking::Connection;

use crate::engine::{Event, FocusInfo};
use crate::profile::MediaKey;

pub const BUS_NAME: &str = "dev.slidewire.Slidewire";
const PATH: &str = "/dev/slidewire/Slidewire";
const IFACE: &str = "dev.slidewire.Slidewire";
const KWIN_SCRIPT: &str = "slidewire-focus";

struct Service {
    tx: Sender<Event>,
    own_pid: i32,
}

#[zbus::interface(name = "dev.slidewire.Slidewire")]
impl Service {
    /// Called by our KWin script whenever the active window changes.
    fn activated(&self, pid: i32, caption: String, class: String) {
        if pid <= 0 || pid == self.own_pid {
            return;
        }
        let _ = self.tx.send(Event::Focus(FocusInfo { pid: Some(pid as u32), caption, class }));
    }

    /// Bring the window up (used by a second instance and the tray).
    fn show(&self) {
        let _ = self.tx.send(Event::ShowWindow);
    }
}

/// Test instances (`SLIDEWIRE_INSTANCE=x`) get their own bus name so they
/// never collide with a running copy.
pub fn bus_name() -> String {
    match crate::config::env("INSTANCE") {
        Some(suffix) if !suffix.is_empty() => {
            let clean: String = suffix.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
            format!("{BUS_NAME}.i{clean}")
        }
        _ => BUS_NAME.to_string(),
    }
}

/// Claim the bus name. Returns `Ok(None)` if another instance owns it (that
/// instance has then been asked to show its window).
pub fn start(tx: Sender<Event>) -> Result<Option<Connection>> {
    let svc = Service { tx, own_pid: std::process::id() as i32 };
    let name = bus_name();
    let built = zbus::blocking::connection::Builder::session()?.name(name.as_str())?.serve_at(PATH, svc)?.build();
    match built {
        Ok(conn) => Ok(Some(conn)),
        Err(zbus::Error::NameTaken) => {
            let conn = Connection::session()?;
            conn.call_method(Some(name.as_str()), PATH, Some(IFACE), "Show", &())?;
            Ok(None)
        }
        Err(e) => Err(e.into()),
    }
}

/// Install the KWin script that reports focus changes back to us. Works on
/// Plasma 5 and 6, X11 and Wayland.
pub fn install_focus_script(conn: &Connection) -> Result<()> {
    let script = format!(
        r#"
function report(w) {{
    if (!w) return;
    callDBus("{BUS_NAME}", "{PATH}", "{IFACE}", "Activated", w.pid, String(w.caption), String(w.resourceClass));
}}
if (workspace.windowActivated) {{
    workspace.windowActivated.connect(report);
    report(workspace.activeWindow);
}} else {{
    workspace.clientActivated.connect(report);
    report(workspace.activeClient);
}}
"#
    );
    let dir = std::env::var_os("XDG_RUNTIME_DIR").map(std::path::PathBuf::from).unwrap_or_else(std::env::temp_dir);
    let path = dir.join("slidewire-focus.js");
    std::fs::write(&path, script)?;

    let scripting = |method: &str, args: &(&str,)| {
        conn.call_method(Some("org.kde.KWin"), "/Scripting", Some("org.kde.kwin.Scripting"), method, args)
    };
    let _ = scripting("unloadScript", &(KWIN_SCRIPT,));
    let path = path.to_string_lossy().to_string();
    let reply = conn
        .call_method(
            Some("org.kde.KWin"),
            "/Scripting",
            Some("org.kde.kwin.Scripting"),
            "loadScript",
            &(path.as_str(), KWIN_SCRIPT),
        )
        .context("KWin scripting not available (not running KDE?)")?;
    let id: i32 = reply.body().deserialize().unwrap_or(-1);
    // Plasma 6 exposes /Scripting/Script<id>, Plasma 5 /<id>; `start` runs
    // every loaded script on both.
    let ran = conn
        .call_method(
            Some("org.kde.KWin"),
            format!("/Scripting/Script{id}").as_str(),
            Some("org.kde.kwin.Script"),
            "run",
            &(),
        )
        .is_ok();
    if !ran {
        conn.call_method(Some("org.kde.KWin"), "/Scripting", Some("org.kde.kwin.Scripting"), "start", &())?;
    }
    Ok(())
}

pub fn uninstall_focus_script(conn: &Connection) {
    let _ = conn.call_method(
        Some("org.kde.KWin"),
        "/Scripting",
        Some("org.kde.kwin.Scripting"),
        "unloadScript",
        &(KWIN_SCRIPT,),
    );
}

/// Send a media command to the most relevant MPRIS player: one that is
/// playing, else one that is paused, else any.
pub fn media(key: MediaKey) -> Result<()> {
    let conn = Connection::session()?;
    let names: Vec<String> = conn
        .call_method(
            Some("org.freedesktop.DBus"),
            "/org/freedesktop/DBus",
            Some("org.freedesktop.DBus"),
            "ListNames",
            &(),
        )?
        .body()
        .deserialize()?;
    let players: Vec<String> = names.into_iter().filter(|n| n.starts_with("org.mpris.MediaPlayer2.")).collect();
    let status = |p: &str| -> String {
        conn.call_method(
            Some(p),
            "/org/mpris/MediaPlayer2",
            Some("org.freedesktop.DBus.Properties"),
            "Get",
            &("org.mpris.MediaPlayer2.Player", "PlaybackStatus"),
        )
        .ok()
        .and_then(|m| m.body().deserialize::<zbus::zvariant::OwnedValue>().ok())
        .and_then(|v| String::try_from(v).ok())
        .unwrap_or_default()
    };
    let ranked: Vec<(u8, &String)> = players
        .iter()
        .map(|p| {
            let rank = match status(p).as_str() {
                "Playing" => 0,
                "Paused" => 1,
                _ => 2,
            };
            (rank, p)
        })
        .collect();
    let Some((_, player)) = ranked.iter().min_by_key(|(r, _)| *r) else {
        anyhow::bail!("no MPRIS media player running");
    };
    let method = match key {
        MediaKey::PlayPause => "PlayPause",
        MediaKey::Play => "Play",
        MediaKey::Pause => "Pause",
        MediaKey::Stop => "Stop",
        MediaKey::Next => "Next",
        MediaKey::Previous => "Previous",
    };
    conn.call_method(
        Some(player.as_str()),
        "/org/mpris/MediaPlayer2",
        Some("org.mpris.MediaPlayer2.Player"),
        method,
        &(),
    )?;
    Ok(())
}

pub struct Osd {
    tx: Sender<(String, i32, String)>,
}

impl Osd {
    /// Plasma's volume OSD. Requests are coalesced so fast fader moves don't
    /// queue up.
    pub fn spawn() -> Osd {
        let (tx, rx) = crossbeam_channel::unbounded::<(String, i32, String)>();
        std::thread::Builder::new()
            .name("osd".into())
            .spawn(move || {
                // Test instances must not pop up on the user's desktop.
                if crate::config::env("INSTANCE").is_some() {
                    while rx.recv().is_ok() {}
                    return;
                }
                let Ok(conn) = Connection::session() else { return };
                while let Ok(mut req) = rx.recv() {
                    while let Ok(newer) = rx.try_recv() {
                        req = newer;
                    }
                    let (icon, percent, text) = req;
                    let _ = if percent < 0 {
                        conn.call_method(
                            Some("org.kde.plasmashell"),
                            "/org/kde/osdService",
                            Some("org.kde.osdService"),
                            "showText",
                            &(icon, text),
                        )
                    } else {
                        conn.call_method(
                            Some("org.kde.plasmashell"),
                            "/org/kde/osdService",
                            Some("org.kde.osdService"),
                            "showProgress",
                            &(icon, percent, 100i32, text),
                        )
                    };
                    std::thread::sleep(Duration::from_millis(30));
                }
            })
            .expect("spawn osd thread");
        Osd { tx }
    }

    pub fn show(&self, icon: &str, percent: i32, text: &str) {
        let _ = self.tx.send((icon.to_string(), percent, text.to_string()));
    }

    /// A text-only pop-up (bank and scene changes).
    pub fn show_text(&self, icon: &str, text: &str) {
        let _ = self.tx.send((icon.to_string(), -1, text.to_string()));
    }
}
