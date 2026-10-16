package io.legado.app.model

import com.script.ScriptBindings
import com.script.rhino.RhinoScriptEngine
import io.legado.app.exception.NoStackTraceException
import io.legado.app.utils.MD5Utils
import io.legado.app.utils.isJsonObject
import org.mozilla.javascript.Scriptable
import java.lang.ref.WeakReference

object SharedJsScope {

    private val scopeMap = hashMapOf<String, WeakReference<Scriptable>>()

    fun getScope(jsLib: String?): Scriptable? {
        if (jsLib.isNullOrBlank()) {
            return null
        }
        val key = MD5Utils.md5Encode(jsLib)
        var scope = scopeMap[key]?.get()
        if (scope == null) {
            scope = RhinoScriptEngine.run {
                getRuntimeScope(ScriptBindings())
            }
            if (jsLib.isJsonObject()) {
                throw NoStackTraceException("jsLib 是 URL 映射，harness 不联网（SharedJsScope 桩）")
            } else {
                RhinoScriptEngine.eval(jsLib, scope)
            }
            scopeMap[key] = WeakReference(scope)
        }
        return scope
    }
}
