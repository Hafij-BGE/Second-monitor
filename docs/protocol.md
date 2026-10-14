# Protocol v1

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
| version | u8 | `1` |
| width | u16 | device screen, landscape |
| height | u16 | |
| dpi | u16 | |

Host → client (10 bytes):

| Field | Type | Notes |
|-------|------|-------|
| magic | 4 bytes | `SMON` |
| version | u8 | `1` |
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

Client → host:

| Type | Name | Payload |
|------|------|---------|
| 10 | touch | `u8 action` (0 down, 1 move, 2 up, 3 cancel), `u8 pointer_id`, `f32 x`, `f32 y` (0.0–1.0 of the video) |
| 11 | keyframe request | empty |

Unknown message types must be skipped using `length`.
