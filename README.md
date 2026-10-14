# Second Monitor

Use an Android phone or tablet as a second monitor for a Windows or Linux PC,
over a **USB cable** or **WiFi**. Touches on the device control the PC.

| Part | Package | Folder |
|------|---------|--------|
| Linux host (GNOME) | `.deb` | `host/` (Rust) |
| Windows host | `.exe` installer + portable `.exe` | `host/` (Rust) |
| Android viewer | `.apk` | `android/` (Kotlin) |

GitHub Actions builds all three on every push. Download them from the run's
**Artifacts**, or push a tag like `v0.1.0` to publish a GitHub Release.

## How it works

1. The Android app connects to the host and sends its screen size.
2. The host captures a monitor, encodes it to H.264 and streams it back.
3. The phone decodes it with the hardware decoder and sends touches back.

- **Linux:** the host asks GNOME's Mutter for a real virtual monitor sized to
  the phone (`RecordVirtual`), the same API gnome-remote-desktop uses. It
  appears in Settings → Displays and can be arranged like a physical screen.
  Touches go in as real touchscreen events. Needs GNOME 40+ (Wayland or X11).
- **Windows:** Windows can't create monitors without a driver. Install
  [Virtual Display Driver](https://github.com/VirtualDrivers/Virtual-Display-Driver)
  to get an extra screen; the host streams the first non-primary monitor. With
  only one monitor it mirrors it. Touches move and click the mouse.

## Connect over USB

1. On the phone: enable **Developer options → USB debugging**.
2. Install `adb` on the PC:
   - Linux: `sudo apt install adb`
   - Windows: [Android platform-tools](https://developer.android.com/tools/releases/platform-tools);
     put `adb.exe` next to `second-monitor.exe` or on `PATH`.
3. Plug in the phone, accept the "Allow USB debugging" prompt.
4. Start the host, then tap **Connect over USB** in the app.

The host runs `adb reverse tcp:52020 tcp:52020` for every attached device, so
the app reaches the PC at `127.0.0.1` through the cable.

## Connect over WiFi

1. Put the phone and PC on the same network.
2. Start the host. It answers discovery on UDP 52021 and streams on TCP 52020.
   The Windows installer adds a firewall rule; on Linux with `ufw` run
   `sudo ufw allow 52020/tcp && sudo ufw allow 52021/udp`.
3. Tap **Search for computers** in the app, or type the PC's IP address.

## Host options

```
second-monitor [--mode extend|mirror] [--monitor NAME] [--list]
               [--fps 60] [--bitrate 12] [--scale 1.0] [--port 52020]
               [--no-usb] [--no-discovery]
```

- `--scale 1.5` makes the virtual monitor smaller than the phone's resolution,
  so text looks bigger on high-DPI phones.
- `--mode mirror --monitor eDP-1` (Linux) or `--monitor 0` (Windows) shows an
  existing screen. `--list` prints the names/indexes.
- `RUST_LOG=debug second-monitor` prints more detail.

## Build locally

Host on Linux:

```bash
sudo apt install libpipewire-0.3-dev libspa-0.2-dev libclang-dev clang pkg-config nasm
cd host
cargo build --release
cargo install cargo-deb && cargo deb
```

Host on Windows: install Rust, then `cd host && cargo build --release`.

Android: JDK 17 + Android SDK, then `cd android && gradle assembleRelease`.

### Signing the APK

Without secrets, CI signs the APK with a throwaway debug key, so each build
must be uninstalled before installing the next. For in-place updates, add these
repository secrets: `ANDROID_KEYSTORE_BASE64` (base64 of a `.jks`),
`ANDROID_KEYSTORE_PASSWORD`, `ANDROID_KEY_ALIAS`, `ANDROID_KEY_PASSWORD`.

## Protocol

See [docs/protocol.md](docs/protocol.md).
