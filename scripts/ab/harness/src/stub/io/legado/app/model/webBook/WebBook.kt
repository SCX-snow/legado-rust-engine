package io.legado.app.model.webBook

import io.legado.app.data.entities.BaseSource
import io.legado.app.data.entities.Book
import io.legado.app.data.entities.BookSource
import kotlinx.coroutines.CoroutineScope

object WebBook {

    suspend fun preciseSearchAwait(
        scope: CoroutineScope,
        bookSource: BookSource,
        name: String,
        author: String,
    ): Result<Book> = throw UnsupportedOperationException(
        "WebBook stub: 规则 harness 不跑抓书链路"
    )

    suspend fun getBookInfoAwait(
        bookSource: BaseSource,
        book: Book,
        canReName: Boolean = true,
    ): Unit = throw UnsupportedOperationException(
        "WebBook stub: 规则 harness 不跑抓书链路"
    )
}
