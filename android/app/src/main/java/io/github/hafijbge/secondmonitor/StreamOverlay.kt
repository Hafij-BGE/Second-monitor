package io.github.hafijbge.secondmonitor

import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.slideInHorizontally
import androidx.compose.animation.slideOutHorizontally
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.KeyboardArrowLeft
import androidx.compose.material.icons.automirrored.filled.KeyboardArrowRight
import androidx.compose.material.icons.automirrored.filled.Undo
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.filled.ContentCopy
import androidx.compose.material.icons.filled.ContentCut
import androidx.compose.material.icons.filled.ContentPaste
import androidx.compose.material.icons.filled.Keyboard
import androidx.compose.material.icons.filled.Lock
import androidx.compose.material.icons.filled.Mouse
import androidx.compose.material.icons.filled.Speed
import androidx.compose.material.icons.filled.TouchApp
import androidx.compose.material.icons.filled.UploadFile
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Icon
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlinx.coroutines.delay

private val Glass = Color(0xE6171822)

/** Everything drawn over the video: status, sidebar, stats and transfers. */
@Composable
fun StreamOverlay(state: OverlayState, actions: OverlayActions) {
    var sidebar by remember { mutableStateOf(false) }
    val phase by state.phase

    Box(Modifier.fillMaxSize()) {
        if (state.showStats.value && phase == Phase.STREAMING) {
            Text(
                "${state.fps.floatValue.toInt()} fps · ${"%.1f".format(state.mbps.floatValue)} Mbit/s · ${state.resolution.value}",
                color = Color.White,
                fontFamily = FontFamily.Monospace,
                fontSize = 12.sp,
                modifier = Modifier
                    .align(Alignment.TopStart)
                    .padding(12.dp)
                    .clip(RoundedCornerShape(8.dp))
                    .background(Color(0x99000000))
                    .padding(horizontal = 10.dp, vertical = 5.dp),
            )
        }

        when (phase) {
            Phase.STREAMING -> Unit
            Phase.VERIFY, Phase.APPROVAL -> PairingCard(state, actions, Modifier.align(Alignment.Center))
            else -> StatusCard(state, actions, Modifier.align(Alignment.Center))
        }

        // Edge handle that opens the toolbar.
        AnimatedVisibility(
            visible = !sidebar && phase == Phase.STREAMING,
            enter = fadeIn(),
            exit = fadeOut(),
            modifier = Modifier.align(Alignment.CenterEnd),
        ) {
            Box(
                Modifier
                    .padding(end = 4.dp)
                    .size(width = 22.dp, height = 72.dp)
                    .clip(RoundedCornerShape(11.dp))
                    .background(Color(0x66000000))
                    .clickable { sidebar = true },
                contentAlignment = Alignment.Center,
            ) {
                Icon(Icons.AutoMirrored.Filled.KeyboardArrowLeft, contentDescription = "Open toolbar", tint = Color.White)
            }
        }

        AnimatedVisibility(
            visible = sidebar && phase == Phase.STREAMING,
            enter = slideInHorizontally { it } + fadeIn(),
            exit = slideOutHorizontally { it } + fadeOut(),
            modifier = Modifier.align(Alignment.CenterEnd),
        ) {
            Sidebar(state, actions, onClose = { sidebar = false })
        }

        Column(
            Modifier.align(Alignment.BottomCenter).padding(bottom = 20.dp).widthIn(max = 420.dp),
            verticalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            state.transfers.filter { it.state == TransferState.RUNNING }.forEach { TransferCard(it) }
            Toast(state)
        }
    }
}

@Composable
private fun StatusCard(state: OverlayState, actions: OverlayActions, modifier: Modifier) {
    Surface(modifier.widthIn(max = 380.dp), color = Glass, shape = RoundedCornerShape(24.dp)) {
        Column(Modifier.padding(28.dp), horizontalAlignment = Alignment.CenterHorizontally) {
            CircularProgressIndicator(Modifier.size(40.dp), strokeWidth = 3.dp)
            Spacer(Modifier.height(16.dp))
            val reconnecting = state.phase.value == Phase.RECONNECTING
            Text(
                if (reconnecting) "Reconnecting…" else "Connecting to ${state.hostName.value}…",
                style = MaterialTheme.typography.titleMedium,
                color = Color.White,
            )
            state.error.value?.takeIf { reconnecting }?.let {
                Spacer(Modifier.height(6.dp))
                Text(it, style = MaterialTheme.typography.bodySmall, color = Color(0xFFB0B3C6))
            }
            Spacer(Modifier.height(18.dp))
            OutlinedButton(onClick = actions::disconnect) { Text("Cancel") }
        }
    }
}

/** First connection to a PC over WiFi: compare codes on both screens. */
@Composable
private fun PairingCard(state: OverlayState, actions: OverlayActions, modifier: Modifier) {
    val verify = state.phase.value == Phase.VERIFY
    Surface(modifier.widthIn(max = 420.dp), color = Glass, shape = RoundedCornerShape(24.dp)) {
        Column(Modifier.padding(28.dp), horizontalAlignment = Alignment.CenterHorizontally) {
            Icon(Icons.Default.Lock, contentDescription = null, tint = MaterialTheme.colorScheme.primary, modifier = Modifier.size(36.dp))
            Spacer(Modifier.height(12.dp))
            Text(
                if (verify) "New PC: check the code" else "Allow this tablet on your PC",
                style = MaterialTheme.typography.titleLarge,
                color = Color.White,
            )
            Spacer(Modifier.height(8.dp))
            Text(
                if (verify) {
                    "Make sure the Extendo window on ${state.hostName.value} shows this code (Screen → Security)."
                } else {
                    "Your PC is asking whether to let this tablet connect. Check it shows the same code, then choose Always allow."
                },
                style = MaterialTheme.typography.bodyMedium,
                color = Color(0xFFB0B3C6),
            )
            Spacer(Modifier.height(16.dp))
            Text(
                state.code.value.chunked(3).joinToString(" "),
                fontFamily = FontFamily.Monospace,
                fontSize = 40.sp,
                fontWeight = FontWeight.Bold,
                color = MaterialTheme.colorScheme.primary,
            )
            Spacer(Modifier.height(20.dp))
            if (verify) {
                Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                    OutlinedButton(onClick = actions::disconnect) { Text("Cancel") }
                    androidx.compose.material3.Button(onClick = { state.verifyAnswer?.invoke(true) }) { Text("Codes match") }
                }
            } else {
                CircularProgressIndicator(Modifier.size(28.dp), strokeWidth = 3.dp)
                Spacer(Modifier.height(12.dp))
                OutlinedButton(onClick = actions::disconnect) { Text("Cancel") }
            }
        }
    }
}

@Composable
private fun Sidebar(state: OverlayState, actions: OverlayActions, onClose: () -> Unit) {
    val mods = state.modifiers
    Surface(color = Glass, shape = RoundedCornerShape(topStart = 22.dp, bottomStart = 22.dp)) {
        Column(
            Modifier.width(76.dp).verticalScroll(rememberScrollState()).padding(vertical = 10.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.spacedBy(4.dp),
        ) {
            ToolButton(Icons.AutoMirrored.Filled.KeyboardArrowRight, "Hide", onClick = onClose)
            ToolButton(Icons.Default.Keyboard, "Keyboard", active = state.keyboard.value, onClick = actions::toggleKeyboard)
            val trackpad = state.inputMode.value == InputMode.TRACKPAD
            ToolButton(
                if (trackpad) Icons.Default.Mouse else Icons.Default.TouchApp,
                if (trackpad) "Trackpad" else "Touch",
                onClick = actions::toggleInputMode,
            )
            Divider()
            ModifierKey("Ctrl", Keysyms.CTRL, mods, actions)
            ModifierKey("Alt", Keysyms.ALT, mods, actions)
            ModifierKey("Shift", Keysyms.SHIFT, mods, actions)
            ModifierKey("Super", Keysyms.SUPER, mods, actions)
            TextKey("Esc") { actions.key(Keysyms.ESCAPE) }
            TextKey("Tab") { actions.key(Keysyms.TAB) }
            Divider()
            ToolButton(Icons.AutoMirrored.Filled.Undo, "Undo") { actions.shortcut('z'.code, listOf(Keysyms.CTRL)) }
            ToolButton(Icons.Default.ContentCut, "Cut") { actions.shortcut('x'.code, listOf(Keysyms.CTRL)) }
            ToolButton(Icons.Default.ContentCopy, "Copy") { actions.shortcut('c'.code, listOf(Keysyms.CTRL)) }
            ToolButton(Icons.Default.ContentPaste, "Paste") { actions.shortcut('v'.code, listOf(Keysyms.CTRL)) }
            Divider()
            ToolButton(Icons.Default.UploadFile, "Send file", onClick = actions::sendFiles)
            ToolButton(Icons.Default.Speed, "Stats", active = state.showStats.value, onClick = actions::toggleStats)
            ToolButton(Icons.Default.Close, "Disconnect", tint = Color(0xFFFF8A8A), onClick = actions::disconnect)
        }
    }
}

@Composable
private fun Divider() {
    Box(Modifier.padding(vertical = 4.dp).width(36.dp).height(1.dp).background(Color(0x33FFFFFF)))
}

@Composable
private fun ToolButton(
    icon: ImageVector,
    label: String,
    active: Boolean = false,
    tint: Color = Color.White,
    onClick: () -> Unit,
) {
    Column(
        Modifier
            .clip(RoundedCornerShape(14.dp))
            .background(if (active) MaterialTheme.colorScheme.primary.copy(alpha = 0.35f) else Color.Transparent)
            .clickable(onClick = onClick)
            .padding(horizontal = 6.dp, vertical = 6.dp)
            .width(60.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Icon(icon, contentDescription = label, tint = tint, modifier = Modifier.size(22.dp))
        Text(label, color = Color(0xFFCDD0E0), fontSize = 10.sp, maxLines = 1)
    }
}

@Composable
private fun ModifierKey(label: String, keysym: Int, mods: List<Int>, actions: OverlayActions) {
    val on = keysym in mods
    Box(
        Modifier
            .width(60.dp)
            .height(34.dp)
            .clip(RoundedCornerShape(10.dp))
            .background(if (on) MaterialTheme.colorScheme.primary else Color(0x22FFFFFF))
            .clickable { actions.toggleModifier(keysym) },
        contentAlignment = Alignment.Center,
    ) {
        Text(label, color = if (on) MaterialTheme.colorScheme.onPrimary else Color.White, fontWeight = FontWeight.SemiBold, fontSize = 13.sp)
    }
}

@Composable
private fun TextKey(label: String, onClick: () -> Unit) {
    Box(
        Modifier.width(60.dp).height(34.dp).clip(RoundedCornerShape(10.dp)).background(Color(0x22FFFFFF)).clickable(onClick = onClick),
        contentAlignment = Alignment.Center,
    ) {
        Text(label, color = Color.White, fontWeight = FontWeight.SemiBold, fontSize = 13.sp)
    }
}

@Composable
private fun TransferCard(t: Transfer) {
    Surface(color = Glass, shape = RoundedCornerShape(16.dp), modifier = Modifier.fillMaxWidth()) {
        Column(Modifier.padding(horizontal = 16.dp, vertical = 12.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text(if (t.incoming) "⬇" else "⬆", color = MaterialTheme.colorScheme.primary)
                Spacer(Modifier.width(8.dp))
                Text(t.name, color = Color.White, maxLines = 1, modifier = Modifier.weight(1f))
                Text("${(t.progress * 100).toInt()}%", color = Color(0xFFB0B3C6), fontSize = 12.sp)
            }
            Spacer(Modifier.height(8.dp))
            LinearProgressIndicator(progress = { t.progress }, modifier = Modifier.fillMaxWidth().clip(CircleShape))
        }
    }
}

@Composable
private fun Toast(state: OverlayState) {
    val message = state.message.value
    LaunchedEffect(message) {
        if (message != null) {
            delay(2500)
            state.message.value = null
        }
    }
    AnimatedVisibility(message != null, enter = fadeIn(), exit = fadeOut()) {
        Surface(color = Glass, shape = RoundedCornerShape(16.dp), modifier = Modifier.fillMaxWidth()) {
            Text(message ?: "", color = Color.White, modifier = Modifier.padding(horizontal = 18.dp, vertical = 12.dp))
        }
    }
}
