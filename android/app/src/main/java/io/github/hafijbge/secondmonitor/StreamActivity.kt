package io.github.hafijbge.secondmonitor

import android.annotation.SuppressLint
import android.app.Activity
import android.content.Intent
import android.graphics.Color
import android.os.Build
import android.os.Bundle
import android.util.DisplayMetrics
import android.util.Log
import android.view.Gravity
import android.view.MotionEvent
import android.view.SurfaceHolder
import android.view.SurfaceView
import android.view.View
import android.view.WindowInsets
import android.view.WindowInsetsController
import android.view.WindowManager
import android.widget.FrameLayout
import android.widget.TextView

class StreamActivity : Activity(), SurfaceHolder.Callback, StreamClient.Listener {
    private lateinit var root: FrameLayout
    private lateinit var surfaceView: SurfaceView
    private lateinit var status: TextView
    private var client: StreamClient? = null
    private var videoWidth = 0
    private var videoHeight = 0

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)

        root = FrameLayout(this).apply { setBackgroundColor(Color.BLACK) }
        surfaceView = SurfaceView(this)
        root.addView(surfaceView, FrameLayout.LayoutParams(
            FrameLayout.LayoutParams.MATCH_PARENT, FrameLayout.LayoutParams.MATCH_PARENT, Gravity.CENTER,
        ))
        status = TextView(this).apply {
            setTextColor(Color.WHITE)
            textSize = 18f
            text = getString(R.string.connecting, host())
        }
        root.addView(status, FrameLayout.LayoutParams(
            FrameLayout.LayoutParams.WRAP_CONTENT, FrameLayout.LayoutParams.WRAP_CONTENT, Gravity.CENTER,
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
        if (hasFocus) hideSystemBars()
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

    /** Full physical screen size in landscape, which is what we ask the host for. */
    @Suppress("DEPRECATION")
    private fun screenSize(): Triple<Int, Int, Int> {
        val metrics = DisplayMetrics()
        windowManager.defaultDisplay.getRealMetrics(metrics)
        val w = maxOf(metrics.widthPixels, metrics.heightPixels)
        val h = minOf(metrics.widthPixels, metrics.heightPixels)
        return Triple(w, h, metrics.densityDpi)
    }

    override fun surfaceCreated(holder: SurfaceHolder) {
        val (w, h, dpi) = screenSize()
        // Ask for the biggest size this phone's decoder can play, not the
        // raw screen size (e.g. 2560x1600 fails on many MediaTek decoders).
        val decoder = DecoderLimits.choose(w, h)
        Log.i(TAG, "screen ${w}x$h, decoder ${decoder.codecName} -> ${decoder.width}x${decoder.height}")
        client = StreamClient(
            host(), intent.getIntExtra(EXTRA_PORT, Protocol.DEFAULT_PORT),
            holder.surface, decoder.codecName, decoder.width, decoder.height, dpi, this,
        ).also { it.start() }
    }

    override fun surfaceChanged(holder: SurfaceHolder, format: Int, width: Int, height: Int) {}

    override fun surfaceDestroyed(holder: SurfaceHolder) {
        client?.close()
        client = null
    }

    override fun onDestroy() {
        client?.close()
        client = null
        super.onDestroy()
    }

    // StreamClient.Listener (background threads)

    override fun onVideoSize(width: Int, height: Int) {
        runOnUiThread {
            videoWidth = width
            videoHeight = height
            fitSurface()
        }
    }

    override fun onFirstFrame() {
        runOnUiThread { status.visibility = View.GONE }
    }

    override fun onClosed(error: String?) {
        runOnUiThread {
            if (isFinishing) return@runOnUiThread
            startActivity(Intent(this, MainActivity::class.java).apply {
                flags = Intent.FLAG_ACTIVITY_CLEAR_TOP or Intent.FLAG_ACTIVITY_SINGLE_TOP
                putExtra(MainActivity.EXTRA_ERROR, error ?: "host closed the connection")
            })
            finish()
        }
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
        surfaceView.setOnTouchListener { v, e ->
            val c = client ?: return@setOnTouchListener true
            val w = v.width.coerceAtLeast(1).toFloat()
            val h = v.height.coerceAtLeast(1).toFloat()
            fun send(action: Int, index: Int) {
                c.sendTouch(action, e.getPointerId(index), e.getX(index) / w, e.getY(index) / h)
            }
            when (e.actionMasked) {
                MotionEvent.ACTION_DOWN, MotionEvent.ACTION_POINTER_DOWN ->
                    send(Protocol.TOUCH_DOWN, e.actionIndex)
                MotionEvent.ACTION_MOVE ->
                    for (i in 0 until e.pointerCount) send(Protocol.TOUCH_MOVE, i)
                MotionEvent.ACTION_UP, MotionEvent.ACTION_POINTER_UP ->
                    send(Protocol.TOUCH_UP, e.actionIndex)
                MotionEvent.ACTION_CANCEL ->
                    for (i in 0 until e.pointerCount) send(Protocol.TOUCH_CANCEL, i)
            }
            true
        }
    }

    companion object {
        private const val TAG = "StreamActivity"
        const val EXTRA_HOST = "host"
        const val EXTRA_PORT = "port"
    }
}
