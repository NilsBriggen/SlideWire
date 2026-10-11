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
        "slidewire".into()
    }
    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        vec![crate::icon::tray_icon()]
    }
    fn tool_tip(&self) -> ksni::ToolTip {
        let sh = self.shared.lock().unwrap();
        let profile = sh.active().map(|p| p.name.clone()).unwrap_or_else(|| "no profile".into());
        let device = if sh.connected_in.is_some() { "connected" } else { "device not connected" };
        ksni::ToolTip {
            title: "Slidewire".into(), description: format!("{profile} — {device}"), ..Default::default()
        }
    }
    fn activate(&mut self, _x: i32, _y: i32) {
        let _ = self.window.send(WindowCmd::Show);
    }
    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        use ksni::menu::*;
        let sh = self.shared.lock().unwrap();
        let ids: Vec<String> = sh.store.profiles.keys().cloned().collect();
        let active = sh.active_id();
        let options =
            sh.store.profiles.values().map(|p| RadioItem { label: p.name.clone(), ..Default::default() }).collect();
        let selected = active.and_then(|a| ids.iter().position(|i| *i == a)).unwrap_or(usize::MAX);
        drop(sh);
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
            StandardItem {
                label: "Mute all outputs".into(),
                icon_name: "audio-volume-muted".into(),
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
        ]
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
