package io.legado.app.model.analyzeRule

import kotlin.coroutines.CoroutineContext
import kotlin.coroutines.EmptyCoroutineContext

class AnalyzeUrl(
    val urlStr: String,
    val source: io.legado.app.data.entities.BaseSource? = null,
    val ruleData: io.legado.app.data.entities.BaseBook? = null,
    val coroutineContext: CoroutineContext = EmptyCoroutineContext
) {

    fun getStrResponseAwait(): io.legado.app.help.http.StrResponse =
        throw UnsupportedOperationException("AnalyzeUrl stub: network disabled for rule harness")

    companion object {
        val paramPattern: java.util.regex.Pattern =
            java.util.regex.Pattern.compile("\\s*,\\s*(?=\\{)")
    }
}
