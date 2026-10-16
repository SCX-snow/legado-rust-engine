package io.legado.app.utils

import com.google.gson.Gson
import com.google.gson.GsonBuilder
import com.google.gson.JsonSyntaxException
import com.google.gson.reflect.TypeToken
import java.io.InputStream
import java.io.InputStreamReader
import java.lang.reflect.Type

val INITIAL_GSON: Gson by lazy {
    GsonBuilder()
        .disableHtmlEscaping()
        .create()
}

val GSON: Gson by lazy { INITIAL_GSON }

inline fun <reified T> genericType(): Type = object : TypeToken<T>() {}.type

inline fun <reified T> Gson.fromJsonObject(json: String?): Result<T> = runCatching {
    if (json == null) throw JsonSyntaxException("解析字符串为空")
    fromJson(json, genericType<T>()) as T
}

inline fun <reified T> Gson.fromJsonObject(inputStream: InputStream?): Result<T> = runCatching {
    if (inputStream == null) throw JsonSyntaxException("解析流为空")
    fromJson(InputStreamReader(inputStream), genericType<T>()) as T
}

inline fun <reified T> Gson.fromJsonArray(json: String?): Result<List<T>> = runCatching {
    if (json == null) throw JsonSyntaxException("解析字符串为空")
    fromJson<List<T>>(json, genericType<List<T>>())
}


fun String?.isJson(): Boolean =
    this?.run {
        val str = this.trim()
        when {
            str.startsWith("{") && str.endsWith("}") -> true
            str.startsWith("[") && str.endsWith("]") -> true
            else -> false
        }
    } ?: false

fun String?.isJsonObject(): Boolean =
    this?.run {
        val str = this.trim()
        str.startsWith("{") && str.endsWith("}")
    } ?: false

fun String?.isJsonArray(): Boolean =
    this?.run {
        val str = this.trim()
        str.startsWith("[") && str.endsWith("]")
    } ?: false

fun String?.isTrue(nullIsTrue: Boolean = false): Boolean {
    if (this.isNullOrBlank() || this == "null") return nullIsTrue
    return !this.trim().matches("(?i)^(false|no|not|0)$".toRegex())
}

fun String.splitNotBlank(vararg delimiter: String, limit: Int = 0): Array<String> =
    this.split(*delimiter, limit = limit).map { it.trim() }.filterNot { it.isBlank() }.toTypedArray()

fun String.splitNotBlank(regex: Regex, limit: Int = 0): Array<String> =
    this.split(regex, limit).map { it.trim() }.filterNot { it.isBlank() }.toTypedArray()

val Throwable.stackTraceStr: String
    get() {
        val stackTrace = stackTraceToString()
        val lMsg = this.localizedMessage ?: "noErrorMsg"
        return if (stackTrace.isNotEmpty()) stackTrace else lMsg
    }

fun Throwable.printOnDebug() {
    System.err.println(this.stackTraceStr)
}

fun String.printOnDebug() {
    System.err.println(this)
}
