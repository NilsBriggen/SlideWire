//! Side-effect actions for buttons: shell commands and key combinations.

use anyhow::{Result, anyhow, bail};

/// Run a shell command detached from us.
pub fn run_command(cmd: &str) -> Result<()> {
    spawn_detached(std::process::Command::new("sh").arg("-c").arg(cmd))
}

/// Start a program without waiting for it. A thread waits instead, so it
/// doesn't linger as a zombie when it exits.
pub fn spawn_detached(cmd: &mut std::process::Command) -> Result<()> {
    let mut child = cmd
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()?;
    std::thread::Builder::new().name("reaper".into()).spawn(move || {
        let _ = child.wait();
    })?;
    Ok(())
}

/// The keys of a combination like `ctrl+shift+m`. The plus key itself is
/// `plus`, or a `+` of its own: `ctrl++`.
fn split_combo(combo: &str) -> Vec<&str> {
    let parts: Vec<&str> = combo.split('+').map(str::trim).collect();
    let mut keys = Vec::new();
    let mut i = 0;
    while i < parts.len() {
        if parts[i].is_empty() && parts.get(i + 1).is_some_and(|p| p.is_empty()) {
            keys.push("+");
            i += 2;
        } else {
            keys.push(parts[i]);
            i += 1;
        }
    }
    keys
}

/// Press a combination like `ctrl+shift+m` through ydotool (works on Wayland;
/// requires the ydotoold daemon).
pub fn press_keys(combo: &str) -> Result<()> {
    let codes: Vec<u16> = split_combo(combo)
        .into_iter()
        .map(|k| keycode(k).ok_or_else(|| anyhow!("unknown key '{k}'")))
        .collect::<Result<_>>()?;
    if codes.is_empty() {
        bail!("empty key combination");
    }
    let mut args: Vec<String> = codes.iter().map(|c| format!("{c}:1")).collect();
    args.extend(codes.iter().rev().map(|c| format!("{c}:0")));
    let status = std::process::Command::new("ydotool")
        .arg("key")
        .args(&args)
        .status()
        .map_err(|e| anyhow!("ydotool not available ({e}); install ydotool and start ydotoold"))?;
    if !status.success() {
        bail!("ydotool failed");
    }
    Ok(())
}

/// Linux input event codes (linux/input-event-codes.h).
fn keycode(name: &str) -> Option<u16> {
    let n = name.to_lowercase();
    let letters = "qwertyuiop";
    let letters2 = "asdfghjkl";
    let letters3 = "zxcvbnm";
    if n.len() == 1 {
        let c = n.chars().next()?;
        if let Some(i) = letters.find(c) {
            return Some(16 + i as u16);
        }
        if let Some(i) = letters2.find(c) {
            return Some(30 + i as u16);
        }
        if let Some(i) = letters3.find(c) {
            return Some(44 + i as u16);
        }
        if c == '0' {
            return Some(11);
        }
        if let Some(d) = c.to_digit(10) {
            return Some(1 + d as u16);
        }
    }
    if let Some(f) = n.strip_prefix('f').and_then(|f| f.parse::<u16>().ok()) {
        return match f {
            1..=10 => Some(58 + f),
            11 => Some(87),
            12 => Some(88),
            13..=24 => Some(183 + f - 13),
            _ => None,
        };
    }
    Some(match n.as_str() {
        "ctrl" | "control" | "lctrl" => 29,
        "rctrl" => 97,
        "shift" | "lshift" => 42,
        "rshift" => 54,
        "alt" | "lalt" => 56,
        "ralt" | "altgr" => 100,
        "super" | "meta" | "win" => 125,
        "esc" | "escape" => 1,
        "tab" => 15,
        "enter" | "return" => 28,
        "space" => 57,
        "backspace" => 14,
        "delete" | "del" => 111,
        "insert" => 110,
        "home" => 102,
        "end" => 107,
        "pageup" => 104,
        "pagedown" => 109,
        "up" => 103,
        "down" => 108,
        "left" => 105,
        "right" => 106,
        "minus" | "-" => 12,
        "equal" | "=" => 13,
        "plus" | "+" | "kpplus" => 78,
        "kpminus" => 74,
        "comma" | "," => 51,
        "dot" | "." => 52,
        "slash" | "/" => 53,
        "mute" => 113,
        "volumedown" => 114,
        "volumeup" => 115,
        "playpause" => 164,
        "nextsong" => 163,
        "previoussong" => 165,
        "print" => 99,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::{keycode, split_combo};

    #[test]
    fn keycodes() {
        assert_eq!(keycode("a"), Some(30));
        assert_eq!(keycode("M"), Some(50));
        assert_eq!(keycode("1"), Some(2));
        assert_eq!(keycode("0"), Some(11));
        assert_eq!(keycode("F12"), Some(88));
        assert_eq!(keycode("ctrl"), Some(29));
        assert_eq!(keycode("nope"), None);
    }

    #[test]
    fn combos_with_plus() {
        assert_eq!(split_combo("ctrl+shift+m"), ["ctrl", "shift", "m"]);
        assert_eq!(split_combo("ctrl++"), ["ctrl", "+"]);
        assert_eq!(split_combo("ctrl + plus"), ["ctrl", "plus"]);
        assert_eq!(split_combo("+"), ["+"]);
        assert_eq!(keycode("+"), keycode("plus"));
    }
}
