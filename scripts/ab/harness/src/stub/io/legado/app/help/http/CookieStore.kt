package io.legado.app.help.http

import io.legado.app.constant.AppPattern.equalsRegex
import io.legado.app.constant.AppPattern.semicolonRegex

object CookieStore {

    private val jar = HashMap<String, String>()

    fun setCookie(url: String, cookie: String?) {
        jar[url] = cookie ?: ""
    }

    fun replaceCookie(url: String, cookie: String) {
        if (url.isEmpty() || cookie.isEmpty()) {
            return
        }
        val oldCookie = jar[url]
        if (oldCookie.isNullOrEmpty()) {
            setCookie(url, cookie)
        } else {
            val cookieMap = cookieToMap(oldCookie)
            cookieMap.putAll(cookieToMap(cookie))
            setCookie(url, mapToCookie(cookieMap) ?: "")
        }
    }

    fun getCookie(url: String): String = jar[url] ?: ""

    fun getKey(url: String, key: String): String {
        return cookieToMap(getCookie(url))[key] ?: ""
    }

    fun removeCookie(url: String) {
        jar.remove(url)
    }

    fun cookieToMap(cookie: String): MutableMap<String, String> {
        val cookieMap = mutableMapOf<String, String>()
        if (cookie.isBlank()) {
            return cookieMap
        }
        val pairArray = cookie.split(semicolonRegex).dropLastWhile { it.isEmpty() }.toTypedArray()
        for (pair in pairArray) {
            val pairs = pair.split(equalsRegex, 2).dropLastWhile { it.isEmpty() }.toTypedArray()
            if (pairs.size <= 1) {
                continue
            }
            val key = pairs[0].trim { it <= ' ' }
            val value = pairs[1]
            if (value.isNotBlank() || value.trim { it <= ' ' } == "null") {
                cookieMap[key] = value.trim { it <= ' ' }
            }
        }
        return cookieMap
    }

    fun mapToCookie(cookieMap: Map<String, String>?): String? {
        if (cookieMap.isNullOrEmpty()) {
            return null
        }
        val builder = StringBuilder()
        cookieMap.keys.forEachIndexed { index, key ->
            if (index > 0) builder.append("; ")
            builder.append(key).append("=").append(cookieMap[key])
        }
        return builder.toString()
    }

    fun clear() {
        jar.clear()
    }
}
