//! Single-key capture hotkey (e.g. tapping Right Ctrl on its own, like
//! Bandicam on Windows).
//!
//! KDE/GNOME shortcuts can't bind a lone modifier, so the background app reads
//! key events straight from /dev/input (evdev). That needs the user to be in
//! the `input` group. This module holds the parts both apps share: the key
//! list, the tap detector and the access check.

use std::time::{Duration, Instant};

/// Keys offered in Settings: (config name, label, Linux evdev key code).
pub const KEYS: &[(&str, &str, u16)] = &[
    ("RightCtrl", "Right Ctrl", 97),
    ("RightAlt", "Right Alt", 100),
    ("RightShift", "Right Shift", 54),
    ("Menu", "Menu", 127),
    ("Pause", "Pause", 119),
    ("ScrollLock", "Scroll Lock", 70),
    ("Insert", "Insert", 110),
    ("F12", "F12", 88),
];

/// evdev code for a config key name; `None` for "" (off) or an unknown name.
pub fn key_code(name: &str) -> Option<u16> {
    KEYS.iter().find(|k| k.0 == name).map(|k| k.2)
}

/// A press longer than this is a hold (e.g. Ctrl held while thinking), not a tap.
pub const MAX_TAP: Duration = Duration::from_millis(1000);

/// Fires when the target key is pressed and released on its own: no other
/// key or mouse button in between, and released within [`MAX_TAP`].
/// That keeps Right Ctrl usable as Ctrl (Ctrl+C, Ctrl+click, ...).
#[derive(Debug, Default)]
pub struct TapDetector {
    down_at: Option<Instant>,
}

impl TapDetector {
    /// Feed one EV_KEY event (`value`: 1 press, 0 release, 2 auto-repeat).
    /// Returns true when a tap of `target` just completed.
    pub fn feed(&mut self, target: u16, code: u16, value: i32, now: Instant) -> bool {
        if value == 2 {
            return false; // auto-repeat of a held key
        }
        if code != target {
            if value == 1 {
                self.down_at = None; // used as part of a combo
            }
            return false;
        }
        match value {
            1 => {
                self.down_at = Some(now);
                false
            }
            0 => self
                .down_at
                .take()
                .is_some_and(|t| now.duration_since(t) <= MAX_TAP),
            _ => false,
        }
    }
}

/// Can this process read at least one keyboard? False usually means the
/// user isn't in the `input` group yet (or hasn't logged in again since).
pub fn keyboard_access() -> bool {
    let Ok(dir) = std::fs::read_dir("/dev/input/by-path") else {
        return false;
    };
    dir.flatten()
        .filter(|e| e.file_name().to_string_lossy().ends_with("-event-kbd"))
        .any(|e| std::fs::File::open(e.path()).is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    const RCTRL: u16 = 97;
    const KEY_C: u16 = 46;
    const BTN_LEFT: u16 = 0x110;

    fn at(ms: u64, base: Instant) -> Instant {
        base + Duration::from_millis(ms)
    }

    #[test]
    fn lone_tap_fires() {
        let t0 = Instant::now();
        let mut d = TapDetector::default();
        assert!(!d.feed(RCTRL, RCTRL, 1, at(0, t0)));
        assert!(d.feed(RCTRL, RCTRL, 0, at(120, t0)));
    }

    #[test]
    fn combo_does_not_fire() {
        let t0 = Instant::now();
        let mut d = TapDetector::default();
        d.feed(RCTRL, RCTRL, 1, at(0, t0));
        d.feed(RCTRL, KEY_C, 1, at(50, t0));
        d.feed(RCTRL, KEY_C, 0, at(80, t0));
        assert!(!d.feed(RCTRL, RCTRL, 0, at(120, t0)));
    }

    #[test]
    fn ctrl_click_does_not_fire() {
        let t0 = Instant::now();
        let mut d = TapDetector::default();
        d.feed(RCTRL, RCTRL, 1, at(0, t0));
        d.feed(RCTRL, BTN_LEFT, 1, at(40, t0));
        assert!(!d.feed(RCTRL, RCTRL, 0, at(90, t0)));
    }

    #[test]
    fn long_hold_does_not_fire() {
        let t0 = Instant::now();
        let mut d = TapDetector::default();
        d.feed(RCTRL, RCTRL, 1, at(0, t0));
        d.feed(RCTRL, RCTRL, 2, at(600, t0)); // auto-repeat is ignored
        assert!(!d.feed(RCTRL, RCTRL, 0, at(1500, t0)));
    }

    #[test]
    fn autorepeat_alone_keeps_tap() {
        let t0 = Instant::now();
        let mut d = TapDetector::default();
        d.feed(RCTRL, RCTRL, 1, at(0, t0));
        d.feed(RCTRL, RCTRL, 2, at(500, t0));
        assert!(d.feed(RCTRL, RCTRL, 0, at(700, t0)));
    }

    #[test]
    fn release_without_press_does_not_fire() {
        let mut d = TapDetector::default();
        assert!(!d.feed(RCTRL, RCTRL, 0, Instant::now()));
    }

    #[test]
    fn other_key_releases_dont_cancel() {
        // a key that was already held before Right Ctrl went down
        let t0 = Instant::now();
        let mut d = TapDetector::default();
        d.feed(RCTRL, RCTRL, 1, at(0, t0));
        d.feed(RCTRL, KEY_C, 0, at(30, t0));
        assert!(d.feed(RCTRL, RCTRL, 0, at(100, t0)));
    }

    #[test]
    fn second_tap_fires_again() {
        let t0 = Instant::now();
        let mut d = TapDetector::default();
        d.feed(RCTRL, RCTRL, 1, at(0, t0));
        assert!(d.feed(RCTRL, RCTRL, 0, at(100, t0)));
        d.feed(RCTRL, RCTRL, 1, at(400, t0));
        assert!(d.feed(RCTRL, RCTRL, 0, at(500, t0)));
    }

    #[test]
    fn key_names() {
        assert_eq!(key_code("RightCtrl"), Some(97));
        assert_eq!(key_code(""), None);
        assert_eq!(key_code("Nope"), None);
    }
}
