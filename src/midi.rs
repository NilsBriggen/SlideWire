//! MIDI port discovery and connections (ALSA sequencer via midir).

use anyhow::{Result, anyhow};
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

    pub fn inputs(&self) -> Vec<String> {
        self.input.as_ref().map(|i| i.ports().iter().filter_map(|p| i.port_name(p).ok()).collect()).unwrap_or_default()
    }

    pub fn outputs(&self) -> Vec<String> {
        self.output.as_ref().map(|o| o.ports().iter().filter_map(|p| o.port_name(p).ok()).collect()).unwrap_or_default()
    }
}

pub fn connect_input(
    port_name: &str,
    mut callback: impl FnMut(&[u8]) + Send + 'static,
) -> Result<MidiInputConnection<()>> {
    let mut input = MidiInput::new(CLIENT)?;
    input.ignore(Ignore::All);
    let port = input
        .ports()
        .into_iter()
        .find(|p| input.port_name(p).ok().as_deref() == Some(port_name))
        .ok_or_else(|| anyhow!("input port {port_name} vanished"))?;
    input.connect(&port, "in", move |_ts, msg, _| callback(msg), ()).map_err(|e| anyhow!("connecting {port_name}: {e}"))
}

pub fn connect_output(port_name: &str) -> Result<MidiOutputConnection> {
    let output = MidiOutput::new(CLIENT)?;
    let port = output
        .ports()
        .into_iter()
        .find(|p| output.port_name(p).ok().as_deref() == Some(port_name))
        .ok_or_else(|| anyhow!("output port {port_name} vanished"))?;
    output.connect(&port, "out").map_err(|e| anyhow!("connecting {port_name}: {e}"))
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
