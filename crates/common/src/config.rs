use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ImageFormat {
    Jpg,
    Png,
}

/// Everything Bandicam's "Rectangle on a screen" + Image tab settings cover.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Monitor index as returned by xcap::Monitor::all()
    pub monitor: usize,
    /// Capture rectangle in screen pixels, relative to the monitor's top-left.
    /// Measured against `ref_w` x `ref_h` (your native resolution), so it
    /// stays correct whatever scaling GNOME/KDE/XWayland applies.
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    /// Native resolution of that monitor (yours: 2560x1600)
    pub ref_w: u32,
    pub ref_h: u32,
    pub out_dir: PathBuf,
    /// File name = "<prefix> YYYY-MM-DD HH-MM-SS-mmm.<ext>" (Bandicam style)
    pub prefix: String,
    pub format: ImageFormat,
    pub jpeg_quality: u8,
    pub shutter_sound: bool,
    /// Desktop notification (with thumbnail) after each capture
    pub notify: bool,
    /// Draw the coloured frame around the capture area (like Bandicam)
    pub show_frame: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            monitor: 0,
            // Measured from your EDOPro screenshot (2560x1600 panel):
            // Bandicam's frame shows 753x1487 starting just under its toolbar.
            x: 1,
            y: 38,
            width: 753,
            height: 1487,
            ref_w: 2560,
            ref_h: 1600,
            out_dir: dirs::picture_dir()
                .unwrap_or_else(|| dirs::home_dir().unwrap_or_default())
                .join("Ultimate_Deck"),
            prefix: "bandicam".into(),
            format: ImageFormat::Jpg,
            jpeg_quality: 95,
            shutter_sound: true,
            notify: true,
            show_frame: true,
        }
    }
}

impl Config {
    pub fn path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("bcshot")
            .join("config.toml")
    }

    pub fn load() -> Self {
        std::fs::read_to_string(Self::path())
            .ok()
            .and_then(|s| toml::from_str(&s).ok())
            .unwrap_or_default()
    }

    /// Modification time of the config file, to notice changes made by the
    /// other process.
    pub fn mtime() -> Option<std::time::SystemTime> {
        std::fs::metadata(Self::path())
            .and_then(|m| m.modified())
            .ok()
    }

    pub fn save(&self) -> anyhow::Result<()> {
        let p = Self::path();
        if let Some(dir) = p.parent() {
            std::fs::create_dir_all(dir)?;
        }
        // write + rename so the other process never reads a half-written file
        let tmp = p.with_extension("toml.tmp");
        std::fs::write(&tmp, toml::to_string_pretty(self)?)?;
        std::fs::rename(tmp, p)?;
        Ok(())
    }
}
