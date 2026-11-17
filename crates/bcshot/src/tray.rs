//! System tray icon (KDE: built in; GNOME: needs the AppIndicator extension).

use crate::server::{self, SharedRef};
use eframe::egui;
use ksni::{MenuItem, blocking::TrayMethods, menu::*};

pub struct BcTray {
    shared: SharedRef,
    ctx: egui::Context,
}

pub type TrayHandle = ksni::blocking::Handle<BcTray>;

pub fn spawn(shared: SharedRef, ctx: egui::Context) -> Option<TrayHandle> {
    BcTray { shared, ctx }.spawn().ok()
}

impl ksni::Tray for BcTray {
    fn id(&self) -> String {
        "bcshot".into()
    }
    fn title(&self) -> String {
        "bcshot".into()
    }
    fn icon_name(&self) -> String {
        "camera-photo".into()
    }
    fn tool_tip(&self) -> ksni::ToolTip {
        let s = self.shared.lock().unwrap();
        ksni::ToolTip {
            title: "bcshot".into(),
            description: format!(
                "{}x{} at {},{}{}",
                s.cfg.width,
                s.cfg.height,
                s.cfg.x,
                s.cfg.y,
                if s.editing { " (adjusting)" } else { "" }
            ),
            ..Default::default()
        }
    }
    /// Left click: open settings.
    fn activate(&mut self, _x: i32, _y: i32) {
        server::open_settings();
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        let (editing, show_frame) = {
            let s = self.shared.lock().unwrap();
            (s.editing, s.cfg.show_frame)
        };
        vec![
            StandardItem {
                label: "Capture now".into(),
                icon_name: "camera-photo".into(),
                enabled: !editing,
                activate: Box::new(|t: &mut Self| {
                    let shared = t.shared.clone();
                    std::thread::spawn(move || server::do_capture(&shared));
                }),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: if editing {
                    "Lock frame"
                } else {
                    "Adjust frame…"
                }
                .into(),
                icon_name: if editing {
                    "object-locked"
                } else {
                    "transform-move"
                }
                .into(),
                activate: Box::new(|t: &mut Self| {
                    t.shared.lock().unwrap().toggle_edit = true;
                    t.ctx.request_repaint();
                }),
                ..Default::default()
            }
            .into(),
            CheckmarkItem {
                label: "Show frame".into(),
                checked: show_frame,
                activate: Box::new(|t: &mut Self| {
                    let mut s = t.shared.lock().unwrap();
                    s.cfg.show_frame = !s.cfg.show_frame;
                    let _ = s.cfg.save();
                    t.ctx.request_repaint();
                }),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "Open screenshots folder".into(),
                icon_name: "folder-pictures".into(),
                activate: Box::new(|t: &mut Self| {
                    let cfg = t.shared.lock().unwrap().cfg.clone();
                    server::open_folder(&cfg);
                }),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Settings…".into(),
                icon_name: "configure".into(),
                activate: Box::new(|_: &mut Self| server::open_settings()),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "Quit".into(),
                icon_name: "application-exit".into(),
                activate: Box::new(|t: &mut Self| {
                    t.shared.lock().unwrap().quit = true;
                    t.ctx.request_repaint();
                }),
                ..Default::default()
            }
            .into(),
        ]
    }
}
