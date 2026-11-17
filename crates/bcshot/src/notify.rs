//! "Screenshot saved" desktop notification with a thumbnail and actions.
//! Each new one replaces the previous one, so rapid captures don't pile up.

use notify_rust::{Notification, Timeout};
use std::{
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicU32, Ordering},
};

static LAST_ID: AtomicU32 = AtomicU32::new(0);

pub fn captured(path: &Path) {
    let path = path.to_path_buf();
    std::thread::spawn(move || {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let mut n = Notification::new();
        n.appname("bcshot")
            .summary("Screenshot saved")
            .body(&name)
            .icon("camera-photo")
            .image_path(&path.to_string_lossy())
            .action("default", "Open")
            .action("open", "Open")
            .action("folder", "Show in folder")
            .timeout(Timeout::Milliseconds(4000));
        let prev = LAST_ID.load(Ordering::Relaxed);
        if prev != 0 {
            n.id(prev); // replace the previous notification
        }
        if let Ok(handle) = n.show() {
            LAST_ID.store(handle.id(), Ordering::Relaxed);
            handle.wait_for_action(|action| match action {
                "default" | "open" => {
                    let _ = Command::new("xdg-open").arg(&path).spawn();
                }
                "folder" => show_in_folder(&path),
                _ => {}
            });
        }
    });
}

pub fn failed(reason: &str) {
    let reason = reason.trim_start_matches("ERR ").to_string();
    std::thread::spawn(move || {
        let _ = Notification::new()
            .appname("bcshot")
            .summary("Screenshot failed")
            .body(&reason)
            .icon("dialog-error")
            .timeout(Timeout::Milliseconds(6000))
            .show();
    });
}

/// Open Dolphin / Nautilus with the file selected (freedesktop FileManager1),
/// falling back to just opening the folder.
pub fn show_in_folder(path: &PathBuf) {
    let uri = format!("file://{}", path.display());
    let ok = Command::new("dbus-send")
        .args([
            "--session",
            "--dest=org.freedesktop.FileManager1",
            "--type=method_call",
            "/org/freedesktop/FileManager1",
            "org.freedesktop.FileManager1.ShowItems",
            &format!("array:string:{uri}"),
            "string:",
        ])
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !ok {
        if let Some(dir) = path.parent() {
            let _ = Command::new("xdg-open").arg(dir).spawn();
        }
    }
}
