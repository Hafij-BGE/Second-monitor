package io.github.hafijbge.secondmonitor

import android.app.Activity
import android.content.Intent
import android.os.Bundle

/**
 * Opened by the PC over adb when a known tablet is plugged in, so plugging
 * in is enough to connect. The manifest guards it with a permission only
 * adb's shell holds, so other apps can't start it.
 */
class UsbConnectActivity : Activity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        if (!StreamActivity.running) {
            val port = intent.getIntExtra(StreamActivity.EXTRA_PORT, Protocol.DEFAULT_PORT)
            AppSettings(this).apply {
                lastHost = "127.0.0.1"
                lastPort = port
                lastHostName = "USB"
            }
            startActivity(Intent(this, StreamActivity::class.java).apply {
                putExtra(StreamActivity.EXTRA_HOST, "127.0.0.1")
                putExtra(StreamActivity.EXTRA_PORT, port)
                putExtra(StreamActivity.EXTRA_NAME, "USB")
                addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
            })
        }
        finish()
    }
}
