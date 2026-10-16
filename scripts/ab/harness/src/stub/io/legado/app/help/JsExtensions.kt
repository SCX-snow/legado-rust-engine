package io.legado.app.help

import io.legado.app.constant.AppConst
import io.legado.app.constant.AppLog
import io.legado.app.constant.AppPattern
import io.legado.app.data.entities.BaseSource
import io.legado.app.help.config.AppConfig
import io.legado.app.help.http.CookieStore
import io.legado.app.help.http.StrResponse
import io.legado.app.model.Debug
import io.legado.app.model.analyzeRule.AnalyzeUrl
import io.legado.app.model.analyzeRule.QueryTTF
import io.legado.app.utils.ChineseUtils
import io.legado.app.utils.EncoderUtils
import io.legado.app.utils.HtmlFormatter
import io.legado.app.utils.JsURL
import io.legado.app.utils.StringUtils
import io.legado.app.utils.printOnDebug
import io.legado.app.utils.stackTraceStr
import java.io.File
import java.net.URLEncoder
import java.nio.charset.Charset
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale
import java.util.SimpleTimeZone
import java.util.UUID

@Suppress("unused")
interface JsExtensions : JsEncodeUtils {

    fun getSource(): BaseSource?

    fun ajax(url: Any): String? {
        val urlStr = if (url is List<*>) {
            url.firstOrNull().toString()
        } else {
            url.toString()
        }
        val analyzeUrl = AnalyzeUrl(
            urlStr,
            source = getSource(),
            ruleData = null,
        )
        return kotlin.runCatching {
            analyzeUrl.getStrResponseAwait().body
        }.onFailure {
            AppLog.put("ajax(${urlStr}) error\n${it.localizedMessage}", it)
            it.printOnDebug()
        }.getOrElse {
            it.stackTraceStr
        }
    }

    fun ajaxAll(urlList: Array<String>): Array<StrResponse?> {
        return Array(urlList.size) {
            AnalyzeUrl(urlList[it], source = getSource()).getStrResponseAwait()
        }
    }

    fun connect(urlStr: String): StrResponse = networkDisabled()

    fun connect(urlStr: String, header: String?): StrResponse = networkDisabled()

    fun webView(html: String?, url: String?, js: String?): String? =
        throw UnsupportedOperationException("JsExtensions stub: harness 没有 WebView")

    fun webViewGetSource(html: String?, url: String?, js: String?, sourceRegex: String): String? =
        throw UnsupportedOperationException("JsExtensions stub: harness 没有 WebView")

    fun startBrowser(url: String, title: String) {
        System.err.println("[startBrowser] $title $url")
    }

    fun startBrowserAwait(url: String, title: String): StrResponse = networkDisabled()

    fun getVerificationCode(imageUrl: String): String =
        throw UnsupportedOperationException("JsExtensions stub: 验证码需要 UI（harness 没有）")

    fun importScript(path: String): String =
        throw UnsupportedOperationException("JsExtensions stub: 不加载外部脚本（本地文件）")

    fun cacheFile(urlStr: String): String = networkDisabled()

    fun cacheFile(urlStr: String, saveTime: Int): String = networkDisabled()

    fun getCookie(tag: String): String {
        return getCookie(tag, null)
    }

    fun getCookie(tag: String, key: String?): String {
        return if (key != null) {
            CookieStore.getKey(tag, key)
        } else {
            CookieStore.getCookie(tag)
        }
    }

    fun downloadFile(url: String): String = networkDisabled()

    fun downloadFile(content: String, url: String): String {
        throw UnsupportedOperationException("JsExtensions stub: 不落盘（downloadFile）")
    }

    fun strToBytes(str: String): ByteArray {
        return str.toByteArray(Charsets.UTF_8)
    }

    fun strToBytes(str: String, charset: String): ByteArray {
        return str.toByteArray(Charset.forName(charset))
    }

    fun bytesToStr(bytes: ByteArray): String {
        return String(bytes, Charsets.UTF_8)
    }

    fun bytesToStr(bytes: ByteArray, charset: String): String {
        return String(bytes, Charset.forName(charset))
    }

    fun base64Decode(str: String?): String {
        if (str.isNullOrBlank()) return ""
        val bytes: ByteArray = runCatching { java.util.Base64.getMimeDecoder().decode(str) }
            .getOrElse { EncoderUtils.base64Decode(str).toByteArray(Charsets.UTF_8) }
        return String(bytes, Charsets.UTF_8)
    }

    fun base64Decode(str: String?, charset: String): String {
        if (str.isNullOrBlank()) return ""
        val bytes: ByteArray = runCatching { java.util.Base64.getMimeDecoder().decode(str) }
            .getOrElse { EncoderUtils.base64Decode(str).toByteArray(Charsets.UTF_8) }
        return String(bytes, Charset.forName(charset))
    }

    fun base64Decode(str: String, flags: Int): String {
        return EncoderUtils.base64Decode(str, flags)
    }

    fun base64DecodeToByteArray(str: String?): ByteArray? {
        if (str.isNullOrBlank()) {
            return null
        }
        return EncoderUtils.base64DecodeToByteArray(str, 0)
    }

    fun base64DecodeToByteArray(str: String?, flags: Int): ByteArray? {
        if (str.isNullOrBlank()) {
            return null
        }
        return EncoderUtils.base64DecodeToByteArray(str, flags)
    }

    fun base64Encode(str: String): String? {
        return EncoderUtils.base64Encode(str, 2)
    }

    fun base64Encode(str: String, flags: Int): String? {
        return EncoderUtils.base64Encode(str, flags)
    }

    fun hexDecodeToByteArray(hex: String): ByteArray? {
        return runCatching {
            ByteArray(hex.trim().length / 2) {
                hex.trim().substring(it * 2, it * 2 + 2).toInt(16).toByte()
            }
        }.getOrNull()
    }

    fun hexDecodeToString(hex: String): String? {
        return hexDecodeToByteArray(hex)?.let { String(it, Charsets.UTF_8) }
    }

    fun hexEncodeToString(utf8: String): String? {
        return utf8.toByteArray(Charsets.UTF_8).joinToString("") { "%02x".format(it) }
    }

    fun timeFormatUTC(time: Long, format: String, sh: Int): String? {
        val utc = SimpleTimeZone(sh, "UTC")
        return SimpleDateFormat(format, Locale.getDefault()).run {
            timeZone = utc
            format(Date(time))
        }
    }

    fun timeFormat(time: Long): String {
        return AppConst.dateFormat.format(Date(time))
    }

    fun utf8ToGbk(str: String): String {
        val utf8 = String(str.toByteArray(Charsets.UTF_8))
        val unicode = String(utf8.toByteArray(), Charsets.UTF_8)
        return String(unicode.toByteArray(Charset.forName("GBK")))
    }

    fun encodeURI(str: String): String {
        return try {
            URLEncoder.encode(str, "UTF-8")
        } catch (e: Exception) {
            ""
        }
    }

    fun encodeURI(str: String, enc: String): String {
        return try {
            URLEncoder.encode(str, enc)
        } catch (e: Exception) {
            ""
        }
    }

    fun htmlFormat(str: String): String {
        return HtmlFormatter.formatKeepImg(str)
    }

    fun t2s(text: String): String {
        return ChineseUtils.t2s(text)
    }

    fun s2t(text: String): String {
        return ChineseUtils.s2t(text)
    }

    fun getWebViewUA(): String {
        return AppConfig.userAgent
    }


    fun getFile(path: String): File =
        throw UnsupportedOperationException("JsExtensions stub: 不碰文件系统（getFile）")

    fun readFile(path: String): ByteArray? =
        throw UnsupportedOperationException("JsExtensions stub: 不碰文件系统（readFile）")

    fun readTxtFile(path: String): String =
        throw UnsupportedOperationException("JsExtensions stub: 不碰文件系统（readTxtFile）")

    fun readTxtFile(path: String, charsetName: String): String =
        throw UnsupportedOperationException("JsExtensions stub: 不碰文件系统（readTxtFile）")

    fun deleteFile(path: String): Boolean =
        throw UnsupportedOperationException("JsExtensions stub: 不碰文件系统（deleteFile）")


    fun queryBase64TTF(data: String?): QueryTTF? =
        throw UnsupportedOperationException("JsExtensions stub: 不做 TTF 反爬（queryBase64TTF）")

    fun queryTTF(data: Any?, useCache: Boolean): QueryTTF? =
        throw UnsupportedOperationException("JsExtensions stub: 不做 TTF 反爬（queryTTF）")

    fun queryTTF(data: Any?): QueryTTF? = queryTTF(data, true)

    fun toNumChapter(s: String?): String? {
        s ?: return null
        val matcher = AppPattern.titleNumPattern.matcher(s)
        if (matcher.find()) {
            return "${matcher.group(1)}${StringUtils.stringToInt(matcher.group(2))}${matcher.group(3)}"
        }
        return s
    }

    fun toURL(urlStr: String): JsURL {
        return JsURL(urlStr)
    }

    fun toURL(url: String, baseUrl: String? = null): JsURL {
        return JsURL(url, baseUrl)
    }

    fun toast(msg: Any?) {
        System.err.println("[toast] ${getSource()?.getTag()}: $msg")
    }

    fun longToast(msg: Any?) {
        System.err.println("[longToast] ${getSource()?.getTag()}: $msg")
    }

    fun log(msg: Any?): Any? {
        getSource()?.let {
            Debug.log(it.getKey(), msg.toString())
        } ?: Debug.log(msg.toString())
        return msg
    }

    fun logType(any: Any?) {
        if (any == null) {
            log("null")
        } else {
            log(any.javaClass.name)
        }
    }

    fun randomUUID(): String {
        return UUID.randomUUID().toString()
    }

    fun androidId(): String {
        return "legado-ab-harness"
    }

    private fun networkDisabled(): Nothing = throw UnsupportedOperationException(
        "AnalyzeUrl stub: network disabled for rule harness"
    )
}
