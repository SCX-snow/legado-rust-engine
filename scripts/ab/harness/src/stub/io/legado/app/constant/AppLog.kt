package io.legado.app.constant

object AppLog {
    fun put(msg: String, e: Throwable? = null) {
        System.err.println("[AppLog] $msg${e?.let { " | $it" } ?: ""}")
    }

    fun putDebug(msg: String) {
        System.err.println("[AppLog.debug] $msg")
    }

    fun putNotSave(msg: String, e: Throwable? = null) {
        put(msg, e)
    }
}
