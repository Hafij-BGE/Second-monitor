package io.github.hafijbge.secondmonitor

import android.util.Log
import android.view.Surface
import java.io.BufferedInputStream
import java.io.BufferedOutputStream
import java.io.DataInputStream
import java.io.DataOutputStream
import java.io.IOException
import java.net.InetSocketAddress
import java.net.Socket
import java.util.concurrent.Executors
import kotlin.concurrent.thread

/**
 * Connects to the host, decodes the video onto [surface] and sends touches back.
 * Callbacks run on background threads.
 */
class StreamClient(
    private val host: String,
    private val port: Int,
    private val surface: Surface,
    private val decoderName: String?,
    private val screenWidth: Int,
    private val screenHeight: Int,
    private val dpi: Int,
    private val listener: Listener,
) {
    interface Listener {
        fun onVideoSize(width: Int, height: Int)
        fun onFirstFrame()
        fun onClosed(error: String?)
    }

    @Volatile private var closed = false
    private var socket: Socket? = null
    private var output: DataOutputStream? = null
    private val sender = Executors.newSingleThreadExecutor()
    private var decoder: VideoDecoder? = null
    private var videoWidth = 0
    private var videoHeight = 0
    private var sawFrame = false

    fun start() {
        thread(name = "stream-client") {
            val error = try {
                run()
                null
            } catch (e: Exception) {
                if (closed) null else (e.message ?: e.javaClass.simpleName)
            }
            decoder?.release()
            decoder = null
            if (!closed) {
                closed = true
                listener.onClosed(error)
            }
        }
    }

    fun close() {
        closed = true
        runCatching { socket?.close() }
        sender.shutdownNow()
    }

    fun sendTouch(action: Int, pointerId: Int, x: Float, y: Float) {
        send { out ->
            out.writeByte(Protocol.MSG_TOUCH)
            out.writeInt(10)
            out.writeByte(action)
            out.writeByte(pointerId and 0xff)
            out.writeFloat(x)
            out.writeFloat(y)
        }
    }

    private fun requestKeyframe() {
        send { out ->
            out.writeByte(Protocol.MSG_KEYFRAME)
            out.writeInt(0)
        }
    }

    private fun send(block: (DataOutputStream) -> Unit) {
        val out = output ?: return
        if (closed) return
        runCatching {
            sender.execute {
                try {
                    synchronized(out) {
                        block(out)
                        out.flush()
                    }
                } catch (e: IOException) {
                    Log.d(TAG, "send failed", e)
                }
            }
        }
    }

    private fun run() {
        val s = Socket()
        socket = s
        s.tcpNoDelay = true
        s.connect(InetSocketAddress(host, port), 5000)
        val out = DataOutputStream(BufferedOutputStream(s.getOutputStream()))
        val input = DataInputStream(BufferedInputStream(s.getInputStream(), 1 shl 16))

        out.write(Protocol.MAGIC)
        out.writeByte(Protocol.VERSION)
        out.writeShort(screenWidth)
        out.writeShort(screenHeight)
        out.writeShort(dpi)
        out.flush()
        output = out

        val magic = ByteArray(4)
        input.readFully(magic)
        if (!magic.contentEquals(Protocol.MAGIC)) throw IOException("not a Second Monitor host")
        val version = input.readUnsignedByte()
        if (version != Protocol.VERSION) throw IOException("host protocol v$version, app v${Protocol.VERSION}")
        setVideoSize(input.readUnsignedShort(), input.readUnsignedShort())
        val codec = input.readUnsignedByte()
        if (codec != Protocol.CODEC_H264) throw IOException("unsupported codec $codec")

        while (!closed) {
            val type = input.readUnsignedByte()
            val length = input.readInt()
            if (length < 0 || length > 64 * 1024 * 1024) throw IOException("bad message length $length")
            when (type) {
                Protocol.MSG_VIDEO -> {
                    val pts = input.readLong()
                    val data = ByteArray(length - 8)
                    input.readFully(data)
                    onVideo(data, pts)
                }
                Protocol.MSG_RESIZE -> {
                    val w = input.readUnsignedShort()
                    val h = input.readUnsignedShort()
                    input.skipBytes(length - 4)
                    decoder?.release()
                    decoder = null
                    setVideoSize(w, h)
                }
                else -> input.skipBytes(length)
            }
        }
    }

    private fun setVideoSize(w: Int, h: Int) {
        videoWidth = w
        videoHeight = h
        listener.onVideoSize(w, h)
    }

    private fun onVideo(data: ByteArray, ptsUs: Long) {
        var dec = decoder
        if (dec == null) {
            // Wait for a keyframe that carries SPS/PPS, then build the decoder.
            val nals = VideoDecoder.nalUnits(data)
            val sps = nals.firstOrNull { it.isNotEmpty() && (it[0].toInt() and 0x1f) == 7 }
            val pps = nals.firstOrNull { it.isNotEmpty() && (it[0].toInt() and 0x1f) == 8 }
            if (sps == null || pps == null) {
                requestKeyframe()
                return
            }
            dec = VideoDecoder(
                decoderName, surface, videoWidth, videoHeight,
                VideoDecoder.withStartCode(sps), VideoDecoder.withStartCode(pps),
            )
            decoder = dec
        }
        val ok = try {
            dec.decode(data, ptsUs)
        } catch (e: IllegalStateException) {
            Log.w(TAG, "decoder failed, restarting", e)
            dec.release()
            decoder = null
            false
        }
        if (!ok) {
            requestKeyframe()
            return
        }
        if (!sawFrame) {
            sawFrame = true
            listener.onFirstFrame()
        }
    }

    companion object {
        private const val TAG = "StreamClient"
    }
}
