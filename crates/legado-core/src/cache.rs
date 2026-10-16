use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use bytes::Bytes;
use moka::policy::Expiry;
use moka::sync::Cache;

use crate::{BoxFuture, Error, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheNamespace {
    CacheFile,
    JsLib,
    QueryTtf,
    JsCache,
}

impl CacheNamespace {
    pub const fn prefix(self) -> &'static str {
        match self {
            Self::CacheFile => "cacheFile",
            Self::JsLib => "jsLib",
            Self::QueryTtf => "queryTTF",
            Self::JsCache => "jsCache",
        }
    }

    pub fn key(self, raw: &str) -> String {
        format!("{}:{}", self.prefix(), raw)
    }
}

pub trait CacheStore: Send + Sync {
    fn get<'a>(&'a self, key: &'a str) -> BoxFuture<'a, Result<Option<Bytes>>>;
    fn put<'a>(
        &'a self,
        key: &'a str,
        value: Bytes,
        ttl: Option<Duration>,
    ) -> BoxFuture<'a, Result<()>>;
    fn delete<'a>(&'a self, key: &'a str) -> BoxFuture<'a, Result<()>>;
}

#[derive(Clone)]
struct Entry {
    value: Bytes,
    ttl: Option<Duration>,
}

struct EntryExpiry;

impl Expiry<String, Entry> for EntryExpiry {
    fn expire_after_create(
        &self,
        _key: &String,
        value: &Entry,
        _created_at: Instant,
    ) -> Option<Duration> {
        value.ttl
    }
}

pub struct MemoryCacheStore {
    cache: Cache<String, Entry>,
}

impl MemoryCacheStore {
    pub fn new(max_entries: u64) -> Self {
        Self {
            cache: Cache::builder()
                .max_capacity(max_entries)
                .expire_after(EntryExpiry)
                .build(),
        }
    }
}

impl Default for MemoryCacheStore {
    fn default() -> Self {
        Self::new(1024)
    }
}

impl CacheStore for MemoryCacheStore {
    fn get<'a>(&'a self, key: &'a str) -> BoxFuture<'a, Result<Option<Bytes>>> {
        Box::pin(async move { Ok(self.cache.get(key).map(|entry| entry.value)) })
    }

    fn put<'a>(
        &'a self,
        key: &'a str,
        value: Bytes,
        ttl: Option<Duration>,
    ) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let ttl = ttl.filter(|ttl| !ttl.is_zero());
            self.cache.insert(key.to_string(), Entry { value, ttl });
            Ok(())
        })
    }

    fn delete<'a>(&'a self, key: &'a str) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            self.cache.invalidate(key);
            Ok(())
        })
    }
}

pub struct FileCacheStore {
    root: PathBuf,
}

impl FileCacheStore {
    pub fn new(root: impl AsRef<Path>) -> Result<Self> {
        let root = root.as_ref().to_path_buf();
        std::fs::create_dir_all(&root)?;
        Ok(Self { root })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn path_of(&self, key: &str, suffix: &str) -> PathBuf {
        use std::hash::{Hash as _, Hasher as _};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        key.hash(&mut hasher);
        self.root.join(format!("{:016x}{suffix}", hasher.finish()))
    }

    fn alive(&self, key: &str) -> bool {
        let meta = self.path_of(key, ".ttl");
        let Ok(text) = std::fs::read_to_string(&meta) else {
            return true;
        };
        let Ok(expire_at) = text.trim().parse::<u128>() else {
            return true;
        };
        if now_millis() < expire_at {
            return true;
        }
        let _ = std::fs::remove_file(&meta);
        let _ = std::fs::remove_file(self.path_of(key, ".data"));
        false
    }
}

impl CacheStore for FileCacheStore {
    fn get<'a>(&'a self, key: &'a str) -> BoxFuture<'a, Result<Option<Bytes>>> {
        Box::pin(async move {
            if !self.alive(key) {
                return Ok(None);
            }
            match std::fs::read(self.path_of(key, ".data")) {
                Ok(bytes) => Ok(Some(bytes.into())),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
                Err(error) => Err(Error::Cache(format!("读缓存文件失败: {error}"))),
            }
        })
    }

    fn put<'a>(
        &'a self,
        key: &'a str,
        value: Bytes,
        ttl: Option<Duration>,
    ) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            std::fs::write(self.path_of(key, ".data"), &value)
                .map_err(|error| Error::Cache(format!("写缓存文件失败: {error}")))?;
            let meta = self.path_of(key, ".ttl");
            match ttl.filter(|ttl| !ttl.is_zero()) {
                Some(ttl) => {
                    let expire_at = now_millis() + ttl.as_millis();
                    std::fs::write(meta, expire_at.to_string())
                        .map_err(|error| Error::Cache(format!("写缓存 TTL 失败: {error}")))?;
                }
                None => {
                    let _ = std::fs::remove_file(meta);
                }
            }
            Ok(())
        })
    }

    fn delete<'a>(&'a self, key: &'a str) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            for suffix in [".data", ".ttl"] {
                match std::fs::remove_file(self.path_of(key, suffix)) {
                    Ok(()) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => {
                        return Err(Error::Cache(format!("删缓存文件失败: {error}")));
                    }
                }
            }
            Ok(())
        })
    }
}

fn now_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|since| since.as_millis())
        .unwrap_or_default()
}

#[cfg(feature = "redis")]
pub struct RedisCacheStore {
    connection: redis::aio::ConnectionManager,
    prefix: String,
}

#[cfg(feature = "redis")]
impl RedisCacheStore {
    pub async fn connect(url: &str) -> Result<Self> {
        Self::connect_with_prefix(url, "legado:").await
    }

    pub async fn connect_with_prefix(url: &str, prefix: &str) -> Result<Self> {
        let client = redis::Client::open(url)
            .map_err(|error| Error::Cache(format!("Redis 地址不合法: {error}")))?;
        let connection = redis::aio::ConnectionManager::new(client)
            .await
            .map_err(|error| Error::Cache(format!("连 Redis 失败: {error}")))?;
        Ok(Self {
            connection,
            prefix: prefix.to_string(),
        })
    }

    fn key_of(&self, key: &str) -> String {
        format!("{}{key}", self.prefix)
    }

    fn connection(&self) -> redis::aio::ConnectionManager {
        self.connection.clone()
    }
}

#[cfg(feature = "redis")]
impl CacheStore for RedisCacheStore {
    fn get<'a>(&'a self, key: &'a str) -> BoxFuture<'a, Result<Option<Bytes>>> {
        Box::pin(async move {
            let mut connection = self.connection();
            redis::cmd("GET")
                .arg(self.key_of(key))
                .query_async::<Option<Vec<u8>>>(&mut connection)
                .await
                .map(|value| value.map(Bytes::from))
                .map_err(|error| Error::Cache(format!("Redis GET 失败: {error}")))
        })
    }

    fn put<'a>(
        &'a self,
        key: &'a str,
        value: Bytes,
        ttl: Option<Duration>,
    ) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let mut connection = self.connection();
            let mut command = redis::cmd("SET");
            command.arg(self.key_of(key)).arg(value.as_ref());
            if let Some(millis) = ttl
                .filter(|ttl| !ttl.is_zero())
                .map(|ttl| ttl.as_millis().max(1))
            {
                command.arg("PX").arg(millis);
            }
            command
                .query_async::<()>(&mut connection)
                .await
                .map_err(|error| Error::Cache(format!("Redis SET 失败: {error}")))
        })
    }

    fn delete<'a>(&'a self, key: &'a str) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let mut connection = self.connection();
            redis::cmd("DEL")
                .arg(self.key_of(key))
                .query_async::<i64>(&mut connection)
                .await
                .map(|_| ())
                .map_err(|error| Error::Cache(format!("Redis DEL 失败: {error}")))
        })
    }
}
