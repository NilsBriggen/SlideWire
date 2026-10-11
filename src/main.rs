//! Slidewire: control application and device volumes with a MIDI
//! controller.

mod actions;
mod audio;
mod config;
mod dbus;
mod engine;
mod icon;
mod mapping;
mod midi;
mod profile;
mod tray;
mod ui;

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use engine::{Engine, Event, FocusInfo, Shared};

/// Set when the user chose Quit while the window was open.
pub static QUIT: AtomicBool = AtomicBool::new(false);

pub enum WindowCmd {
    Show,
    Quit,
}

const HELP: &str = "Slidewire

USAGE: slidewire [OPTIONS]

OPTIONS:
    --minimized          start in the tray without opening the window
    --list-ports         list MIDI ports and exit
    --monitor [PATTERN]  print incoming MIDI messages (from ports matching PATTERN)
    --install-icon       install the icon and a desktop entry for the current user
    -h, --help           show this help
";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut minimized = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-h" | "--help" => {
                print!("{HELP}");
                return;
            }
            "-V" | "--version" => {
                println!("slidewire {}", env!("CARGO_PKG_VERSION"));
                return;
            }
            "--list-ports" => {
                midi::print_ports();
                return;
            }
            "--monitor" => {
                let pattern = args.get(i + 1).map(String::as_str);
                if let Err(e) = midi::monitor(pattern) {
                    eprintln!("{e:#}");
                    std::process::exit(1);
                }
                return;
            }
            "--install-icon" => {
                if let Err(e) = install_desktop_files() {
                    eprintln!("{e:#}");
                    std::process::exit(1);
                }
                return;
            }
            "--minimized" => minimized = true,
            other => {
                eprintln!("unknown option {other}\n\n{HELP}");
                std::process::exit(2);
            }
        }
        i += 1;
    }

    let (tx, rx) = crossbeam_channel::unbounded::<Event>();

    // Single instance: a second launch just raises the running one.
    let bus = match dbus::start(tx.clone()) {
        Ok(Some(conn)) => Some(conn),
        Ok(None) => {
            println!("Slidewire is already running; showing its window.");
            return;
        }
        Err(e) => {
            eprintln!("session bus unavailable ({e}); focus tracking and the OSD are disabled");
            None
        }
    };

    let store = config::Store::load();
    minimized |= store.settings.start_minimized;
    let shared = Arc::new(Mutex::new(Shared {
        store,
        audio: Default::default(),
        focus: FocusInfo::default(),
        ports_in: Vec::new(),
        ports_out: Vec::new(),
        connected_in: None,
        connected_out: None,
        last_msg: None,
        learn: None,
        strips: Vec::new(),
        log: VecDeque::new(),
        ui_visible: false,
        egui: None,
        activity: Default::default(),
        led_state: Default::default(),
        last_midi: None,
        source_peaks: Default::default(),
    }));

    let test_instance = crate::config::env("INSTANCE").is_some();
    if let Some(conn) = bus.as_ref().filter(|_| !test_instance)
        && let Err(e) = dbus::install_focus_script(conn)
    {
        shared.lock().unwrap().log(format!("Focus tracking unavailable: {e:#}"));
    }

    let audio_tx = tx.clone();
    let audio = audio::spawn(move |st| {
        let _ = audio_tx.send(Event::Audio(st));
    });

    let (window_tx, window_rx) = crossbeam_channel::unbounded::<WindowCmd>();
    let (show_tx, show_rx) = crossbeam_channel::unbounded::<()>();
    {
        let window_tx = window_tx.clone();
        std::thread::spawn(move || {
            while show_rx.recv().is_ok() {
                let _ = window_tx.send(WindowCmd::Show);
            }
        });
    }

    let engine = Engine::new(shared.clone(), rx, tx.clone(), audio, show_tx);
    std::thread::Builder::new().name("engine".into()).spawn(move || engine.run()).expect("spawn engine");

    // Test instances stay off the real panel.
    let tray = if test_instance { None } else { tray::spawn(shared.clone(), tx.clone(), window_tx.clone()) };
    if let Some(handle) = &tray {
        // Keep the tray menu / tooltip in sync with profile and device changes.
        let handle = handle.clone();
        std::thread::spawn(move || {
            loop {
                std::thread::sleep(Duration::from_secs(2));
                if handle.update(|_| {}).is_none() {
                    break;
                }
            }
        });
    }

    let mut open = !minimized || tray.is_none();
    loop {
        if open {
            run_window(shared.clone(), tx.clone(), window_rx.clone());
            if QUIT.load(Ordering::SeqCst) || tray.is_none() {
                break;
            }
        }
        match window_rx.recv() {
            Ok(WindowCmd::Show) => open = true,
            Ok(WindowCmd::Quit) | Err(_) => break,
        }
    }

    if let Some(conn) = bus.as_ref().filter(|_| !test_instance) {
        dbus::uninstall_focus_script(conn);
    }
    std::process::exit(0);
}

fn run_window(
    shared: engine::SharedRef,
    tx: crossbeam_channel::Sender<Event>,
    window_rx: crossbeam_channel::Receiver<WindowCmd>,
) {
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("Slidewire")
            .with_app_id("slidewire")
            .with_inner_size([1040.0, 720.0])
            .with_min_inner_size([560.0, 400.0])
            .with_icon(Arc::new(icon::window_icon())),
        ..Default::default()
    };
    let result = eframe::run_native(
        "Slidewire",
        options,
        Box::new(move |cc| Ok(Box::new(ui::App::new(cc, shared, tx, window_rx)))),
    );
    if let Err(e) = result {
        eprintln!("window: {e}");
    }
}

fn install_desktop_files() -> anyhow::Result<()> {
    let base = directories::BaseDirs::new().ok_or_else(|| anyhow::anyhow!("no home directory"))?;
    let data = base.data_dir();
    let icon_dir = data.join("icons/hicolor/scalable/apps");
    std::fs::create_dir_all(&icon_dir)?;
    std::fs::write(icon_dir.join("slidewire.svg"), icon::svg())?;
    let apps = data.join("applications");
    std::fs::create_dir_all(&apps)?;
    let exe = std::env::current_exe()?;
    std::fs::write(
        apps.join("slidewire.desktop"),
        format!(
            "[Desktop Entry]\nType=Application\nName=Slidewire\nComment=Control application and device volumes with a MIDI controller\nExec=\"{}\"\nIcon=slidewire\nCategories=AudioVideo;Audio;Mixer;\nStartupWMClass=slidewire\n",
            exe.display()
        ),
    )?;
    println!("Installed {} and the slidewire icon.", apps.join("slidewire.desktop").display());
    Ok(())
}
