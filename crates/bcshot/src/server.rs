//! Shared state, the actions every entry point uses (hotkey socket, tray,
//! settings app), and the Unix-socket server.
//!
//! Wayland doesn't let apps grab global keys, so GNOME/KDE own the hotkeys and
//! run `bcshot capture` / `bcshot edit`, which talk to this server.

use crate::{capture, notify};
use bcshot_common::{Config, socket_path};
use eframe::egui;
use std::{
    io::{BufRead, BufReader, Write},
    os::unix::net::{UnixListener, UnixStream},
    path::PathBuf,
    process::Command,
    sync::{Arc, Mutex},
};

pub struct Shared {
    pub cfg: Config,
    /// true while the frame is unlocked for adjusting
    pub editing: bool,
    /// set by `edit` (socket / tray); consumed by the GUI thread
    pub toggle_edit: bool,
    pub last_capture: Option<PathBuf>,
    pub quit: bool,
}

pub type SharedRef = Arc<Mutex<Shared>>;

/// Take a screenshot with the current settings. Returns "OK <path>" / "ERR ...".
pub fn do_capture(shared: &SharedRef) -> String {
    let (cfg, editing) = {
        let s = shared.lock().unwrap();
        (s.cfg.clone(), s.editing)
    };
    if editing {
        return "ERR the frame is being adjusted, press Enter to lock it first".into();
    }
    // lock is NOT held during the (possibly slow) capture
    let reply = capture::run_capture(&cfg);
    if let Some(p) = reply.strip_prefix("OK ") {
        let p = PathBuf::from(p);
        shared.lock().unwrap().last_capture = Some(p.clone());
        if cfg.notify {
            notify::captured(&p);
        }
    } else if cfg.notify {
        notify::failed(&reply);
    }
    reply
}

/// Open the GPUI settings window, natively on Wayland (crisp, not XWayland).
pub fn open_settings() {
    let mut cmd = Command::new(bcshot_common::sibling_exe("bcshot-settings"));
    if let Some(wd) = crate::wayland_display() {
        cmd.env("WAYLAND_DISPLAY", wd);
    }
    let _ = cmd.spawn();
}

pub fn open_folder(cfg: &Config) {
    let _ = std::fs::create_dir_all(&cfg.out_dir);
    let _ = Command::new("xdg-open").arg(&cfg.out_dir).spawn();
}

pub fn serve(shared: SharedRef, ctx: egui::Context) -> std::io::Result<()> {
    let path = socket_path();
    // A previous crash may have left a stale socket file behind.
    if UnixStream::connect(&path).is_err() {
        let _ = std::fs::remove_file(&path);
    }
    let listener = UnixListener::bind(&path)?;

    std::thread::spawn(move || {
        for conn in listener.incoming().flatten() {
            // one thread per request, so a slow capture never blocks `status`
            let shared = shared.clone();
            let ctx = ctx.clone();
            std::thread::spawn(move || handle(conn, &shared, &ctx));
        }
    });
    Ok(())
}

fn handle(conn: UnixStream, shared: &SharedRef, ctx: &egui::Context) {
    let mut line = String::new();
    if BufReader::new(&conn).read_line(&mut line).is_err() {
        return;
    }
    let reply = match line.trim() {
        "capture" => do_capture(shared),
        "edit" => {
            shared.lock().unwrap().toggle_edit = true;
            "OK".into()
        }
        "reload" => {
            shared.lock().unwrap().cfg = Config::load();
            "OK".into()
        }
        "settings" => {
            open_settings();
            "OK".into()
        }
        "status" => {
            let s = shared.lock().unwrap();
            let last = s
                .last_capture
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "-".into());
            format!("OK {}\t{}", s.editing as u8, last)
        }
        "quit" => {
            shared.lock().unwrap().quit = true;
            "OK".into()
        }
        "ping" => "OK".into(),
        other => format!("ERR unknown command '{other}'"),
    };
    let _ = writeln!(&conn, "{reply}");
    ctx.request_repaint();
}
