# Extendo protocol v4

The Android app is always the TCP client; the host listens on TCP 52020
(only on localhost when the PC is set to "USB only"). All integers are
big-endian. Floats are IEEE 754 `f32`.

## Encryption and pairing

Everything after the TCP connect runs inside TLS (1.2 or 1.3). The PC uses a
long-lived self-signed certificate stored in its config folder. The app pins
the certificate's SHA-256 fingerprint:

- **Pairing code:** the first 4 bytes of the fingerprint, as a big-endian
  number, modulo 1,000,000, shown as 6 digits on both screens.
- **New PC over WiFi:** before sending anything, the app shows the code and
  the user confirms it matches the PC. Over USB (`127.0.0.1`) the PC is
  trusted automatically.
- **New tablet over WiFi:** after the hello the PC asks its user (Deny /
  Allow once / Always allow). USB is always allowed.

## Discovery (WiFi)

The app broadcasts the ASCII string `SMON_DISCOVER` to UDP port 52021. Each
host replies to the sender with:

```
SMON_HOST <tcp_port> <host_name>
```

## USB

The host runs `adb reverse tcp:52020 tcp:52020`; the app connects to
`127.0.0.1:52020`. Everything else is identical to WiFi.

## Handshake

Client → host:

| Field | Type | Notes |
|-------|------|-------|
| magic | 4 bytes | `SMON` |
| version | u8 | `4` |
| width | u16 | requested size (fits the device's decoder), landscape |
| height | u16 | |
| dpi | u16 | |
| flags | u8 | v2+. bit 0: device draws the pointer; bit 1: clipboard sync |
| fps | u8 | v3. 0 = host default |
| bitrate | u32 | v3. kbit/s, 0 = host default |
| name length | u8 | v3 |
| name | bytes | v3. UTF-8 device name |
| device id | 16 bytes | v4. random, generated once by the app |

Host → client, auth (v4): one byte at a time. `0` accepted (the server hello
follows), `1` waiting for the PC's user (another auth byte follows), `2`
denied (the connection closes).

Host → client (10 bytes):

| Field | Type | Notes |
|-------|------|-------|
| magic | 4 bytes | `SMON` |
| version | u8 | client's version |
| width | u16 | video size |
| height | u16 | |
| codec | u8 | `1` = H.264 |

## Messages

Every message after the handshake is `u8 type | u32 length | payload[length]`.
Unknown types must be skipped using `length`.

Host → client:

| Type | Name | Payload |
|------|------|---------|
| 1 | video | `u64 pts_us`, then one H.264 access unit in Annex B format. Keyframes carry SPS and PPS. |
| 2 | resize | `u16 width`, `u16 height`. The next video message is a keyframe. |
| 3 | cursor | `u8 flags` (bit 0 visible, bit 1 image follows), `f32 x`, `f32 y` (hotspot, 0.0–1.0 of the video), `u16 source_width`. If bit 1: `u16 width`, `u16 height`, `u16 hot_x`, `u16 hot_y`, then `width*height*4` bytes of premultiplied RGBA. |

Client → host:

| Type | Name | Payload |
|------|------|---------|
| 10 | touch | `u8 action` (0 down, 1 move, 2 up, 3 cancel), `u8 pointer_id`, `f32 x`, `f32 y` (0.0–1.0 of the video) |
| 11 | keyframe request | empty |
| 12 | mouse | `u8 op`, then: 0 move to `f32 x, f32 y` (0.0–1.0) · 1 move by `f32 dx, f32 dy` (host pixels) · 2 button `u8 button` (0 left, 1 right, 2 middle), `u8 pressed` · 3 scroll `f32 dx, f32 dy` (host pixels), `u8 finish` |
| 13 | key | `u32 keysym` (X11; text uses `0x01000000 + code point`), `u8 pressed` |

Both directions:

| Type | Name | Payload |
|------|------|---------|
| 20 | clipboard | UTF-8 text |
| 30 | file begin | `u32 id`, `u64 size`, `u16 name_length`, UTF-8 name |
| 31 | file data | `u32 id`, then up to 128 KB of data |
| 32 | file end | `u32 id`, `u8 status` (0 ok, 1 cancelled) |

File ids are chosen by the sender and are unique per direction.
