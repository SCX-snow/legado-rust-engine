package io.legado.app.help

import javax.crypto.Cipher
import javax.crypto.spec.IvParameterSpec
import javax.crypto.spec.SecretKeySpec
import java.nio.charset.Charset

class SymmetricCrypto(
    private val transformation: String,
    private val key: ByteArray,
) {

    private var iv: ByteArray? = null

    fun setIv(iv: ByteArray): SymmetricCrypto {
        this.iv = iv
        return this
    }

    private fun cipher(mode: Int): Cipher {
        val algorithm = transformation.substringBefore("/")
        val cipher = Cipher.getInstance(transformation)
        val keySpec = SecretKeySpec(key, algorithm)
        val ivSpec = iv?.let { IvParameterSpec(it) }
        if (ivSpec != null) {
            cipher.init(mode, keySpec, ivSpec)
        } else {
            cipher.init(mode, keySpec)
        }
        return cipher
    }

    fun encrypt(data: ByteArray): ByteArray = cipher(Cipher.ENCRYPT_MODE).doFinal(data)

    fun encrypt(data: String): ByteArray = encrypt(data.toByteArray(Charsets.UTF_8))

    fun encryptBase64(data: String): String =
        java.util.Base64.getEncoder().encodeToString(encrypt(data))

    fun encryptBase64(data: ByteArray): String =
        java.util.Base64.getEncoder().encodeToString(encrypt(data))

    fun encryptHex(data: String): String = encrypt(data).joinToString("") { "%02x".format(it) }

    fun decrypt(data: ByteArray): ByteArray = cipher(Cipher.DECRYPT_MODE).doFinal(data)

    fun decrypt(data: String): ByteArray = decrypt(decode(data))

    fun decryptStr(data: String): String = String(decrypt(data), Charsets.UTF_8)

    fun decryptStr(data: String, charset: String): String =
        String(decrypt(data), Charset.forName(charset))

    private fun decode(data: String): ByteArray {
        val trimmed = data.trim()
        val isHex = trimmed.isNotEmpty() && trimmed.length % 2 == 0 &&
            trimmed.all { it in "0123456789abcdefABCDEF" }
        return if (isHex) {
            ByteArray(trimmed.length / 2) { trimmed.substring(it * 2, it * 2 + 2).toInt(16).toByte() }
        } else {
            java.util.Base64.getMimeDecoder().decode(trimmed)
        }
    }
}
