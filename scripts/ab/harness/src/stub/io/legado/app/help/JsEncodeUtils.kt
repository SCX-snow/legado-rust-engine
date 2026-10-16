package io.legado.app.help

import android.util.Base64
import io.legado.app.help.crypto.AsymmetricCrypto
import io.legado.app.help.crypto.Sign
import io.legado.app.utils.MD5Utils
import java.security.MessageDigest
import javax.crypto.Mac
import javax.crypto.spec.SecretKeySpec

interface JsEncodeUtils {

    fun md5Encode(str: String): String {
        return MD5Utils.md5Encode(str)
    }

    fun md5Encode16(str: String): String {
        return MD5Utils.md5Encode16(str)
    }


    fun createSymmetricCrypto(
        transformation: String,
        key: ByteArray?,
        iv: ByteArray?
    ): SymmetricCrypto {
        val symmetricCrypto = SymmetricCrypto(transformation, key ?: ByteArray(16))
        return if (iv != null && iv.isNotEmpty()) symmetricCrypto.setIv(iv) else symmetricCrypto
    }

    fun createSymmetricCrypto(
        transformation: String,
        key: ByteArray
    ): SymmetricCrypto {
        return createSymmetricCrypto(transformation, key, null)
    }

    fun createSymmetricCrypto(
        transformation: String,
        key: String
    ): SymmetricCrypto {
        return createSymmetricCrypto(transformation, key, null)
    }

    fun createSymmetricCrypto(
        transformation: String,
        key: String,
        iv: String?
    ): SymmetricCrypto {
        return createSymmetricCrypto(
            transformation, key.encodeToByteArray(), iv?.encodeToByteArray()
        )
    }


    fun createAsymmetricCrypto(
        transformation: String
    ): AsymmetricCrypto {
        return AsymmetricCrypto(transformation)
    }


    fun createSign(
        algorithm: String
    ): Sign {
        return Sign(algorithm)
    }


    fun aesDecodeToByteArray(
        str: String, key: String, transformation: String, iv: String
    ): ByteArray? {
        return createSymmetricCrypto(transformation, key, iv).decrypt(str)
    }

    fun aesDecodeToString(
        str: String, key: String, transformation: String, iv: String
    ): String? {
        return createSymmetricCrypto(transformation, key, iv).decryptStr(str)
    }

    fun aesDecodeArgsBase64Str(
        data: String,
        key: String,
        mode: String,
        padding: String,
        iv: String
    ): String? {
        return createSymmetricCrypto(
            "AES/$mode/$padding",
            Base64.decode(key, Base64.NO_WRAP),
            Base64.decode(iv, Base64.NO_WRAP)
        ).decryptStr(data)
    }

    fun aesBase64DecodeToByteArray(
        str: String, key: String, transformation: String, iv: String
    ): ByteArray? {
        return createSymmetricCrypto(transformation, key, iv).decrypt(str)
    }

    fun aesBase64DecodeToString(
        str: String, key: String, transformation: String, iv: String
    ): String? {
        return createSymmetricCrypto(transformation, key, iv).decryptStr(str)
    }

    fun aesEncodeToByteArray(
        data: String, key: String, transformation: String, iv: String
    ): ByteArray? {
        return createSymmetricCrypto(transformation, key, iv).encrypt(data)
    }

    fun aesEncodeToString(
        data: String, key: String, transformation: String, iv: String
    ): String? {
        return createSymmetricCrypto(transformation, key, iv).decryptStr(data)
    }

    fun aesEncodeToBase64ByteArray(
        data: String, key: String, transformation: String, iv: String
    ): ByteArray? {
        return createSymmetricCrypto(transformation, key, iv).encryptBase64(data).toByteArray()
    }

    fun aesEncodeToBase64String(
        data: String, key: String, transformation: String, iv: String
    ): String? {
        return createSymmetricCrypto(transformation, key, iv).encryptBase64(data)
    }

    fun desDecodeToString(
        str: String, key: String, transformation: String, iv: String
    ): String? {
        return createSymmetricCrypto(transformation, key, iv).decryptStr(str)
    }

    fun desBase64DecodeToString(
        str: String, key: String, transformation: String, iv: String
    ): String? {
        return createSymmetricCrypto(transformation, key, iv).decryptStr(str)
    }

    fun desEncodeToString(
        data: String, key: String, transformation: String, iv: String
    ): String? {
        return createSymmetricCrypto(transformation, key, iv).decryptStr(data)
    }

    fun desEncodeToBase64String(
        data: String, key: String, transformation: String, iv: String
    ): String? {
        return createSymmetricCrypto(transformation, key, iv).encryptBase64(data)
    }

    fun tripleDESDecodeStr(
        data: String,
        key: String,
        mode: String,
        padding: String,
        iv: String
    ): String? {
        return createSymmetricCrypto("DESede/$mode/$padding", key, iv).decryptStr(data)
    }

    fun tripleDESDecodeArgsBase64Str(
        data: String,
        key: String,
        mode: String,
        padding: String,
        iv: String
    ): String? {
        return createSymmetricCrypto(
            "DESede/$mode/$padding",
            Base64.decode(key, Base64.NO_WRAP),
            iv.encodeToByteArray()
        ).decryptStr(data)
    }

    fun tripleDESEncodeBase64Str(
        data: String,
        key: String,
        mode: String,
        padding: String,
        iv: String
    ): String? {
        return createSymmetricCrypto("DESede/$mode/$padding", key, iv)
            .encryptBase64(data)
    }

    fun tripleDESEncodeArgsBase64Str(
        data: String,
        key: String,
        mode: String,
        padding: String,
        iv: String
    ): String? {
        return createSymmetricCrypto(
            "DESede/$mode/$padding",
            Base64.decode(key, Base64.NO_WRAP),
            iv.encodeToByteArray()
        ).encryptBase64(data)
    }


    fun digestHex(
        data: String,
        algorithm: String,
    ): String {
        return MessageDigest.getInstance(algorithm).digest(data.toByteArray(Charsets.UTF_8))
            .joinToString("") { "%02x".format(it) }
    }

    fun digestBase64Str(
        data: String,
        algorithm: String,
    ): String {
        return Base64.encodeToString(
            MessageDigest.getInstance(algorithm).digest(data.toByteArray(Charsets.UTF_8)),
            Base64.NO_WRAP
        )
    }

    @Suppress("FunctionName")
    fun HMacHex(
        data: String,
        algorithm: String,
        key: String
    ): String {
        return hmac(algorithm, key, data).joinToString("") { "%02x".format(it) }
    }

    @Suppress("FunctionName")
    fun HMacBase64(
        data: String,
        algorithm: String,
        key: String
    ): String {
        return Base64.encodeToString(hmac(algorithm, key, data), Base64.NO_WRAP)
    }

    private fun hmac(algorithm: String, key: String, data: String): ByteArray {
        val mac = Mac.getInstance(algorithm)
        mac.init(SecretKeySpec(key.toByteArray(Charsets.UTF_8), algorithm))
        return mac.doFinal(data.toByteArray(Charsets.UTF_8))
    }
}
