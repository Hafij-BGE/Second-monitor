package io.github.hafijbge.secondmonitor

import android.annotation.SuppressLint
import android.content.Context
import java.security.MessageDigest
import java.security.SecureRandom
import java.security.cert.X509Certificate
import javax.net.ssl.X509TrustManager

/** PCs this tablet has verified, by certificate fingerprint. */
class TrustStore(context: Context) {
    private val prefs = context.applicationContext.getSharedPreferences("trust", Context.MODE_PRIVATE)

    fun isTrusted(fingerprint: String): Boolean = prefs.contains("pc_$fingerprint")

    fun trust(fingerprint: String, name: String) {
        prefs.edit().putString("pc_$fingerprint", name).apply()
    }

    fun count(): Int = prefs.all.keys.count { it.startsWith("pc_") }

    fun forgetAll() {
        prefs.edit().clear().apply()
    }

    /** Random id identifying this tablet to PCs (16 bytes). */
    fun deviceId(): ByteArray {
        val hex = prefs.getString("device_id", null) ?: run {
            val bytes = ByteArray(16).also { SecureRandom().nextBytes(it) }
            bytes.toHex().also { prefs.edit().putString("device_id", it).apply() }
        }
        return hex.chunked(2).map { it.toInt(16).toByte() }.toByteArray()
    }
}

/**
 * Accepts the PC's self-signed certificate during the handshake and records
 * it. Trust is decided afterwards by fingerprint (pinning), with the user
 * comparing pairing codes the first time.
 */
@SuppressLint("CustomX509TrustManager", "TrustAllX509TrustManager")
class CapturingTrustManager : X509TrustManager {
    @Volatile var certificate: X509Certificate? = null
        private set

    override fun checkClientTrusted(chain: Array<out X509Certificate>?, authType: String?) {
        throw java.security.cert.CertificateException("client certificates are not used")
    }

    override fun checkServerTrusted(chain: Array<out X509Certificate>?, authType: String?) {
        certificate = chain?.firstOrNull() ?: throw java.security.cert.CertificateException("no certificate")
    }

    override fun getAcceptedIssuers(): Array<X509Certificate> = emptyArray()
}

fun ByteArray.toHex(): String = joinToString("") { "%02x".format(it) }

fun sha256(data: ByteArray): ByteArray = MessageDigest.getInstance("SHA-256").digest(data)

/** Six digits from a fingerprint; the PC computes the same code. */
fun pairingCode(fingerprint: ByteArray): String {
    val n = ((fingerprint[0].toLong() and 0xff) shl 24) or
        ((fingerprint[1].toLong() and 0xff) shl 16) or
        ((fingerprint[2].toLong() and 0xff) shl 8) or
        (fingerprint[3].toLong() and 0xff)
    return "%06d".format(n % 1_000_000)
}
