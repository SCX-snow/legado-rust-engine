package io.legado.app.data.entities

import io.legado.app.help.RuleBigDataHelp
import io.legado.app.model.analyzeRule.RuleDataInterface
import io.legado.app.utils.GSON
import io.legado.app.utils.fromJsonObject

@Suppress("unused")
data class BookChapter(
    var url: String = "",
    var title: String = "",
    var isVolume: Boolean = false,
    var baseUrl: String = "",
    var bookUrl: String = "",
    var index: Int = 0,
    var isVip: Boolean = false,
    var isPay: Boolean = false,
    var resourceUrl: String? = null,
    var tag: String? = null,
    var start: Long? = null,
    var end: Long? = null,
    var startFragmentId: String? = null,
    var endFragmentId: String? = null,
    var variable: String? = null
) : RuleDataInterface {

    override val variableMap: HashMap<String, String> by lazy {
        GSON.fromJsonObject<HashMap<String, String>>(variable).getOrNull() ?: hashMapOf()
    }

    override fun putVariable(key: String, value: String?): Boolean {
        if (super.putVariable(key, value)) {
            variable = GSON.toJson(variableMap)
        }
        return true
    }

    override fun putBigVariable(key: String, value: String?) {
        RuleBigDataHelp.putChapterVariable(bookUrl, url, key, value)
    }

    override fun getBigVariable(key: String): String? {
        return RuleBigDataHelp.getChapterVariable(bookUrl, url, key)
    }

    override fun hashCode() = url.hashCode()

    override fun equals(other: Any?): Boolean {
        if (other is BookChapter) {
            return other.url == url
        }
        return false
    }

    fun primaryStr(): String {
        return bookUrl + url
    }
}
