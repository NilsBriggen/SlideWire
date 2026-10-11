//! Session-bus integration: single instance, focused-window tracking through
//! a KWin script, MPRIS media control and the Plasma OSD.

use std::time::Duration;

use anyhow::{Context, Result};
use crossbeam_channel::Sender;
use zbus::blocking::Connection;

use crate::engine::{Event, FocusInfo, SharedRef, UiCmd};
use crate::profile::MediaKey;

/// The app state, for the scripting methods (set once the engine exists).
pub static SHARED: std::sync::OnceLock<SharedRef> = std::sync::OnceLock::new();

fn failed(msg: impl Into<String>) -> zbus::fdo::Error {
    zbus::fdo::Error::Failed(msg.into())
}

/// Find a strip by its name ("Fader 1", case-insensitive) or channel number
/// ("1" = "Fader 1"). Returns the group id and the strip's current state.
fn find_strip(name: &str) -> zbus::fdo::Result<crate::engine::StripView> {
    let shared = SHARED.get().ok_or_else(|| failed("not ready"))?;
    let strips = shared.lock().unwrap().strips.clone();
    let want = name.trim().to_lowercase();
    let by_number = format!("fader {want}");
    strips
        .iter()
        .find(|s| s.name.to_lowercase() == want)
        .or_else(|| strips.iter().find(|s| s.name.to_lowercase() == by_number))
        .cloned()
        .ok_or_else(|| failed(format!("no strip called \"{name}\" (see `slidewire strips`)")))
}

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

    /// Every strip: name, volume in percent, muted, what it controls.
    fn strips(&self) -> zbus::fdo::Result<Vec<(String, f64, bool, String)>> {
        let shared = SHARED.get().ok_or_else(|| failed("not ready"))?;
        let strips = shared.lock().unwrap().strips.clone();
        Ok(strips
            .into_iter()
            .map(|s| {
                let what = if s.resolved.is_empty() {
                    s.targets.iter().map(|t| t.label()).collect::<Vec<_>>().join(" + ")
                } else {
                    s.resolved.join(" + ")
                };
                (s.name, (s.position * 100.0).round() as f64, s.muted, what)
            })
            .collect())
    }

    /// Set a strip to `percent` (0-100), or change it by `percent` if `relative`.
    fn set_volume(&self, strip: String, percent: f64, relative: bool) -> zbus::fdo::Result<f64> {
        let s = find_strip(&strip)?;
        let target = if relative { s.position as f64 * 100.0 + percent } else { percent };
        let pos = (target / 100.0).clamp(0.0, 1.0) as f32;
        let _ = self.tx.send(Event::Ui(UiCmd::SetGroupPosition(s.id, pos)));
        Ok((pos * 100.0).round() as f64)
    }

    /// `mode`: "toggle", "on" or "off". Returns whether the strip is now muted.
    fn mute(&self, strip: String, mode: String) -> zbus::fdo::Result<bool> {
        let s = find_strip(&strip)?;
        if s.resolved.is_empty() {
            return Err(failed(format!("{} has nothing to mute", s.name)));
        }
        let want = match mode.as_str() {
            "on" => true,
            "off" => false,
            "toggle" | "" => !s.muted,
            other => return Err(failed(format!("mute mode must be on, off or toggle, not \"{other}\""))),
        };
        if want != s.muted {
            let _ = self.tx.send(Event::Ui(UiCmd::ToggleGroupMute(s.id)));
        }
        Ok(want)
    }

    /// Switch to bank `bank` (1 = first).
    fn bank(&self, bank: u32) -> zbus::fdo::Result<()> {
        if bank == 0 {
            return Err(failed("banks start at 1"));
        }
        let _ = self.tx.send(Event::Ui(UiCmd::SetBank(bank as usize - 1)));
        Ok(())
    }

    fn scene(&self, name: String) -> zbus::fdo::Result<()> {
        let shared = SHARED.get().ok_or_else(|| failed("not ready"))?;
        let exists = shared.lock().unwrap().active().is_some_and(|p| p.scenes.contains_key(&name));
        if !exists {
            return Err(failed(format!("no scene called \"{name}\"")));
        }
        let _ = self.tx.send(Event::Ui(UiCmd::RecallScene(name)));
        Ok(())
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

/// `slidewire strips|set|mute|bank|scene ...`: talk to the running app.
pub fn remote(args: &[String]) -> Result<()> {
    let conn = Connection::session().context("no session bus")?;
    let name = bus_name();
    let call = |method: &str, body: &dyn erased::Body| -> Result<zbus::Message> {
        body.call(&conn, &name, method).map_err(|e| match e {
            zbus::Error::MethodError(n, Some(msg), _) if n.as_str() == "org.freedesktop.DBus.Error.Failed" => {
                anyhow::anyhow!(msg)
            }
            zbus::Error::MethodError(n, _, _) if n.as_str() == "org.freedesktop.DBus.Error.ServiceUnknown" => {
                anyhow::anyhow!("Slidewire isn't running")
            }
            e => e.into(),
        })
    };
    let arg = |i: usize, what: &str| args.get(i).cloned().with_context(|| format!("missing {what}"));
    match args[0].as_str() {
        "strips" => {
            let reply = call("Strips", &())?;
            let strips: Vec<(String, f64, bool, String)> = reply.body().deserialize()?;
            // Ignore write errors: `slidewire strips | head` closes the pipe early.
            use std::io::Write;
            let mut out = std::io::stdout().lock();
            for (name, pct, muted, what) in strips {
                let state = if muted { "muted".to_string() } else { format!("{pct:>3.0}%") };
                let _ = writeln!(out, "{name:<12} {state:>6}  {}", if what.is_empty() { "-" } else { &what });
            }
        }
        "set" => {
            let strip = arg(1, "strip name")?;
            let value = arg(2, "volume (50, +5 or -5)")?;
            let relative = value.starts_with('+') || value.starts_with('-');
            let pct: f64 = value.trim_end_matches('%').parse().context("volume must be a number like 50, +5 or -5")?;
            let reply = call("SetVolume", &(strip.as_str(), pct, relative))?;
            let now: f64 = reply.body().deserialize()?;
            println!("{strip}: {now:.0}%");
        }
        "mute" => {
            let strip = arg(1, "strip name")?;
            let mode = args.get(2).cloned().unwrap_or_else(|| "toggle".into());
            let reply = call("Mute", &(strip.as_str(), mode.as_str()))?;
            let muted: bool = reply.body().deserialize()?;
            println!("{strip}: {}", if muted { "muted" } else { "unmuted" });
        }
        "bank" => {
            let n: u32 = arg(1, "bank number")?.parse().context("bank must be a number")?;
            call("Bank", &(n,))?;
        }
        "scene" => {
            call("Scene", &(arg(1, "scene name")?.as_str(),))?;
        }
        other => anyhow::bail!("unknown command {other}"),
    }
    Ok(())
}

/// Method calls with differently typed bodies through one closure.
mod erased {
    use zbus::blocking::Connection;

    pub trait Body {
        fn call(&self, conn: &Connection, name: &str, method: &str) -> zbus::Result<zbus::Message>;
    }

    impl<T: serde::Serialize + zbus::zvariant::DynamicType> Body for T {
        fn call(&self, conn: &Connection, name: &str, method: &str) -> zbus::Result<zbus::Message> {
            conn.call_method(Some(name), super::PATH, Some(super::IFACE), method, self)
        }
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

fn has_owner(conn: &Connection, name: &str) -> bool {
    zbus::blocking::fdo::DBusProxy::new(conn)
        .ok()
        .and_then(|p| p.name_has_owner(name.try_into().ok()?).ok())
        .unwrap_or(false)
}

/// A volume pop-up as a desktop notification (GNOME, dunst, mako, ...).
/// The "value" hint draws a bar where supported; `replaces` keeps it to one
/// notification. Returns its id.
fn notify(conn: &Connection, replaces: u32, icon: &str, percent: i32, text: &str) -> Option<u32> {
    use std::collections::HashMap;
    use zbus::zvariant::Value;
    let mut hints: HashMap<&str, Value> = HashMap::new();
    hints.insert("transient", Value::Bool(true));
    hints.insert("x-canonical-private-synchronous", Value::from("slidewire"));
    let summary = if percent >= 0 {
        hints.insert("value", Value::I32(percent.clamp(0, 100)));
        format!("{percent}%")
    } else {
        text.to_string()
    };
    let body = if percent >= 0 { text } else { "" };
    let reply = conn
        .call_method(
            Some("org.freedesktop.Notifications"),
            "/org/freedesktop/Notifications",
            Some("org.freedesktop.Notifications"),
            "Notify",
            &("Slidewire", replaces, icon, summary.as_str(), body, Vec::<&str>::new(), hints, 1500i32),
        )
        .ok()?;
    reply.body().deserialize::<u32>().ok()
}

impl Osd {
    /// Plasma's volume OSD, or a notification on other desktops. Requests
    /// are coalesced so fast fader moves don't queue up.
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
                let plasma = has_owner(&conn, "org.kde.plasmashell");
                // Elsewhere: a desktop notification that replaces itself.
                let mut note_id = 0u32;
                while let Ok(mut req) = rx.recv() {
                    while let Ok(newer) = rx.try_recv() {
                        req = newer;
                    }
                    let (icon, percent, text) = req;
                    if plasma {
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
                    } else if let Some(id) = notify(&conn, note_id, &icon, percent, &text) {
                        note_id = id;
                    }
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
