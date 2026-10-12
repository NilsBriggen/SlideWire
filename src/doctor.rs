//! "Why isn't my controller showing up?": checks the chain from USB to the
//! MIDI port, using plain file reads (no root needed).

use std::path::Path;

use rust_i18n::t;

use crate::midi;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Status {
    Ok,
    Warn,
    Fail,
}

#[derive(Clone, Debug)]
pub struct Check {
    pub status: Status,
    pub title: String,
    pub detail: String,
}

fn check(status: Status, title: impl Into<String>, detail: impl Into<String>) -> Check {
    Check { status, title: title.into(), detail: detail.into() }
}

/// Check the chain from USB to the MIDI port for a device whose ports match
/// `pattern`. `seq_ports` are the input ports midir lists.
pub fn diagnose(pattern: &str, seq_ports: &[String]) -> Vec<Check> {
    let pattern = pattern.trim();
    let mut out = Vec::new();
    if let Some(c) = kernel_check(Path::new("/proc/sys/kernel/osrelease"), Path::new("/lib/modules")) {
        out.push(c);
    }
    if pattern.is_empty() {
        out.push(check(
            Status::Fail,
            t!("No MIDI input set"),
            t!("Set the profile's MIDI input on the Controller page."),
        ));
        return out;
    }

    // A software port (another app, or a virtual device) has no hardware to check.
    let raw = midi::raw_devices(Path::new("/proc/asound"));
    let raw_match = raw.iter().find(|r| matches(&r.name, pattern));
    let seq_match = seq_ports.iter().find(|p| matches(p, pattern) && !p.contains("[raw hw:"));
    if let (Some(p), None) = (seq_match, raw_match) {
        out.push(check(Status::Ok, t!("Software MIDI port"), t!("%{port} (not a hardware device)", port = p)));
        return out;
    }

    let usb = usb_products(Path::new("/sys/bus/usb/devices"));
    let usb_match = usb.iter().find(|p| matches(p, pattern));
    out.push(match usb_match {
        Some(name) => check(Status::Ok, t!("Connected over USB"), name.clone()),
        None => check(
            Status::Warn,
            t!("Not found on USB"),
            t!(
                "No USB device is named like \"%{pattern}\". Check the cable and try another port; avoid unpowered hubs. (Devices on other buses, or with a different USB name, can still work.)",
                pattern = pattern
            ),
        ),
    });

    out.push(match raw_match {
        Some(r) => check(
            Status::Ok,
            t!("Sound driver loaded"),
            t!(
                "%{name} is ALSA card %{card}, MIDI device hw:%{card},%{device}",
                name = r.name,
                card = r.card,
                device = r.device
            ),
        ),
        None if usb_match.is_some() => check(
            Status::Fail,
            t!("No sound driver for it"),
            t!(
                "The device is on USB but has no ALSA MIDI device. The snd-usb-audio driver may have failed to load; replug it, and see `journalctl -k` for errors."
            ),
        ),
        None => check(
            Status::Fail,
            t!("No ALSA MIDI device"),
            t!("No MIDI device named like \"%{pattern}\".", pattern = pattern),
        ),
    });

    let seq_available = Path::new("/proc/asound/seq/clients").exists();
    out.push(match (seq_match, seq_available) {
        (Some(p), _) => check(Status::Ok, t!("In the ALSA sequencer"), p.clone()),
        (None, false) => check(
            Status::Warn,
            t!("ALSA sequencer unavailable"),
            t!(
                "The snd-seq module isn't loaded, so other MIDI apps won't see the device. Slidewire uses the raw port instead."
            ),
        ),
        (None, true) if raw_match.is_some() => check(
            Status::Warn,
            t!("Missing from the ALSA sequencer"),
            t!(
                "The snd-seq-midi bridge didn't pick the device up, so most MIDI apps won't see it. Slidewire uses the raw port instead."
            ),
        ),
        (None, true) => check(Status::Fail, t!("Missing from the ALSA sequencer"), t!("No sequencer port matches.")),
    });
    out
}

/// The running app's own connection state.
pub fn connection(connected: Option<&str>, error: Option<&str>) -> Check {
    match (connected, error) {
        (Some(port), _) => check(Status::Ok, t!("Slidewire is connected"), port.to_string()),
        (None, Some(e)) => check(Status::Fail, t!("Slidewire couldn't connect"), busy_hint(e)),
        (None, None) => {
            check(Status::Warn, t!("Not connected yet"), t!("Slidewire looks for the device every 2 seconds."))
        }
    }
}

fn matches(name: &str, pattern: &str) -> bool {
    name.to_lowercase().contains(&pattern.to_lowercase())
}

fn busy_hint(e: &str) -> String {
    if e.contains("busy") || e.contains("EBUSY") || e.contains("os error 16") {
        t!("%{error}. Another app has the device open exclusively; close it and Slidewire reconnects.", error = e)
            .to_string()
    } else if e.contains("ermission") || e.contains("os error 13") {
        t!("%{error}. Your user may need to be in the \"audio\" group.", error = e).to_string()
    } else {
        e.to_string()
    }
}

/// After a kernel update without a reboot, the running kernel's modules are
/// gone, so drivers that weren't loaded yet (USB MIDI among them) can't load.
fn kernel_check(osrelease: &Path, modules: &Path) -> Option<Check> {
    let release = std::fs::read_to_string(osrelease).ok()?;
    let release = release.trim();
    if modules.join(release).is_dir() {
        return None;
    }
    let installed: Vec<String> =
        std::fs::read_dir(modules).ok()?.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    Some(check(
        Status::Fail,
        t!("Restart needed after a kernel update"),
        t!(
            "You're running kernel %{release}, but only %{installed} is installed, so new devices can't load their drivers. Restart your computer.",
            release = release,
            installed = if installed.is_empty() { t!("another version").to_string() } else { installed.join(", ") }
        ),
    ))
}

/// "Manufacturer Product" for every USB device.
fn usb_products(root: &Path) -> Vec<String> {
    let Ok(rd) = std::fs::read_dir(root) else { return Vec::new() };
    let mut out: Vec<String> = rd
        .flatten()
        .filter_map(|d| {
            let product = std::fs::read_to_string(d.path().join("product")).ok()?;
            let maker = std::fs::read_to_string(d.path().join("manufacturer")).unwrap_or_default();
            Some(format!("{} {}", maker.trim(), product.trim()).trim().to_string())
        })
        .collect();
    out.sort();
    out.dedup();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kernel_mismatch() {
        let root = std::env::temp_dir().join(format!("slidewire-kernel-{}", std::process::id()));
        let modules = root.join("modules");
        std::fs::create_dir_all(modules.join("7.2.9-2-cachyos")).unwrap();
        std::fs::write(root.join("osrelease"), "7.2.7-1-cachyos\n").unwrap();
        let c = kernel_check(&root.join("osrelease"), &modules).unwrap();
        assert_eq!(c.status, Status::Fail);
        assert!(c.detail.contains("7.2.9-2-cachyos"));
        std::fs::write(root.join("osrelease"), "7.2.9-2-cachyos\n").unwrap();
        assert!(kernel_check(&root.join("osrelease"), &modules).is_none());
        std::fs::remove_dir_all(&root).unwrap();
    }
}
