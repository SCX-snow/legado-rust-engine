use std::collections::HashMap;
use std::future::Future;
use std::process::ExitCode;
use std::task::{Context, Poll, Waker};
use std::time::Duration;

use legado_rust_engine::core::{
    Book, BookChapter, BookSource, Bytes, CacheNamespace, CacheStore, Error, FileCacheStore,
    JsVariables, MemoryCacheStore, ReplaceRule, Result, RuleError, Source, Variables,
};

const LEGACY_SOURCE: &str = r#"{
    "bookSourceUrl": "https://example.com",
    "bookSourceName": "示例源",
    "bookSourceType": "0",
    "enabled": "true",
    "customOrder": 3.0,
    "ruleContent": {"content": "id.content@html", "imageStyle": 2},
    "ruleSearch": "{\"bookList\": \"class.book\", \"name\": \"tag.h3@text\", \"bookUrl\": \"tag.a@href\"}",
    "unknownFieldFromNewerApp": {"a": 1}
}"#;

#[derive(Default)]
struct Checks {
    passed: usize,
    failures: Vec<String>,
}

impl Checks {
    fn eq(&mut self, label: &str, actual: impl std::fmt::Display, expected: &str) {
        let actual = actual.to_string();
        if actual == expected {
            self.passed += 1;
            println!("  ok   {label}");
        } else {
            let message = format!("{label}: 得到 {actual:?}，期望 {expected:?}");
            println!("  FAIL {message}");
            self.failures.push(message);
        }
    }

    fn truthy(&mut self, label: &str, ok: bool) {
        self.eq(label, ok, "true");
    }

    fn contains(&mut self, label: &str, haystack: &str, needle: &str) {
        if haystack.contains(needle) {
            self.passed += 1;
            println!("  ok   {label}");
        } else {
            let message = format!("{label}: {haystack:?} 不含 {needle:?}");
            println!("  FAIL {message}");
            self.failures.push(message);
        }
    }

    fn guard(&mut self, expected: usize) {
        let ran = self.passed + self.failures.len();
        if ran == expected {
            println!("  ok   断言执行数 {expected}");
        } else {
            let message = format!("断言执行数 {ran}，预期 {expected}（有分支被跳过？）");
            self.failures.push(message.clone());
            println!("  FAIL {message}");
        }
    }
}

fn main() -> ExitCode {
    let mut checks = Checks::default();
    if let Err(error) = run(&mut checks) {
        eprintln!("{error}");
        return ExitCode::FAILURE;
    }
    checks.guard(45);
    println!();
    println!("{} 项通过，{} 项失败", checks.passed, checks.failures.len());
    if checks.failures.is_empty() {
        println!("模型用法自检通过");
        ExitCode::SUCCESS
    } else {
        for failure in &checks.failures {
            eprintln!("失败: {failure}");
        }
        ExitCode::FAILURE
    }
}

fn run(checks: &mut Checks) -> Result<()> {
    parse_source(checks)?;
    variables(checks)?;
    replace_rule(checks)?;
    js_variables(checks);
    cache(checks)?;
    file_cache(checks)?;
    rule_error(checks);
    Ok(())
}

fn parse_source(checks: &mut Checks) -> Result<()> {
    let source: BookSource = serde_json::from_str(LEGACY_SOURCE)?;
    checks.eq("书源名", source.tag(), "示例源");
    checks.eq("书源地址（key）", source.key(), "https://example.com");
    checks.eq("类型（字符串 \"0\"）", source.book_source_type, "0");
    checks.eq("启用（字符串 \"true\"）", source.enabled, "true");
    checks.eq("自定义排序（3.0）", source.custom_order, "3");

    let search = source
        .rule_search
        .as_ref()
        .expect("字符串形态的规则要能解析出来");
    checks.eq(
        "搜索列表规则（字符串里的 JSON）",
        search.list.book_list.as_deref().unwrap_or("-"),
        "class.book",
    );
    checks.eq(
        "搜索书名规则",
        search.list.name.as_deref().unwrap_or("-"),
        "tag.h3@text",
    );
    let content = source.rule_content.as_ref().expect("对象形态的规则");
    checks.eq(
        "正文规则（对象形态）",
        content.content.as_deref().unwrap_or("-"),
        "id.content@html",
    );
    Ok(())
}

fn variables(checks: &mut Checks) -> Result<()> {
    let book_json = r#"{
        "name": "斗破苍穹",
        "author": "天蚕土豆",
        "bookUrl": "https://example.com/book/1",
        "type": 0,
        "kind": "玄幻",
        "variable": "{\"page\":\"1\"}"
    }"#;
    let mut book: Book = serde_json::from_str(book_json)?;
    checks.eq("书名", &book.name, "斗破苍穹");
    checks.eq("作者", &book.author, "天蚕土豆");
    checks.eq("类型", book.book_type, "0");

    book.put_variable("page", Some("2"));
    book.put_variable("tmp", Some("用完就删"));
    book.put_variable("tmp", None);
    checks.eq("变量覆盖", book.get_variable("page").unwrap_or("-"), "2");
    checks.eq("变量删除", book.get_variable("tmp").unwrap_or("-"), "-");

    let back: Book = serde_json::from_str(&serde_json::to_string(&book)?)?;
    checks.truthy("book 往返一致", back == book);

    let mut chapter = BookChapter {
        title: "第一章".into(),
        ..Default::default()
    };
    chapter.put_variable("token", Some("abc123"));
    let json = serde_json::to_string(&chapter)?;
    let back: BookChapter = serde_json::from_str(&json)?;
    checks.truthy("章节变量往返一致", back.variables == chapter.variables);
    Ok(())
}

fn replace_rule(checks: &mut Checks) -> Result<()> {
    let rule: ReplaceRule = serde_json::from_str(
        r#"{"name": "去广告", "pattern": "广告.*?\\n", "scope": "正文", "isRegex": "true", "timeoutMillisecond": 5000}"#,
    )?;
    checks.eq("替换规则名", &rule.name, "去广告");
    checks.eq("isRegex（字符串 \"true\"）", rule.is_regex, "true");
    checks.eq("timeoutMillisecond", rule.timeout_millisecond, "5000");
    checks.eq("scope", rule.scope.as_deref().unwrap_or("-"), "正文");
    checks.eq("replacement 默认空串", &rule.replacement, "");
    checks.eq("scopeContent 默认 true", rule.scope_content, "true");
    checks.eq("isEnabled 默认 true", rule.is_enabled, "true");
    println!("  往返 JSON: {}", serde_json::to_string(&rule)?);
    Ok(())
}

fn js_variables(checks: &mut Checks) {
    let mut vars = JsVariables::new(
        Some(HashMap::from([
            ("k".to_string(), "chapter".to_string()),
            ("empty".to_string(), String::new()),
        ])),
        Some(HashMap::from([
            ("k".to_string(), "book".to_string()),
            ("empty".to_string(), "book".to_string()),
            ("only".to_string(), "book".to_string()),
        ])),
        None,
    );
    checks.eq("章节优先", vars.get("k").unwrap_or("-"), "chapter");
    checks.eq(
        "章节空串当没设，落到书籍",
        vars.get("empty").unwrap_or("-"),
        "book",
    );
    checks.eq("只在书籍里的", vars.get("only").unwrap_or("-"), "book");
    checks.eq("都没有", vars.get("miss").unwrap_or("-"), "-");

    vars.put("k", Some("wrote"));
    checks.eq(
        "有章节写章节",
        vars.chapter()
            .and_then(|map| map.get("k"))
            .map(String::as_str)
            .unwrap_or("-"),
        "wrote",
    );
    checks.eq(
        "写章节不动书籍",
        vars.book()
            .and_then(|map| map.get("k"))
            .map(String::as_str)
            .unwrap_or("-"),
        "book",
    );

    vars.put("k", None);
    checks.eq(
        "put None 删掉章节键后回落书籍",
        vars.get("k").unwrap_or("-"),
        "book",
    );

    let mut book_only = JsVariables::new(None, Some(HashMap::new()), None);
    book_only.put("b", Some("v"));
    checks.eq(
        "没有章节就写书籍",
        book_only
            .book()
            .and_then(|map| map.get("b"))
            .map(String::as_str)
            .unwrap_or("-"),
        "v",
    );
}

fn cache(checks: &mut Checks) -> Result<()> {
    let cache = MemoryCacheStore::new(16);
    let key = CacheNamespace::JsLib.key("https://example.com/lib.js");

    block_on(cache.put(
        &key,
        Bytes::from_static(b"var a = 1"),
        Some(Duration::from_secs(60)),
    ))?;
    checks.eq(
        "内存缓存命中",
        cache_text(block_on(cache.get(&key))?),
        "var a = 1",
    );

    let other = CacheNamespace::CacheFile.key("https://example.com/lib.js");
    checks.truthy("命名空间隔离", block_on(cache.get(&other))?.is_none());

    block_on(cache.delete(&key))?;
    checks.truthy("删除后取不到", block_on(cache.get(&key))?.is_none());
    Ok(())
}

fn file_cache(checks: &mut Checks) -> Result<()> {
    let root = std::env::temp_dir().join("legado-rust-engine-file-cache");
    let _ = std::fs::remove_dir_all(&root);
    let cache = FileCacheStore::new(&root)?;
    let key = CacheNamespace::CacheFile.key("https://example.com/book/1");

    block_on(cache.put(&key, Bytes::from("<html>正文</html>"), None))?;
    checks.eq(
        "文件缓存命中",
        cache_text(block_on(cache.get(&key))?),
        "<html>正文</html>",
    );
    let reopened = FileCacheStore::new(&root)?;
    checks.eq(
        "重新打开后仍命中",
        cache_text(block_on(reopened.get(&key))?),
        "<html>正文</html>",
    );

    let names = file_names(&root)?;
    checks.truthy(
        "落盘了缓存文件",
        names.iter().any(|name| name.ends_with(".data")),
    );
    checks.truthy(
        "文件名是哈希不是 URL",
        names.iter().all(|name| !name.contains("example.com")),
    );
    checks.truthy(
        "不带 TTL 时没有 .ttl",
        names.iter().all(|name| !name.ends_with(".ttl")),
    );

    let ttl_key = CacheNamespace::CacheFile.key("https://example.com/ttl");
    block_on(reopened.put(
        &ttl_key,
        Bytes::from_static(b"x"),
        Some(Duration::from_secs(60)),
    ))?;
    checks.truthy(
        "带 TTL 时旁挂 .ttl 文件",
        file_names(&root)?.iter().any(|name| name.ends_with(".ttl")),
    );

    let soon_key = CacheNamespace::CacheFile.key("https://example.com/soon");
    block_on(reopened.put(
        &soon_key,
        Bytes::from_static(b"x"),
        Some(Duration::from_millis(1)),
    ))?;
    std::thread::sleep(Duration::from_millis(20));
    checks.truthy(
        "TTL 过期后取不到",
        block_on(reopened.get(&soon_key))?.is_none(),
    );

    block_on(reopened.delete(&key))?;
    checks.truthy("删除后取不到", block_on(reopened.get(&key))?.is_none());
    let _ = std::fs::remove_dir_all(&root);
    Ok(())
}

fn file_names(root: &std::path::Path) -> Result<Vec<String>> {
    Ok(std::fs::read_dir(root)?
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect())
}

fn cache_text(value: Option<Bytes>) -> String {
    value
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
        .unwrap_or_else(|| "-".to_string())
}

fn rule_error(checks: &mut Checks) {
    let error = Error::Rule(RuleError::new(
        r#"@js:var a = "x"#,
        13..14,
        "字符串没有闭合",
    ));
    let text = error.to_string();
    checks.contains("错误位置（第 1 行第 14 列）", &text, "第 1 行第 14 列");
    checks.contains("错误带规则原文", &text, r#"@js:var a = "x"#);
    checks.contains("错误标出插入符", &text, "^");

    let multiline = RuleError::new("第一行\n第二行", 4..5, "第二行出错").to_string();
    checks.contains("多行规则的位置", &multiline, "第 2 行第 1 列");
}

fn block_on<F: Future>(future: F) -> F::Output {
    let mut context = Context::from_waker(Waker::noop());
    let mut future = Box::pin(future);
    loop {
        match future.as_mut().poll(&mut context) {
            Poll::Ready(output) => return output,
            Poll::Pending => std::thread::yield_now(),
        }
    }
}
