package io.github.hafijbge.secondmonitor

/** Wire protocol shared with the host. See docs/protocol.md. All integers big-endian. */
object Protocol {
    val MAGIC = byteArrayOf('S'.code.toByte(), 'M'.code.toByte(), 'O'.code.toByte(), 'N'.code.toByte())
    const val VERSION = 1
    const val DEFAULT_PORT = 52020
    const val DISCOVERY_PORT = 52021
    const val DISCOVERY_QUERY = "SMON_DISCOVER"
    const val DISCOVERY_REPLY = "SMON_HOST"
    const val CODEC_H264 = 1

    // Host -> client
    const val MSG_VIDEO = 1
    const val MSG_RESIZE = 2

    // Client -> host
    const val MSG_TOUCH = 10
    const val MSG_KEYFRAME = 11

    const val TOUCH_DOWN = 0
    const val TOUCH_MOVE = 1
    const val TOUCH_UP = 2
    const val TOUCH_CANCEL = 3
}
