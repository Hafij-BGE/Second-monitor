package io.github.hafijbge.secondmonitor

/** The live connection, so the share sheet can send files through it. */
object Session {
    @Volatile
    var client: StreamClient? = null
}
