//! Single-key capture hotkey, read straight from /dev/input (evdev).
//!
//! Wayland doesn't let apps grab global keys, and KDE/GNOME shortcuts can't
//! bind a lone modifier such as Right Ctrl. Reading evdev works on any
//! desktop. It only *reads* events: the key still reaches other apps.
//! Needs the user in the `input` group; without it this quietly does nothing
//! and Settings shows how to fix it.

use crate::server::{self, SharedRef};
use bcshot_common::hotkey::{TapDetector, key_code};
use evdev::{Device, EventType};
use std::{
    collections::HashSet,
    path::PathBuf,
    sync::{Arc, Mutex, mpsc},
    time::{Duration, Instant},
};

/// How often to look for newly plugged-in keyboards and mice.
const RESCAN: Duration = Duration::from_secs(3);

pub fn spawn(shared: SharedRef) {
    let (tx, rx) = mpsc::channel::<(u16, i32)>();
    let open: Arc<Mutex<HashSet<PathBuf>>> = Default::default();

    // Device scanner: one reader thread per keyboard/mouse.
    std::thread::spawn(move || {
        let mut warned = false;
        loop {
            let mut denied = 0;
            if let Ok(dir) = std::fs::read_dir("/dev/input") {
                for path in dir.flatten().map(|e| e.path()) {
                    let is_event = path
                        .file_name()
                        .is_some_and(|n| n.to_string_lossy().starts_with("event"));
                    if !is_event || open.lock().unwrap().contains(&path) {
                        continue;
                    }
                    match Device::open(&path) {
                        // keyboards and mice both report EV_KEY (mouse buttons
                        // count as "another key" so Ctrl+click doesn't capture)
                        Ok(dev) if dev.supported_events().contains(EventType::KEY) => {
                            open.lock().unwrap().insert(path.clone());
                            let (tx, open) = (tx.clone(), open.clone());
                            std::thread::spawn(move || {
                                read_device(dev, &tx);
                                open.lock().unwrap().remove(&path); // unplugged
                            });
                        }
                        Ok(_) => {}
                        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => denied += 1,
                        Err(_) => {}
                    }
                }
            }
            if denied > 0 && open.lock().unwrap().is_empty() && !warned {
                eprintln!(
                    "bcshot: can't read the keyboard, so the capture key is off. \
                     Fix: sudo usermod -aG input $USER, then log out and back in."
                );
                warned = true;
            }
            std::thread::sleep(RESCAN);
        }
    });

    // Detector: one place that sees events from every device, in order.
    std::thread::spawn(move || {
        let mut tap = TapDetector::default();
        for (code, value) in rx {
            let target = key_code(&shared.lock().unwrap().cfg.capture_key);
            let Some(target) = target else {
                tap = TapDetector::default();
                continue;
            };
            if tap.feed(target, code, value, Instant::now()) {
                let shared = shared.clone();
                std::thread::spawn(move || {
                    let _ = server::do_capture(&shared);
                });
            }
        }
    });
}

fn read_device(mut dev: Device, tx: &mpsc::Sender<(u16, i32)>) {
    loop {
        let Ok(events) = dev.fetch_events() else {
            return; // device gone
        };
        for ev in events {
            if ev.event_type() == EventType::KEY && tx.send((ev.code(), ev.value())).is_err() {
                return;
            }
        }
    }
}
