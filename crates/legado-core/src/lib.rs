pub mod cache;
pub mod error;
pub mod model;
pub mod serde_ext;
pub mod traits;

#[cfg(feature = "redis")]
pub use cache::RedisCacheStore;
pub use cache::{CacheNamespace, CacheStore, FileCacheStore, MemoryCacheStore};
pub use error::{Error, Position, Result, RuleError};
pub use model::{
    Book, BookChapter, BookInfoRule, BookListRule, BookSource, ContentRule, ExploreRule,
    ReplaceRule, ReviewRule, RssArticle, RssSource, SearchBook, SearchRule, Source, SourceBase,
    TocRule, Variables,
};
pub use traits::{
    CookieStore, HttpFetcher, HttpRequest, HttpResponse, JsEvaluator, JsRequest, JsValue,
    JsVariables, RuleRequest, RuleRunner, java_list_to_string,
};

pub use bytes::Bytes;
pub use http::{HeaderMap, Method, StatusCode};

pub type BoxFuture<'a, T> = std::pin::Pin<Box<dyn std::future::Future<Output = T> + Send + 'a>>;

pub type LocalBoxFuture<'a, T> = std::pin::Pin<Box<dyn std::future::Future<Output = T> + 'a>>;
