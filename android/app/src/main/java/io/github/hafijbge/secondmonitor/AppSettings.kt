package io.github.hafijbge.secondmonitor

import android.content.Context
import android.content.SharedPreferences

enum class Resolution(val label: String, val maxWidth: Int) {
    AUTO("Best for this device", Int.MAX_VALUE),
    HIGH("High (1920 wide)", 1920),
    MEDIUM("Medium (1600 wide)", 1600),
    LOW("Low (1280 wide)", 1280),
}

enum class Quality(val label: String, val description: String, val fps: Int, val bitrateMbps: Int) {
    AUTO("Auto", "Use the PC's settings", 0, 0),
    SMOOTH("Smooth", "60 fps, balanced sharpness", 60, 12),
    LOW_LATENCY("Low latency", "60 fps, smaller frames for slow WiFi", 60, 6),
    SHARP("Sharp", "30 fps, best for text and images", 30, 30),
    CUSTOM("Custom", "Choose frame rate and bitrate", 60, 12),
}

enum class InputMode(val label: String) {
    TOUCH("Touchscreen"),
    TRACKPAD("Trackpad"),
}

/** User settings, stored in SharedPreferences. */
class AppSettings(context: Context) {
    private val prefs: SharedPreferences =
        context.applicationContext.getSharedPreferences("settings", Context.MODE_PRIVATE)

    var resolution: Resolution
        get() = enumPref("resolution", Resolution.AUTO)
        set(v) = prefs.edit().putString("resolution", v.name).apply()

    /** Divides the resolution: 1.5 makes text 1.5x bigger. */
    var scale: Float
        get() = prefs.getFloat("scale", 1f)
        set(v) = prefs.edit().putFloat("scale", v).apply()

    var quality: Quality
        get() = enumPref("quality", Quality.AUTO)
        set(v) = prefs.edit().putString("quality", v.name).apply()

    var customFps: Int
        get() = prefs.getInt("custom_fps", 60)
        set(v) = prefs.edit().putInt("custom_fps", v).apply()

    var customBitrateMbps: Int
        get() = prefs.getInt("custom_bitrate", 12)
        set(v) = prefs.edit().putInt("custom_bitrate", v).apply()

    var inputMode: InputMode
        get() = enumPref("input_mode", InputMode.TOUCH)
        set(v) = prefs.edit().putString("input_mode", v.name).apply()

    var clipboardSync: Boolean
        get() = prefs.getBoolean("clipboard", true)
        set(v) = prefs.edit().putBoolean("clipboard", v).apply()

    var showStats: Boolean
        get() = prefs.getBoolean("stats", false)
        set(v) = prefs.edit().putBoolean("stats", v).apply()

    var autoReconnect: Boolean
        get() = prefs.getBoolean("auto_reconnect", true)
        set(v) = prefs.edit().putBoolean("auto_reconnect", v).apply()

    var lastHost: String?
        get() = prefs.getString("last_host", null)
        set(v) = prefs.edit().putString("last_host", v).apply()

    var lastHostName: String?
        get() = prefs.getString("last_host_name", null)
        set(v) = prefs.edit().putString("last_host_name", v).apply()

    var lastPort: Int
        get() = prefs.getInt("last_port", Protocol.DEFAULT_PORT)
        set(v) = prefs.edit().putInt("last_port", v).apply()

    val fps: Int get() = if (quality == Quality.CUSTOM) customFps else quality.fps
    val bitrateMbps: Int get() = if (quality == Quality.CUSTOM) customBitrateMbps else quality.bitrateMbps

    /**
     * Size to ask the PC for: the decoder's best size, capped by the chosen
     * resolution, then divided by the scale. Keeps the screen's shape.
     */
    fun requestSize(decoderWidth: Int, decoderHeight: Int): Pair<Int, Int> {
        var w = decoderWidth.toFloat()
        var h = decoderHeight.toFloat()
        val cap = resolution.maxWidth
        if (w > cap) {
            h = h * cap / w
            w = cap.toFloat()
        }
        val s = scale.coerceIn(1f, 3f)
        w /= s
        h /= s
        return (w.toInt() and 1.inv()).coerceAtLeast(320) to (h.toInt() and 1.inv()).coerceAtLeast(240)
    }

    private inline fun <reified T : Enum<T>> enumPref(key: String, default: T): T =
        prefs.getString(key, null)?.let { name -> enumValues<T>().firstOrNull { it.name == name } } ?: default
}
