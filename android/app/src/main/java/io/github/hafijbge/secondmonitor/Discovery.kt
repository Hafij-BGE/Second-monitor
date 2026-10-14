package io.github.hafijbge.secondmonitor

import android.content.Context
import android.net.wifi.WifiManager
import java.net.DatagramPacket
import java.net.DatagramSocket
import java.net.InetAddress
import java.net.SocketTimeoutException

data class Host(val name: String, val address: String, val port: Int)

/** Finds hosts on the LAN by UDP broadcast. Blocking: call from a background thread. */
object Discovery {
    fun scan(context: Context, timeoutMs: Long = 1500): List<Host> {
        val found = LinkedHashMap<String, Host>()
        DatagramSocket().use { socket ->
            socket.broadcast = true
            socket.soTimeout = 250
            val query = Protocol.DISCOVERY_QUERY.toByteArray()
            for (target in broadcastAddresses(context)) {
                runCatching {
                    socket.send(DatagramPacket(query, query.size, target, Protocol.DISCOVERY_PORT))
                }
            }
            val buf = ByteArray(256)
            val deadline = System.currentTimeMillis() + timeoutMs
            while (System.currentTimeMillis() < deadline) {
                val packet = DatagramPacket(buf, buf.size)
                try {
                    socket.receive(packet)
                } catch (e: SocketTimeoutException) {
                    continue
                }
                val parts = String(packet.data, 0, packet.length).trim().split(" ", limit = 3)
                if (parts.size < 2 || parts[0] != Protocol.DISCOVERY_REPLY) continue
                val port = parts[1].toIntOrNull() ?: continue
                val address = packet.address.hostAddress ?: continue
                found[address] = Host(parts.getOrElse(2) { address }, address, port)
            }
        }
        return found.values.toList()
    }

    private fun broadcastAddresses(context: Context): List<InetAddress> {
        val result = mutableListOf(InetAddress.getByName("255.255.255.255"))
        // Some routers drop 255.255.255.255; also try the subnet broadcast.
        val wifi = context.applicationContext.getSystemService(Context.WIFI_SERVICE) as? WifiManager
        @Suppress("DEPRECATION")
        val dhcp = wifi?.dhcpInfo
        if (dhcp != null && dhcp.ipAddress != 0 && dhcp.netmask != 0) {
            val broadcast = (dhcp.ipAddress and dhcp.netmask) or dhcp.netmask.inv()
            val bytes = ByteArray(4) { i -> (broadcast shr (i * 8)).toByte() }
            runCatching { result.add(InetAddress.getByAddress(bytes)) }
        }
        return result
    }
}
