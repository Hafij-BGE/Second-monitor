package io.github.hafijbge.secondmonitor

/** Wire protocol shared with the host. See docs/protocol.md. All integers big-endian. */
object Protocol {
    val MAGIC = byteArrayOf('S'.code.toByte(), 'M'.code.toByte(), 'O'.code.toByte(), 'N'.code.toByte())
    /** v4: TLS, device id and pairing (v3 added quality, input, clipboard, files). */
    const val VERSION = 4
    const val AUTH_OK = 0
    const val AUTH_PENDING = 1
    const val AUTH_DENIED = 2
    const val FLAG_CURSOR_OVERLAY = 1
    const val FLAG_CLIPBOARD = 2
    const val DEFAULT_PORT = 52020
    const val DISCOVERY_PORT = 52021
    const val DISCOVERY_QUERY = "SMON_DISCOVER"
    const val DISCOVERY_REPLY = "SMON_HOST"
    const val CODEC_H264 = 1

    // Host -> client
    const val MSG_VIDEO = 1
    const val MSG_RESIZE = 2
    const val MSG_CURSOR = 3

    // Client -> host
    const val MSG_TOUCH = 10
    const val MSG_KEYFRAME = 11
    const val MSG_MOUSE = 12
    const val MSG_KEY = 13

    // Both directions
    const val MSG_CLIPBOARD = 20
    const val MSG_FILE_BEGIN = 30
    const val MSG_FILE_DATA = 31
    const val MSG_FILE_END = 32

    const val TOUCH_DOWN = 0
    const val TOUCH_MOVE = 1
    const val TOUCH_UP = 2
    const val TOUCH_CANCEL = 3

    const val MOUSE_MOVE_TO = 0
    const val MOUSE_MOVE_BY = 1
    const val MOUSE_BUTTON = 2
    const val MOUSE_SCROLL = 3

    const val BUTTON_LEFT = 0
    const val BUTTON_RIGHT = 1
    const val BUTTON_MIDDLE = 2

    const val FILE_CHUNK = 128 * 1024
}
