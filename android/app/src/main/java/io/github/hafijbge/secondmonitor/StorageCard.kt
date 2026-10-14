package io.github.hafijbge.secondmonitor

import android.Manifest
import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import android.os.Environment
import android.provider.Settings
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.ContentCopy
import androidx.compose.material.icons.filled.Folder
import androidx.compose.material.icons.filled.Refresh
import androidx.compose.material3.Button
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.unit.dp
import androidx.core.content.ContextCompat

/** Bumped by MainActivity.onResume so permission state is re-read. */
val resumeTick = mutableIntStateOf(0)

private fun hasFileAccess(context: Context): Boolean = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
    Environment.isExternalStorageManager()
} else {
    ContextCompat.checkSelfPermission(context, Manifest.permission.WRITE_EXTERNAL_STORAGE) == PackageManager.PERMISSION_GRANTED
}

@Composable
fun StorageCard() {
    val context = LocalContext.current
    val tick = resumeTick.intValue
    val allowed = remember(tick) { hasFileAccess(context) }
    val running by StorageState.running
    val error by StorageState.error
    var password by remember { mutableStateOf(StorageState.password(context)) }
    var addresses by remember(running, tick) { mutableStateOf(StorageState.localAddresses()) }

    val legacyPermission = rememberLauncherForActivityResult(ActivityResultContracts.RequestMultiplePermissions()) {
        resumeTick.intValue++
    }
    val notifications = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) {
        StorageService.start(context)
    }

    fun askForAccess() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            val intent = Intent(Settings.ACTION_MANAGE_APP_ALL_FILES_ACCESS_PERMISSION, Uri.parse("package:${context.packageName}"))
            runCatching { context.startActivity(intent) }
                .onFailure { context.startActivity(Intent(Settings.ACTION_MANAGE_ALL_FILES_ACCESS_PERMISSION)) }
        } else {
            legacyPermission.launch(arrayOf(Manifest.permission.READ_EXTERNAL_STORAGE, Manifest.permission.WRITE_EXTERNAL_STORAGE))
        }
    }

    fun start() {
        // The notification shows the address and the Stop button.
        if (Build.VERSION.SDK_INT >= 33 &&
            ContextCompat.checkSelfPermission(context, Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED
        ) {
            notifications.launch(Manifest.permission.POST_NOTIFICATIONS)
        } else {
            StorageService.start(context)
        }
    }

    SectionCard(
        icon = Icons.Default.Folder,
        title = "Phone storage",
        subtitle = "Open this phone's files from your PC, over WiFi or USB.",
        action = {
            Switch(
                checked = running,
                enabled = allowed,
                onCheckedChange = { on -> if (on) start() else StorageService.stop(context) },
            )
        },
    ) {
        if (!allowed) {
            Text(
                "Extendo needs access to your files to share them.",
                style = MaterialTheme.typography.bodyMedium,
            )
            Spacer(Modifier.height(10.dp))
            Button(onClick = ::askForAccess, modifier = Modifier.fillMaxWidth()) { Text("Allow access to files") }
            return@SectionCard
        }
        error?.let {
            Text(it, color = MaterialTheme.colorScheme.error, style = MaterialTheme.typography.bodySmall)
            Spacer(Modifier.height(8.dp))
        }
        if (!running) {
            Text(
                "Turn on, then on the PC open Extendo → Phone storage. Only use it on networks you trust.",
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            return@SectionCard
        }
        Column(
            Modifier.fillMaxWidth().clip(RoundedCornerShape(16.dp)).background(MaterialTheme.colorScheme.surfaceContainerHigh).padding(14.dp),
            verticalArrangement = Arrangement.spacedBy(6.dp),
        ) {
            val urls = addresses.map { "ftp://$it:${FtpServer.PORT}" }.ifEmpty { listOf("Connect to WiFi, or use USB from the PC app") }
            urls.forEach { CopyRow("Address", it) }
            CopyRow("User", StorageState.USER)
            Row(verticalAlignment = Alignment.CenterVertically) {
                Column(Modifier.weight(1f)) { CopyRow("Password", password) }
                IconButton(onClick = {
                    StorageService.stop(context)
                    password = StorageState.newPassword(context)
                    StorageService.start(context)
                }) { Icon(Icons.Default.Refresh, contentDescription = "New password") }
            }
        }
        Spacer(Modifier.height(10.dp))
        FilledTonalButton(onClick = { addresses = StorageState.localAddresses() }, modifier = Modifier.fillMaxWidth()) {
            Text("Refresh address")
        }
    }
}

@Composable
private fun CopyRow(label: String, value: String) {
    val context = LocalContext.current
    Row(verticalAlignment = Alignment.CenterVertically) {
        Text(label, style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.padding(end = 10.dp))
        Text(value, fontFamily = FontFamily.Monospace, style = MaterialTheme.typography.bodyLarge, modifier = Modifier.weight(1f))
        IconButton(onClick = {
            val cm = context.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
            cm.setPrimaryClip(ClipData.newPlainText(label, value))
        }, modifier = Modifier.size(36.dp)) {
            Icon(Icons.Default.ContentCopy, contentDescription = "Copy", modifier = Modifier.size(18.dp))
        }
    }
}
