package io.legado.app.utils

import java.net.URL
import java.net.URLDecoder

class JsURL(private val relativeUrl: String, baseUrl: String? = null) {

    private val url: URL = if (baseUrl == null) URL(relativeUrl) else URL(URL(baseUrl), relativeUrl)

    val href: String get() = url.toString()

    fun getHost(): String = url.host

    fun getPath(): String = url.path

    fun getQuery(): String = url.query ?: ""

    fun getProtocol(): String = url.protocol

    fun decode(charset: String = "UTF-8"): String = URLDecoder.decode(relativeUrl, charset)

    override fun toString(): String = href
}
