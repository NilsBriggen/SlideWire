//! A pretend Korg nanoKONTROL2 on virtual ALSA ports (client "FakeNano",
//! so it never shadows a real device), for testing without hardware. Reads commands from stdin:
//!   f <strip 1-8> <0-127>   move a fader
//!   k <knob 1-8> <0-127>    turn a knob
//!   b <cc>                  press and release a button (e.g. 48 = M1)
//! and prints every message the app sends back (LED feedback).

use std::io::BufRead;

use midir::os::unix::{VirtualInput, VirtualOutput};
use midir::{MidiInput, MidiOutput};

fn main() {
    let mut out =
        MidiOutput::new("FakeNano").unwrap().create_virtual("nanoKONTROL2 MIDI 1").expect("create virtual output");
    let _in = MidiInput::new("FakeNano")
        .unwrap()
        .create_virtual(
            "nanoKONTROL2 LEDs",
            |_, msg, _| {
                let hex: Vec<String> = msg.iter().map(|b| format!("{b:02X}")).collect();
                println!("LED <- {}", hex.join(" "));
            },
            (),
        )
        .expect("create virtual input");
    println!("fake nanoKONTROL2 ready");
    for line in std::io::stdin().lock().lines() {
        let line = line.unwrap();
        let parts: Vec<&str> = line.split_whitespace().collect();
        match parts.as_slice() {
            ["f", n, v] => {
                let _ = out.send(&[0xB0, n.parse::<u8>().unwrap() - 1, v.parse().unwrap()]);
            }
            ["k", n, v] => {
                let _ = out.send(&[0xB0, 15 + n.parse::<u8>().unwrap(), v.parse().unwrap()]);
            }
            ["b", cc] => {
                let cc: u8 = cc.parse().unwrap();
                let _ = out.send(&[0xB0, cc, 127]);
                std::thread::sleep(std::time::Duration::from_millis(30));
                let _ = out.send(&[0xB0, cc, 0]);
            }
            ["sleep", ms] => std::thread::sleep(std::time::Duration::from_millis(ms.parse().unwrap())),
            _ => eprintln!("?"),
        }
    }
}
