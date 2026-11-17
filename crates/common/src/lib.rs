//! Shared between the background app (`bcshot`) and the settings app.

mod config;
pub use config::{Config, ImageFormat};

use std::{
    io::{BufRead, BufReader, Write},
    os::unix::net::UnixStream,
    path::PathBuf,
    time::Duration,
};

/// Socket the background app listens on.
pub fn socket_path() -> PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("bcshot.sock")
}

/// Send one command to the running background app and return its reply line.
/// Commands: capture, edit, reload, settings, status, quit, ping.
pub fn send(cmd: &str) -> std::io::Result<String> {
    let mut s = UnixStream::connect(socket_path())?;
    // a capture can wait on a screenshot permission dialog
    s.set_read_timeout(Some(Duration::from_secs(60)))?;
    writeln!(s, "{cmd}")?;
    let mut reply = String::new();
    BufReader::new(s).read_line(&mut reply)?;
    Ok(reply.trim_end().to_string())
}

/// Parsed reply of the `status` command.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Status {
    pub editing: bool,
    pub last_capture: Option<PathBuf>,
}

pub fn status() -> std::io::Result<Status> {
    let r = send("status")?;
    let body = r.strip_prefix("OK ").unwrap_or("");
    let mut parts = body.splitn(2, '\t');
    let editing = parts.next() == Some("1");
    let last_capture = parts
        .next()
        .filter(|p| !p.is_empty() && *p != "-")
        .map(PathBuf::from);
    Ok(Status {
        editing,
        last_capture,
    })
}

/// Path of a sibling binary (bcshot <-> bcshot-settings live side by side).
pub fn sibling_exe(name: &str) -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join(name)))
        .filter(|p| p.exists())
        .unwrap_or_else(|| PathBuf::from(name))
}
