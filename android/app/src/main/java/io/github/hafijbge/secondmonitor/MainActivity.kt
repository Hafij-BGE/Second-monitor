package io.github.hafijbge.secondmonitor

import android.app.Activity
import android.content.Intent
import android.os.Bundle
import android.view.View
import android.widget.Button
import android.widget.EditText
import android.widget.LinearLayout
import android.widget.TextView
import android.widget.Toast
import kotlin.concurrent.thread

class MainActivity : Activity() {
    private lateinit var wifiScan: Button
    private lateinit var wifiStatus: TextView
    private lateinit var hostList: LinearLayout
    private lateinit var manualHost: EditText

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContentView(R.layout.activity_main)

        wifiScan = findViewById(R.id.wifiScan)
        wifiStatus = findViewById(R.id.wifiStatus)
        hostList = findViewById(R.id.hostList)
        manualHost = findViewById(R.id.manualHost)

        val prefs = getPreferences(MODE_PRIVATE)
        manualHost.setText(prefs.getString(PREF_LAST_HOST, ""))

        findViewById<Button>(R.id.usbConnect).setOnClickListener {
            // The host runs `adb reverse`, so the computer is reachable on localhost.
            connect("127.0.0.1", Protocol.DEFAULT_PORT)
        }
        wifiScan.setOnClickListener { scan() }
        findViewById<Button>(R.id.manualConnect).setOnClickListener {
            val text = manualHost.text.toString().trim()
            if (text.isEmpty()) return@setOnClickListener
            val host = text.substringBefore(':')
            val port = text.substringAfter(':', "").toIntOrNull() ?: Protocol.DEFAULT_PORT
            prefs.edit().putString(PREF_LAST_HOST, text).apply()
            connect(host, port)
        }
    }

    override fun onResume() {
        super.onResume()
        intent.getStringExtra(EXTRA_ERROR)?.let {
            Toast.makeText(this, getString(R.string.disconnected, it), Toast.LENGTH_LONG).show()
            intent.removeExtra(EXTRA_ERROR)
        }
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        setIntent(intent)
    }

    private fun scan() {
        wifiScan.isEnabled = false
        wifiStatus.visibility = View.VISIBLE
        wifiStatus.setText(R.string.wifi_scanning)
        hostList.removeAllViews()
        thread(name = "discovery") {
            val hosts = runCatching { Discovery.scan(this) }.getOrDefault(emptyList())
            runOnUiThread {
                wifiScan.isEnabled = true
                if (hosts.isEmpty()) {
                    wifiStatus.setText(R.string.wifi_none)
                } else {
                    wifiStatus.visibility = View.GONE
                }
                for (h in hosts) {
                    hostList.addView(Button(this).apply {
                        text = "${h.name}  (${h.address})"
                        isAllCaps = false
                        setOnClickListener { connect(h.address, h.port) }
                    })
                }
            }
        }
    }

    private fun connect(host: String, port: Int) {
        startActivity(Intent(this, StreamActivity::class.java).apply {
            putExtra(StreamActivity.EXTRA_HOST, host)
            putExtra(StreamActivity.EXTRA_PORT, port)
        })
    }

    companion object {
        const val EXTRA_ERROR = "error"
        private const val PREF_LAST_HOST = "last_host"
    }
}
