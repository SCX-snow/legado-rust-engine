package io.legado.app.help

import io.legado.app.model.analyzeRule.QueryTTF
import java.util.concurrent.ConcurrentHashMap

object CacheManager {

    private val memory = ConcurrentHashMap<String, Any>()
    private val disk = ConcurrentHashMap<String, String>()

    fun put(key: String, value: Any, saveTime: Int = 0) {
        memory[key] = value
        if (value is String) disk[key] = value
    }

    fun putMemory(key: String, value: Any) {
        memory[key] = value
    }

    fun getFromMemory(key: String): Any? = memory[key]

    fun deleteMemory(key: String) {
        memory.remove(key)
    }

    fun get(key: String): String? = (memory[key] ?: disk[key])?.toString()

    fun getInt(key: String): Int? = get(key)?.toIntOrNull()

    fun getLong(key: String): Long? = get(key)?.toLongOrNull()

    fun getDouble(key: String): Double? = get(key)?.toDoubleOrNull()

    fun getFloat(key: String): Float? = get(key)?.toFloatOrNull()

    fun getByteArray(key: String): ByteArray? = memory[key] as? ByteArray

    fun getQueryTTF(key: String): QueryTTF? = memory[key] as? QueryTTF

    fun putFile(key: String, value: String, saveTime: Int = 0) {
        disk[key] = value
    }

    fun getFile(key: String): String? = disk[key]

    fun delete(key: String) {
        memory.remove(key)
        disk.remove(key)
    }
}
