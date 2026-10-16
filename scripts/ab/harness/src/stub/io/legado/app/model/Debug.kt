package io.legado.app.model

object Debug {
    fun log(key: String, msg: String, state: Int = 0) {
        System.err.println("[Debug:$key] $msg")
    }

    fun log(msg: String) {
        System.err.println("[Debug] $msg")
    }
}
