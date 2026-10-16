package cli

import com.google.gson.JsonObject
import com.google.gson.JsonParser
import com.script.rhino.RhinoScriptEngine
import java.io.BufferedReader
import java.io.InputStreamReader

private data class Rule(
    val name: String,
    val pattern: String,
    val replacement: String,
    val isRegex: Boolean,
    val isEnabled: Boolean,
    val scope: String?,
    val excludeScope: String?,
    val scopeTitle: Boolean,
    val scopeContent: Boolean,
    val order: Int,
)

private fun str(o: JsonObject, k: String): String? =
    o.get(k)?.takeIf { it.isJsonPrimitive }?.asString

private fun bool(o: JsonObject, k: String, fallback: Boolean): Boolean =
    o.get(k)?.takeIf { it.isJsonPrimitive }?.asBoolean ?: fallback

private fun int(o: JsonObject, k: String, fallback: Int): Int =
    o.get(k)?.takeIf { it.isJsonPrimitive }?.asInt ?: fallback

private fun ruleFrom(o: JsonObject) = Rule(
    name = str(o, "name") ?: "",
    pattern = str(o, "pattern") ?: "",
    replacement = str(o, "replacement") ?: "",
    isRegex = bool(o, "isRegex", true),
    isEnabled = bool(o, "isEnabled", true),
    scope = str(o, "scope"),
    excludeScope = str(o, "excludeScope"),
    scopeTitle = bool(o, "scopeTitle", false),
    scopeContent = bool(o, "scopeContent", true),
    order = int(o, "order", Int.MIN_VALUE),
)

private fun selected(rules: List<Rule>, target: String, name: String, origin: String): List<Rule> =
    rules.filter { rule ->
        rule.isEnabled &&
            (if (target == "title") rule.scopeTitle else rule.scopeContent) &&
            (rule.scope == null || rule.scope.isEmpty() ||
                rule.scope.contains(name) || rule.scope.contains(origin)) &&
            (rule.excludeScope == null ||
                (!rule.excludeScope.contains(name) && !rule.excludeScope.contains(origin)))
    }.sortedBy { it.order }

private fun replaceRegex(text: String, pattern: String, replacement: String): String {
    val matcher = pattern.toRegex().toPattern().matcher(text)
    val isJs = replacement.startsWith("@js:")
    val literal = if (isJs) replacement.substring(4) else replacement
    val buffer = StringBuffer()
    while (matcher.find()) {
        if (isJs) {
            val jsResult = RhinoScriptEngine.eval(literal) {
                this["result"] = matcher.group()
            }.toString()
            matcher.appendReplacement(buffer, jsResult)
        } else {
            matcher.appendReplacement(buffer, literal)
        }
    }
    matcher.appendTail(buffer)
    return buffer.toString()
}

private fun applyRules(source: String, rules: List<Rule>, title: Boolean): String {
    var text = source
    for (rule in rules) {
        if (rule.pattern.isEmpty()) continue
        val next = try {
            if (rule.isRegex) replaceRegex(text, rule.pattern, rule.replacement)
            else text.replace(rule.pattern, rule.replacement)
        } catch (e: Exception) {
            continue
        }
        if (title && next.isBlank()) continue
        text = next
    }
    return text
}

fun main() {
    val reader = BufferedReader(InputStreamReader(System.`in`, Charsets.UTF_8))
    val writer = System.out.bufferedWriter()
    reader.forEachLine { line ->
        if (line.isBlank()) return@forEachLine
        val out = JsonObject()
        try {
            val req = JsonParser.parseString(line).asJsonObject
            out.addProperty("id", str(req, "id") ?: "")
            val target = str(req, "target") ?: "content"
            val text = str(req, "text") ?: ""
            val name = str(req, "bookName") ?: ""
            val origin = str(req, "bookOrigin") ?: ""
            val rules = req.getAsJsonArray("rules")?.map { ruleFrom(it.asJsonObject) } ?: emptyList()
            val picked = selected(rules, target, name, origin)
            out.addProperty("ok", true)
            out.addProperty("result", applyRules(text, picked, target == "title"))
        } catch (t: Throwable) {
            out.addProperty("ok", false)
            out.addProperty("error", (t.toString() + " | " + t.message).take(500))
        }
        writer.write(out.toString())
        writer.newLine()
        writer.flush()
    }
}
