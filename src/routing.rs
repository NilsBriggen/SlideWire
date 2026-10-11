//! Buses, remembered routes and monitoring: audio plumbing built from the
//! sound server's own modules.
//!
//! A bus is a virtual output (`module-null-sink`) whose sound a loopback
//! plays on a real output. Apps routed into a bus can be turned up and down
//! together, like the "Game" and "Chat" mixes of streaming setups.

use serde::{Deserialize, Serialize};

use crate::audio::{AudioCmd, AudioState, Module};
use crate::profile::AppMatch;

/// Sink names of buses start with this.
pub const BUS_PREFIX: &str = "slidewire_";
/// `media.name` of our loopback streams, which keeps them out of the
/// Applications list.
pub const BUS_STREAM: &str = "SlidewireBus";
pub const MONITOR_STREAM: &str = "SlidewireMonitor";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Bus {
    pub name: String,
    /// The output it plays on; None: the default output.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
}

impl Bus {
    /// "Game chat" -> "slidewire_game_chat".
    pub fn sink_name(&self) -> String {
        let slug: String = self
            .name
            .trim()
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '_' })
            .collect();
        format!("{BUS_PREFIX}{slug}")
    }
}

pub fn is_bus(sink_name: &str) -> bool {
    sink_name.starts_with(BUS_PREFIX)
}

/// An app that always plays on (or records from) a device.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Route {
    pub app: AppMatch,
    /// Device name.
    pub device: String,
    /// A recording stream (from an input) rather than playback.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub input: bool,
}

/// The value of `key=` in a module argument string we wrote.
pub fn arg_value<'a>(args: &'a str, key: &str) -> Option<&'a str> {
    let want = format!("{key}=");
    args.split_whitespace().find_map(|part| part.strip_prefix(want.as_str()))
}

fn null_sink_args(bus: &Bus) -> String {
    let label: String = bus.name.chars().filter(|c| !matches!(c, '"' | '\'' | '\\')).collect();
    format!("sink_name={} sink_properties='device.description=\"{label}\"'", bus.sink_name())
}

fn loopback_args(source: &str, sink: &str, stream: &str, latency_ms: u32) -> String {
    format!(
        "source={source} sink={sink} latency_msec={latency_ms} source_dont_move=true sink_dont_move=true \
         sink_input_properties=media.name={stream} source_output_properties=media.name={stream}"
    )
}

/// The default output, unless that is a bus: playing a bus into itself
/// would feed back. Then the first real output.
pub fn real_default_sink(audio: &AudioState) -> Option<String> {
    if !is_bus(&audio.default_sink) && audio.sinks.iter().any(|d| d.name == audio.default_sink) {
        return Some(audio.default_sink.clone());
    }
    audio.sinks.iter().find(|d| !is_bus(&d.name)).map(|d| d.name.clone())
}

fn loopbacks_from<'a>(audio: &'a AudioState, source: &str) -> Vec<&'a Module> {
    audio
        .modules
        .iter()
        .filter(|m| m.name == "module-loopback" && arg_value(&m.argument, "source") == Some(source))
        .collect()
}

/// What to load and unload so the server has exactly the configured buses,
/// each playing on its output. Call again after the server reported the
/// result: a bus's loopback is loaded once its sink exists.
pub fn sync_buses(buses: &[Bus], audio: &AudioState) -> Vec<AudioCmd> {
    let mut cmds = Vec::new();
    if !audio.connected {
        return cmds;
    }
    for bus in buses {
        let sink = bus.sink_name();
        if !audio.sinks.iter().any(|d| d.name == sink) {
            if !audio.modules.iter().any(|m| arg_value(&m.argument, "sink_name") == Some(&sink)) {
                cmds.push(AudioCmd::LoadModule("module-null-sink".into(), null_sink_args(bus)));
            }
            continue;
        }
        let output = bus
            .output
            .clone()
            .filter(|o| !is_bus(o) && audio.sinks.iter().any(|d| d.name == *o))
            .or_else(|| real_default_sink(audio));
        let Some(output) = output else { continue };
        let monitor = format!("{sink}.monitor");
        match loopbacks_from(audio, &monitor).as_slice() {
            [] => cmds
                .push(AudioCmd::LoadModule("module-loopback".into(), loopback_args(&monitor, &output, BUS_STREAM, 20))),
            [m] if arg_value(&m.argument, "sink") == Some(output.as_str()) => {}
            // Wrong output (or duplicates): unload; the next sync loads it again.
            wrong => cmds.extend(wrong.iter().map(|m| AudioCmd::UnloadModule(m.index))),
        }
    }
    // Buses that were removed.
    let configured = |sink: &str| buses.iter().any(|b| b.sink_name() == sink);
    for m in &audio.modules {
        let stale = match m.name.as_str() {
            "module-null-sink" => arg_value(&m.argument, "sink_name").is_some_and(|n| is_bus(n) && !configured(n)),
            "module-loopback" => arg_value(&m.argument, "source")
                .and_then(|s| s.strip_suffix(".monitor"))
                .is_some_and(|n| is_bus(n) && !configured(n)),
            _ => false,
        };
        if stale {
            cmds.push(AudioCmd::UnloadModule(m.index));
        }
    }
    cmds
}

/// Our monitor loopbacks ("hear yourself"), from `source`.
pub fn monitors_from<'a>(audio: &'a AudioState, source: &'a str) -> impl Iterator<Item = &'a Module> + 'a {
    audio.modules.iter().filter(move |m| {
        m.name == "module-loopback"
            && m.argument.contains(MONITOR_STREAM)
            && arg_value(&m.argument, "source") == Some(source)
    })
}

/// All monitor loopbacks, to remove when quitting or after a crash.
pub fn all_monitors(audio: &AudioState) -> impl Iterator<Item = &Module> {
    audio.modules.iter().filter(|m| m.name == "module-loopback" && m.argument.contains(MONITOR_STREAM))
}

/// Start monitoring `source` on `sink` (low latency, so it sounds live).
pub fn monitor_cmd(source: &str, sink: &str) -> AudioCmd {
    AudioCmd::LoadModule("module-loopback".into(), loopback_args(source, sink, MONITOR_STREAM, 10))
}

/// EasyEffects presets: (name, true for input presets).
pub fn easyeffects_presets() -> Vec<(String, bool)> {
    let Some(base) = directories::BaseDirs::new() else { return Vec::new() };
    let roots = [
        base.config_dir().join("easyeffects"),
        base.data_dir().join("easyeffects"),
        base.home_dir().join(".var/app/com.github.wwmm.easyeffects/config/easyeffects"),
        base.home_dir().join(".var/app/com.github.wwmm.easyeffects/data/easyeffects"),
    ];
    let mut out = Vec::new();
    for root in roots {
        for (dir, input) in [("output", false), ("input", true)] {
            let Ok(rd) = std::fs::read_dir(root.join(dir)) else { continue };
            for e in rd.flatten() {
                let p = e.path();
                if p.extension().is_some_and(|x| x == "json")
                    && let Some(stem) = p.file_stem()
                {
                    out.push((stem.to_string_lossy().to_string(), input));
                }
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

/// Load an EasyEffects preset (the app, or its Flatpak).
pub fn easyeffects_load(preset: &str) -> anyhow::Result<()> {
    let native = std::env::var_os("PATH")
        .is_some_and(|path| std::env::split_paths(&path).any(|d| d.join("easyeffects").is_file()));
    let mut cmd = if native {
        std::process::Command::new("easyeffects")
    } else {
        let mut c = std::process::Command::new("flatpak");
        c.args(["run", "com.github.wwmm.easyeffects"]);
        c
    };
    cmd.args(["-l", preset]);
    crate::actions::spawn_detached(&mut cmd).map_err(|e| anyhow::anyhow!("EasyEffects isn't installed ({e})"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::Device;

    fn sink(name: &str) -> Device {
        Device { name: name.into(), ..Default::default() }
    }

    fn module(index: u32, name: &str, argument: String) -> Module {
        Module { index, name: name.into(), argument }
    }

    fn loads(cmds: &[AudioCmd]) -> Vec<String> {
        cmds.iter()
            .map(|c| match c {
                AudioCmd::LoadModule(n, a) => format!("load {n} {a}"),
                AudioCmd::UnloadModule(i) => format!("unload {i}"),
                _ => "other".into(),
            })
            .collect()
    }

    #[test]
    fn bus_lifecycle() {
        let game = Bus { name: "Game chat".into(), output: None };
        assert_eq!(game.sink_name(), "slidewire_game_chat");
        let mut audio = AudioState {
            connected: true,
            sinks: vec![sink("speakers")],
            default_sink: "speakers".into(),
            ..Default::default()
        };
        // First the sink.
        let cmds = loads(&sync_buses(std::slice::from_ref(&game), &audio));
        assert_eq!(cmds.len(), 1);
        assert!(cmds[0].starts_with("load module-null-sink sink_name=slidewire_game_chat "), "{cmds:?}");
        assert!(cmds[0].contains("device.description=\"Game chat\""));
        // Once it exists, the loopback to the default output.
        audio.sinks.push(sink("slidewire_game_chat"));
        audio.modules.push(module(7, "module-null-sink", null_sink_args(&game)));
        let cmds = loads(&sync_buses(std::slice::from_ref(&game), &audio));
        assert_eq!(cmds.len(), 1);
        assert!(cmds[0].contains("source=slidewire_game_chat.monitor sink=speakers "), "{cmds:?}");
        // Then nothing to do.
        let args = loopback_args("slidewire_game_chat.monitor", "speakers", BUS_STREAM, 20);
        audio.modules.push(module(8, "module-loopback", args));
        assert!(sync_buses(std::slice::from_ref(&game), &audio).is_empty());
        // A different output: the loopback is replaced.
        audio.sinks.push(sink("headset"));
        let moved = Bus { output: Some("headset".into()), ..game.clone() };
        assert_eq!(loads(&sync_buses(std::slice::from_ref(&moved), &audio)), ["unload 8"]);
        // Removed: both modules go.
        assert_eq!(loads(&sync_buses(&[], &audio)), ["unload 7", "unload 8"]);
    }

    #[test]
    fn a_bus_never_plays_into_a_bus() {
        let audio = AudioState {
            connected: true,
            sinks: vec![sink("slidewire_music"), sink("speakers")],
            default_sink: "slidewire_music".into(),
            ..Default::default()
        };
        assert_eq!(real_default_sink(&audio).as_deref(), Some("speakers"));
    }

    #[test]
    fn module_arguments() {
        let args = loopback_args("mic", "headset", MONITOR_STREAM, 10);
        assert_eq!(arg_value(&args, "source"), Some("mic"));
        assert_eq!(arg_value(&args, "sink"), Some("headset"));
        assert_eq!(arg_value(&args, "latency_msec"), Some("10"));
        let audio = AudioState { modules: vec![module(3, "module-loopback", args)], ..Default::default() };
        assert_eq!(monitors_from(&audio, "mic").count(), 1);
        assert_eq!(monitors_from(&audio, "other").count(), 0);
    }
}
