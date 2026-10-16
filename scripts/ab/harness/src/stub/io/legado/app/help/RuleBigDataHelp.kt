package io.legado.app.help

object RuleBigDataHelp {

    private val store = HashMap<String, String>()

    fun putBookVariable(bookUrl: String, key: String, value: String?) {
        val id = "book_${bookUrl}_$key"
        if (value == null) store.remove(id) else store[id] = value
    }

    fun getBookVariable(bookUrl: String, key: String): String? = store["book_${bookUrl}_$key"]

    fun putChapterVariable(bookUrl: String, chapterUrl: String, key: String, value: String?) {
        val id = "chapter_${bookUrl}_${chapterUrl}_$key"
        if (value == null) store.remove(id) else store[id] = value
    }

    fun getChapterVariable(bookUrl: String, chapterUrl: String, key: String): String? =
        store["chapter_${bookUrl}_${chapterUrl}_$key"]
}
