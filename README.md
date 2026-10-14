<p align="center"><img src="assets/extendo-256.png" width="128" alt="Extendo icon"></p>

<h1 align="center">Extendo</h1>

<p align="center">Use your Android tablet as a second monitor for a Windows or Linux PC, over a USB cable or WiFi.</p>

<p align="center">Made by <b>Md Hafijur Rahman</b> · <a href="https://github.com/Hafij-BGE">github.com/Hafij-BGE</a></p>

## Features

- **Extend or mirror.** On GNOME (Linux) Extendo creates a real virtual monitor sized for the tablet. On Windows it streams any monitor (use a virtual display driver to extend).
- **USB or WiFi.** USB goes through `adb` for the lowest latency; on WiFi the app finds the PC automatically.
- **Touchscreen or trackpad.** Touch the PC directly, or use the tablet as a trackpad: tap to click, two-finger tap to right-click, two-finger scroll, press and hold to drag. A stylus or mouse always points directly.
- **Keyboard and shortcuts.** On-screen keyboard, hardware keyboards, and a sidebar with Ctrl / Alt / Shift / Super, Esc, Tab, undo, cut, copy and paste.
- **Clipboard sync.** Copy on one device, paste on the other.
- **File transfer.** Send files both ways: drop files on the PC window, use the sidebar, or "Share → Send to PC" from any Android app.
- **Resolution, scale and quality.** Pick the resolution, make everything bigger, and choose Smooth, Low-latency or Sharp presets (or your own frame rate and bitrate).
- **Smooth pointer.** The tablet draws the mouse pointer itself, so it moves without waiting for video.
- **Auto-reconnect** and an optional performance overlay.

## Downloads

| Part | File |
|------|------|
| Linux PC (GNOME) | `extendo_<version>_amd64.deb` |
| Windows PC | `extendo-setup-<version>.exe` (installer) or `extendo-portable.exe` |
| Android tablet | `extendo.apk` |

GitHub Actions builds all of them on every push; tags like `v0.2.0` publish a Release.

## Getting started

1. Install Extendo on the PC and open it.
2. Install `extendo.apk` on the tablet and open it.
3. Connect:
   - **USB:** turn on *Developer options → USB debugging* on the tablet, plug it in, accept the prompt, then tap **Connect over USB**. The PC needs `adb` (`sudo apt install adb`, or Android platform-tools on Windows next to `extendo.exe`).
   - **WiFi:** put both on the same network and tap your PC in the list. The Windows installer opens the firewall; with `ufw` on Linux run `sudo ufw allow 52020/tcp && sudo ufw allow 52021/udp`.

On Windows, install [Virtual Display Driver](https://github.com/VirtualDrivers/Virtual-Display-Driver) to get an extra screen to extend onto.

## PC app options

The PC app opens a window. For a terminal-only setup:

```
extendo --headless [--mode extend|mirror] [--monitor NAME] [--fps 60]
                   [--bitrate 12] [--scale 1.5] [--port 52020] [--no-usb] [--no-discovery]
extendo --list
```

Settings from the window are saved in `~/.config/extendo/settings.json` (Linux) or `%APPDATA%\extendo` (Windows). Received files go to `Downloads/Extendo`.

## Building

PC app on Linux:

```bash
sudo apt install libpipewire-0.3-dev libspa-0.2-dev libclang-dev clang pkg-config nasm
cd host
cargo build --release
cargo install cargo-deb && cargo deb
```

Windows `.exe` from Linux: `sudo apt install gcc-mingw-w64-x86-64 g++-mingw-w64-x86-64`, then `cargo build --release --target x86_64-pc-windows-gnu`. On Windows itself: `cargo build --release`.

Android: JDK 17 and the Android SDK, then `cd android && gradle assembleRelease`.

Without signing secrets, CI signs the APK with a throwaway key, so each build must be uninstalled before installing the next. For in-place updates add the repository secrets `ANDROID_KEYSTORE_BASE64`, `ANDROID_KEYSTORE_PASSWORD`, `ANDROID_KEY_ALIAS` and `ANDROID_KEY_PASSWORD`.

## How it works

The PC captures the screen (GNOME's Mutter screencast API over PipeWire, or DXGI on Windows), encodes H.264 with OpenH264 using several threads, and streams it over TCP. The tablet decodes it with its hardware decoder. See [docs/protocol.md](docs/protocol.md).

## Credits

Extendo is designed and built by **Md Hafijur Rahman**.
