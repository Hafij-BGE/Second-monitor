package io.github.hafijbge.secondmonitor

import android.content.Intent
import android.net.Uri
import android.os.Bundle
import android.widget.Toast
import androidx.activity.ComponentActivity
import androidx.activity.compose.BackHandler
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.animation.AnimatedContent
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.togetherWith
import androidx.compose.foundation.Image
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
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.automirrored.filled.ArrowForward
import androidx.compose.material.icons.filled.Bolt
import androidx.compose.material.icons.filled.Computer
import androidx.compose.material.icons.filled.ContentPaste
import androidx.compose.material.icons.filled.Info
import androidx.compose.material.icons.filled.Lock
import androidx.compose.material.icons.filled.Refresh
import androidx.compose.material.icons.filled.Settings
import androidx.compose.material.icons.filled.Speed
import androidx.compose.material.icons.filled.Sync
import androidx.compose.material.icons.filled.TouchApp
import androidx.compose.material.icons.filled.Tune
import androidx.compose.material.icons.filled.Usb
import androidx.compose.material.icons.filled.Wifi
import androidx.compose.material.icons.filled.ZoomIn
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.CenterAlignedTopAppBar
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.FilterChip
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.ListItem
import androidx.compose.material3.ListItemDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.RadioButton
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SegmentedButton
import androidx.compose.material3.SegmentedButtonDefaults
import androidx.compose.material3.SingleChoiceSegmentedButtonRow
import androidx.compose.material3.Slider
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.input.nestedscroll.nestedScroll
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import io.github.hafijbge.secondmonitor.ui.Brand
import io.github.hafijbge.secondmonitor.ui.ExtendoTheme
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        val settings = AppSettings(this)
        setContent {
            ExtendoTheme {
                var screen by remember { mutableStateOf(Screen.HOME) }
                BackHandler(enabled = screen != Screen.HOME) { screen = Screen.HOME }
                AnimatedContent(targetState = screen, transitionSpec = { fadeIn() togetherWith fadeOut() }, label = "screen") {
                    when (it) {
                        Screen.HOME -> HomeScreen(settings, onSettings = { screen = Screen.SETTINGS }, onConnect = ::connect)
                        Screen.SETTINGS -> SettingsScreen(settings, onBack = { screen = Screen.HOME })
                    }
                }
            }
        }
    }

    override fun onResume() {
        super.onResume()
        resumeTick.intValue++
        intent.getStringExtra(EXTRA_ERROR)?.let {
            Toast.makeText(this, getString(R.string.disconnected, it), Toast.LENGTH_LONG).show()
            intent.removeExtra(EXTRA_ERROR)
        }
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        setIntent(intent)
    }

    private fun connect(host: String, port: Int, name: String) {
        AppSettings(this).apply {
            lastHost = host
            lastPort = port
            lastHostName = name
        }
        startActivity(Intent(this, StreamActivity::class.java).apply {
            putExtra(StreamActivity.EXTRA_HOST, host)
            putExtra(StreamActivity.EXTRA_PORT, port)
            putExtra(StreamActivity.EXTRA_NAME, name)
        })
    }

    companion object {
        const val EXTRA_ERROR = "error"
    }
}

private enum class Screen { HOME, SETTINGS }

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun HomeScreen(settings: AppSettings, onSettings: () -> Unit, onConnect: (String, Int, String) -> Unit) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val hosts = remember { mutableStateListOf<Host>() }
    var scanning by remember { mutableStateOf(false) }
    var manual by remember { mutableStateOf(settings.lastHost?.takeIf { it != "127.0.0.1" } ?: "") }

    fun scan() {
        if (scanning) return
        scanning = true
        scope.launch {
            val found = withContext(Dispatchers.IO) { runCatching { Discovery.scan(context) }.getOrDefault(emptyList()) }
            hosts.clear()
            hosts.addAll(found)
            scanning = false
        }
    }
    LaunchedEffect(Unit) { scan() }

    val scroll = TopAppBarDefaults.pinnedScrollBehavior()
    Scaffold(
        modifier = Modifier.nestedScroll(scroll.nestedScrollConnection),
        topBar = {
            CenterAlignedTopAppBar(
                title = { Text("Extendo") },
                actions = {
                    IconButton(onClick = onSettings) { Icon(Icons.Default.Settings, contentDescription = "Settings") }
                },
                scrollBehavior = scroll,
            )
        },
    ) { padding ->
        Column(
            Modifier
                .padding(padding)
                .fillMaxSize()
                .verticalScroll(rememberScrollState())
                .padding(horizontal = 20.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
            Column(Modifier.widthIn(max = 720.dp), verticalArrangement = Arrangement.spacedBy(16.dp)) {
                Hero()

                val last = settings.lastHost
                if (last != null) {
                    val name = settings.lastHostName ?: last
                    FilledTonalButton(
                        onClick = { onConnect(last, settings.lastPort, name) },
                        modifier = Modifier.fillMaxWidth().height(52.dp),
                    ) {
                        Icon(Icons.Default.Bolt, contentDescription = null)
                        Spacer(Modifier.size(8.dp))
                        Text("Reconnect to $name")
                    }
                }

                SectionCard(icon = Icons.Default.Usb, title = "USB cable", subtitle = "Lowest latency. Turn on USB debugging, plug in, and start Extendo on the PC.") {
                    Button(
                        onClick = { onConnect("127.0.0.1", Protocol.DEFAULT_PORT, "USB") },
                        modifier = Modifier.fillMaxWidth().height(52.dp),
                    ) { Text("Connect over USB") }
                }

                SectionCard(
                    icon = Icons.Default.Wifi,
                    title = "WiFi",
                    subtitle = "Your tablet and PC must be on the same network.",
                    action = {
                        IconButton(onClick = { scan() }, enabled = !scanning) {
                            Icon(Icons.Default.Refresh, contentDescription = "Search again")
                        }
                    },
                ) {
                    AnimatedVisibility(scanning) {
                        LinearProgressIndicator(Modifier.fillMaxWidth().padding(bottom = 8.dp))
                    }
                    if (!scanning && hosts.isEmpty()) {
                        Text(
                            "No computers found. Is Extendo open on the PC? Check that its firewall allows TCP 52020 and UDP 52021.",
                            style = MaterialTheme.typography.bodyMedium,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                    hosts.forEach { h ->
                        ListItem(
                            headlineContent = { Text(h.name) },
                            supportingContent = { Text(h.address) },
                            leadingContent = {
                                Box(
                                    Modifier.size(40.dp).clip(CircleShape).background(MaterialTheme.colorScheme.primaryContainer),
                                    contentAlignment = Alignment.Center,
                                ) { Icon(Icons.Default.Computer, contentDescription = null, tint = MaterialTheme.colorScheme.onPrimaryContainer) }
                            },
                            trailingContent = { Icon(Icons.AutoMirrored.Filled.ArrowForward, contentDescription = "Connect") },
                            colors = ListItemDefaults.colors(containerColor = MaterialTheme.colorScheme.surfaceContainerHigh),
                            modifier = Modifier
                                .padding(vertical = 4.dp)
                                .clip(RoundedCornerShape(14.dp))
                                .clickable { onConnect(h.address, h.port, h.name) },
                        )
                    }
                    Spacer(Modifier.height(8.dp))
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        OutlinedTextField(
                            value = manual,
                            onValueChange = { manual = it.trim() },
                            label = { Text("PC address") },
                            placeholder = { Text("192.168.1.20") },
                            singleLine = true,
                            keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Uri, imeAction = ImeAction.Go),
                            keyboardActions = KeyboardActions(onGo = {
                                if (manual.isNotEmpty()) connectManual(manual, onConnect)
                            }),
                            modifier = Modifier.weight(1f),
                        )
                        Spacer(Modifier.size(8.dp))
                        FilledTonalButton(onClick = { connectManual(manual, onConnect) }, enabled = manual.isNotEmpty(), modifier = Modifier.height(56.dp)) {
                            Text("Connect")
                        }
                    }
                }

                StorageCard()

                FeatureRow()
                Spacer(Modifier.height(24.dp))
            }
        }
    }
}

private fun connectManual(text: String, onConnect: (String, Int, String) -> Unit) {
    val host = text.substringBefore(':')
    val port = text.substringAfter(':', "").toIntOrNull() ?: Protocol.DEFAULT_PORT
    onConnect(host, port, host)
}

@Composable
private fun Hero() {
    Box(
        Modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(28.dp))
            .background(Brand.gradient)
            .padding(24.dp),
    ) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Image(
                painter = painterResource(R.drawable.ic_launcher_foreground),
                contentDescription = null,
                modifier = Modifier.size(84.dp),
            )
            Spacer(Modifier.size(12.dp))
            Column {
                Text("Your tablet, as a second screen", style = MaterialTheme.typography.titleLarge, color = androidx.compose.ui.graphics.Color.White)
                Spacer(Modifier.height(4.dp))
                Text(
                    "Extend or mirror your Windows or Linux PC. Touch, trackpad, keyboard, files and clipboard included.",
                    style = MaterialTheme.typography.bodyMedium,
                    color = androidx.compose.ui.graphics.Color.White.copy(alpha = 0.85f),
                )
            }
        }
    }
}

@Composable
private fun FeatureRow() {
    val items = listOf(
        Icons.Default.TouchApp to "Touch & trackpad",
        Icons.Default.ContentPaste to "Shared clipboard",
        Icons.Default.Sync to "File transfer",
        Icons.Default.Speed to "Up to 120 fps",
    )
    Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        items.forEach { (icon, label) ->
            Column(
                Modifier
                    .weight(1f)
                    .clip(RoundedCornerShape(16.dp))
                    .background(MaterialTheme.colorScheme.surfaceContainerLow)
                    .padding(vertical = 14.dp, horizontal = 6.dp),
                horizontalAlignment = Alignment.CenterHorizontally,
            ) {
                Icon(icon, contentDescription = null, tint = MaterialTheme.colorScheme.primary)
                Spacer(Modifier.height(6.dp))
                Text(label, style = MaterialTheme.typography.labelMedium, textAlign = TextAlign.Center)
            }
        }
    }
}

@Composable
internal fun SectionCard(
    icon: ImageVector,
    title: String,
    subtitle: String,
    action: @Composable () -> Unit = {},
    content: @Composable () -> Unit,
) {
    Card(
        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceContainerLow),
        shape = RoundedCornerShape(24.dp),
    ) {
        Column(Modifier.padding(20.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Box(
                    Modifier.size(44.dp).clip(RoundedCornerShape(14.dp)).background(MaterialTheme.colorScheme.primaryContainer),
                    contentAlignment = Alignment.Center,
                ) { Icon(icon, contentDescription = null, tint = MaterialTheme.colorScheme.onPrimaryContainer) }
                Spacer(Modifier.size(14.dp))
                Column(Modifier.weight(1f)) {
                    Text(title, style = MaterialTheme.typography.titleMedium)
                    Text(subtitle, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                }
                action()
            }
            Spacer(Modifier.height(14.dp))
            content()
        }
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun SettingsScreen(settings: AppSettings, onBack: () -> Unit) {
    val context = LocalContext.current
    var resolution by remember { mutableStateOf(settings.resolution) }
    var scale by remember { mutableFloatStateOf(settings.scale) }
    var quality by remember { mutableStateOf(settings.quality) }
    var fps by remember { mutableIntStateOf(settings.customFps) }
    var bitrate by remember { mutableFloatStateOf(settings.customBitrateMbps.toFloat()) }
    var inputMode by remember { mutableStateOf(settings.inputMode) }
    var clipboard by remember { mutableStateOf(settings.clipboardSync) }
    var stats by remember { mutableStateOf(settings.showStats) }
    var reconnect by remember { mutableStateOf(settings.autoReconnect) }

    val scroll = TopAppBarDefaults.pinnedScrollBehavior()
    Scaffold(
        modifier = Modifier.nestedScroll(scroll.nestedScrollConnection),
        topBar = {
            CenterAlignedTopAppBar(
                title = { Text("Settings") },
                navigationIcon = { IconButton(onClick = onBack) { Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = "Back") } },
                scrollBehavior = scroll,
            )
        },
    ) { padding ->
        Column(
            Modifier.padding(padding).fillMaxSize().verticalScroll(rememberScrollState()).padding(horizontal = 20.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
            Column(Modifier.widthIn(max = 720.dp), verticalArrangement = Arrangement.spacedBy(16.dp)) {
                SectionCard(Icons.Default.ZoomIn, "Resolution & scale", "Lower resolution is smoother; higher scale makes everything bigger.") {
                    Resolution.entries.forEach { r ->
                        Row(
                            Modifier.fillMaxWidth().clip(RoundedCornerShape(12.dp)).clickable {
                                resolution = r; settings.resolution = r
                            }.padding(vertical = 2.dp),
                            verticalAlignment = Alignment.CenterVertically,
                        ) {
                            RadioButton(selected = resolution == r, onClick = { resolution = r; settings.resolution = r })
                            Text(r.label)
                        }
                    }
                    HorizontalDivider(Modifier.padding(vertical = 10.dp), color = MaterialTheme.colorScheme.outlineVariant)
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Text("Scale", Modifier.weight(1f), style = MaterialTheme.typography.titleSmall)
                        Text("${(scale * 100).toInt()}%", color = MaterialTheme.colorScheme.primary, style = MaterialTheme.typography.titleSmall)
                    }
                    Slider(
                        value = scale,
                        onValueChange = { scale = (it * 4).toInt() / 4f },
                        onValueChangeFinished = { settings.scale = scale },
                        valueRange = 1f..2.5f,
                        steps = 5,
                    )
                }

                SectionCard(Icons.Default.Tune, "Quality", "Trade sharpness for smoothness.") {
                    Quality.entries.forEach { q ->
                        Row(
                            Modifier.fillMaxWidth().clip(RoundedCornerShape(12.dp)).clickable { quality = q; settings.quality = q }.padding(vertical = 2.dp),
                            verticalAlignment = Alignment.CenterVertically,
                        ) {
                            RadioButton(selected = quality == q, onClick = { quality = q; settings.quality = q })
                            Column {
                                Text(q.label)
                                Text(q.description, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                            }
                        }
                    }
                    AnimatedVisibility(quality == Quality.CUSTOM) {
                        Column(Modifier.padding(top = 8.dp)) {
                            Text("Frame rate", style = MaterialTheme.typography.titleSmall)
                            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                                listOf(30, 60, 90, 120).forEach { f ->
                                    FilterChip(selected = fps == f, onClick = { fps = f; settings.customFps = f }, label = { Text("$f fps") })
                                }
                            }
                            Spacer(Modifier.height(8.dp))
                            Row {
                                Text("Bitrate", Modifier.weight(1f), style = MaterialTheme.typography.titleSmall)
                                Text("${bitrate.toInt()} Mbit/s", color = MaterialTheme.colorScheme.primary)
                            }
                            Slider(
                                value = bitrate,
                                onValueChange = { bitrate = it },
                                onValueChangeFinished = { settings.customBitrateMbps = bitrate.toInt() },
                                valueRange = 2f..60f,
                            )
                        }
                    }
                }

                SectionCard(Icons.Default.TouchApp, "Input", "How your fingers control the PC. A stylus or mouse always points directly.") {
                    SingleChoiceSegmentedButtonRow(Modifier.fillMaxWidth()) {
                        InputMode.entries.forEachIndexed { i, m ->
                            SegmentedButton(
                                selected = inputMode == m,
                                onClick = { inputMode = m; settings.inputMode = m },
                                shape = SegmentedButtonDefaults.itemShape(i, InputMode.entries.size),
                            ) { Text(m.label) }
                        }
                    }
                    Spacer(Modifier.height(8.dp))
                    Text(
                        if (inputMode == InputMode.TRACKPAD) {
                            "Tap to click · two-finger tap = right click · two fingers scroll · press and hold, then drag"
                        } else {
                            "Touch the PC's screen directly. Pinch and swipe gestures work on GNOME."
                        },
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }

                SectionCard(Icons.Default.Settings, "Features", "Applied when you connect.") {
                    SwitchRow("Sync clipboard", "Copy on one device, paste on the other", clipboard) { clipboard = it; settings.clipboardSync = it }
                    SwitchRow("Performance overlay", "Show frame rate and bandwidth", stats) { stats = it; settings.showStats = it }
                    SwitchRow("Reconnect automatically", "Retry if the connection drops", reconnect) { reconnect = it; settings.autoReconnect = it }
                }

                var trustedPcs by remember { mutableIntStateOf(TrustStore(context).count()) }
                SectionCard(Icons.Default.Lock, "Security", "Connections are encrypted. New PCs on WiFi are confirmed with a code.") {
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Text(
                            if (trustedPcs == 1) "1 trusted PC" else "$trustedPcs trusted PCs",
                            Modifier.weight(1f),
                        )
                        FilledTonalButton(
                            onClick = { TrustStore(context).forgetAll(); trustedPcs = 0 },
                            enabled = trustedPcs > 0,
                        ) { Text("Forget all") }
                    }
                    Text(
                        "Forgetting makes every PC show its code again on the next WiFi connection.",
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }

                SectionCard(Icons.Default.Info, "About", "Extendo ${appVersion(context)}") {
                    Text("Made by Md Hafijur Rahman", style = MaterialTheme.typography.titleSmall)
                    Spacer(Modifier.height(4.dp))
                    Text(
                        "github.com/Hafij-BGE",
                        color = MaterialTheme.colorScheme.primary,
                        modifier = Modifier.clickable {
                            context.startActivity(Intent(Intent.ACTION_VIEW, Uri.parse("https://github.com/Hafij-BGE")))
                        },
                    )
                }
                Spacer(Modifier.height(24.dp))
            }
        }
    }
}

@Composable
private fun SwitchRow(title: String, subtitle: String, checked: Boolean, onChange: (Boolean) -> Unit) {
    Row(
        Modifier.fillMaxWidth().clip(RoundedCornerShape(12.dp)).clickable { onChange(!checked) }.padding(vertical = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Column(Modifier.weight(1f)) {
            Text(title)
            Text(subtitle, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
        Switch(checked = checked, onCheckedChange = onChange)
    }
}

private fun appVersion(context: android.content.Context): String =
    runCatching { context.packageManager.getPackageInfo(context.packageName, 0).versionName }.getOrNull() ?: ""
