# Third-party notices

Extendo is MIT licensed (see LICENSE). It includes or uses the following third-party software.

| Component | Used for | License |
| --- | --- | --- |
| [scrcpy](https://github.com/Genymobile/scrcpy) (patched, "Extendo Mirror") | Mirror engine and scrcpy-server | Apache-2.0 (LICENSE-scrcpy ships with the app) |
| [Android Debug Bridge](https://developer.android.com/tools/adb) (platform-tools, Windows packages) | USB connection | Apache-2.0 |
| [OpenH264](https://github.com/cisco/openh264) (Cisco) | H.264 encoding | BSD-2-Clause |
| [Parsec Virtual Display Driver](https://github.com/nomi-san/parsec-vdd) 0.41 | Virtual monitor on Windows; downloaded by you with "Set up extend", not bundled | Driver signed by Parsec; driver interface code MIT |
| [egui / eframe](https://github.com/emilk/egui) | Desktop user interface | MIT OR Apache-2.0 |
| [rustls](https://github.com/rustls/rustls), rcgen, sha2 | TLS and pairing | Apache-2.0 / ISC / MIT |
| [PipeWire bindings](https://crates.io/crates/pipewire), [zbus](https://github.com/dbus2/zbus), [ksni](https://crates.io/crates/ksni) | Linux screen capture, D-Bus, tray icon | MIT, MIT, Unlicense |
| [windows-rs](https://github.com/microsoft/windows-rs) | Windows APIs | MIT OR Apache-2.0 |
| anyhow, arboard, clap, dirs, if-addrs, log, rayon, rfd, serde, serde_json and their dependencies | Rust libraries | MIT, Apache-2.0, BSD, ISC, Zlib, Unicode-3.0, Unlicense, or a choice of these |
| Jetpack Compose, AndroidX, Kotlin (Android app) | Android app | Apache-2.0 |

No dependency is licensed under the GPL only. The full dependency list with versions and licenses
comes from `cargo metadata` on `host/Cargo.lock`.
