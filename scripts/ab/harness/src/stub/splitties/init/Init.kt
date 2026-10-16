package splitties.init

import java.io.File

class AppCtx {
    val cacheDir: File = File(System.getProperty("java.io.tmpdir"), "legado-ab-cache").apply {
        mkdirs()
    }
    val externalCache: File = cacheDir

    fun getString(resId: Int, vararg formatArgs: Any?): String =
        if (formatArgs.isEmpty()) "" else formatArgs.joinToString(" ")

    fun toastOnUi(msg: String) {
        System.err.println("[toast] $msg")
    }

    fun longToastOnUi(msg: String) {
        System.err.println("[longToast] $msg")
    }
}

val appCtx: AppCtx = AppCtx()
