package io.github.hafijbge.secondmonitor

import android.annotation.SuppressLint
import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
import android.graphics.Color
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.util.DisplayMetrics
import android.util.Log
import android.view.Gravity
import android.view.InputDevice
import android.view.KeyEvent
import android.view.SurfaceHolder
import android.view.SurfaceView
import android.view.View
import android.view.WindowInsets
import android.view.WindowInsetsController
import android.view.WindowManager
import android.widget.FrameLayout
import android.widget.ImageView
import androidx.activity.ComponentActivity
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.ui.platform.ComposeView
import io.github.hafijbge.secondmonitor.ui.ExtendoTheme

class StreamActivity : ComponentActivity(), SurfaceHolder.Callback, StreamClient.Listener {
    private lateinit var settings: AppSettings
    private lateinit var root: FrameLayout
    private lateinit var surfaceView: SurfaceView
    private lateinit var cursorView: ImageView
    private lateinit var keyboardView: KeyboardInputView
    private lateinit var input: InputController
    private lateinit var files: FileStore
    private val ui = Handler(Looper.getMainLooper())

    private var client: StreamClient? = null
    private var surfaceReady = false
    private var cursorImage: CursorImage? = null
    private var videoWidth = 0
    private var videoHeight = 0

    // Clipboard: last text exchanged with the PC, to avoid echoing it back.
    private var lastClip: String? = null
    private val clipboard by lazy { getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager }
    private val clipListener = ClipboardManager.OnPrimaryClipChangedListener { pushClipboard() }

    /** State the Compose overlay renders. */
    private val state = OverlayState()

    private val pickFiles = registerForActivityResult(ActivityResultContracts.OpenMultipleDocuments()) { uris ->
        uris.forEach { sendUri(it) }
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
        settings = AppSettings(this)
        files = FileStore(this)

        root = FrameLayout(this).apply { setBackgroundColor(Color.BLACK) }
        surfaceView = SurfaceView(this)
        root.addView(surfaceView, FrameLayout.LayoutParams(
            FrameLayout.LayoutParams.MATCH_PARENT, FrameLayout.LayoutParams.MATCH_PARENT, Gravity.CENTER,
        ))
        cursorView = ImageView(this).apply {
            scaleType = ImageView.ScaleType.FIT_XY
            visibility = View.GONE
            // Let touches fall through to the video surface.
            isClickable = false
            isFocusable = false
        }
        root.addView(cursorView, FrameLayout.LayoutParams(1, 1, Gravity.TOP or Gravity.START))

        input = InputController(surfaceView) { client }
        input.mode = settings.inputMode
        input.onModifiersChanged = {
            state.modifiers.clear()
            state.modifiers.addAll(input.modifiers)
        }
        state.inputMode.value = input.mode
        state.showStats.value = settings.showStats
        state.hostName.value = intent.getStringExtra(EXTRA_NAME) ?: host()

        keyboardView = KeyboardInputView(this, input)
        root.addView(keyboardView, FrameLayout.LayoutParams(1, 1))

        val overlay = ComposeView(this).apply {
            setContent { ExtendoTheme(dark = true) { StreamOverlay(state, actions) } }
        }
        root.addView(overlay, FrameLayout.LayoutParams(
            FrameLayout.LayoutParams.MATCH_PARENT, FrameLayout.LayoutParams.MATCH_PARENT,
        ))
        setContentView(root)
        hideSystemBars()

        surfaceView.holder.addCallback(this)
        setupTouch()
        root.addOnLayoutChangeListener { _, _, _, _, _, _, _, _, _ -> fitSurface() }
    }

    private fun host() = intent.getStringExtra(EXTRA_HOST) ?: "127.0.0.1"

    override fun onWindowFocusChanged(hasFocus: Boolean) {
        super.onWindowFocusChanged(hasFocus)
        if (hasFocus) {
            hideSystemBars()
            // Android only lets the focused app read the clipboard.
            pushClipboard()
        }
    }

    @Suppress("DEPRECATION")
    private fun hideSystemBars() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            window.setDecorFitsSystemWindows(false)
            window.insetsController?.let {
                it.hide(WindowInsets.Type.systemBars())
                it.systemBarsBehavior = WindowInsetsController.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE
            }
        } else {
            window.decorView.systemUiVisibility = (View.SYSTEM_UI_FLAG_IMMERSIVE_STICKY
                or View.SYSTEM_UI_FLAG_FULLSCREEN
                or View.SYSTEM_UI_FLAG_HIDE_NAVIGATION
                or View.SYSTEM_UI_FLAG_LAYOUT_STABLE
                or View.SYSTEM_UI_FLAG_LAYOUT_HIDE_NAVIGATION
                or View.SYSTEM_UI_FLAG_LAYOUT_FULLSCREEN)
        }
    }

    /** Full physical screen size in landscape. */
    @Suppress("DEPRECATION")
    private fun screenSize(): Triple<Int, Int, Int> {
        val metrics = DisplayMetrics()
        windowManager.defaultDisplay.getRealMetrics(metrics)
        val w = maxOf(metrics.widthPixels, metrics.heightPixels)
        val h = minOf(metrics.widthPixels, metrics.heightPixels)
        return Triple(w, h, metrics.densityDpi)
    }

    private fun connect() {
        if (!surfaceReady || isFinishing || client != null) return
        val (w, h, dpi) = screenSize()
        // The decoder's limit first (e.g. 2560x1600 fails on many MediaTek
        // decoders), then the user's resolution and scale choices.
        val decoder = DecoderLimits.choose(w, h, settings.fps.takeIf { it > 0 }?.toDouble() ?: 60.0)
        val (rw, rh) = settings.requestSize(decoder.width, decoder.height)
        Log.i(TAG, "screen ${w}x$h, decoder ${decoder.codecName} ${decoder.width}x${decoder.height}, request ${rw}x$rh")
        val request = StreamRequest(
            width = rw, height = rh, dpi = dpi,
            fps = settings.fps, bitrateKbps = settings.bitrateMbps * 1000,
            clipboard = settings.clipboardSync, decoderName = decoder.codecName,
        )
        if (state.phase.value != Phase.RECONNECTING) state.phase.value = Phase.CONNECTING
        client = StreamClient(
            host(), intent.getIntExtra(EXTRA_PORT, Protocol.DEFAULT_PORT),
            surfaceView.holder.surface, request, files, TrustStore(this), this,
        ).also {
            Session.client = it
            it.start()
        }
    }

    override fun surfaceCreated(holder: SurfaceHolder) {
        surfaceReady = true
        connect()
    }

    override fun surfaceChanged(holder: SurfaceHolder, format: Int, width: Int, height: Int) {}

    override fun surfaceDestroyed(holder: SurfaceHolder) {
        surfaceReady = false
        ui.removeCallbacksAndMessages(null)
        disconnect()
    }

    private fun disconnect() {
        client?.close()
        if (Session.client === client) Session.client = null
        client = null
    }

    override fun onStart() {
        super.onStart()
        if (settings.clipboardSync) clipboard.addPrimaryClipChangedListener(clipListener)
    }

    override fun onStop() {
        clipboard.removePrimaryClipChangedListener(clipListener)
        super.onStop()
    }

    override fun onDestroy() {
        disconnect()
        super.onDestroy()
    }

    override fun dispatchKeyEvent(event: KeyEvent): Boolean {
        // A hardware keyboard on the tablet types on the PC.
        val fromKeyboard = event.device?.isVirtual == false &&
            event.source and InputDevice.SOURCE_KEYBOARD == InputDevice.SOURCE_KEYBOARD &&
            event.keyCode !in listOf(KeyEvent.KEYCODE_VOLUME_UP, KeyEvent.KEYCODE_VOLUME_DOWN, KeyEvent.KEYCODE_BACK)
        if (fromKeyboard && !keyboardView.hasFocus() && input.onHardwareKey(event)) return true
        return super.dispatchKeyEvent(event)
    }

    // StreamClient.Listener (background threads)

    override fun onVerifyPc(code: String, answer: (Boolean) -> Unit) {
        runOnUiThread {
            state.code.value = code
            state.phase.value = Phase.VERIFY
            state.verifyAnswer = { ok ->
                state.verifyAnswer = null
                state.phase.value = Phase.CONNECTING
                answer(ok)
            }
        }
    }

    override fun onWaitingForApproval(code: String) {
        runOnUiThread {
            state.code.value = code
            state.phase.value = Phase.APPROVAL
        }
    }

    override fun onConnected() {
        runOnUiThread {
            state.error.value = null
            if (state.phase.value == Phase.APPROVAL) state.phase.value = Phase.CONNECTING
        }
    }

    override fun onVideoSize(width: Int, height: Int) {
        runOnUiThread {
            videoWidth = width
            videoHeight = height
            input.videoWidth = width
            input.videoHeight = height
            state.resolution.value = "$width × $height"
            fitSurface()
        }
    }

    override fun onFirstFrame() {
        runOnUiThread {
            state.phase.value = Phase.STREAMING
            pushClipboard()
        }
    }

    override fun onCursor(visible: Boolean, x: Float, y: Float, sourceWidth: Int, image: CursorImage?) {
        runOnUiThread {
            if (image != null) {
                cursorImage = image
                cursorView.setImageBitmap(image.bitmap)
            }
            val img = cursorImage
            val sw = surfaceView.width
            val inside = x in 0f..1f && y in 0f..1f
            if (img == null || !visible || !inside || sw == 0 || sourceWidth == 0) {
                cursorView.visibility = View.GONE
                return@runOnUiThread
            }
            // Same scale as the video, so the pointer matches the desktop.
            val scale = sw.toFloat() / sourceWidth
            val w = (img.bitmap.width * scale).toInt().coerceAtLeast(1)
            val h = (img.bitmap.height * scale).toInt().coerceAtLeast(1)
            val lp = cursorView.layoutParams
            if (lp.width != w || lp.height != h) {
                lp.width = w
                lp.height = h
                cursorView.layoutParams = lp
            }
            cursorView.translationX = surfaceView.left + x * sw - img.hotX * scale
            cursorView.translationY = surfaceView.top + y * surfaceView.height - img.hotY * scale
            cursorView.visibility = View.VISIBLE
        }
    }

    override fun onClipboard(text: String) {
        runOnUiThread {
            lastClip = text
            runCatching { clipboard.setPrimaryClip(ClipData.newPlainText("Extendo", text)) }
        }
    }

    override fun onTransfer(transfer: Transfer) {
        runOnUiThread {
            val i = state.transfers.indexOfFirst { it.id == transfer.id && it.incoming == transfer.incoming }
            if (i >= 0) state.transfers[i] = transfer else state.transfers.add(transfer)
            if (transfer.state != TransferState.RUNNING) {
                state.message.value = when {
                    transfer.state == TransferState.FAILED -> "Couldn't transfer ${transfer.name}"
                    transfer.incoming -> "Saved ${transfer.name} to Downloads/${FileStore.FOLDER}"
                    else -> "Sent ${transfer.name} to the PC"
                }
                ui.postDelayed({
                    state.transfers.removeAll { it.id == transfer.id && it.incoming == transfer.incoming }
                }, 2500)
            }
        }
    }

    override fun onStats(fps: Float, mbps: Float) {
        runOnUiThread {
            state.fps.floatValue = fps
            state.mbps.floatValue = mbps
        }
    }

    override fun onClosed(error: String?) {
        runOnUiThread {
            if (isFinishing) return@runOnUiThread
            if (Session.client === client) Session.client = null
            client = null
            cursorView.visibility = View.GONE
            if (settings.autoReconnect && surfaceReady) {
                state.phase.value = Phase.RECONNECTING
                state.error.value = error ?: "Connection closed"
                ui.postDelayed({ connect() }, 2000)
            } else {
                goHome(error ?: "the PC closed the connection")
            }
        }
    }

    private fun goHome(error: String?) {
        startActivity(Intent(this, MainActivity::class.java).apply {
            flags = Intent.FLAG_ACTIVITY_CLEAR_TOP or Intent.FLAG_ACTIVITY_SINGLE_TOP
            if (error != null) putExtra(MainActivity.EXTRA_ERROR, error)
        })
        finish()
    }

    private fun pushClipboard() {
        if (!settings.clipboardSync) return
        val c = client ?: return
        val text = runCatching { clipboard.primaryClip?.getItemAt(0)?.coerceToText(this)?.toString() }.getOrNull()
        if (text.isNullOrEmpty() || text == lastClip) return
        lastClip = text
        c.sendClipboard(text)
    }

    private fun sendUri(uri: Uri) {
        val c = client ?: return
        val (name, size) = FileStore.describe(this, uri)
        c.sendFile(name, size) { contentResolver.openInputStream(uri) ?: throw java.io.IOException("cannot open $name") }
    }

    /** Letterbox the surface to the video's aspect ratio. */
    private fun fitSurface() {
        val rw = root.width
        val rh = root.height
        if (videoWidth == 0 || videoHeight == 0 || rw == 0 || rh == 0) return
        val scale = minOf(rw.toFloat() / videoWidth, rh.toFloat() / videoHeight)
        val w = (videoWidth * scale).toInt()
        val h = (videoHeight * scale).toInt()
        val lp = surfaceView.layoutParams as FrameLayout.LayoutParams
        if (lp.width != w || lp.height != h) {
            lp.width = w
            lp.height = h
            lp.gravity = Gravity.CENTER
            surfaceView.layoutParams = lp
        }
    }

    @SuppressLint("ClickableViewAccessibility")
    private fun setupTouch() {
        surfaceView.setOnTouchListener { _, e -> input.onTouch(e) }
        surfaceView.setOnGenericMotionListener { _, e -> input.onGenericMotion(e) }
    }

    private val actions = object : OverlayActions {
        override fun toggleKeyboard() {
            val show = !state.keyboard.value
            state.keyboard.value = show
            if (show) keyboardView.show() else keyboardView.hide()
        }

        override fun toggleInputMode() {
            input.mode = if (input.mode == InputMode.TOUCH) InputMode.TRACKPAD else InputMode.TOUCH
            settings.inputMode = input.mode
            state.inputMode.value = input.mode
            state.message.value = if (input.mode == InputMode.TRACKPAD) "Trackpad mode" else "Touchscreen mode"
        }

        override fun toggleModifier(keysym: Int) = input.toggleModifier(keysym)

        override fun shortcut(keysym: Int, modifiers: List<Int>) {
            client?.tapKey(keysym, modifiers)
        }

        override fun key(keysym: Int) = input.type(keysym)

        override fun sendFiles() {
            pickFiles.launch(arrayOf("*/*"))
        }

        override fun toggleStats() {
            state.showStats.value = !state.showStats.value
            settings.showStats = state.showStats.value
        }

        override fun disconnect() {
            state.verifyAnswer?.invoke(false)
            this@StreamActivity.disconnect()
            goHome(null)
        }
    }

    companion object {
        private const val TAG = "StreamActivity"
        const val EXTRA_HOST = "host"
        const val EXTRA_PORT = "port"
        const val EXTRA_NAME = "name"
    }
}

enum class Phase { CONNECTING, VERIFY, APPROVAL, STREAMING, RECONNECTING }

/** Observable state for the Compose overlay. */
class OverlayState {
    val phase = mutableStateOf(Phase.CONNECTING)
    val hostName = mutableStateOf("")
    val error = mutableStateOf<String?>(null)
    val resolution = mutableStateOf("")
    val fps = mutableFloatStateOf(0f)
    val mbps = mutableFloatStateOf(0f)
    val showStats = mutableStateOf(false)
    val inputMode = mutableStateOf(InputMode.TOUCH)
    val keyboard = mutableStateOf(false)
    val modifiers = mutableStateListOf<Int>()
    val transfers = mutableStateListOf<Transfer>()
    val message = mutableStateOf<String?>(null)
    /** Pairing code shown while verifying a new PC. */
    val code = mutableStateOf("")
    var verifyAnswer: ((Boolean) -> Unit)? = null
}

interface OverlayActions {
    fun toggleKeyboard()
    fun toggleInputMode()
    fun toggleModifier(keysym: Int)
    fun shortcut(keysym: Int, modifiers: List<Int>)
    fun key(keysym: Int)
    fun sendFiles()
    fun toggleStats()
    fun disconnect()
}
