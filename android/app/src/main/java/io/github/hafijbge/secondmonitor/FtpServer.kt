package io.github.hafijbge.secondmonitor

import android.util.Log
import java.io.BufferedReader
import java.io.File
import java.io.FileInputStream
import java.io.FileOutputStream
import java.io.IOException
import java.io.InputStreamReader
import java.io.OutputStream
import java.net.Inet4Address
import java.net.InetAddress
import java.net.ServerSocket
import java.net.Socket
import java.net.SocketTimeoutException
import java.text.SimpleDateFormat
import java.util.Calendar
import java.util.Collections
import java.util.Date
import java.util.Locale
import java.util.TimeZone
import kotlin.concurrent.thread

/**
 * Small passive-mode FTP server rooted at [root]. Enough for file managers
 * (GNOME Files, Windows Explorer, FileZilla): browse, download, upload,
 * rename, delete, make folders, resume.
 */
class FtpServer(
    private val root: File,
    private val user: String,
    private val password: String,
    val port: Int = PORT,
) {
    private var server: ServerSocket? = null
    private val clients = Collections.synchronizedSet(mutableSetOf<Socket>())
    @Volatile private var running = false

    fun start() {
        val s = ServerSocket()
        s.reuseAddress = true
        s.bind(java.net.InetSocketAddress(port))
        server = s
        running = true
        thread(name = "ftp-accept", isDaemon = true) {
            while (running) {
                val client = try {
                    s.accept()
                } catch (e: IOException) {
                    break
                }
                clients.add(client)
                thread(name = "ftp-session", isDaemon = true) {
                    try {
                        Session(client).run()
                    } catch (e: Exception) {
                        Log.d(TAG, "session ended", e)
                    } finally {
                        clients.remove(client)
                        runCatching { client.close() }
                    }
                }
            }
        }
    }

    fun stop() {
        running = false
        runCatching { server?.close() }
        synchronized(clients) { clients.forEach { runCatching { it.close() } } }
        clients.clear()
    }

    private inner class Session(private val socket: Socket) {
        private val reader = BufferedReader(InputStreamReader(socket.getInputStream(), Charsets.UTF_8))
        private val out: OutputStream = socket.getOutputStream()
        private var userOk = false
        private var loggedIn = false
        private var cwd = "/"
        private var passive: ServerSocket? = null
        private var renameFrom: File? = null
        private var restart = 0L

        private fun reply(text: String) {
            out.write("$text\r\n".toByteArray(Charsets.UTF_8))
            out.flush()
        }

        fun run() {
            socket.soTimeout = IDLE_TIMEOUT_MS
            reply("220 Extendo phone storage ready")
            while (running) {
                val line = try {
                    reader.readLine() ?: break
                } catch (e: SocketTimeoutException) {
                    reply("421 Idle timeout")
                    break
                }
                val verb = line.substringBefore(' ').uppercase(Locale.US)
                val arg = line.substringAfter(' ', "").trim()
                if (!handle(verb, arg)) break
            }
            closePassive()
        }

        /** Returns false to end the session. */
        private fun handle(verb: String, arg: String): Boolean {
            val open = verb in setOf("USER", "PASS", "QUIT", "FEAT", "SYST", "OPTS", "AUTH", "NOOP")
            if (!loggedIn && !open) {
                reply("530 Please log in")
                return true
            }
            when (verb) {
                "USER" -> {
                    userOk = arg == user
                    reply("331 Password required")
                }
                "PASS" -> if (userOk && arg == password) {
                    loggedIn = true
                    reply("230 Logged in")
                } else {
                    Thread.sleep(1000) // slow down guessing
                    reply("530 Login incorrect")
                }
                "AUTH" -> reply("502 TLS not supported")
                "SYST" -> reply("215 UNIX Type: L8")
                "FEAT" -> {
                    reply("211-Features:")
                    listOf("UTF8", "MLST type*;size*;modify*;", "SIZE", "MDTM", "REST STREAM", "EPSV", "PASV")
                        .forEach { reply(" $it") }
                    reply("211 End")
                }
                "OPTS" -> reply("200 OK")
                "NOOP", "MODE", "STRU", "ALLO" -> reply("200 OK")
                "TYPE" -> reply("200 Type set")
                "PWD", "XPWD" -> reply("257 \"$cwd\" is the current directory")
                "CWD", "XCWD" -> {
                    val target = virtual(arg)
                    if (resolve(target)?.isDirectory == true) {
                        cwd = target
                        reply("250 OK")
                    } else reply("550 No such folder")
                }
                "CDUP", "XCUP" -> {
                    cwd = virtual("..")
                    reply("250 OK")
                }
                "PASV" -> pasv()
                "EPSV" -> epsv()
                "PORT", "EPRT" -> reply("502 Use passive mode")
                "LIST" -> list(arg, detailed = true)
                "NLST" -> list(arg, detailed = false)
                "MLSD" -> mlsd(arg)
                "MLST" -> {
                    val f = resolve(virtual(arg.ifEmpty { "." }))
                    if (f == null || !f.exists()) reply("550 Not found") else {
                        reply("250-Listing")
                        reply(" ${facts(f)} ${virtual(arg.ifEmpty { "." })}")
                        reply("250 End")
                    }
                }
                "SIZE" -> {
                    val f = resolve(virtual(arg))
                    if (f?.isFile == true) reply("213 ${f.length()}") else reply("550 Not a file")
                }
                "MDTM" -> {
                    val f = resolve(virtual(arg))
                    if (f?.exists() == true) reply("213 ${stamp(f.lastModified())}") else reply("550 Not found")
                }
                "REST" -> {
                    restart = arg.toLongOrNull() ?: 0
                    reply("350 Restarting at $restart")
                }
                "RETR" -> retr(arg)
                "STOR" -> stor(arg, append = false)
                "APPE" -> stor(arg, append = true)
                "DELE" -> {
                    val f = resolve(virtual(arg))
                    if (f?.isFile == true && f.delete()) reply("250 Deleted") else reply("550 Cannot delete")
                }
                "MKD", "XMKD" -> {
                    val path = virtual(arg)
                    val f = resolve(path)
                    if (f != null && (f.isDirectory || f.mkdirs())) reply("257 \"$path\" created") else reply("550 Cannot create folder")
                }
                "RMD", "XRMD" -> {
                    val f = resolve(virtual(arg))
                    if (f != null && f != root && f.isDirectory && f.delete()) reply("250 Removed") else reply("550 Folder not empty or missing")
                }
                "RNFR" -> {
                    val f = resolve(virtual(arg))
                    if (f?.exists() == true) {
                        renameFrom = f
                        reply("350 Ready")
                    } else reply("550 Not found")
                }
                "RNTO" -> {
                    val from = renameFrom
                    val to = resolve(virtual(arg))
                    renameFrom = null
                    if (from != null && to != null && from.renameTo(to)) reply("250 Renamed") else reply("553 Cannot rename")
                }
                "ABOR" -> {
                    closePassive()
                    reply("226 Aborted")
                }
                "QUIT" -> {
                    reply("221 Goodbye")
                    return false
                }
                else -> reply("502 Not implemented")
            }
            return true
        }

        /** Absolute virtual path ("/DCIM/Camera") for an argument relative to cwd. */
        private fun virtual(arg: String): String {
            val raw = if (arg.startsWith("/")) arg else "$cwd/$arg"
            val parts = ArrayDeque<String>()
            raw.split('/').forEach { p ->
                when (p) {
                    "", "." -> Unit
                    ".." -> parts.removeLastOrNull()
                    else -> parts.addLast(p)
                }
            }
            return "/" + parts.joinToString("/")
        }

        /** Real file for a virtual path, never outside [root]. */
        private fun resolve(path: String): File? {
            val f = File(root, path.removePrefix("/"))
            val rootPath = root.canonicalPath
            val canonical = runCatching { f.canonicalPath }.getOrNull() ?: return null
            return if (canonical == rootPath || canonical.startsWith("$rootPath/")) f else null
        }

        private fun pasv() {
            val local = socket.localAddress
            val v4 = (local as? Inet4Address) ?: InetAddress.getByName("127.0.0.1") as Inet4Address
            val p = openPassive(local) ?: return reply("425 No free data port")
            val h = v4.hostAddress!!.replace('.', ',')
            reply("227 Entering Passive Mode ($h,${p shr 8},${p and 0xff})")
        }

        private fun epsv() {
            val p = openPassive(socket.localAddress) ?: return reply("425 No free data port")
            reply("229 Entering Extended Passive Mode (|||$p|)")
        }

        private fun openPassive(local: InetAddress): Int? {
            closePassive()
            for (p in PASSIVE_PORTS) {
                try {
                    val s = ServerSocket()
                    s.reuseAddress = true
                    s.bind(java.net.InetSocketAddress(local, p))
                    s.soTimeout = 20_000
                    passive = s
                    return p
                } catch (e: IOException) {
                    continue
                }
            }
            return null
        }

        private fun closePassive() {
            runCatching { passive?.close() }
            passive = null
        }

        private inline fun withData(block: (Socket) -> Unit) {
            val p = passive ?: return reply("425 Use PASV first")
            reply("150 Opening data connection")
            try {
                p.accept().use { block(it) }
                reply("226 Transfer complete")
            } catch (e: IOException) {
                Log.d(TAG, "data transfer failed", e)
                reply("426 Transfer failed")
            } finally {
                closePassive()
            }
        }

        private fun listTarget(arg: String): File? {
            // Clients send flags like "-la"; ignore them.
            val path = arg.split(' ').filterNot { it.startsWith("-") }.joinToString(" ")
            return resolve(virtual(path.ifEmpty { "." }))
        }

        private fun list(arg: String, detailed: Boolean) {
            val dir = listTarget(arg) ?: return reply("550 Not found")
            val entries = if (dir.isDirectory) dir.listFiles()?.sortedBy { it.name.lowercase() } ?: emptyList() else listOf(dir)
            withData { data ->
                val w = data.getOutputStream().bufferedWriter(Charsets.UTF_8)
                entries.forEach { f -> w.write((if (detailed) lsLine(f) else f.name) + "\r\n") }
                w.flush()
            }
        }

        private fun mlsd(arg: String) {
            val dir = listTarget(arg) ?: return reply("550 Not found")
            if (!dir.isDirectory) return reply("501 Not a folder")
            val entries = dir.listFiles()?.sortedBy { it.name.lowercase() } ?: emptyList()
            withData { data ->
                val w = data.getOutputStream().bufferedWriter(Charsets.UTF_8)
                entries.forEach { f -> w.write("${facts(f)} ${f.name}\r\n") }
                w.flush()
            }
        }

        private fun retr(arg: String) {
            val f = resolve(virtual(arg))
            if (f == null || !f.isFile) {
                restart = 0
                return reply("550 Not a file")
            }
            val offset = restart
            restart = 0
            withData { data ->
                FileInputStream(f).use { input ->
                    if (offset > 0) input.channel.position(offset)
                    input.copyTo(data.getOutputStream(), BUFFER)
                }
            }
        }

        private fun stor(arg: String, append: Boolean) {
            val f = resolve(virtual(arg)) ?: return reply("553 Not allowed")
            f.parentFile?.mkdirs()
            val offset = restart
            restart = 0
            withData { data ->
                val stream = if (offset > 0 && f.exists()) {
                    java.io.RandomAccessFile(f, "rw").apply { setLength(offset); seek(offset) }.let { raf ->
                        object : OutputStream() {
                            override fun write(b: Int) = raf.write(b)
                            override fun write(b: ByteArray, off: Int, len: Int) = raf.write(b, off, len)
                            override fun close() = raf.close()
                        }
                    }
                } else FileOutputStream(f, append)
                stream.use { data.getInputStream().copyTo(it, BUFFER) }
            }
        }
    }

    companion object {
        private const val TAG = "FtpServer"
        const val PORT = 2121
        /** Kept small and fixed so the PC can forward them all over USB. */
        val PASSIVE_PORTS = 2122..2129
        private const val BUFFER = 256 * 1024
        private const val IDLE_TIMEOUT_MS = 10 * 60 * 1000

        private fun stamp(time: Long): String = SimpleDateFormat("yyyyMMddHHmmss", Locale.US)
            .apply { timeZone = TimeZone.getTimeZone("UTC") }
            .format(Date(time))

        private fun facts(f: File): String {
            val type = if (f.isDirectory) "dir" else "file"
            val size = if (f.isFile) "size=${f.length()};" else ""
            return "type=$type;${size}modify=${stamp(f.lastModified())};"
        }

        private fun lsLine(f: File): String {
            val date = Date(f.lastModified())
            val thisYear = Calendar.getInstance().get(Calendar.YEAR)
            val year = Calendar.getInstance().apply { time = date }.get(Calendar.YEAR)
            val when_ = SimpleDateFormat(if (year == thisYear) "MMM dd HH:mm" else "MMM dd  yyyy", Locale.US).format(date)
            val perms = if (f.isDirectory) "drwxr-xr-x" else "-rw-r--r--"
            val size = if (f.isFile) f.length() else 4096
            return "$perms 1 phone phone %13d %s %s".format(Locale.US, size, when_, f.name)
        }
    }
}
