package android.text

object TextUtils {
    @JvmStatic
    fun isEmpty(str: CharSequence?): Boolean = str == null || str.isEmpty()

    @JvmStatic
    fun isBlank(str: CharSequence?): Boolean = str == null || str.isBlank()

    @JvmStatic
    fun isDigitsOnly(str: CharSequence?): Boolean =
        str != null && str.isNotEmpty() && str.all { it.isDigit() }

    @JvmStatic
    fun equals(a: CharSequence?, b: CharSequence?): Boolean = a?.toString() == b?.toString()

    @JvmStatic
    fun join(delimiter: CharSequence, tokens: Iterable<*>?): String =
        tokens?.joinToString(delimiter.toString()) ?: ""

    @JvmStatic
    fun join(delimiter: CharSequence, vararg tokens: Any?): String =
        tokens.joinToString(delimiter.toString())

    @JvmStatic
    fun split(text: String?, expression: String?): Array<String> =
        if (text.isNullOrEmpty()) emptyArray() else text.split(Regex(expression ?: ""))
            .filter { it.isNotEmpty() }.toTypedArray()
}
