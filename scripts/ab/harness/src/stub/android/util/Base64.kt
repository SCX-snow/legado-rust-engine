package android.util

object Base64 {
    const val DEFAULT = 0
    const val NO_PADDING = 1
    const val NO_WRAP = 2
    const val CRLF = 4
    const val URL_SAFE = 8
    const val NO_CLOSE = 16

    private fun decoder(flags: Int) = when {
        flags and URL_SAFE != 0 -> java.util.Base64.getUrlDecoder()
        else -> java.util.Base64.getMimeDecoder()
    }

    private fun encoder(flags: Int) = when {
        flags and URL_SAFE != 0 -> java.util.Base64.getUrlEncoder()
        flags and NO_WRAP != 0 -> java.util.Base64.getEncoder().withoutPadding()
        else -> java.util.Base64.getMimeEncoder()
    }

    @JvmStatic
    fun decode(str: String, flags: Int): ByteArray = decoder(flags).decode(str)

    @JvmStatic
    fun decode(str: String, flags: Int, offset: Int, len: Int): ByteArray =
        decode(str.substring(offset, offset + len), flags)

    @JvmStatic
    fun encodeToString(input: ByteArray, flags: Int): String = encoder(flags).encodeToString(input)

    @JvmStatic
    fun encode(input: ByteArray, flags: Int): ByteArray = encoder(flags).encode(input)
}
