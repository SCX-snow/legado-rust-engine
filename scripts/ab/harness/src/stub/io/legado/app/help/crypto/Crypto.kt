package io.legado.app.help.crypto

class AsymmetricCrypto(private val transformation: String) {
    fun encrypt(data: String): ByteArray = todo("encrypt")
    fun encryptBase64(data: String): String = todo("encryptBase64")
    fun encryptHex(data: String): String = todo("encryptHex")
    fun decrypt(data: String): ByteArray = todo("decrypt")
    fun decryptStr(data: String): String = todo("decryptStr")

    private fun todo(what: String): Nothing =
        throw NotImplementedError("AsymmetricCrypto 桩（$transformation）未实现 $what：harness 没接 hutool/RSA")
}

class Sign(private val algorithm: String) {
    fun sign(data: String): ByteArray = todo("sign")
    fun signHex(data: String): String = todo("signHex")
    fun signBase64(data: String): String = todo("signBase64")
    fun verify(data: String, sign: String): Boolean = todo("verify")

    private fun todo(what: String): Nothing =
        throw NotImplementedError("Sign 桩（$algorithm）未实现 $what：harness 没接 hutool/签名")
}
