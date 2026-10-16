package io.legado.app.utils

import java.io.InputStream
import java.security.MessageDigest

object MD5Utils {

    fun md5Encode(str: String?): String {
        val digest = MessageDigest.getInstance("MD5")
        val bytes = digest.digest((str ?: "").toByteArray(Charsets.UTF_8))
        return bytes.joinToString("") { "%02x".format(it) }
    }

    fun md5Encode(inputStream: InputStream): String {
        val digest = MessageDigest.getInstance("MD5")
        val buffer = ByteArray(8192)
        while (true) {
            val read = inputStream.read(buffer)
            if (read <= 0) break
            digest.update(buffer, 0, read)
        }
        return digest.digest().joinToString("") { "%02x".format(it) }
    }

    fun md5Encode16(str: String): String {
        var reStr = md5Encode(str)
        reStr = reStr.substring(8, 24)
        return reStr
    }
}
