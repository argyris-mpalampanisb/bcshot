//! bcshot-settings: the settings window, built with GPUI Kit (dark theme).
//! It edits ~/.config/bcshot/config.toml and talks to the background app
//! (`bcshot`) over its socket; starting it if it isn't running.

mod view;

use gpui_kit::component::theme::{Theme, ThemeMode};
use gpui_kit::{prelude::*, *};
use std::time::Duration;

fn main() {
    // Make sure the background app (frame, tray, hotkeys) is running.
    if bcshot_common::send("ping").is_err() {
        let _ = std::process::Command::new(bcshot_common::sibling_exe("bcshot"))
            .env("BCSHOT_FROM_SETTINGS", "1")
            .spawn();
        std::thread::sleep(Duration::from_millis(400));
    }

    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .run(|cx: &mut App| {
            gpui_kit::init(cx);

            let bounds = Bounds::centered(None, size(px(980.), px(720.)), cx);
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    titlebar: Some(TitlebarOptions {
                        title: Some("bcshot".into()),
                        ..Default::default()
                    }),
                    app_id: Some("bcshot-settings".into()),
                    window_min_size: Some(size(px(860.), px(600.))),
                    ..Default::default()
                },
                cx,
                |window, cx| {
                    Theme::change(ThemeMode::Dark, Some(&mut *window), cx);
                    cx.new(|cx| view::SettingsView::new(window, cx))
                },
            )
            .expect("failed to open the settings window");

            cx.on_window_closed(|cx| {
                if cx.windows().is_empty() {
                    cx.quit();
                }
            })
            .detach();
            cx.activate(true);
        });
}
