package io.github.hafijbge.secondmonitor

import android.graphics.Bitmap
import android.os.Build
import android.os.SystemClock
import android.util.Log
import android.view.Surface
import java.io.BufferedInputStream
import java.io.BufferedOutputStream
import java.io.DataInputStream
import java.io.DataOutputStream
import java.io.IOException
import java.io.InputStream
import java.net.InetSocketAddress
import java.net.Socket
import java.nio.ByteBuffer
import java.util.concurrent.CompletableFuture
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicInteger
import javax.net.ssl.SSLContext
import javax.net.ssl.SSLSocket
import kotlin.concurrent.thread

/** What the client asks the host for. */
data class StreamRequest(
    val width: Int,
    val height: Int,
    val dpi: Int,
    val fps: Int,
    val bitrateKbps: Int,
    val clipboard: Boolean,
    val decoderName: String?,
)

/** Pointer image with its hotspot, in host pixels. */
class CursorImage(val bitmap: Bitmap, val hotX: Int, val hotY: Int)

/**
 * Connects to the host, decodes the video onto [surface], and carries input,
 * clipboard and files both ways. Callbacks run on background threads.
 */
class StreamClient(
    private val host: String,
    private val port: Int,
    private val surface: Surface,
    private val request: StreamRequest,
    private val files: FileStore,
    private val trust: TrustStore,
    private val listener: Listener,
) {
    interface Listener {
        /**
         * First connection to this PC over WiFi: show [code] and ask the user
         * whether the PC shows the same one. Call [answer] with the result.
         */
        fun onVerifyPc(code: String, answer: (Boolean) -> Unit)
        /** The PC is asking its user to allow this tablet. */
        fun onWaitingForApproval(code: String)
        fun onConnected()
        fun onVideoSize(width: Int, height: Int)
        fun onFirstFrame()
        /** Pointer drawn by the app. [x]/[y] are 0..1 of the video; [image] only when it changed. */
        fun onCursor(visible: Boolean, x: Float, y: Float, sourceWidth: Int, image: CursorImage?)
        fun onClipboard(text: String)
        fun onTransfer(transfer: Transfer)
        fun onStats(fps: Float, mbps: Float)
        fun onClosed(error: String?)
    }

    @Volatile private var closed = false
    private var socket: Socket? = null
    @Volatile private var output: DataOutputStream? = null
    private val sender = Executors.newSingleThreadExecutor()
    private var decoder: VideoDecoder? = null
    private var videoWidth = 0
    private var videoHeight = 0
    private var sawFrame = false
    private val incoming = HashMap<Int, FileStore.Sink>()
    private val nextFileId = AtomicInteger(1)

    // Stats over the last second.
    private var statFrames = 0
    private var statBytes = 0L
    private var statSince = SystemClock.elapsedRealtime()

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
            incoming.values.forEach { it.abort() }
            incoming.clear()
            if (!closed) {
                closed = true
                sender.shutdownNow()
                listener.onClosed(error)
            }
        }
    }

    fun close() {
        closed = true
        runCatching { socket?.close() }
        sender.shutdownNow()
    }

    fun sendTouch(action: Int, pointerId: Int, x: Float, y: Float) = send { out ->
        out.writeByte(Protocol.MSG_TOUCH)
        out.writeInt(10)
        out.writeByte(action)
        out.writeByte(pointerId and 0xff)
        out.writeFloat(x)
        out.writeFloat(y)
    }

    fun sendMouseMoveTo(x: Float, y: Float) = sendMouse(Protocol.MOUSE_MOVE_TO, 9) { out ->
        out.writeFloat(x)
        out.writeFloat(y)
    }

    fun sendMouseMoveBy(dx: Float, dy: Float) = sendMouse(Protocol.MOUSE_MOVE_BY, 9) { out ->
        out.writeFloat(dx)
        out.writeFloat(dy)
    }

    fun sendMouseButton(button: Int, pressed: Boolean) = sendMouse(Protocol.MOUSE_BUTTON, 3) { out ->
        out.writeByte(button)
        out.writeByte(if (pressed) 1 else 0)
    }

    fun sendScroll(dx: Float, dy: Float, finish: Boolean) = sendMouse(Protocol.MOUSE_SCROLL, 10) { out ->
        out.writeFloat(dx)
        out.writeFloat(dy)
        out.writeByte(if (finish) 1 else 0)
    }

    fun sendKey(keysym: Int, pressed: Boolean) = send { out ->
        out.writeByte(Protocol.MSG_KEY)
        out.writeInt(5)
        out.writeInt(keysym)
        out.writeByte(if (pressed) 1 else 0)
    }

    /** Presses and releases [keysym] while holding [modifiers]. */
    fun tapKey(keysym: Int, modifiers: Collection<Int> = emptyList()) {
        modifiers.forEach { sendKey(it, true) }
        sendKey(keysym, true)
        sendKey(keysym, false)
        modifiers.reversed().forEach { sendKey(it, false) }
    }

    fun sendClipboard(text: String) = send { out ->
        val bytes = text.toByteArray(Charsets.UTF_8)
        out.writeByte(Protocol.MSG_CLIPBOARD)
        out.writeInt(bytes.size)
        out.write(bytes)
    }

    /** Streams a file to the host on a background thread. */
    fun sendFile(name: String, size: Long, open: () -> InputStream) {
        val id = nextFileId.getAndIncrement()
        thread(name = "file-send") {
            var transfer = Transfer(id, name, incoming = false, size = size, done = 0, state = TransferState.RUNNING)
            listener.onTransfer(transfer)
            val ok = try {
                open().use { input ->
                    val nameBytes = name.toByteArray(Charsets.UTF_8).take(1000).toByteArray()
                    sendBlocking { out ->
                        out.writeByte(Protocol.MSG_FILE_BEGIN)
                        out.writeInt(14 + nameBytes.size)
                        out.writeInt(id)
                        out.writeLong(size)
                        out.writeShort(nameBytes.size)
                        out.write(nameBytes)
                    }
                    val buf = ByteArray(Protocol.FILE_CHUNK)
                    var lastReport = 0L
                    while (true) {
                        val n = input.read(buf)
                        if (n < 0) break
                        if (n == 0) continue
                        // One chunk per write so touches and keyframe requests interleave.
                        sendBlocking { out ->
                            out.writeByte(Protocol.MSG_FILE_DATA)
                            out.writeInt(4 + n)
                            out.writeInt(id)
                            out.write(buf, 0, n)
                        }
                        transfer = transfer.copy(done = transfer.done + n)
                        val now = SystemClock.elapsedRealtime()
                        if (now - lastReport > 150) {
                            lastReport = now
                            listener.onTransfer(transfer)
                        }
                    }
                }
                true
            } catch (e: Exception) {
                Log.w(TAG, "sending $name failed", e)
                false
            }
            runCatching {
                sendBlocking { out ->
                    out.writeByte(Protocol.MSG_FILE_END)
                    out.writeInt(5)
                    out.writeInt(id)
                    out.writeByte(if (ok) 0 else 1)
                }
            }
            listener.onTransfer(transfer.copy(state = if (ok) TransferState.DONE else TransferState.FAILED))
        }
    }

    fun requestKeyframe() = send { out ->
        out.writeByte(Protocol.MSG_KEYFRAME)
        out.writeInt(0)
    }

    private fun sendMouse(op: Int, length: Int, body: (DataOutputStream) -> Unit) = send { out ->
        out.writeByte(Protocol.MSG_MOUSE)
        out.writeInt(length)
        out.writeByte(op)
        body(out)
    }

    /** Queues a small message; never blocks the caller (UI thread). */
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

    /** Writes on the calling (background) thread. */
    private fun sendBlocking(block: (DataOutputStream) -> Unit) {
        val out = output ?: throw IOException("not connected")
        if (closed) throw IOException("closed")
        synchronized(out) {
            block(out)
            out.flush()
        }
    }

    /** TLS to the PC; returns the socket and the PC's pairing code. */
    private fun secureConnect(): Pair<SSLSocket, String> {
        val raw = Socket()
        socket = raw
        raw.tcpNoDelay = true
        raw.connect(InetSocketAddress(host, port), 5000)
        val tm = CapturingTrustManager()
        val ctx = SSLContext.getInstance("TLS").apply { init(null, arrayOf(tm), null) }
        val s = ctx.socketFactory.createSocket(raw, host, port, true) as SSLSocket
        socket = s
        s.useClientMode = true
        s.soTimeout = 15_000
        s.startHandshake()
        val cert = tm.certificate ?: throw IOException("the PC sent no certificate")
        val fingerprint = sha256(cert.encoded)
        val fp = fingerprint.toHex()
        val code = pairingCode(fingerprint)
        if (!trust.isTrusted(fp)) {
            // A USB cable can't be intercepted; on WiFi the user compares codes.
            val viaUsb = host == "127.0.0.1"
            if (!viaUsb) {
                val answer = CompletableFuture<Boolean>()
                listener.onVerifyPc(code) { answer.complete(it) }
                val ok = runCatching { answer.get(3, TimeUnit.MINUTES) }.getOrDefault(false)
                if (!ok) throw IOException("cancelled: the PC's code wasn't confirmed")
            }
            pendingTrust = fp
        }
        return s to code
    }

    /** Fingerprint to remember once the PC accepts us. */
    private var pendingTrust: String? = null

    private fun run() {
        val (s, code) = secureConnect()
        val out = DataOutputStream(BufferedOutputStream(s.getOutputStream(), 1 shl 16))
        val input = DataInputStream(BufferedInputStream(s.getInputStream(), 1 shl 16))

        val name = "${Build.MANUFACTURER} ${Build.MODEL}".trim()
            .replaceFirstChar { it.uppercase() }
            .toByteArray(Charsets.UTF_8).take(60).toByteArray()
        var flags = Protocol.FLAG_CURSOR_OVERLAY
        if (request.clipboard) flags = flags or Protocol.FLAG_CLIPBOARD
        out.write(Protocol.MAGIC)
        out.writeByte(Protocol.VERSION)
        out.writeShort(request.width)
        out.writeShort(request.height)
        out.writeShort(request.dpi)
        out.writeByte(flags)
        out.writeByte(request.fps)
        out.writeInt(request.bitrateKbps)
        out.writeByte(name.size)
        out.write(name)
        out.write(trust.deviceId())
        out.flush()

        // The PC may ask its user first (new tablet on WiFi).
        s.soTimeout = 0
        while (true) {
            when (input.readUnsignedByte()) {
                Protocol.AUTH_OK -> break
                Protocol.AUTH_PENDING -> listener.onWaitingForApproval(code)
                else -> throw IOException("The PC didn't allow this tablet.")
            }
        }
        pendingTrust?.let { trust.trust(it, host) }

        val magic = ByteArray(4)
        input.readFully(magic)
        if (!magic.contentEquals(Protocol.MAGIC)) throw IOException("not an Extendo PC")
        val version = input.readUnsignedByte()
        if (version != Protocol.VERSION) {
            throw IOException("The PC app is out of date (protocol v$version). Update Extendo on the PC.")
        }
        setVideoSize(input.readUnsignedShort(), input.readUnsignedShort())
        val codec = input.readUnsignedByte()
        if (codec != Protocol.CODEC_H264) throw IOException("unsupported codec $codec")
        output = out
        listener.onConnected()

        while (!closed) {
            val type = input.readUnsignedByte()
            val length = input.readInt()
            if (length < 0 || length > 64 * 1024 * 1024) throw IOException("bad message length $length")
            statBytes += length + 5
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
                Protocol.MSG_CURSOR -> readCursor(input, length)
                Protocol.MSG_CLIPBOARD -> {
                    val bytes = ByteArray(length)
                    input.readFully(bytes)
                    listener.onClipboard(String(bytes, Charsets.UTF_8))
                }
                Protocol.MSG_FILE_BEGIN -> readFileBegin(input, length)
                Protocol.MSG_FILE_DATA -> {
                    val id = input.readInt()
                    val data = ByteArray(length - 4)
                    input.readFully(data)
                    incoming[id]?.let { sink ->
                        if (!sink.write(data)) {
                            incoming.remove(id)
                            listener.onTransfer(sink.transfer.copy(state = TransferState.FAILED))
                        } else if (sink.shouldReport()) {
                            listener.onTransfer(sink.transfer)
                        }
                    }
                }
                Protocol.MSG_FILE_END -> {
                    val id = input.readInt()
                    val ok = input.readUnsignedByte() == 0
                    input.skipBytes(length - 5)
                    incoming.remove(id)?.let { sink ->
                        val saved = if (ok) sink.finish() else { sink.abort(); false }
                        listener.onTransfer(sink.transfer.copy(state = if (saved) TransferState.DONE else TransferState.FAILED))
                    }
                }
                else -> input.skipBytes(length)
            }
            reportStats()
        }
    }

    private fun readFileBegin(input: DataInputStream, length: Int) {
        val id = input.readInt()
        val size = input.readLong()
        val nameLen = input.readUnsignedShort()
        val nameBytes = ByteArray(nameLen)
        input.readFully(nameBytes)
        input.skipBytes(length - 14 - nameLen)
        val name = String(nameBytes, Charsets.UTF_8)
        val sink = files.create(id, name, size)
        if (sink == null) {
            listener.onTransfer(Transfer(id, name, incoming = true, size = size, done = 0, state = TransferState.FAILED))
            return
        }
        incoming[id] = sink
        listener.onTransfer(sink.transfer)
    }

    private fun readCursor(input: DataInputStream, length: Int) {
        val flags = input.readUnsignedByte()
        val x = input.readFloat()
        val y = input.readFloat()
        val sourceWidth = input.readUnsignedShort()
        var consumed = 11
        var image: CursorImage? = null
        if (flags and 2 != 0) {
            val w = input.readUnsignedShort()
            val h = input.readUnsignedShort()
            val hotX = input.readUnsignedShort()
            val hotY = input.readUnsignedShort()
            val pixels = ByteArray(w * h * 4)
            input.readFully(pixels)
            consumed += 8 + pixels.size
            if (w > 0 && h > 0) {
                // Host sends premultiplied RGBA, which is ARGB_8888's memory layout.
                val bitmap = Bitmap.createBitmap(w, h, Bitmap.Config.ARGB_8888)
                bitmap.copyPixelsFromBuffer(ByteBuffer.wrap(pixels))
                image = CursorImage(bitmap, hotX, hotY)
            }
        }
        input.skipBytes(length - consumed)
        listener.onCursor(flags and 1 != 0, x, y, sourceWidth, image)
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
                request.decoderName, surface, videoWidth, videoHeight,
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
        statFrames++
        if (!sawFrame) {
            sawFrame = true
            listener.onFirstFrame()
        }
    }

    private fun reportStats() {
        val now = SystemClock.elapsedRealtime()
        val elapsed = now - statSince
        if (elapsed < 1000) return
        listener.onStats(statFrames * 1000f / elapsed, statBytes * 8f / elapsed / 1000f)
        statFrames = 0
        statBytes = 0
        statSince = now
    }

    companion object {
        private const val TAG = "StreamClient"
    }
}
