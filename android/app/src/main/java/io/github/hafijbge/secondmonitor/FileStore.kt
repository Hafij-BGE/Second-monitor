package io.github.hafijbge.secondmonitor

import android.content.ContentValues
import android.content.Context
import android.net.Uri
import android.os.Build
import android.os.Environment
import android.os.SystemClock
import android.provider.MediaStore
import android.provider.OpenableColumns
import android.util.Log
import java.io.File
import java.io.OutputStream

enum class TransferState { RUNNING, DONE, FAILED }

data class Transfer(
    val id: Int,
    val name: String,
    /** true = PC to this device. */
    val incoming: Boolean,
    val size: Long,
    val done: Long,
    val state: TransferState,
) {
    val progress: Float get() = if (size > 0) (done.toFloat() / size).coerceIn(0f, 1f) else 0f
}

/** Saves files from the PC into Download/Extendo. */
class FileStore(context: Context) {
    private val app = context.applicationContext

    /** An open incoming file. */
    inner class Sink(
        private val stream: OutputStream,
        private val uri: Uri?,
        private val file: File?,
        var transfer: Transfer,
    ) {
        private var lastReport = 0L

        fun write(data: ByteArray): Boolean = try {
            stream.write(data)
            transfer = transfer.copy(done = transfer.done + data.size)
            true
        } catch (e: Exception) {
            Log.w(TAG, "write failed", e)
            abort()
            false
        }

        fun shouldReport(): Boolean {
            val now = SystemClock.elapsedRealtime()
            if (now - lastReport < 150) return false
            lastReport = now
            return true
        }

        fun finish(): Boolean = try {
            stream.close()
            if (uri != null && Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
                // Make the file visible to other apps.
                app.contentResolver.update(uri, ContentValues().apply {
                    put(MediaStore.Downloads.IS_PENDING, 0)
                }, null, null)
            }
            transfer = transfer.copy(state = TransferState.DONE)
            true
        } catch (e: Exception) {
            Log.w(TAG, "finish failed", e)
            abort()
            false
        }

        fun abort() {
            runCatching { stream.close() }
            uri?.let { runCatching { app.contentResolver.delete(it, null, null) } }
            file?.delete()
        }
    }

    fun create(id: Int, name: String, size: Long): Sink? {
        val safe = name.substringAfterLast('/').substringAfterLast('\\')
            .replace(Regex("[\\x00-\\x1f<>:\"|?*]"), "_").trim().trimStart('.').ifEmpty { "file" }
        val transfer = Transfer(id, safe, incoming = true, size = size, done = 0, state = TransferState.RUNNING)
        return try {
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
                val values = ContentValues().apply {
                    put(MediaStore.Downloads.DISPLAY_NAME, safe)
                    put(MediaStore.Downloads.RELATIVE_PATH, "${Environment.DIRECTORY_DOWNLOADS}/$FOLDER")
                    put(MediaStore.Downloads.IS_PENDING, 1)
                }
                val uri = app.contentResolver.insert(MediaStore.Downloads.EXTERNAL_CONTENT_URI, values)
                    ?: return null
                val stream = app.contentResolver.openOutputStream(uri) ?: return null
                Sink(stream.buffered(1 shl 16), uri, null, transfer)
            } else {
                // Android 7-9: app folder, no storage permission needed.
                val dir = File(app.getExternalFilesDir(Environment.DIRECTORY_DOWNLOADS), FOLDER).apply { mkdirs() }
                var file = File(dir, safe)
                var n = 2
                while (file.exists()) file = File(dir, "${safe.substringBeforeLast('.')} (${n++})" +
                    (if ('.' in safe) "." + safe.substringAfterLast('.') else ""))
                Sink(file.outputStream().buffered(1 shl 16), null, file, transfer)
            }
        } catch (e: Exception) {
            Log.w(TAG, "cannot save $name", e)
            null
        }
    }

    companion object {
        private const val TAG = "FileStore"
        const val FOLDER = "Extendo"

        /** Display name and size of a picked or shared document. */
        fun describe(context: Context, uri: Uri): Pair<String, Long> {
            var name = uri.lastPathSegment ?: "file"
            var size = -1L
            runCatching {
                context.contentResolver.query(uri, null, null, null, null)?.use { c ->
                    if (c.moveToFirst()) {
                        val n = c.getColumnIndex(OpenableColumns.DISPLAY_NAME)
                        val s = c.getColumnIndex(OpenableColumns.SIZE)
                        if (n >= 0) c.getString(n)?.let { name = it }
                        if (s >= 0 && !c.isNull(s)) size = c.getLong(s)
                    }
                }
            }
            return name to size
        }
    }
}
