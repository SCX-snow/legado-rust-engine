package io.legado.app.data.entities

import com.script.ScriptBindings
import com.script.buildScriptBindings
import com.script.rhino.RhinoScriptEngine
import io.legado.app.data.entities.rule.RowUi
import io.legado.app.help.CacheManager
import io.legado.app.help.JsExtensions
import io.legado.app.help.http.CookieStore
import io.legado.app.model.SharedJsScope
import io.legado.app.utils.GSON
import io.legado.app.utils.fromJsonArray
import io.legado.app.utils.fromJsonObject
import io.legado.app.utils.printOnDebug
import org.mozilla.javascript.Scriptable

@Suppress("unused")
interface BaseSource : JsExtensions {
    var concurrentRate: String?

    var loginUrl: String?

    var loginUi: String?

    var header: String?

    var enabledCookieJar: Boolean?

    var jsLib: String?

    fun getTag(): String

    fun getKey(): String

    override fun getSource(): BaseSource? {
        return this
    }

    fun loginUi(): List<RowUi>? {
        return GSON.fromJsonArray<RowUi>(loginUi).onFailure {
            it.printOnDebug()
        }.getOrNull()
    }

    fun getLoginJs(): String? {
        val loginJs = loginUrl
        return when {
            loginJs == null -> null
            loginJs.startsWith("@js:") -> loginJs.substring(4)
            loginJs.startsWith("<js>") -> loginJs.substring(4, loginJs.lastIndexOf("<"))
            else -> loginJs
        }
    }

    fun getLoginHeader(): String? {
        return CacheManager.get("loginHeader_${getKey()}")
    }

    fun getLoginHeaderMap(): Map<String, String>? {
        val cache = getLoginHeader() ?: return null
        return GSON.fromJsonObject<Map<String, String>>(cache).getOrNull()
    }

    fun putLoginHeader(header: String) {
        val headerMap = GSON.fromJsonObject<Map<String, String>>(header).getOrNull()
        val cookie = headerMap?.get("Cookie") ?: headerMap?.get("cookie")
        cookie?.let {
            CookieStore.replaceCookie(getKey(), it)
        }
        CacheManager.put("loginHeader_${getKey()}", header)
    }

    fun removeLoginHeader() {
        CacheManager.delete("loginHeader_${getKey()}")
        CookieStore.removeCookie(getKey())
    }

    fun setVariable(variable: String?) {
        if (variable != null) {
            CacheManager.put("sourceVariable_${getKey()}", variable)
        } else {
            CacheManager.delete("sourceVariable_${getKey()}")
        }
    }

    fun getVariable(): String {
        return CacheManager.get("sourceVariable_${getKey()}") ?: ""
    }

    fun put(key: String, value: String): String {
        CacheManager.put("v_${getKey()}_${key}", value)
        return value
    }

    fun get(key: String): String {
        return CacheManager.get("v_${getKey()}_${key}") ?: ""
    }

    @Throws(Exception::class)
    fun evalJS(jsStr: String, bindingsConfig: ScriptBindings.() -> Unit = {}): Any? {
        val bindings = buildScriptBindings { bindings ->
            bindings.apply(bindingsConfig)
            bindings["java"] = this
            bindings["source"] = this
            bindings["baseUrl"] = getKey()
            bindings["cookie"] = CookieStore
            bindings["cache"] = CacheManager
        }
        val scope = RhinoScriptEngine.getRuntimeScope(bindings)
        getShareScope()?.let {
            scope.prototype = it
        }
        return RhinoScriptEngine.eval(jsStr, scope)
    }

    fun getShareScope(): Scriptable? {
        return SharedJsScope.getScope(jsLib)
    }
}
