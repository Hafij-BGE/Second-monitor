package io.github.hafijbge.secondmonitor

import android.media.MediaCodec
import android.media.MediaFormat
import android.os.Build
import android.util.Log
import android.view.Surface
import java.nio.ByteBuffer
import kotlin.concurrent.thread

/** Hardware H.264 decoder rendering straight to a Surface. */
class VideoDecoder(
    codecName: String?,
    surface: Surface,
    width: Int,
    height: Int,
    sps: ByteArray,
    pps: ByteArray,
) {
    private val codec: MediaCodec = codecName?.let { MediaCodec.createByCodecName(it) }
        ?: MediaCodec.createDecoderByType(MediaFormat.MIMETYPE_VIDEO_AVC)
    @Volatile private var running = true
    private val outputThread: Thread

    init {
        val format = MediaFormat.createVideoFormat(MediaFormat.MIMETYPE_VIDEO_AVC, width, height).apply {
            setByteBuffer("csd-0", ByteBuffer.wrap(sps))
            setByteBuffer("csd-1", ByteBuffer.wrap(pps))
            setInteger(MediaFormat.KEY_MAX_INPUT_SIZE, width * height)
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
                setInteger(MediaFormat.KEY_LOW_LATENCY, 1)
            }
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.M) {
                setInteger(MediaFormat.KEY_PRIORITY, 0) // realtime
            }
        }
        try {
            codec.configure(format, surface, null, 0)
            codec.start()
        } catch (e: Exception) {
            codec.release()
            throw IllegalStateException("video decoder rejected ${width}x$height: ${e.message}", e)
        }
        outputThread = thread(name = "decoder-output") { drainOutput() }
    }

    /** Queues one access unit. Returns false if the decoder had no free buffer. */
    fun decode(data: ByteArray, ptsUs: Long): Boolean {
        val index = codec.dequeueInputBuffer(200_000)
        if (index < 0) return false
        val buffer = codec.getInputBuffer(index) ?: return false
        buffer.clear()
        if (data.size > buffer.capacity()) {
            codec.queueInputBuffer(index, 0, 0, ptsUs, 0)
            return false
        }
        buffer.put(data)
        codec.queueInputBuffer(index, 0, data.size, ptsUs, 0)
        return true
    }

    private fun drainOutput() {
        val info = MediaCodec.BufferInfo()
        while (running) {
            val index = try {
                codec.dequeueOutputBuffer(info, 10_000)
            } catch (e: IllegalStateException) {
                break
            }
            if (index >= 0) {
                // Render immediately: lowest latency, no A/V sync needed.
                codec.releaseOutputBuffer(index, true)
            }
        }
    }

    fun release() {
        running = false
        runCatching { outputThread.join(500) }
        runCatching { codec.stop() }.onFailure { Log.w(TAG, "stop", it) }
        runCatching { codec.release() }
    }

    companion object {
        private const val TAG = "VideoDecoder"

        /** Splits an Annex B buffer into NAL units (without start codes). */
        fun nalUnits(data: ByteArray): List<ByteArray> {
            val starts = mutableListOf<Int>() // index of first byte after each start code
            var i = 0
            while (i + 2 < data.size) {
                if (data[i].toInt() == 0 && data[i + 1].toInt() == 0 && data[i + 2].toInt() == 1) {
                    starts.add(i + 3)
                    i += 3
                } else {
                    i++
                }
            }
            return starts.mapIndexed { n, start ->
                var end = if (n + 1 < starts.size) starts[n + 1] - 3 else data.size
                // A 4-byte start code leaves a trailing zero on the previous NAL.
                while (end > start && n + 1 < starts.size && data[end - 1].toInt() == 0) end--
                data.copyOfRange(start, end)
            }
        }

        fun withStartCode(nal: ByteArray): ByteArray = byteArrayOf(0, 0, 0, 1) + nal
    }
}
