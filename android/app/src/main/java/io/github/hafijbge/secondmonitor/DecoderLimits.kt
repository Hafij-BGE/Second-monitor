package io.github.hafijbge.secondmonitor

import android.media.MediaCodecInfo
import android.media.MediaCodecList
import android.media.MediaFormat
import android.os.Build

/** The H.264 decoder we will use and the largest stream size it can play. */
data class DecoderChoice(val codecName: String?, val width: Int, val height: Int)

object DecoderLimits {
    /**
     * Picks a hardware H.264 decoder and shrinks [width]x[height] (keeping the
     * aspect ratio) until that decoder supports it. Many phone decoders stop
     * at 1920x1088 even when the screen is bigger.
     */
    fun choose(width: Int, height: Int, fps: Double = 60.0): DecoderChoice {
        val decoders = MediaCodecList(MediaCodecList.REGULAR_CODECS).codecInfos.filter { info ->
            !info.isEncoder && info.supportedTypes.any { it.equals(MediaFormat.MIMETYPE_VIDEO_AVC, true) }
        }
        val info = decoders.firstOrNull { isHardware(it) } ?: decoders.firstOrNull()
            ?: return DecoderChoice(null, width, height)
        val caps = info.getCapabilitiesForType(MediaFormat.MIMETYPE_VIDEO_AVC).videoCapabilities
            ?: return DecoderChoice(info.name, width, height)

        val wAlign = caps.widthAlignment.coerceAtLeast(2)
        val hAlign = caps.heightAlignment.coerceAtLeast(2)
        var scale = 1.0
        while (scale >= 0.25) {
            val w = alignDown((width * scale).toInt(), wAlign)
            val h = alignDown((height * scale).toInt(), hAlign)
            val ok = runCatching {
                caps.areSizeAndRateSupported(w, h, fps) || caps.isSizeSupported(w, h)
            }.getOrDefault(false)
            if (ok) return DecoderChoice(info.name, w, h)
            scale -= 0.05
        }
        return DecoderChoice(info.name, 1280, 720)
    }

    private fun alignDown(v: Int, align: Int) = (v / align) * align

    private fun isHardware(info: MediaCodecInfo): Boolean {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) return info.isHardwareAccelerated
        val name = info.name.lowercase()
        return !name.startsWith("omx.google.") && !name.startsWith("c2.android.")
    }
}
