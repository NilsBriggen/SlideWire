//! Slidewire: control application and device volumes with a MIDI
//! controller.

mod actions;
mod appearance;
mod audio;
mod config;
mod dbus;
mod doctor;
mod engine;
mod focus;
mod icon;
mod mapping;
mod midi;
mod obs;
mod portal;
mod profile;
mod routing;
mod tray;
mod ui;

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use engine::{Engine, Event, FocusInfo, Shared};

/// The app's id: D-Bus name, desktop file, icon and window class.
pub const APP_ID: &str = "dev.briggen.Slidewire";

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
    --doctor [PATTERN]   check why a controller doesn't show up (default: nanoKONTROL2)
    --install-icon       install the icon and a desktop entry for the current user
    --uninstall          remove what --install-icon and autostart added
    -V, --version        print the version
    -h, --help           show this help

CONTROL THE RUNNING APP (for scripts and keyboard shortcuts):
    slidewire strips                     list strips with volume and assignment
    slidewire set STRIP 50|+5|-5         set or change a strip's volume (percent)
    slidewire mute STRIP [on|off|toggle]
    slidewire bank N                     switch to bank N
    slidewire scene NAME                 recall a scene
  STRIP is a strip name like \"Knob 3\", or a number for that fader.
";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|a| ["strips", "set", "mute", "bank", "scene"].contains(&a.as_str())) {
        if let Err(e) = dbus::remote(&args) {
            eprintln!("{e:#}");
            std::process::exit(1);
        }
        return;
    }
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
            "--doctor" => {
                let pattern = args.get(i + 1).map(String::as_str).unwrap_or("nanoKONTROL2");
                let ports = midi::PortLister::new().inputs();
                for c in doctor::diagnose(pattern, &ports) {
                    let mark = match c.status {
                        doctor::Status::Ok => "ok  ",
                        doctor::Status::Warn => "warn",
                        doctor::Status::Fail => "FAIL",
                    };
                    println!("[{mark}] {}\n       {}", c.title, c.detail);
                }
                return;
            }
            "--install-icon" => {
                match config::install_desktop_files() {
                    Ok(entry) => println!("Installed {} and the icon.", entry.display()),
                    Err(e) => {
                        eprintln!("{e:#}");
                        std::process::exit(1);
                    }
                }
                return;
            }
            "--uninstall" => {
                match config::uninstall() {
                    Ok(removed) => {
                        for r in &removed {
                            println!("removed {r}");
                        }
                        if removed.is_empty() {
                            println!("Nothing to remove.");
                        }
                        println!(
                            "Your settings are in {}; delete that folder to remove them too.",
                            config::Store::config_dir().display()
                        );
                    }
                    Err(e) => {
                        eprintln!("{e:#}");
                        std::process::exit(1);
                    }
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

    install_panic_hook();
    // Handle SIGTERM/SIGINT/SIGHUP on one thread (blocked everywhere else,
    // which threads spawned from here inherit).
    let signals = block_quit_signals();

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

    if crate::config::env("INSTANCE").is_none() {
        std::thread::spawn(config::migrate_desktop_files);
    }
    let mut store = config::Store::load();
    minimized |= store.settings.start_minimized;
    let warnings = std::mem::take(&mut store.warnings);
    let shared = Arc::new(Mutex::new(Shared {
        store,
        audio: Default::default(),
        focus: FocusInfo::default(),
        ports_in: Vec::new(),
        ports_out: Vec::new(),
        connected_in: None,
        connected_out: None,
        midi_error: None,
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
        notices: Vec::new(),
    }));
    for w in warnings {
        shared.lock().unwrap().warn(w);
    }
    let _ = dbus::SHARED.set(shared.clone());

    let test_instance = crate::config::env("INSTANCE").is_some();
    // Focus tracking: Hyprland or sway directly, otherwise a KWin script.
    let compositor = if test_instance { None } else { focus::spawn(tx.clone()) };
    if let Some(name) = compositor {
        shared.lock().unwrap().log(format!("Following the focused window on {name}"));
    } else if let Some(conn) = bus.as_ref().filter(|_| !test_instance)
        && let Err(e) = dbus::install_focus_script(conn)
    {
        shared.lock().unwrap().warn(format!("Focus tracking unavailable: {e:#}"));
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
    let engine = std::thread::Builder::new().name("engine".into()).spawn(move || engine.run()).expect("spawn engine");

    {
        // Quit cleanly on logout, `systemctl stop` or Ctrl+C.
        let (window_tx, shared) = (window_tx.clone(), shared.clone());
        std::thread::Builder::new()
            .name("signals".into())
            .spawn(move || {
                let mut sig = 0;
                // SAFETY: waits on the set blocked above; writes only `sig`.
                if unsafe { libc::sigwait(&signals, &mut sig) } == 0 {
                    QUIT.store(true, Ordering::SeqCst);
                    let _ = window_tx.send(WindowCmd::Quit);
                    shared.lock().unwrap().repaint();
                }
            })
            .expect("spawn signal thread");
    }

    // Test instances stay off the real panel.
    let tray = if test_instance { None } else { tray::spawn(shared.clone(), tx.clone(), window_tx.clone()) };
    if let Some(handle) = &tray {
        // Keep the tray menu / tooltip in sync with profile and device changes.
        let handle = handle.clone();
        std::thread::spawn(move || {
            loop {
                std::thread::sleep(Duration::from_secs(1));
                if handle.update(|_| {}).is_none() {
                    break;
                }
            }
        });
    }

    // Without a tray to come back from, closing the window has to quit.
    let tray_shown = tray.is_some() && dbus::has_tray_host();
    let mut open = !minimized || !tray_shown;
    loop {
        if open {
            run_window(shared.clone(), tx.clone(), window_rx.clone());
            let close_quits = shared.lock().unwrap().store.settings.close_quits;
            if QUIT.load(Ordering::SeqCst) || !tray_shown || close_quits {
                break;
            }
            // The first time, say where the app went.
            let mut sh = shared.lock().unwrap();
            if !sh.store.settings.tray_hint_shown {
                sh.store.settings.tray_hint_shown = true;
                sh.store.touch_settings();
                drop(sh);
                dbus::notify_text(
                    "Slidewire is still running",
                    "It keeps your controller working from the tray. Quit from the tray menu, or make the close button quit in Settings.",
                );
            }
        }
        match window_rx.recv() {
            Ok(WindowCmd::Show) => open = true,
            Ok(WindowCmd::Quit) | Err(_) => break,
        }
    }

    // Let the engine put things back, then go.
    let (done_tx, done_rx) = crossbeam_channel::bounded(1);
    let _ = tx.send(Event::Ui(engine::UiCmd::Shutdown(done_tx)));
    let _ = done_rx.recv_timeout(Duration::from_secs(2));
    drop(engine);
    if let Some(conn) = bus.as_ref().filter(|_| !test_instance) {
        dbus::uninstall_focus_script(conn);
    }
    std::process::exit(0);
}

fn block_quit_signals() -> libc::sigset_t {
    // SAFETY: plain libc calls on a set we own.
    unsafe {
        let mut set: libc::sigset_t = std::mem::zeroed();
        libc::sigemptyset(&mut set);
        for sig in [libc::SIGTERM, libc::SIGINT, libc::SIGHUP] {
            libc::sigaddset(&mut set, sig);
        }
        libc::pthread_sigmask(libc::SIG_BLOCK, &set, std::ptr::null_mut());
        set
    }
}

/// A panic on any thread ends the process: half the app dead (say, the
/// engine) with the window still up is worse. The report is also written to
/// `~/.local/state/slidewire/crash.log`, and systemd restarts the service.
fn install_panic_hook() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        default(info);
        if let Some(dir) = directories::BaseDirs::new().and_then(|b| b.state_dir().map(|d| d.join("slidewire"))) {
            use std::io::Write;
            let _ = std::fs::create_dir_all(&dir);
            if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(dir.join("crash.log")) {
                let secs = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or_default();
                let _ = writeln!(
                    f,
                    "--- slidewire {} at unix time {secs}\n{info}\n{}",
                    env!("CARGO_PKG_VERSION"),
                    std::backtrace::Backtrace::force_capture()
                );
            }
        }
        // SAFETY: ends the process at once, without running exit handlers
        // that could wait on the thread that just died.
        unsafe { libc::_exit(101) };
    }));
}

fn run_window(
    shared: engine::SharedRef,
    tx: crossbeam_channel::Sender<Event>,
    window_rx: crossbeam_channel::Receiver<WindowCmd>,
) {
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("Slidewire")
            .with_app_id(APP_ID)
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
