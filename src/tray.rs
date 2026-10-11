//! System tray icon (StatusNotifierItem).

use crossbeam_channel::Sender;
use ksni::blocking::TrayMethods;

use crate::WindowCmd;
use crate::engine::{Event, SharedRef, UiCmd};

struct Tray {
    shared: SharedRef,
    events: Sender<Event>,
    window: Sender<WindowCmd>,
}

impl ksni::Tray for Tray {
    fn id(&self) -> String {
        "slidewire".into()
    }
    fn title(&self) -> String {
        "Slidewire".into()
    }
    fn icon_name(&self) -> String {
        crate::APP_ID.into()
    }
    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        vec![crate::icon::tray_icon()]
    }
    /// A muted-speaker badge while the default output is muted.
    fn overlay_icon_name(&self) -> String {
        let sh = self.shared.lock().unwrap();
        let muted = sh.audio.sinks.iter().any(|d| d.name == sh.audio.default_sink && d.mute);
        if muted { "audio-volume-muted".into() } else { String::new() }
    }
    fn tool_tip(&self) -> ksni::ToolTip {
        let sh = self.shared.lock().unwrap();
        let profile = sh.active().map(|p| p.name.clone()).unwrap_or_else(|| "no profile".into());
        let device = if sh.connected_in.is_some() { "connected" } else { "device not connected" };
        let mut lines = vec![format!("{profile} — {device}")];
        if let Some(p) = sh.active().filter(|p| p.bank_count() > 1) {
            lines.push(format!("Bank {} of {}", p.bank + 1, p.bank_count()));
        }
        if let Some(d) = sh.audio.sinks.iter().find(|d| d.name == sh.audio.default_sink) {
            let vol = if d.mute { "muted".to_string() } else { format!("{:.0}%", d.volume * 100.0) };
            lines.push(format!("{}: {vol} (scroll to change)", d.label));
        }
        ksni::ToolTip { title: "Slidewire".into(), description: lines.join("\n"), ..Default::default() }
    }
    fn activate(&mut self, _x: i32, _y: i32) {
        let _ = self.window.send(WindowCmd::Show);
    }
    /// The scroll wheel over the icon changes the default output's volume.
    fn scroll(&mut self, delta: i32, orientation: ksni::Orientation) {
        if orientation == ksni::Orientation::Vertical && delta != 0 {
            // Wheel notches arrive as multiples of 120; 2% per notch.
            let notches = (delta as f32 / 120.0).clamp(-5.0, 5.0);
            let notches = if notches.abs() < 1.0 { notches.signum() } else { notches };
            // Plasma passes Qt's angle delta: positive is wheel up, so louder.
            let _ = self.events.send(Event::Ui(UiCmd::NudgeOutput(notches * 0.02)));
        }
    }
    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        use ksni::menu::*;
        let sh = self.shared.lock().unwrap();
        let ids: Vec<String> = sh.store.profiles.keys().cloned().collect();
        let active = sh.active_id();
        let options =
            sh.store.profiles.values().map(|p| RadioItem { label: p.name.clone(), ..Default::default() }).collect();
        let selected = active.and_then(|a| ids.iter().position(|i| *i == a)).unwrap_or(usize::MAX);
        let (bank, banks, scenes) = sh
            .active()
            .map(|p| (p.bank, p.bank_count(), p.scenes.keys().cloned().collect::<Vec<_>>()))
            .unwrap_or((0, 1, Vec::new()));
        let mic_muted = sh.audio.sources.iter().any(|d| d.name == sh.audio.default_source && d.mute);
        let all_muted = !sh.audio.sinks.is_empty() && sh.audio.sinks.iter().all(|d| d.mute);
        drop(sh);
        let scene_items: Vec<ksni::MenuItem<Self>> = scenes
            .into_iter()
            .map(|name| {
                StandardItem {
                    label: name.clone(),
                    activate: Box::new(move |t: &mut Self| {
                        let _ = t.events.send(Event::Ui(UiCmd::RecallScene(name.clone())));
                    }),
                    ..Default::default()
                }
                .into()
            })
            .collect();
        let mut extra: Vec<ksni::MenuItem<Self>> = Vec::new();
        if !scene_items.is_empty() {
            extra.push(
                SubMenu {
                    label: "Scenes".into(),
                    icon_name: "bookmarks".into(),
                    submenu: scene_items,
                    ..Default::default()
                }
                .into(),
            );
        }
        if banks > 1 {
            let options =
                (0..banks).map(|b| RadioItem { label: format!("Bank {}", b + 1), ..Default::default() }).collect();
            extra.push(
                SubMenu {
                    label: "Bank".into(),
                    submenu: vec![
                        RadioGroup {
                            selected: bank,
                            select: Box::new(|t: &mut Self, i| {
                                let _ = t.events.send(Event::Ui(UiCmd::SetBank(i)));
                            }),
                            options,
                        }
                        .into(),
                    ],
                    ..Default::default()
                }
                .into(),
            );
        }
        vec![
            StandardItem {
                label: "Open Slidewire".into(),
                icon_name: "configure".into(),
                activate: Box::new(|t: &mut Self| {
                    let _ = t.window.send(WindowCmd::Show);
                }),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            SubMenu {
                label: "Profile".into(),
                submenu: vec![
                    RadioGroup {
                        selected,
                        select: Box::new(move |t: &mut Self, i| {
                            if let Some(id) = ids.get(i) {
                                let _ = t.events.send(Event::Ui(UiCmd::ActivateProfile(id.clone())));
                            }
                        }),
                        options,
                    }
                    .into(),
                ],
                ..Default::default()
            }
            .into(),
        ]
        .into_iter()
        .chain(extra)
        .chain([
            CheckmarkItem {
                label: "Mute microphone".into(),
                checked: mic_muted,
                activate: Box::new(|t: &mut Self| {
                    let _ = t.events.send(Event::Ui(UiCmd::ToggleMicMute));
                }),
                ..Default::default()
            }
            .into(),
            CheckmarkItem {
                label: "Mute all outputs".into(),
                checked: all_muted,
                activate: Box::new(|t: &mut Self| {
                    let _ = t.events.send(Event::Ui(UiCmd::MuteAll));
                }),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "Quit".into(),
                icon_name: "application-exit".into(),
                activate: Box::new(|t: &mut Self| {
                    let _ = t.window.send(WindowCmd::Quit);
                }),
                ..Default::default()
            }
            .into(),
        ])
        .collect()
    }
}

pub fn spawn(
    shared: SharedRef,
    events: Sender<Event>,
    window: Sender<WindowCmd>,
) -> Option<ksni::blocking::Handle<impl ksni::Tray>> {
    match (Tray { shared, events, window }).spawn() {
        Ok(h) => Some(h),
        Err(e) => {
            eprintln!("tray icon unavailable: {e}");
            None
        }
    }
}
