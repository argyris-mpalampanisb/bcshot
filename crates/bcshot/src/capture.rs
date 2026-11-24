use anyhow::{Context, Result, anyhow};
use bcshot_common::{Config, ImageFormat};
use image::{DynamicImage, codecs::jpeg::JpegEncoder, imageops};
use std::{fs::File, io::BufWriter, path::PathBuf, process::Command};
use xcap::Monitor;

fn monitor(idx: usize) -> Result<Monitor> {
    let mut all = Monitor::all().map_err(|e| anyhow!("listing monitors: {e}"))?;
    if idx >= all.len() {
        return Err(anyhow!(
            "monitor #{idx} not found ({} available)",
            all.len()
        ));
    }
    Ok(all.swap_remove(idx))
}

/// Bandicam-style file name: "bcshot 2026-10-28 20-08-18-261.jpg"
fn next_path(cfg: &Config) -> PathBuf {
    let ext = match cfg.format {
        ImageFormat::Jpg => "jpg",
        ImageFormat::Png => "png",
    };
    let stamp = chrono::Local::now().format("%Y-%m-%d %H-%M-%S-%3f");
    let base = format!("{} {}", cfg.prefix, stamp);
    let mut p = cfg.out_dir.join(format!("{base}.{ext}"));
    let mut n = 1;
    while p.exists() {
        p = cfg.out_dir.join(format!("{base} ({n}).{ext}"));
        n += 1;
    }
    p
}

/// Grab the whole monitor, cut out the rectangle, write it to disk.
///
/// The rectangle is stored against `ref_w x ref_h`. If the capture backend
/// hands back a different size (e.g. a scaled portal screenshot), the
/// rectangle is scaled to match, so it always covers the same screen area.
pub fn capture_direct(cfg: &Config) -> Result<PathBuf> {
    let full = monitor(cfg.monitor)?
        .capture_image()
        .map_err(|e| anyhow!("capture failed: {e}"))?;

    let (iw, ih) = full.dimensions();
    let sx = iw as f64 / cfg.ref_w.max(1) as f64;
    let sy = ih as f64 / cfg.ref_h.max(1) as f64;
    let x = ((cfg.x as f64 * sx).round() as u32).min(iw.saturating_sub(1));
    let y = ((cfg.y as f64 * sy).round() as u32).min(ih.saturating_sub(1));
    let w = ((cfg.width as f64 * sx).round() as u32).clamp(1, iw - x);
    let h = ((cfg.height as f64 * sy).round() as u32).clamp(1, ih - y);
    let img = imageops::crop_imm(&full, x, y, w, h).to_image();

    std::fs::create_dir_all(&cfg.out_dir)
        .with_context(|| format!("creating {}", cfg.out_dir.display()))?;
    let path = next_path(cfg);

    match cfg.format {
        ImageFormat::Jpg => {
            let rgb = DynamicImage::ImageRgba8(img).to_rgb8(); // JPEG has no alpha
            let out = BufWriter::new(File::create(&path)?);
            JpegEncoder::new_with_quality(out, cfg.jpeg_quality.clamp(1, 100))
                .encode_image(&rgb)?;
        }
        ImageFormat::Png => img.save(&path)?,
    }

    if cfg.shutter_sound {
        play_shutter();
    }
    Ok(path)
}

/// Used by the GUI process. The GUI runs under XWayland (so the frame can be
/// placed and kept on top), but an XWayland process can't see Wayland windows,
/// so the actual grab runs in a short-lived child with the real Wayland
/// environment. Returns "OK <path>" or "ERR <reason>".
pub fn run_capture(cfg: &Config) -> String {
    match crate::wayland_display() {
        None => match capture_direct(cfg) {
            Ok(p) => format!("OK {}", p.display()),
            Err(e) => format!("ERR {e:#}"),
        },
        Some(wd) => {
            if let Err(e) = cfg.save() {
                return format!("ERR saving config: {e:#}");
            }
            let exe = match std::env::current_exe() {
                Ok(p) => p,
                Err(e) => return format!("ERR {e}"),
            };
            match Command::new(exe)
                .arg("capture-direct")
                .env("WAYLAND_DISPLAY", wd)
                .output()
            {
                Ok(out) => {
                    let s = String::from_utf8_lossy(if out.stdout.is_empty() {
                        &out.stderr
                    } else {
                        &out.stdout
                    })
                    .trim()
                    .to_string();
                    if s.is_empty() {
                        "ERR capture process gave no output".into()
                    } else {
                        s
                    }
                }
                Err(e) => format!("ERR starting capture process: {e}"),
            }
        }
    }
}

/// Fire-and-forget shutter sound. Spawned as its own process (not a thread)
/// so it keeps playing after the short-lived capture process exits.
fn play_shutter() {
    let spawned = Command::new("canberra-gtk-play")
        .args(["-i", "camera-shutter"])
        .spawn()
        .is_ok();
    if !spawned {
        let _ = Command::new("paplay")
            .arg("/usr/share/sounds/freedesktop/stereo/camera-shutter.oga")
            .spawn();
    }
}
