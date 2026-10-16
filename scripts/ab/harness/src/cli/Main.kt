package cli

import com.google.gson.JsonObject
import com.google.gson.JsonParser
import io.legado.app.data.entities.Book
import io.legado.app.data.entities.BookChapter
import io.legado.app.data.entities.BookSource
import io.legado.app.model.analyzeRule.AnalyzeRule
import io.legado.app.utils.GSON
import java.io.BufferedReader
import java.io.InputStreamReader

private fun str(o: JsonObject, key: String): String? =
    o.get(key)?.takeIf { it.isJsonPrimitive }?.asString

fun main() {
    val reader = BufferedReader(InputStreamReader(System.`in`, Charsets.UTF_8))
    val writer = System.out.bufferedWriter()
    reader.forEachLine { line ->
        if (line.isBlank()) return@forEachLine
        val out = JsonObject()
        var id = ""
        try {
            val req = JsonParser.parseString(line).asJsonObject
            id = str(req, "id") ?: ""
            val mode = str(req, "mode") ?: "string"
            val ruleStr = str(req, "rule") ?: ""
            val content = str(req, "content") ?: ""
            val baseUrl = str(req, "baseUrl")
            val source = req.get("source")?.takeIf { !it.isJsonNull }
                ?.let { GSON.fromJson(it, BookSource::class.java) }
            val book = req.get("book")?.takeIf { !it.isJsonNull }
                ?.let { GSON.fromJson(it, Book::class.java) }
            val chapter = req.get("chapter")?.takeIf { !it.isJsonNull }
                ?.let { GSON.fromJson(it, BookChapter::class.java) }

            val rule = AnalyzeRule(ruleData = book, source = source)
            rule.chapter = chapter
            rule.setContent(content, baseUrl)

            val result: Any? = if (mode == "list") {
                rule.getStringList(ruleStr) ?: emptyList<String>()
            } else {
                rule.getString(ruleStr)
            }
            out.addProperty("id", id)
            out.addProperty("ok", true)
            out.add("result", GSON.toJsonTree(result))
        } catch (t: Throwable) {
            out.addProperty("id", id)
            out.addProperty("ok", false)
            out.addProperty("error", (t.toString() + " | " + t.message).take(500))
        }
        writer.write(out.toString())
        writer.newLine()
        writer.flush()
    }
}
