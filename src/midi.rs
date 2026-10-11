//! MIDI port discovery and connections.
//!
//! Ports normally come from the ALSA sequencer (via midir). A device the
//! sequencer doesn't know about (for example when the kernel's `snd_seq_midi`
//! bridge can't load after a kernel update) is opened through its raw MIDI
//! device `/dev/snd/midiC*D*` instead, listed as `<name> [raw hw:C,D]`.

use std::fs::File;
use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Context, Result, anyhow};
use midir::{Ignore, MidiInput, MidiInputConnection, MidiOutput, MidiOutputConnection};

pub const CLIENT: &str = "Slidewire";

/// Find a port whose name contains `pattern` (case-insensitive), skipping our
/// own ports so we never connect to ourselves.
pub fn find_port<'a>(names: &'a [String], pattern: &str) -> Option<&'a String> {
    let p = pattern.trim().to_lowercase();
    if p.is_empty() {
        return None;
    }
    names.iter().find(|n| !n.starts_with(CLIENT) && n.to_lowercase().contains(&p))
}

/// Short device name for display: the sequencer client, or the raw device name.
pub fn display_name(port: &str) -> String {
    match port.rsplit_once(" [raw hw:") {
        Some((name, _)) => format!("{name} (raw)"),
        None => port.split(':').next().unwrap_or(port).to_string(),
    }
}

/// Lists ports without reconnecting the ALSA client every time.
pub struct PortLister {
    input: Option<MidiInput>,
    output: Option<MidiOutput>,
}

impl PortLister {
    pub fn new() -> Self {
        Self {
            input: MidiInput::new(&format!("{CLIENT} scan")).ok(),
            output: MidiOutput::new(&format!("{CLIENT} scan")).ok(),
        }
    }

    /// Sequencer ports first, then raw devices the sequencer doesn't cover.
    pub fn inputs(&self) -> Vec<String> {
        let seq: Vec<String> = self
            .input
            .as_ref()
            .map(|i| i.ports().iter().filter_map(|p| i.port_name(p).ok()).collect())
            .unwrap_or_default();
        with_raw(seq, |r| r.input)
    }

    pub fn outputs(&self) -> Vec<String> {
        let seq: Vec<String> = self
            .output
            .as_ref()
            .map(|o| o.ports().iter().filter_map(|p| o.port_name(p).ok()).collect())
            .unwrap_or_default();
        with_raw(seq, |r| r.output)
    }
}

fn with_raw(mut ports: Vec<String>, has_direction: impl Fn(&RawDevice) -> bool) -> Vec<String> {
    let raw: Vec<String> = raw_devices(Path::new("/proc/asound"))
        .into_iter()
        .filter(|r| has_direction(r) && (force_raw() || !seq_covers(&ports, &r.name)))
        .map(|r| r.port_name())
        .collect();
    ports.extend(raw);
    ports
}

/// `SLIDEWIRE_RAW=1` lists raw devices even when the sequencer has them (for testing).
fn force_raw() -> bool {
    crate::config::env("RAW").is_some_and(|v| v == "1")
}

/// Does a sequencer port already belong to the device called `name`?
fn seq_covers(seq_ports: &[String], name: &str) -> bool {
    let name = name.to_lowercase();
    seq_ports.iter().any(|p| p.to_lowercase().contains(&name))
}

/// A raw MIDI device from `/proc/asound/cardN/midiD`.
#[derive(Debug, PartialEq)]
pub struct RawDevice {
    pub card: u32,
    pub device: u32,
    pub name: String,
    pub input: bool,
    pub output: bool,
}

impl RawDevice {
    fn port_name(&self) -> String {
        format!("{} [raw hw:{},{}]", self.name, self.card, self.device)
    }
}

/// Parse `... [raw hw:C,D]` back into card and device numbers.
fn parse_raw_name(port: &str) -> Option<(u32, u32)> {
    let inner = port.strip_suffix(']')?.rsplit_once("[raw hw:")?.1;
    let (c, d) = inner.split_once(',')?;
    Some((c.parse().ok()?, d.parse().ok()?))
}

pub fn raw_devices(proc_root: &Path) -> Vec<RawDevice> {
    let mut out = Vec::new();
    let Ok(cards) = std::fs::read_dir(proc_root) else { return out };
    for card in cards.flatten() {
        let fname = card.file_name();
        let Some(card_no) = fname.to_str().and_then(|n| n.strip_prefix("card")).and_then(|n| n.parse().ok()) else {
            continue;
        };
        let Ok(files) = std::fs::read_dir(card.path()) else { continue };
        for f in files.flatten() {
            let fname = f.file_name();
            let Some(dev_no) = fname.to_str().and_then(|n| n.strip_prefix("midi")).and_then(|n| n.parse().ok()) else {
                continue;
            };
            let Ok(text) = std::fs::read_to_string(f.path()) else { continue };
            let name = text.lines().next().unwrap_or("").trim().to_string();
            if name.is_empty() {
                continue;
            }
            out.push(RawDevice {
                card: card_no,
                device: dev_no,
                name,
                input: text.lines().any(|l| l.starts_with("Input ")),
                output: text.lines().any(|l| l.starts_with("Output ")),
            });
        }
    }
    out.sort_by_key(|r| (r.card, r.device));
    out
}

fn raw_path(card: u32, device: u32) -> String {
    format!("/dev/snd/midiC{card}D{device}")
}

/// An open input; dropping it closes the port.
#[allow(dead_code)] // only held to keep the port open
pub enum MidiIn {
    Seq(MidiInputConnection<()>),
    Raw(RawInput),
}

pub struct RawInput {
    stop: Arc<AtomicBool>,
}

impl Drop for RawInput {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

pub enum MidiOut {
    Seq(MidiOutputConnection),
    Raw(File),
}

impl MidiOut {
    pub fn send(&mut self, bytes: &[u8]) -> Result<()> {
        match self {
            MidiOut::Seq(c) => c.send(bytes).map_err(|e| anyhow!("{e}")),
            MidiOut::Raw(f) => Ok(f.write_all(bytes)?),
        }
    }
}

pub fn connect_input(port_name: &str, mut callback: impl FnMut(&[u8]) + Send + 'static) -> Result<MidiIn> {
    if let Some((card, device)) = parse_raw_name(port_name) {
        return connect_raw_input(card, device, callback).map(MidiIn::Raw);
    }
    let mut input = MidiInput::new(CLIENT)?;
    input.ignore(Ignore::All);
    let port = input
        .ports()
        .into_iter()
        .find(|p| input.port_name(p).ok().as_deref() == Some(port_name))
        .ok_or_else(|| anyhow!("input port {port_name} vanished"))?;
    input
        .connect(&port, "in", move |_ts, msg, _| callback(msg), ())
        .map(MidiIn::Seq)
        .map_err(|e| anyhow!("connecting {port_name}: {e}"))
}

pub fn connect_output(port_name: &str) -> Result<MidiOut> {
    if let Some((card, device)) = parse_raw_name(port_name) {
        let path = raw_path(card, device);
        let f = std::fs::OpenOptions::new().write(true).open(&path).with_context(|| format!("opening {path}"))?;
        return Ok(MidiOut::Raw(f));
    }
    let output = MidiOutput::new(CLIENT)?;
    let port = output
        .ports()
        .into_iter()
        .find(|p| output.port_name(p).ok().as_deref() == Some(port_name))
        .ok_or_else(|| anyhow!("output port {port_name} vanished"))?;
    output.connect(&port, "out").map(MidiOut::Seq).map_err(|e| anyhow!("connecting {port_name}: {e}"))
}

fn connect_raw_input(card: u32, device: u32, mut callback: impl FnMut(&[u8]) + Send + 'static) -> Result<RawInput> {
    use std::os::unix::fs::OpenOptionsExt;
    let path = raw_path(card, device);
    let mut f = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(&path)
        .with_context(|| format!("opening {path}"))?;
    let stop = Arc::new(AtomicBool::new(false));
    let stop2 = stop.clone();
    std::thread::Builder::new().name(format!("rawmidi {card},{device}")).spawn(move || {
        let mut parser = StreamParser::default();
        let mut buf = [0u8; 256];
        while !stop2.load(Ordering::Relaxed) {
            // Wake up regularly to notice `stop`.
            let mut pfd = libc::pollfd { fd: f.as_raw_fd(), events: libc::POLLIN, revents: 0 };
            let r = unsafe { libc::poll(&mut pfd, 1, 200) };
            if r < 0 || pfd.revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0 {
                return; // unplugged; the port scan reconnects
            }
            if r == 0 {
                continue;
            }
            match f.read(&mut buf) {
                Ok(0) => return,
                Ok(n) => buf[..n].iter().for_each(|&b| parser.push(b, &mut callback)),
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(_) => return,
            }
        }
    })?;
    Ok(RawInput { stop })
}

/// Splits a raw MIDI byte stream into channel messages (handling running
/// status). SysEx, system common and realtime bytes are dropped, matching the
/// sequencer input's `Ignore::All`.
#[derive(Default)]
struct StreamParser {
    msg: Vec<u8>,
    running: Option<u8>,
    in_sysex: bool,
}

impl StreamParser {
    fn push(&mut self, b: u8, out: &mut impl FnMut(&[u8])) {
        match b {
            0xF8..=0xFF => {}
            0xF0 => {
                self.in_sysex = true;
                self.running = None;
            }
            0xF7 => self.in_sysex = false,
            0xF1..=0xF6 => {
                self.in_sysex = false;
                self.running = None;
                self.msg.clear();
            }
            0x80..=0xEF => {
                self.in_sysex = false;
                self.running = Some(b);
                self.msg.clear();
                self.msg.push(b);
            }
            _ if self.in_sysex => {}
            _ => {
                let Some(status) = self.running else { return };
                if self.msg.is_empty() {
                    self.msg.push(status);
                }
                self.msg.push(b);
                let len = if matches!(status & 0xF0, 0xC0 | 0xD0) { 2 } else { 3 };
                if self.msg.len() == len {
                    out(&self.msg);
                    self.msg.clear();
                }
            }
        }
    }
}

/// Calls `on_change` whenever a sound device node appears or disappears in
/// `/dev/snd` (a controller plugged in or out), so ports can be rescanned
/// right away instead of on the next periodic scan.
pub fn watch_devices(on_change: impl Fn() + Send + 'static) {
    let spawned = std::thread::Builder::new().name("hotplug".into()).spawn(move || unsafe {
        let fd = libc::inotify_init1(libc::IN_CLOEXEC);
        if fd < 0 {
            return;
        }
        let path = c"/dev/snd";
        if libc::inotify_add_watch(fd, path.as_ptr(), libc::IN_CREATE | libc::IN_DELETE) < 0 {
            libc::close(fd);
            return;
        }
        let mut buf = [0u8; 4096];
        loop {
            let n = libc::read(fd, buf.as_mut_ptr().cast(), buf.len());
            if n <= 0 {
                break;
            }
            on_change();
        }
        libc::close(fd);
    });
    if let Err(e) = spawned {
        eprintln!("hotplug watcher: {e}");
    }
}

/// `--list-ports`
pub fn print_ports() {
    let l = PortLister::new();
    println!("MIDI inputs:");
    for p in l.inputs() {
        println!("  {p}");
    }
    println!("MIDI outputs:");
    for p in l.outputs() {
        println!("  {p}");
    }
}

/// `--monitor [pattern]`: print decoded messages from matching inputs.
pub fn monitor(pattern: Option<&str>) -> Result<()> {
    let l = PortLister::new();
    let ports: Vec<String> = l
        .inputs()
        .into_iter()
        .filter(|n| !n.starts_with(CLIENT) && pattern.is_none_or(|p| n.to_lowercase().contains(&p.to_lowercase())))
        .collect();
    if ports.is_empty() {
        return Err(anyhow!("no matching MIDI input ports (try --list-ports)"));
    }
    let mut conns = Vec::new();
    for p in ports {
        let name = p.clone();
        println!("listening on {p}");
        conns.push(connect_input(&p, move |bytes| {
            let hex: Vec<String> = bytes.iter().map(|b| format!("{b:02X}")).collect();
            match crate::mapping::Msg::parse(bytes) {
                Some(m) => println!("[{name}] {:<12} {}", hex.join(" "), m.describe()),
                None => println!("[{name}] {}", hex.join(" ")),
            }
        })?);
    }
    println!("press Ctrl+C to stop");
    loop {
        std::thread::park();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(bytes: &[u8]) -> Vec<Vec<u8>> {
        let mut p = StreamParser::default();
        let mut out = Vec::new();
        for &b in bytes {
            p.push(b, &mut |m: &[u8]| out.push(m.to_vec()));
        }
        out
    }

    #[test]
    fn stream_parser() {
        // Running status, a realtime byte in the middle, program change, sysex.
        let got = parse(&[0xB0, 0x00, 0x40, 0x01, 0xF8, 0x7F, 0xC1, 0x05, 0xF0, 0x42, 0x40, 0xF7, 0x90, 0x3C, 0x00]);
        assert_eq!(got, vec![vec![0xB0, 0x00, 0x40], vec![0xB0, 0x01, 0x7F], vec![0xC1, 0x05], vec![0x90, 0x3C, 0x00]]);
        // Data bytes before any status are ignored.
        assert!(parse(&[0x10, 0x20]).is_empty());
    }

    #[test]
    fn raw_device_listing() {
        let root = std::env::temp_dir().join(format!("slidewire-proc-{}", std::process::id()));
        let card = root.join("card3");
        std::fs::create_dir_all(&card).unwrap();
        std::fs::write(
            card.join("midi0"),
            "nanoKONTROL2\n\nType: Legacy\nOutput 0\n  Tx bytes     : 0\nInput 0\n  Rx bytes     : 0\n",
        )
        .unwrap();
        std::fs::write(card.join("id"), "nanoKONTROL2\n").unwrap();
        let devs = raw_devices(&root);
        std::fs::remove_dir_all(&root).unwrap();
        assert_eq!(
            devs,
            vec![RawDevice { card: 3, device: 0, name: "nanoKONTROL2".into(), input: true, output: true }]
        );
        let name = devs[0].port_name();
        assert_eq!(name, "nanoKONTROL2 [raw hw:3,0]");
        assert_eq!(parse_raw_name(&name), Some((3, 0)));
        assert_eq!(parse_raw_name("nanoKONTROL2:nanoKONTROL2 _ CTRL 16:0"), None);
        // Only offered when the sequencer doesn't have the device.
        assert!(seq_covers(&["nanoKONTROL2:nanoKONTROL2 _ CTRL 16:0".into()], "nanoKONTROL2"));
        assert!(!seq_covers(&["Midi Through:Midi Through Port-0 14:0".into()], "nanoKONTROL2"));
    }
}
