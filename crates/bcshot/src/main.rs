//! bcshot: background app. Owns the on-screen frame, the tray icon,
//! notifications and the hotkey socket. Settings live in `bcshot-settings`.
//!
//!   bcshot              start in the background (once; e.g. at login)
//!   bcshot capture      take a screenshot of the frame      (bind to a key)
//!   bcshot edit         unlock / lock the frame to adjust it (bind to a key)
//!   bcshot settings     open the settings window
//!   bcshot quit         stop the background app

mod capture;
mod frame;
mod notify;
mod server;
mod tray;

use bcshot_common::Config;
use eframe::egui;
use server::{Shared, SharedRef};
use std::{
    ffi::OsString,
    sync::{Arc, Mutex, OnceLock},
};

/// The session's real WAYLAND_DISPLAY, if we hid it to run under XWayland.
static WAYLAND_ORIG: OnceLock<Option<OsString>> = OnceLock::new();
pub fn wayland_display() -> Option<OsString> {
    WAYLAND_ORIG.get().cloned().flatten()
}

fn main() -> eframe::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some(cmd @ ("capture" | "edit" | "settings" | "quit" | "status")) => {
            match bcshot_common::send(cmd) {
                Ok(reply) => println!("{reply}"),
                // Not running: `capture` still works as a one-shot,
                // `settings` opens the window (which starts us).
                Err(_) if cmd == "capture" => print_capture(),
                Err(_) if cmd == "settings" => server::open_settings(),
                Err(_) => {
                    eprintln!("bcshot is not running");
                    std::process::exit(1);
                }
            }
            return Ok(());
        }
        // Internal: run by the background app with the real Wayland env.
        Some("capture-direct") => {
            print_capture();
            return Ok(());
        }
        _ => {}
    }

    if bcshot_common::send("ping").is_ok() {
        eprintln!("bcshot is already running.");
        return Ok(());
    }

    // GNOME/KDE Wayland don't let native Wayland windows pick their position
    // or stay above others; XWayland windows can. So the frame runs under
    // XWayland; captures and the settings window still use real Wayland.
    let wd = std::env::var_os("WAYLAND_DISPLAY");
    if wd.is_some() && std::env::var_os("DISPLAY").is_some() {
        std::env::remove_var("WAYLAND_DISPLAY"); // before any thread exists
        let _ = WAYLAND_ORIG.set(wd);
    } else {
        let _ = WAYLAND_ORIG.set(None);
    }

    // The only window is the frame itself: borderless, transparent,
    // always on top, click-through, not in the taskbar.
    let opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("bcshot frame")
            .with_app_id("bcshot")
            .with_decorations(false)
            .with_transparent(true)
            .with_always_on_top()
            .with_taskbar(false)
            .with_resizable(false)
            .with_mouse_passthrough(true)
            .with_position([0.0, 0.0])
            .with_inner_size([1.0, 1.0]),
        ..Default::default()
    };
    eframe::run_native("bcshot", opts, Box::new(|cc| Ok(Box::new(App::new(cc)))))
}

fn print_capture() {
    match capture::capture_direct(&Config::load()) {
        Ok(p) => println!("OK {}", p.display()),
        Err(e) => {
            println!("ERR {e:#}");
            std::process::exit(1);
        }
    }
}

struct App {
    shared: SharedRef,
    frame: frame::FrameEditor,
    tray: Option<tray::TrayHandle>,
    was_editing: bool,
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        style(&cc.egui_ctx);
        let first_run = !Config::path().exists();
        let cfg = Config::load();
        if first_run {
            let _ = cfg.save();
        }
        let shared = Arc::new(Mutex::new(Shared {
            cfg,
            editing: false,
            toggle_edit: first_run, // first run: let the user place the frame
            last_capture: None,
            quit: false,
        }));
        if let Err(e) = server::serve(shared.clone(), cc.egui_ctx.clone()) {
            eprintln!("bcshot: hotkey socket unavailable: {e}");
        }
        let tray = tray::spawn(shared.clone(), cc.egui_ctx.clone());
        // first run: show the settings window too (unless it started us)
        if first_run && std::env::var_os("BCSHOT_FROM_SETTINGS").is_none() {
            server::open_settings();
        }
        Self {
            shared,
            frame: frame::FrameEditor::default(),
            tray,
            was_editing: false,
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let quit = self.shared.lock().unwrap().quit;
        if quit {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        // Alt+F4 on the frame must not kill the background app.
        if ctx.input(|i| i.viewport().close_requested()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
        }

        {
            let mut s = self.shared.lock().unwrap();
            if std::mem::take(&mut s.toggle_edit) {
                let cfg = s.cfg.clone();
                if s.editing {
                    self.frame.lock(&mut s);
                } else {
                    self.frame.unlock(&mut s, &cfg);
                }
            }
        }

        self.frame.show(ctx, &self.shared);

        // keep the tray menu label ("Adjust frame…" / "Lock frame") in sync
        let editing = self.shared.lock().unwrap().editing;
        if editing != self.was_editing {
            self.was_editing = editing;
            if let Some(t) = &self.tray {
                t.update(|_| {});
            }
        }
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0] // transparent window
    }
}

/// Dark, rounded look for the frame toolbar, to match the settings app.
fn style(ctx: &egui::Context) {
    let mut v = egui::Visuals::dark();
    let accent = egui::Color32::from_rgb(59, 130, 246);
    v.selection.bg_fill = accent;
    v.widgets.inactive.corner_radius = egui::CornerRadius::same(6);
    v.widgets.hovered.corner_radius = egui::CornerRadius::same(6);
    v.widgets.active.corner_radius = egui::CornerRadius::same(6);
    v.widgets.inactive.weak_bg_fill = egui::Color32::from_rgb(39, 39, 46);
    v.widgets.hovered.weak_bg_fill = egui::Color32::from_rgb(52, 52, 62);
    v.widgets.active.weak_bg_fill = accent;
    ctx.set_visuals(v);
}
