use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use legado_rust_engine::book::PageFetcher;
use legado_rust_engine::core::{Result, RssSource};
use legado_rust_engine::js::JsEngine;
use legado_rust_engine::net::HttpClient;
use legado_rust_engine::rss::RssEngine;
use legado_rust_engine::rule::RuleEngine;

const FEEDS: [&str; 4] = [
    "https://hnrss.org/frontpage",
    "https://feeds.bbci.co.uk/news/rss.xml",
    "https://sspai.com/feed",
    "https://www.zhihu.com/rss",
];

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    let custom = std::env::args().nth(1);
    match run(custom).await {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

async fn run(custom: Option<String>) -> Result<bool> {
    let client = Arc::new(
        HttpClient::builder()
            .total_timeout(Duration::from_secs(20))
            .connect_timeout(Duration::from_secs(10))
            .build()?,
    );
    let rules = Arc::new(RuleEngine::new());
    let js = Arc::new(
        JsEngine::builder()
            .http(client.clone())
            .rules(rules.clone())
            .build()?,
    );
    rules.set_js(js.clone());
    let fetcher: Arc<dyn PageFetcher> = client.clone();
    let engine = RssEngine::new(rules, js, fetcher);

    let feeds: Vec<&str> = match &custom {
        Some(url) => vec![url.as_str()],
        None => FEEDS.to_vec(),
    };

    let mut default_ok = 0;
    let mut rule_ok = 0;
    for url in feeds {
        println!();
        println!("[{url}]");
        let source = source_for(url, None);
        match engine.articles(&source, "默认", url, 1).await {
            Ok((articles, _)) => {
                println!("   默认解析: {} 条", articles.len());
                if let Some(first) = articles.first() {
                    println!("     首条: 《{}》 {}", first.title, first.link);
                }
                if !articles.is_empty()
                    && articles
                        .iter()
                        .all(|a| !a.title.trim().is_empty() && !a.link.trim().is_empty())
                {
                    default_ok += 1;
                }
            }
            Err(error) => println!("   默认解析失败: {error}"),
        }

        let rule_source = source_for(
            url,
            Some(serde_json::json!({
                "ruleArticles": "item",
                "ruleTitle": "title@text",
                "ruleLink": "link@text",
                "rulePubDate": "pubDate@text"
            })),
        );
        match engine.articles(&rule_source, "默认", url, 1).await {
            Ok((articles, _)) => {
                println!("   规则解析: {} 条", articles.len());
                if let Some(first) = articles.first() {
                    println!("     首条: 《{}》 {}", first.title, first.link);
                }
                if !articles.is_empty() {
                    rule_ok += 1;
                }
            }
            Err(error) => println!("   规则解析失败: {error}"),
        }
    }

    println!();
    if default_ok > 0 && rule_ok > 0 {
        println!("OK 默认解析成功 {default_ok} 个 feed，规则解析成功 {rule_ok} 个");
        Ok(true)
    } else {
        eprintln!("默认解析成功 {default_ok}，规则解析成功 {rule_ok}；至少各要 1 个");
        Ok(false)
    }
}

fn source_for(url: &str, extra: Option<serde_json::Value>) -> RssSource {
    let mut value = serde_json::json!({
        "sourceUrl": url,
        "sourceName": "真实 feed",
        "enabled": true
    });
    if let Some(extra) = extra
        && let (Some(base), Some(extra)) = (value.as_object_mut(), extra.as_object())
    {
        for (key, item) in extra {
            base.insert(key.clone(), item.clone());
        }
    }
    serde_json::from_value(value).expect("feed 源 JSON")
}
