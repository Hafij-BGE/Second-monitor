package io.github.hafijbge.secondmonitor

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.net.wifi.WifiManager
import android.os.Build
import android.os.Environment
import android.os.IBinder
import android.os.PowerManager
import android.util.Log
import androidx.compose.runtime.mutableStateOf
import java.net.DatagramPacket
import java.net.DatagramSocket
import java.net.Inet4Address
import java.net.NetworkInterface
import kotlin.concurrent.thread

/** Live state of phone storage sharing, shown by the home screen. */
object StorageState {
    val running = mutableStateOf(false)
    val error = mutableStateOf<String?>(null)
    val addresses = mutableStateOf<List<String>>(emptyList())

    const val USER = "extendo"

    private fun prefs(context: Context) = context.getSharedPreferences("storage", Context.MODE_PRIVATE)

    /**
     * An 8-digit password, kept until renewed. Safe with the login lockout:
     * 5 wrong tries lock logins for 5 minutes.
     */
    fun password(context: Context): String =
        prefs(context).getString("password", null)
            ?.takeIf { it.length == 8 && it.all(Char::isDigit) } ?: newPassword(context)

    fun newPassword(context: Context): String {
        val random = java.security.SecureRandom()
        val p = (1..8).map { '0' + random.nextInt(10) }.joinToString("")
        prefs(context).edit().putString("password", p).apply()
        return p
    }

    /** false = USB only: the server listens on localhost (adb forward). */
    fun wifiAccess(context: Context) = prefs(context).getBoolean("wifi", true)
    fun setWifiAccess(context: Context, on: Boolean) = prefs(context).edit().putBoolean("wifi", on).apply()

    /** Stop after [IDLE_MINUTES] without use. */
    fun autoStop(context: Context) = prefs(context).getBoolean("auto_stop", true)
    fun setAutoStop(context: Context, on: Boolean) = prefs(context).edit().putBoolean("auto_stop", on).apply()

    const val IDLE_MINUTES = 15

    fun localAddresses(): List<String> = runCatching {
        NetworkInterface.getNetworkInterfaces().toList()
            .filter { it.isUp && !it.isLoopback }
            .flatMap { it.inetAddresses.toList() }
            .filterIsInstance<Inet4Address>()
            .map { it.hostAddress!! }
    }.getOrDefault(emptyList())
}

/**
 * Runs the FTP server in the foreground so it keeps working while the
 * phone is used for other things, and answers the PC's search.
 */
class StorageService : Service() {
    private var server: FtpServer? = null
    private var discovery: DatagramSocket? = null
    private var wifiLock: WifiManager.WifiLock? = null
    private var wakeLock: PowerManager.WakeLock? = null

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        if (intent?.action == ACTION_STOP) {
            stopSelf()
            return START_NOT_STICKY
        }
        if (server != null) return START_STICKY
        startForegroundCompat()

        @Suppress("DEPRECATION")
        val root = Environment.getExternalStorageDirectory()
        val wifi = StorageState.wifiAccess(this)
        val bind = if (wifi) null else java.net.InetAddress.getLoopbackAddress()
        val s = FtpServer(root, StorageState.USER, StorageState.password(this), bind)
        try {
            s.start()
        } catch (e: Exception) {
            Log.w(TAG, "cannot start FTP server", e)
            StorageState.error.value = "Port ${FtpServer.PORT} is busy: ${e.message}"
            stopSelf()
            return START_NOT_STICKY
        }
        server = s
        StorageState.error.value = null
        StorageState.addresses.value = StorageState.localAddresses()
        StorageState.running.value = true
        holdLocks()
        if (wifi) answerDiscovery()
        if (StorageState.autoStop(this)) stopWhenIdle(s)
        return START_STICKY
    }

    override fun onDestroy() {
        server?.stop()
        server = null
        runCatching { discovery?.close() }
        wifiLock?.takeIf { it.isHeld }?.release()
        wakeLock?.takeIf { it.isHeld }?.release()
        StorageState.running.value = false
        super.onDestroy()
    }

    @Suppress("DEPRECATION")
    private fun holdLocks() {
        val wifi = applicationContext.getSystemService(Context.WIFI_SERVICE) as WifiManager
        wifiLock = wifi.createWifiLock(WifiManager.WIFI_MODE_FULL_HIGH_PERF, "extendo:storage").apply { acquire() }
        val power = getSystemService(Context.POWER_SERVICE) as PowerManager
        wakeLock = power.newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, "extendo:storage").apply { acquire() }
    }

    private fun stopWhenIdle(s: FtpServer) {
        thread(name = "storage-idle", isDaemon = true) {
            val limit = StorageState.IDLE_MINUTES * 60_000L
            while (server === s) {
                Thread.sleep(30_000)
                if (server !== s) break
                if (s.activeSessions == 0 && System.currentTimeMillis() - s.lastActivity > limit) {
                    Log.i(TAG, "no use for ${StorageState.IDLE_MINUTES} minutes; stopping")
                    StorageState.error.value = "Stopped after ${StorageState.IDLE_MINUTES} minutes without use."
                    stopSelf()
                    break
                }
            }
        }
    }

    /** Replies to the PC app's "SMON_FTP?" broadcast. */
    private fun answerDiscovery() {
        thread(name = "storage-discovery", isDaemon = true) {
            try {
                val socket = DatagramSocket(DISCOVERY_PORT)
                discovery = socket
                val buf = ByteArray(64)
                val name = "${Build.MANUFACTURER} ${Build.MODEL}".trim().replaceFirstChar { it.uppercase() }
                val reply = "SMON_FTP ${FtpServer.PORT} $name".toByteArray()
                while (!socket.isClosed) {
                    val packet = DatagramPacket(buf, buf.size)
                    socket.receive(packet)
                    if (String(packet.data, 0, packet.length).startsWith("SMON_FTP?")) {
                        socket.send(DatagramPacket(reply, reply.size, packet.address, packet.port))
                    }
                }
            } catch (e: Exception) {
                Log.d(TAG, "discovery stopped", e)
            }
        }
    }

    private fun startForegroundCompat() {
        val nm = getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            nm.createNotificationChannel(
                NotificationChannel(CHANNEL, "Phone storage", NotificationManager.IMPORTANCE_LOW),
            )
        }
        val open = PendingIntent.getActivity(
            this, 0, Intent(this, MainActivity::class.java), PendingIntent.FLAG_IMMUTABLE,
        )
        val stop = PendingIntent.getService(
            this, 1, Intent(this, StorageService::class.java).setAction(ACTION_STOP), PendingIntent.FLAG_IMMUTABLE,
        )
        val address = StorageState.localAddresses().firstOrNull()
            ?.takeIf { StorageState.wifiAccess(this) }
            ?.let { "ftp://$it:${FtpServer.PORT}" } ?: "USB only"
        val builder = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) Notification.Builder(this, CHANNEL)
        else @Suppress("DEPRECATION") Notification.Builder(this)
        val notification = builder
            .setSmallIcon(R.drawable.ic_stat_storage)
            .setContentTitle("Sharing phone storage")
            .setContentText("$address · password ${StorageState.password(this)}")
            .setContentIntent(open)
            .setOngoing(true)
            .addAction(Notification.Action.Builder(null, "Stop", stop).build())
            .build()
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            startForeground(NOTIFICATION_ID, notification, ServiceInfo.FOREGROUND_SERVICE_TYPE_DATA_SYNC)
        } else {
            startForeground(NOTIFICATION_ID, notification)
        }
    }

    companion object {
        private const val TAG = "StorageService"
        private const val CHANNEL = "storage"
        private const val NOTIFICATION_ID = 7
        private const val ACTION_STOP = "stop"
        private const val DISCOVERY_PORT = 52022

        fun start(context: Context) {
            val intent = Intent(context, StorageService::class.java)
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) context.startForegroundService(intent)
            else context.startService(intent)
        }

        fun stop(context: Context) {
            context.stopService(Intent(context, StorageService::class.java))
        }
    }
}
