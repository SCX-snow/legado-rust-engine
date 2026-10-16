package io.legado.app.utils

import io.legado.app.constant.AppLog
import io.legado.app.constant.AppPattern.dataUriRegex
import java.net.URL

object NetworkUtils {

    fun getAbsoluteURL(baseURL: String?, relativePath: String): String {
        if (baseURL.isNullOrEmpty()) return relativePath.trim()
        var absoluteUrl: URL? = null
        try {
            absoluteUrl = URL(baseURL.substringBefore(","))
        } catch (e: Exception) {
            e.printOnDebug()
        }
        return getAbsoluteURL(absoluteUrl, relativePath)
    }

    fun getAbsoluteURL(baseURL: URL?, relativePath: String): String {
        val relativePathTrim = relativePath.trim()
        if (baseURL == null) return relativePathTrim
        if (relativePathTrim.isAbsUrl()) return relativePathTrim
        if (relativePathTrim.isDataUrl()) return relativePathTrim
        if (relativePathTrim.startsWith("javascript")) return ""
        var relativeUrl = relativePathTrim
        try {
            val parseUrl = URL(baseURL, relativePath)
            relativeUrl = parseUrl.toString()
            return relativeUrl
        } catch (e: Exception) {
            AppLog.put("网址拼接出错\n${e.localizedMessage}", e)
        }
        return relativeUrl
    }

    fun getBaseUrl(url: String?): String? {
        url ?: return null
        if (url.startsWith("http://", true)
            || url.startsWith("https://", true)
        ) {
            val index = url.indexOf("/", 9)
            return if (index == -1) {
                url
            } else url.substring(0, index)
        }
        return null
    }
}

fun String?.isAbsUrl() =
    this?.let {
        it.startsWith("http://", true) || it.startsWith("https://", true)
    } ?: false

fun String?.isDataUrl() =
    this?.let {
        dataUriRegex.matches(it)
    } ?: false
