#[cfg(feature = "redis")]
mod checks {
    use std::process::ExitCode;
    use std::time::Duration;

    use legado_rust_engine::core::{Bytes, CacheNamespace, CacheStore, RedisCacheStore, Result};

    #[derive(Default)]
    struct Checks {
        passed: usize,
        failures: Vec<String>,
    }

    impl Checks {
        fn truthy(&mut self, label: &str, condition: bool, detail: impl std::fmt::Display) {
            if condition {
                self.passed += 1;
                println!("  ✓ {label}");
            } else {
                self.failures.push(format!("{label}: {detail}"));
                println!("  ✗ {label}: {detail}");
            }
        }

        fn eq(&mut self, label: &str, actual: impl std::fmt::Display, expected: &str) {
            let actual = actual.to_string();
            self.truthy(
                label,
                actual == expected,
                format!("得到 {actual:?}，期望 {expected:?}"),
            );
        }

        fn section(&self, title: &str) {
            println!("\n== {title} ==");
        }

        fn guard(&mut self, expected: usize) {
            let ran = self.passed + self.failures.len();
            if ran == expected {
                println!("  ✓ 断言执行数 {expected}");
            } else {
                let message = format!("断言执行数 {ran}，预期 {expected}（有分支被跳过？）");
                self.failures.push(message.clone());
                println!("  ✗ {message}");
            }
        }
    }

    #[tokio::main(flavor = "current_thread")]
    pub async fn main() -> ExitCode {
        let url = std::env::args()
            .nth(1)
            .or_else(|| std::env::var("LEGADO_REDIS_URL").ok())
            .unwrap_or_else(|| "redis://127.0.0.1:6379".to_string());
        match run(&url).await {
            Ok(true) => ExitCode::SUCCESS,
            Ok(false) => ExitCode::FAILURE,
            Err(error) => {
                eprintln!("{url}: {error}");
                ExitCode::FAILURE
            }
        }
    }

    async fn run(url: &str) -> Result<bool> {
        let prefix = format!("legado-test-{}:", std::process::id());
        let cache = RedisCacheStore::connect_with_prefix(url, &prefix).await?;
        let mut checks = Checks::default();
        println!("Redis: {url}  前缀: {prefix}");

        let key = CacheNamespace::CacheFile.key("https://example.com/book/1");
        let raw = Bytes::from(vec![0u8, 1, 2, 0xff, 0xfe, b'x']);

        checks.section("写入 / 读出");
        cache.put(&key, raw.clone(), None).await?;
        checks.eq(
            "读回来的字节原样",
            format!("{:?}", cache.get(&key).await?),
            &format!("Some({raw:?})"),
        );

        let other = RedisCacheStore::connect_with_prefix(url, &prefix).await?;
        checks.truthy(
            "换一个连接仍读得到",
            other.get(&key).await? == Some(raw.clone()),
            "另一个连接读不到数据，说明没真写进 Redis",
        );
        checks.truthy(
            "不同前缀互不可见",
            RedisCacheStore::connect_with_prefix(url, "legado-other-prefix:")
                .await?
                .get(&key)
                .await?
                .is_none(),
            "换个前缀竟然读到了同一个 key",
        );
        let default_store = RedisCacheStore::connect(url).await?;
        let default_key = CacheNamespace::JsCache.key(&format!(
            "https://example.com/default-prefix/{}",
            std::process::id()
        ));
        default_store
            .put(&default_key, Bytes::from_static(b"d"), None)
            .await?;
        checks.truthy(
            "connect 默认前缀是 legado:",
            RedisCacheStore::connect_with_prefix(url, "legado:")
                .await?
                .get(&default_key)
                .await?
                .is_some(),
            "带 legado: 前缀的连接读不到 connect 写的数据（默认前缀变了？）",
        );

        checks.section("TTL");
        let short = CacheNamespace::CacheFile.key("https://example.com/soon");
        cache
            .put(
                &short,
                Bytes::from_static(b"x"),
                Some(Duration::from_millis(300)),
            )
            .await?;
        checks.truthy(
            "TTL 内读得到",
            cache.get(&short).await?.is_some(),
            "刚写进去就读不到",
        );
        tokio::time::sleep(Duration::from_millis(450)).await;
        checks.truthy(
            "TTL 到期后读不到",
            cache.get(&short).await?.is_none(),
            "过期了还能读到",
        );
        let ttl_zero = CacheNamespace::CacheFile.key("https://example.com/forever");
        cache
            .put(&ttl_zero, Bytes::from_static(b"y"), Some(Duration::ZERO))
            .await?;
        tokio::time::sleep(Duration::from_millis(50)).await;
        checks.truthy(
            "ttl=0 当不过期",
            cache.get(&ttl_zero).await?.is_some(),
            "ttl=0 被当成立即过期了",
        );

        checks.section("命名空间与删除");
        let same_raw = "https://example.com/same";
        let file_key = CacheNamespace::CacheFile.key(same_raw);
        let js_key = CacheNamespace::JsCache.key(same_raw);
        cache
            .put(&file_key, Bytes::from_static(b"file"), None)
            .await?;
        cache.put(&js_key, Bytes::from_static(b"js"), None).await?;
        checks.eq(
            "同名不同命名空间互不覆盖",
            format!(
                "{}/{}",
                String::from_utf8_lossy(&cache.get(&file_key).await?.unwrap_or_default()),
                String::from_utf8_lossy(&cache.get(&js_key).await?.unwrap_or_default())
            ),
            "file/js",
        );
        cache.delete(&file_key).await?;
        checks.truthy(
            "delete 之后读不到",
            cache.get(&file_key).await?.is_none(),
            "删了还在",
        );
        checks.truthy(
            "delete 不误伤别的 key",
            cache.get(&js_key).await?.is_some(),
            "把另一个命名空间的 key 也删了",
        );

        let _ = default_store.delete(&default_key).await;
        for key in [&key, &short, &ttl_zero, &file_key, &js_key] {
            let _ = cache.delete(key).await;
        }

        checks.guard(10);
        println!(
            "\n{} 项通过，{} 项失败",
            checks.passed,
            checks.failures.len()
        );
        if checks.failures.is_empty() {
            println!("Redis 缓存自检通过");
            Ok(true)
        } else {
            for failure in &checks.failures {
                eprintln!("失败: {failure}");
            }
            Ok(false)
        }
    }
}

#[cfg(feature = "redis")]
fn main() -> std::process::ExitCode {
    checks::main()
}

#[cfg(not(feature = "redis"))]
fn main() -> std::process::ExitCode {
    eprintln!("这个 example 要开 redis feature：cargo run --example cache_redis --features redis");
    std::process::ExitCode::FAILURE
}
