package io.github.hafijbge.secondmonitor

import android.app.Activity
import android.content.Intent
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.widget.Toast

/** "Share to Extendo": sends shared files to the connected PC. */
class ShareActivity : Activity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val client = Session.client
        val uris = sharedUris(intent)
        when {
            client == null -> Toast.makeText(this, R.string.share_not_connected, Toast.LENGTH_LONG).show()
            uris.isEmpty() -> Unit
            else -> {
                uris.forEach { uri ->
                    val (name, size) = FileStore.describe(this, uri)
                    // Read through the application context: this activity finishes now.
                    val resolver = applicationContext.contentResolver
                    client.sendFile(name, size) { resolver.openInputStream(uri) ?: throw java.io.IOException("cannot open $name") }
                }
                Toast.makeText(this, resources.getQuantityString(R.plurals.share_sending, uris.size, uris.size), Toast.LENGTH_SHORT).show()
            }
        }
        finish()
    }

    @Suppress("DEPRECATION")
    private fun sharedUris(intent: Intent): List<Uri> = when (intent.action) {
        Intent.ACTION_SEND -> listOfNotNull(
            if (Build.VERSION.SDK_INT >= 33) intent.getParcelableExtra(Intent.EXTRA_STREAM, Uri::class.java)
            else intent.getParcelableExtra(Intent.EXTRA_STREAM),
        )
        Intent.ACTION_SEND_MULTIPLE ->
            (if (Build.VERSION.SDK_INT >= 33) intent.getParcelableArrayListExtra(Intent.EXTRA_STREAM, Uri::class.java)
            else intent.getParcelableArrayListExtra(Intent.EXTRA_STREAM)) ?: emptyList()
        else -> emptyList()
    }
}
