#!/usr/bin/env bash
# Build and install bcshot for the current user (~/.local).
set -euo pipefail
cd "$(dirname "$0")"
cargo build --release
install -Dm755 target/release/bcshot          ~/.local/bin/bcshot
install -Dm755 target/release/bcshot-settings ~/.local/bin/bcshot-settings
install -Dm644 packaging/bcshot-settings.desktop ~/.local/share/applications/bcshot-settings.desktop
# start the background app at login (frame + tray + hotkeys)
install -Dm644 packaging/bcshot.desktop ~/.config/autostart/bcshot.desktop
echo "Installed. Launch \"bcshot\" from your app menu, or run: bcshot-settings"
case ":$PATH:" in *":$HOME/.local/bin:"*) ;; *) echo "Note: add ~/.local/bin to your PATH";; esac
