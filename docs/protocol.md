# Protocol v2

v2 adds a flags byte to the client hello and the cursor message. Hosts
accept v1 clients (no flags byte) and echo the client's version.

The Android app is always the TCP client; the host listens on TCP 52020.
All integers are big-endian. Floats are IEEE 754 `f32`.

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

Client → host (11 bytes):

| Field | Type | Notes |
|-------|------|-------|
| magic | 4 bytes | `SMON` |
| version | u8 | `2` |
| width | u16 | largest size the device can decode, landscape |
| height | u16 | |
| dpi | u16 | |
| flags | u8 | v2 only. bit 0: device draws the pointer (send cursor messages) |

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

Host → client:

| Type | Name | Payload |
|------|------|---------|
| 1 | video | `u64 pts_us`, then one H.264 access unit in Annex B format. Keyframes carry SPS and PPS. |
| 2 | resize | `u16 width`, `u16 height`. The next video message is a keyframe. |
| 3 | cursor | `u8 flags` (bit 0 visible, bit 1 image follows), `f32 x`, `f32 y` (hotspot, 0.0–1.0 of the video), `u16 source_width` (frame width the image is sized for). If bit 1: `u16 width`, `u16 height`, `u16 hot_x`, `u16 hot_y`, then `width*height*4` bytes of premultiplied RGBA. |

Client → host:

| Type | Name | Payload |
|------|------|---------|
| 10 | touch | `u8 action` (0 down, 1 move, 2 up, 3 cancel), `u8 pointer_id`, `f32 x`, `f32 y` (0.0–1.0 of the video) |
| 11 | keyframe request | empty |

Unknown message types must be skipped using `length`.
