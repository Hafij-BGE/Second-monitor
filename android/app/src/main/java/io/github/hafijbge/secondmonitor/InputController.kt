package io.github.hafijbge.secondmonitor

import android.os.Handler
import android.os.Looper
import android.view.HapticFeedbackConstants
import android.view.KeyEvent
import android.view.MotionEvent
import android.view.View
import android.view.ViewConfiguration
import kotlin.math.hypot

/**
 * Turns touches on the video into host input.
 *
 * Touchscreen mode: fingers are real touches. Trackpad mode: one finger moves
 * the pointer, tap = click, two-finger tap = right click, three-finger tap =
 * middle click, two fingers scroll, press and hold then drag = drag.
 * A stylus or mouse always acts as an absolute pointer.
 */
class InputController(private val view: View, private val client: () -> StreamClient?) {
    var mode = InputMode.TOUCH
    var videoWidth = 0
    var videoHeight = 0

    /** Modifiers latched from the sidebar; applied to the next key. */
    val modifiers = linkedSetOf<Int>()
    var onModifiersChanged: () -> Unit = {}

    private val slop = ViewConfiguration.get(view.context).scaledTouchSlop.toFloat()
    private val handler = Handler(Looper.getMainLooper())

    // Trackpad gesture state.
    private var downTime = 0L
    private var startX = 0f
    private var startY = 0f
    private var lastX = 0f
    private var lastY = 0f
    private var maxPointers = 0
    private var moved = false
    private var dragging = false
    private var scrolling = false
    private val longPress = Runnable {
        if (!moved && maxPointers == 1) {
            dragging = true
            client()?.sendMouseButton(Protocol.BUTTON_LEFT, true)
            view.performHapticFeedback(HapticFeedbackConstants.LONG_PRESS)
        }
    }

    // Stylus/mouse button state.
    private var penDown = false

    fun onTouch(e: MotionEvent): Boolean {
        val c = client() ?: return true
        val tool = e.getToolType(e.actionIndex)
        if (tool == MotionEvent.TOOL_TYPE_STYLUS || tool == MotionEvent.TOOL_TYPE_MOUSE ||
            tool == MotionEvent.TOOL_TYPE_ERASER
        ) {
            pointer(e, c)
            return true
        }
        when (mode) {
            InputMode.TOUCH -> touchscreen(e, c)
            InputMode.TRACKPAD -> trackpad(e, c)
        }
        return true
    }

    /** Stylus hover and mouse movement without buttons. */
    fun onGenericMotion(e: MotionEvent): Boolean {
        val c = client() ?: return false
        return when (e.actionMasked) {
            MotionEvent.ACTION_HOVER_MOVE, MotionEvent.ACTION_HOVER_ENTER -> {
                c.sendMouseMoveTo(nx(e.x), ny(e.y))
                true
            }
            MotionEvent.ACTION_SCROLL -> {
                // Mouse wheel: about 3 lines (~50 px) per notch.
                c.sendScroll(-e.getAxisValue(MotionEvent.AXIS_HSCROLL) * 50f, -e.getAxisValue(MotionEvent.AXIS_VSCROLL) * 50f, true)
                true
            }
            else -> false
        }
    }

    private fun nx(x: Float) = (x / view.width.coerceAtLeast(1)).coerceIn(0f, 1f)
    private fun ny(y: Float) = (y / view.height.coerceAtLeast(1)).coerceIn(0f, 1f)

    private fun touchscreen(e: MotionEvent, c: StreamClient) {
        fun send(action: Int, i: Int) = c.sendTouch(action, e.getPointerId(i), nx(e.getX(i)), ny(e.getY(i)))
        when (e.actionMasked) {
            MotionEvent.ACTION_DOWN, MotionEvent.ACTION_POINTER_DOWN -> send(Protocol.TOUCH_DOWN, e.actionIndex)
            MotionEvent.ACTION_MOVE -> for (i in 0 until e.pointerCount) send(Protocol.TOUCH_MOVE, i)
            MotionEvent.ACTION_UP, MotionEvent.ACTION_POINTER_UP -> send(Protocol.TOUCH_UP, e.actionIndex)
            MotionEvent.ACTION_CANCEL -> for (i in 0 until e.pointerCount) send(Protocol.TOUCH_CANCEL, i)
        }
    }

    private fun pointer(e: MotionEvent, c: StreamClient) {
        val i = e.actionIndex
        c.sendMouseMoveTo(nx(e.getX(i)), ny(e.getY(i)))
        val secondary = e.buttonState and (MotionEvent.BUTTON_STYLUS_PRIMARY or MotionEvent.BUTTON_SECONDARY) != 0
        when (e.actionMasked) {
            MotionEvent.ACTION_DOWN -> {
                penDown = true
                c.sendMouseButton(if (secondary) Protocol.BUTTON_RIGHT else Protocol.BUTTON_LEFT, true)
            }
            MotionEvent.ACTION_UP, MotionEvent.ACTION_CANCEL -> if (penDown) {
                penDown = false
                c.sendMouseButton(Protocol.BUTTON_LEFT, false)
                c.sendMouseButton(Protocol.BUTTON_RIGHT, false)
            }
        }
    }

    /** Host pixels per view pixel, with a little acceleration for fast swipes. */
    private fun gain(distance: Float): Float {
        val base = if (view.width > 0 && videoWidth > 0) videoWidth.toFloat() / view.width else 1f
        return base * (1.2f + (distance / 40f).coerceAtMost(1.3f))
    }

    private fun centroid(e: MotionEvent): Pair<Float, Float> {
        var x = 0f
        var y = 0f
        for (i in 0 until e.pointerCount) {
            x += e.getX(i)
            y += e.getY(i)
        }
        return x / e.pointerCount to y / e.pointerCount
    }

    private fun trackpad(e: MotionEvent, c: StreamClient) {
        when (e.actionMasked) {
            MotionEvent.ACTION_DOWN -> {
                downTime = e.eventTime
                startX = e.x; startY = e.y
                lastX = e.x; lastY = e.y
                maxPointers = 1
                moved = false
                scrolling = false
                handler.postDelayed(longPress, LONG_PRESS_MS)
            }
            MotionEvent.ACTION_POINTER_DOWN -> {
                maxPointers = maxOf(maxPointers, e.pointerCount)
                handler.removeCallbacks(longPress)
                val (x, y) = centroid(e)
                lastX = x; lastY = y
            }
            MotionEvent.ACTION_MOVE -> {
                val (x, y) = if (e.pointerCount > 1) centroid(e) else e.x to e.y
                val dx = x - lastX
                val dy = y - lastY
                lastX = x; lastY = y
                if (!moved && hypot(e.x - startX, e.y - startY) > slop) {
                    moved = true
                    handler.removeCallbacks(longPress)
                }
                if (!moved) return
                if (e.pointerCount >= 2) {
                    // Natural scrolling: content follows the fingers.
                    scrolling = true
                    val k = gain(0f)
                    c.sendScroll(-dx * k, -dy * k, false)
                } else if (maxPointers == 1) {
                    val k = gain(hypot(dx, dy))
                    c.sendMouseMoveBy(dx * k, dy * k)
                }
            }
            MotionEvent.ACTION_POINTER_UP -> {
                // Keep moving smoothly with the remaining finger.
                val remaining = (0 until e.pointerCount).first { it != e.actionIndex }
                lastX = e.getX(remaining); lastY = e.getY(remaining)
            }
            MotionEvent.ACTION_UP, MotionEvent.ACTION_CANCEL -> {
                handler.removeCallbacks(longPress)
                val tap = !moved && e.eventTime - downTime < TAP_MS && e.actionMasked == MotionEvent.ACTION_UP
                when {
                    dragging -> {
                        dragging = false
                        c.sendMouseButton(Protocol.BUTTON_LEFT, false)
                    }
                    scrolling -> c.sendScroll(0f, 0f, true)
                    tap -> {
                        val button = when (maxPointers) {
                            1 -> Protocol.BUTTON_LEFT
                            2 -> Protocol.BUTTON_RIGHT
                            else -> Protocol.BUTTON_MIDDLE
                        }
                        c.sendMouseButton(button, true)
                        c.sendMouseButton(button, false)
                    }
                }
                scrolling = false
            }
        }
    }

    /** Types one keysym with any latched modifiers, then releases them. */
    fun type(keysym: Int) {
        val c = client() ?: return
        c.tapKey(keysym, modifiers.toList())
        if (modifiers.isNotEmpty()) {
            modifiers.clear()
            onModifiersChanged()
        }
    }

    fun toggleModifier(keysym: Int) {
        if (!modifiers.remove(keysym)) modifiers.add(keysym)
        onModifiersChanged()
    }

    /** Hardware keyboard on the tablet. Returns true if consumed. */
    fun onHardwareKey(e: KeyEvent): Boolean {
        val c = client() ?: return false
        if (e.keyCode == KeyEvent.KEYCODE_BACK || e.isSystem && e.keyCode != KeyEvent.KEYCODE_ESCAPE) return false
        val pressed = when (e.action) {
            KeyEvent.ACTION_DOWN -> true
            KeyEvent.ACTION_UP -> false
            else -> return false
        }
        val keysym = Keysyms.forKeyCode(e.keyCode)
            ?: e.unicodeChar.takeIf { it != 0 }?.let { Keysyms.forChar(it) }
            ?: return false
        c.sendKey(keysym, pressed)
        return true
    }

    companion object {
        private const val TAP_MS = 220L
        private const val LONG_PRESS_MS = 450L
    }
}

/** Keyboard bits shared by the soft keyboard view. */
internal fun Int.isPrintable() = this >= 0x20 && this != 0x7f
