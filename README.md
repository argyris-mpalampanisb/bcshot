# bcshot: Bandicam-style rectangle screenshots for Linux

[![CI](https://github.com/argyris-mpalampanisb/bcshot/actions/workflows/ci.yml/badge.svg)](https://github.com/argyris-mpalampanisb/bcshot/actions/workflows/ci.yml)

> **Alpha, not yet tested on real hardware.** It builds in CI, but hasn't been run on a KDE or GNOME desktop yet. Bug reports are welcome.

**Prebuilt binaries:** open the latest green run under [Actions](https://github.com/argyris-mpalampanisb/bcshot/actions), download `bcshot-linux-x86_64`, extract it and run `./install.sh`.

Two programs that work together:

| | What it is | Built with |
|---|---|---|
| `bcshot` | Small background app: the on-screen frame, tray icon, notifications and hotkey listener. Starts at login. | egui/winit (via XWayland, see below) |
| `bcshot-settings` | The settings window: dark theme, live preview of the last capture | **GPUI Kit** (native Wayland) |

## Install

```bash
# build dependencies
# Fedora
sudo dnf install gcc clang pkgconf-pkg-config libxcb-devel libxkbcommon-devel libxkbcommon-x11-devel \
     wayland-devel vulkan-loader-devel pipewire-devel dbus-devel fontconfig-devel freetype-devel \
     openssl-devel mesa-libEGL-devel
# Debian/Ubuntu
sudo apt install build-essential clang pkg-config libxcb1-dev libxkbcommon-dev libxkbcommon-x11-dev \
     libwayland-dev libvulkan-dev libpipewire-0.3-dev libdbus-1-dev libfontconfig-dev libfreetype-dev \
     libssl-dev libegl1-mesa-dev
# Arch
sudo pacman -S base-devel clang libxcb libxkbcommon libxkbcommon-x11 wayland vulkan-icd-loader \
     pipewire dbus fontconfig freetype2 openssl

./install.sh
```

GPUI renders with Vulkan, so the settings window needs working Vulkan drivers (`vulkaninfo` should list your GPU). GPUI Kit needs the latest stable Rust: `rustup update stable`.

Open **bcshot** from your app menu. On first run the frame is unlocked so you can place it. Press Enter to lock it. After that it stays put and is saved; it only changes when you choose to adjust it again.

## Using it

- **Hotkeys:** bind these in KDE (System Settings → Keyboard → Shortcuts → Add New → Command or Script) or GNOME (Settings → Keyboard → Custom Shortcuts). No plug-in is needed: the desktop catches the key and runs the command.
  - `bcshot capture`: screenshot of the frame area
  - `bcshot edit`: unlock the frame to adjust it, or lock it again
- **Adjusting the frame:** drag inside it to move it, drag an edge or corner to resize it, drag outside it to draw a new one. Arrow keys nudge it (Shift = 10px, Ctrl = resize). Enter or double-click locks it, Esc cancels.
- **Tray icon:** Capture now, Adjust/Lock frame, Show frame, Open folder, Settings…, Quit. Left-click opens Settings. KDE shows it out of the box; GNOME needs the "AppIndicator and KStatusNotifierItem Support" extension.
- **After each capture:** a notification shows a thumbnail with Open / Show in folder. A new one replaces the previous one, so rapid captures don't pile up.
- Files are named like Bandicam: `bandicam 2026-10-28 20-08-18-261.jpg`.

## Why two toolkits

The frame must be transparent, always on top and click-through. GPUI can't do that on GNOME or KDE Wayland yet, so the frame uses winit through XWayland, where all three work. The settings window has no such needs, so it uses GPUI Kit natively on Wayland. That keeps it sharp at 125 %/150 % scaling.

## Notes

- **Screenshot permission:** captures on GNOME and KDE Wayland go through xdg-desktop-portal. Depending on the version, they may ask for permission, or flash the screen, on each capture. If that happens, a ScreenCast/PipeWire backend that asks only once is the fix.
- **Single monitor:** the frame overlay assumes the frame is on the primary monitor.
- **X11 sessions** work the same way, without the XWayland step.
- **Settings file:** `~/.config/bcshot/config.toml`. Both apps watch it, so changes in one show up in the other.

## Extra: `scripts/override_file_dates.sh`

Port of a Windows PowerShell helper. It sets every file in a folder (default `~/Pictures/Ultimate_Deck`) to "created now / modified now". Linux can't set creation time directly, so on BTRFS each file is replaced by an instant `cp --reflink` copy, which takes no extra space. Run `./scripts/override_file_dates.sh -h` for the options.

## License

MIT
