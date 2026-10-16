package io.legado.app.help.config

object AppConfig {
    var chineseConverterType: Int = 0
    var userAgent: String = System.getProperty("http.agent") ?: "legado-ab-harness"
}
