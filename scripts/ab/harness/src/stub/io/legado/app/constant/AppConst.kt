package io.legado.app.constant

import java.text.SimpleDateFormat

object AppConst {
    const val UA_NAME = "User-Agent"

    val timeFormat: SimpleDateFormat by lazy {
        SimpleDateFormat("HH:mm")
    }

    val dateFormat: SimpleDateFormat by lazy {
        SimpleDateFormat("yyyy/MM/dd HH:mm")
    }

    val fileNameFormat: SimpleDateFormat by lazy {
        SimpleDateFormat("yy-MM-dd-HH-mm-ss")
    }
}
