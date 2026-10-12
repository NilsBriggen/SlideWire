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

/// A strip by the name you gave it ("Music"), its control's name
/// ("Knob 3"), or a number for that fader ("1" = "Fader 1"), ignoring case.
fn pick_strip<'a>(strips: &'a [crate::engine::StripView], name: &str) -> Option<&'a crate::engine::StripView> {
    let want = name.trim().to_lowercase();
    let by_number = format!("fader {want}");
    strips
        .iter()
        .find(|s| s.label.as_deref().is_some_and(|l| l.trim().to_lowercase() == want))
        .or_else(|| strips.iter().find(|s| s.name.to_lowercase() == want))
        .or_else(|| strips.iter().find(|s| s.name.to_lowercase() == by_number))
}

fn find_strip(name: &str) -> zbus::fdo::Result<crate::engine::StripView> {
    let shared = SHARED.get().ok_or_else(|| failed("not ready"))?;
    let strips = shared.lock().unwrap().strips.clone();
    pick_strip(&strips, name)
        .cloned()
        .ok_or_else(|| failed(format!("no strip called \"{name}\" (see `slidewire strips`)")))
}

/// The key of the scene called `name`, ignoring case.
fn find_scene(name: &str) -> zbus::fdo::Result<String> {
    let shared = SHARED.get().ok_or_else(|| failed("not ready"))?;
    let sh = shared.lock().unwrap();
    let want = name.trim().to_lowercase();
    sh.active()
        .and_then(|p| p.scenes.keys().find(|k| k.to_lowercase() == want).cloned())
        .ok_or_else(|| failed(format!("no scene called \"{name}\" (see `slidewire scenes`)")))
}

/// What a strip controls, for listings.
fn strip_controls(s: &crate::engine::StripView) -> String {
    if s.resolved.is_empty() {
        s.targets.iter().map(|t| t.label()).collect::<Vec<_>>().join(" + ")
    } else {
        s.resolved.join(" + ")
    }
}

pub const BUS_NAME: &str = crate::APP_ID;
const PATH: &str = "/dev/briggen/Slidewire";
const IFACE: &str = crate::APP_ID;
/// The name before 1.0, still owned by a copy that was running when the
/// app was updated.
const OLD_BUS_NAME: &str = "dev.slidewire.Slidewire";
const KWIN_SCRIPT: &str = "slidewire-focus";

struct Service {
    tx: Sender<Event>,
    own_pid: i32,
}

#[zbus::interface(name = "dev.briggen.Slidewire")]
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
                let what = strip_controls(&s);
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

    /// Set a strip's real volume to `percent`, through its range and curve
    /// (or change it by `percent` if `relative`). Returns the new volume.
    fn set_level(&self, strip: String, percent: f64, relative: bool) -> zbus::fdo::Result<f64> {
        let s = find_strip(&strip)?;
        let now = s.range.volume(s.position) as f64 * 100.0;
        let target = if relative { now + percent } else { percent };
        let vol = (target / 100.0).clamp(s.range.min as f64, s.range.top as f64) as f32;
        let pos = s.range.position(vol).clamp(0.0, 1.0);
        let _ = self.tx.send(Event::Ui(UiCmd::SetGroupPosition(s.id, pos)));
        Ok((s.range.volume(pos) * 100.0).round() as f64)
    }

    /// Switch to bank `bank` (1 = first).
    fn bank(&self, bank: u32) -> zbus::fdo::Result<()> {
        let shared = SHARED.get().ok_or_else(|| failed("not ready"))?;
        let count = shared.lock().unwrap().active().map_or(1, |p| p.bank_count());
        if bank == 0 || bank as usize > count {
            let banks = if count == 1 { "1 bank".to_string() } else { format!("{count} banks") };
            return Err(failed(format!("there is no bank {bank}; this profile has {banks}")));
        }
        let _ = self.tx.send(Event::Ui(UiCmd::SetBank(bank as usize - 1)));
        Ok(())
    }

    fn scene(&self, name: String) -> zbus::fdo::Result<()> {
        let name = find_scene(&name)?;
        let _ = self.tx.send(Event::Ui(UiCmd::RecallScene(name)));
        Ok(())
    }

    /// Save the strips as a scene (replacing one of the same name).
    fn save_scene(&self, name: String) -> zbus::fdo::Result<()> {
        let name = name.trim().to_string();
        if name.is_empty() {
            return Err(failed("a scene needs a name"));
        }
        let _ = self.tx.send(Event::Ui(UiCmd::SaveScene(name)));
        Ok(())
    }

    /// Switch to the profile with this name or id.
    fn profile(&self, name: String) -> zbus::fdo::Result<String> {
        let shared = SHARED.get().ok_or_else(|| failed("not ready"))?;
        let want = name.trim().to_lowercase();
        let found = {
            let sh = shared.lock().unwrap();
            sh.store
                .profiles
                .iter()
                .find(|(id, p)| p.name.to_lowercase() == want || id.to_lowercase() == want)
                .map(|(id, p)| (id.clone(), p.name.clone()))
        };
        let (id, pname) =
            found.ok_or_else(|| failed(format!("no profile called \"{name}\" (see `slidewire profiles`)")))?;
        let _ = self.tx.send(Event::Ui(UiCmd::ActivateProfile(id)));
        Ok(pname)
    }

    /// The default microphone: "on" mutes, "off" unmutes, "toggle".
    /// Returns whether it is now muted.
    fn mic(&self, mode: String) -> zbus::fdo::Result<bool> {
        let shared = SHARED.get().ok_or_else(|| failed("not ready"))?;
        let muted = {
            let sh = shared.lock().unwrap();
            sh.audio.sources.iter().find(|d| d.name == sh.audio.default_source).map(|d| d.mute)
        };
        let muted = muted.ok_or_else(|| failed("there is no microphone"))?;
        let want = match mode.as_str() {
            "on" => true,
            "off" => false,
            "toggle" | "" => !muted,
            other => return Err(failed(format!("mic mode must be on, off or toggle, not \"{other}\""))),
        };
        if want != muted {
            let _ = self.tx.send(Event::Ui(UiCmd::ToggleMicMute));
        }
        Ok(want)
    }

    /// Everything a script or status bar wants to show, as JSON.
    fn state(&self) -> zbus::fdo::Result<String> {
        let shared = SHARED.get().ok_or_else(|| failed("not ready"))?;
        let sh = shared.lock().unwrap();
        let active = sh.active_id();
        let profile = sh.active();
        let strips: Vec<_> = sh
            .strips
            .iter()
            .map(|s| {
                serde_json::json!({
                    "name": s.name,
                    "label": s.label,
                    "position": (s.position * 100.0).round(),
                    "volume": (s.range.volume(s.position) * 100.0).round(),
                    "muted": s.muted,
                    "controls": strip_controls(s),
                })
            })
            .collect();
        let profiles: Vec<_> = sh
            .store
            .profiles
            .iter()
            .map(|(id, p)| serde_json::json!({ "id": id, "name": p.name, "active": Some(id) == active.as_ref() }))
            .collect();
        let mic = sh.audio.sources.iter().find(|d| d.name == sh.audio.default_source);
        let state = serde_json::json!({
            "profile": profile.map(|p| p.name.clone()),
            "profiles": profiles,
            "bank": profile.map_or(1, |p| p.bank + 1),
            "banks": profile.map_or(1, |p| p.bank_count()),
            "scenes": profile.map(|p| p.scenes.keys().cloned().collect::<Vec<_>>()).unwrap_or_default(),
            "strips": strips,
            "mic": mic.map(|d| serde_json::json!({ "name": d.label, "muted": d.mute })),
        });
        Ok(state.to_string())
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

/// `slidewire strips|set|mute|bank|scene|...`: talk to the running app.
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
    let json = args.iter().any(|a| a == "--json");
    let real = args.iter().any(|a| a == "--volume");
    let args: Vec<&String> = args.iter().filter(|a| *a != "--json" && *a != "--volume").collect();
    let arg = |i: usize, what: &str| args.get(i).map(|s| s.to_string()).with_context(|| format!("missing {what}"));
    let state = || -> Result<serde_json::Value> {
        let reply = call("State", &())?;
        let text: String = reply.body().deserialize()?;
        Ok(serde_json::from_str(&text)?)
    };
    // Ignore write errors: `slidewire strips | head` closes the pipe early.
    use std::io::Write;
    let mut out = std::io::stdout().lock();
    let text = |v: &serde_json::Value| v.as_str().unwrap_or_default().to_string();
    match args[0].as_str() {
        "state" => {
            let _ = writeln!(out, "{}", state()?);
        }
        "strips" => {
            let st = state()?;
            if json {
                let _ = writeln!(out, "{}", st["strips"]);
                return Ok(());
            }
            for s in st["strips"].as_array().into_iter().flatten() {
                let state = if s["muted"].as_bool() == Some(true) {
                    "muted".to_string()
                } else {
                    format!("{:>3}%", s["volume"])
                };
                let name = match s["label"].as_str() {
                    Some(l) => format!("{} ({l})", text(&s["name"])),
                    None => text(&s["name"]),
                };
                let what = text(&s["controls"]);
                let _ = writeln!(out, "{name:<20} {state:>6}  {}", if what.is_empty() { "-" } else { &what });
            }
        }
        "set" => {
            let strip = arg(1, "strip name")?;
            let value = arg(2, "volume (50, +5 or -5)")?;
            let relative = value.starts_with('+') || value.starts_with('-');
            let pct: f64 = value.trim_end_matches('%').parse().context("volume must be a number like 50, +5 or -5")?;
            let method = if real { "SetLevel" } else { "SetVolume" };
            let reply = call(method, &(strip.as_str(), pct, relative))?;
            let now: f64 = reply.body().deserialize()?;
            let _ = writeln!(out, "{strip}: {now:.0}%");
        }
        "mute" => {
            let strip = arg(1, "strip name")?;
            let mode = args.get(2).map(|s| s.to_string()).unwrap_or_else(|| "toggle".into());
            let reply = call("Mute", &(strip.as_str(), mode.as_str()))?;
            let muted: bool = reply.body().deserialize()?;
            let _ = writeln!(out, "{strip}: {}", if muted { "muted" } else { "unmuted" });
        }
        "mic" => {
            let mode = args.get(1).map(|s| s.to_string()).unwrap_or_else(|| "toggle".into());
            let reply = call("Mic", &(mode.as_str(),))?;
            let muted: bool = reply.body().deserialize()?;
            let _ = writeln!(out, "microphone: {}", if muted { "muted" } else { "on" });
        }
        "bank" => {
            let n: u32 = arg(1, "bank number")?.parse().context("bank must be a number")?;
            call("Bank", &(n,))?;
        }
        "banks" => {
            let st = state()?;
            if json {
                let _ = writeln!(out, "{}", serde_json::json!({ "bank": st["bank"], "banks": st["banks"] }));
            } else {
                let _ = writeln!(out, "bank {} of {}", st["bank"], st["banks"]);
            }
        }
        "scene" if args.get(1).is_some_and(|a| *a == "save") => {
            let name = args[2..].iter().map(|s| s.as_str()).collect::<Vec<_>>().join(" ");
            call("SaveScene", &(name.as_str(),))?;
        }
        "scene" => {
            call("Scene", &(arg(1, "scene name")?.as_str(),))?;
        }
        "scenes" => {
            let st = state()?;
            if json {
                let _ = writeln!(out, "{}", st["scenes"]);
            } else {
                for s in st["scenes"].as_array().into_iter().flatten() {
                    let _ = writeln!(out, "{}", text(s));
                }
            }
        }
        "profile" => {
            let reply = call("Profile", &(arg(1, "profile name")?.as_str(),))?;
            let name: String = reply.body().deserialize()?;
            let _ = writeln!(out, "profile: {name}");
        }
        "profiles" => {
            let st = state()?;
            if json {
                let _ = writeln!(out, "{}", st["profiles"]);
            } else {
                for p in st["profiles"].as_array().into_iter().flatten() {
                    let mark = if p["active"].as_bool() == Some(true) { "*" } else { " " };
                    let _ = writeln!(out, "{mark} {}", text(&p["name"]));
                }
            }
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
    if name == BUS_NAME {
        // An older version still running: show it rather than start twice.
        let conn = Connection::session()?;
        if has_owner(&conn, OLD_BUS_NAME) {
            let _ = conn.call_method(Some(OLD_BUS_NAME), "/dev/slidewire/Slidewire", Some(OLD_BUS_NAME), "Show", &());
            return Ok(None);
        }
    }
    let conn = zbus::blocking::connection::Builder::session()?.serve_at(PATH, svc)?.build()?;
    // Without DoNotQueue a second copy would wait in line for the name and
    // carry on as if it were the only one.
    let flags = zbus::fdo::RequestNameFlags::DoNotQueue.into();
    match conn.request_name_with_flags(name.as_str(), flags) {
        Ok(_) => Ok(Some(conn)),
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
    // Not /tmp: another user could put a script of their own there first.
    let dir = std::env::var_os("XDG_RUNTIME_DIR").map(std::path::PathBuf::from).context("XDG_RUNTIME_DIR isn't set")?;
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
fn mpris_names(conn: &Connection) -> Result<Vec<String>> {
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
    Ok(names.into_iter().filter(|n| n.starts_with("org.mpris.MediaPlayer2.")).collect())
}

/// One session connection for our outgoing calls. Calls time out, so a
/// hung media player or shell can't block a thread forever.
pub fn client() -> Result<Connection> {
    static CONN: std::sync::Mutex<Option<Connection>> = std::sync::Mutex::new(None);
    let mut c = CONN.lock().unwrap();
    if let Some(conn) = &*c {
        return Ok(conn.clone());
    }
    let conn = zbus::blocking::connection::Builder::session()?.method_timeout(Duration::from_secs(2)).build()?;
    *c = Some(conn.clone());
    Ok(conn)
}

/// Short names of the running media players ("spotify", "firefox", ...).
/// For the UI: answers at once from a list refreshed in the background.
pub fn mpris_players() -> Vec<String> {
    static CACHE: std::sync::Mutex<(Option<std::time::Instant>, Vec<String>, bool)> =
        std::sync::Mutex::new((None, Vec::new(), false));
    let mut cache = CACHE.lock().unwrap();
    let stale = cache.0.is_none_or(|t| t.elapsed() > Duration::from_secs(2));
    if stale && !cache.2 {
        cache.2 = true;
        std::thread::spawn(|| {
            let players = list_players();
            *CACHE.lock().unwrap() = (Some(std::time::Instant::now()), players, false);
        });
    }
    cache.1.clone()
}

fn list_players() -> Vec<String> {
    let Ok(conn) = client() else { return Vec::new() };
    let mut names: Vec<String> = mpris_names(&conn)
        .unwrap_or_default()
        .iter()
        .map(|n| {
            // Drop the prefix and any instance suffix ("firefox.instance_1_23").
            let short = n.trim_start_matches("org.mpris.MediaPlayer2.");
            short.split(".instance").next().unwrap_or(short).to_string()
        })
        .collect();
    names.sort();
    names.dedup();
    names
}

/// Send a media key to `player` (part of its name), or to whichever player
/// is playing when that's empty.
pub fn media(key: MediaKey, player: &str) -> Result<()> {
    let conn = client()?;
    let want = player.trim().to_lowercase();
    let players: Vec<String> =
        mpris_names(&conn)?.into_iter().filter(|n| want.is_empty() || n.to_lowercase().contains(&want)).collect();
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
        if want.is_empty() {
            anyhow::bail!("no MPRIS media player running");
        }
        anyhow::bail!("no media player named like \"{player}\" is running");
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
    /// Bounded: if the shell hangs, pop-ups are dropped rather than queued.
    tx: Sender<(String, i32, String)>,
}

fn has_owner(conn: &Connection, name: &str) -> bool {
    zbus::blocking::fdo::DBusProxy::new(conn)
        .ok()
        .and_then(|p| p.name_has_owner(name.try_into().ok()?).ok())
        .unwrap_or(false)
}

/// Is there a system tray to show our icon in? (GNOME has none without an
/// extension.)
pub fn has_tray_host() -> bool {
    client().is_ok_and(|c| has_owner(&c, "org.kde.StatusNotifierWatcher"))
}

/// A one-off desktop notification.
pub fn notify_text(summary: &str, body: &str) {
    use std::collections::HashMap;
    use zbus::zvariant::Value;
    let Ok(conn) = client() else { return };
    let hints: HashMap<&str, Value> = HashMap::new();
    let _ = conn.call_method(
        Some("org.freedesktop.Notifications"),
        "/org/freedesktop/Notifications",
        Some("org.freedesktop.Notifications"),
        "Notify",
        &("Slidewire", 0u32, crate::APP_ID, summary, body, Vec::<&str>::new(), hints, 6000i32),
    );
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
        let (tx, rx) = crossbeam_channel::bounded::<(String, i32, String)>(8);
        std::thread::Builder::new()
            .name("osd".into())
            .spawn(move || {
                // Test instances must not pop up on the user's desktop.
                if crate::config::env("INSTANCE").is_some() {
                    while rx.recv().is_ok() {}
                    return;
                }
                let Ok(conn) = client() else { return };
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
        let _ = self.tx.try_send((icon.to_string(), percent, text.to_string()));
    }

    /// A text-only pop-up (bank and scene changes).
    pub fn show_text(&self, icon: &str, text: &str) {
        let _ = self.tx.try_send((icon.to_string(), -1, text.to_string()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::StripView;

    #[test]
    fn strips_by_label_name_or_number() {
        let strip = |name: &str, label: Option<&str>| StripView {
            id: name.into(),
            name: name.into(),
            label: label.map(String::from),
            ..Default::default()
        };
        let strips = [strip("Fader 1", Some("Music")), strip("Fader 2", None), strip("Knob 3", Some("Fader 2"))];
        let pick = |n: &str| pick_strip(&strips, n).map(|s| s.id.as_str());
        assert_eq!(pick("music"), Some("Fader 1"));
        assert_eq!(pick(" MUSIC "), Some("Fader 1"));
        assert_eq!(pick("knob 3"), Some("Knob 3"));
        assert_eq!(pick("1"), Some("Fader 1"));
        // A strip the user named "Fader 2" wins over the control of that name.
        assert_eq!(pick("fader 2"), Some("Knob 3"));
        assert_eq!(pick("drums"), None);
    }
}
