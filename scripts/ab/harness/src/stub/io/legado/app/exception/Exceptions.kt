package io.legado.app.exception

class NoStackTraceException(
    override val message: String?,
    override val cause: Throwable? = null
) : RuntimeException(message, cause) {
    override fun fillInStackTrace(): Throwable = this
}

class RegexTimeoutException(message: String? = null) : RuntimeException(message)

class ConcurrentException(message: String? = null) : RuntimeException(message)
