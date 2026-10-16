package io.legado.app.help.http

data class StrResponse(
    val url: String,
    val body: String? = null,
    val headers: Map<String, String> = emptyMap(),
    val isSuccessful: Boolean = true,
)
