use std::process::ExitCode;
use std::sync::Arc;

use legado_rust_engine::core::{
    BoxFuture, Error, HeaderMap, HttpResponse, Result, RssSource, StatusCode,
};
use legado_rust_engine::js::JsEngine;
use legado_rust_engine::net::UrlRule;
use legado_rust_engine::rss::{RssEngine, get_image_url, parse_default};
use legado_rust_engine::rule::RuleEngine;

const LIST: &str = r#"<html><body>
<div class="item"><h3>条目一</h3><span class="date">2024-01-01</span>
  <p class="desc">摘要一</p><img src="/img/1.jpg"><a href="/post/1">正文</a></div>
<div class="item"><h3>条目二</h3><span class="date">2024-01-02</span>
  <p class="desc">摘要二</p><img src="/img/2.jpg"><a href="/post/2">正文</a></div>
<a class="next" href="/list?page=2">下一页</a>
</body></html>"#;

const ARTICLE: &str =
    r#"<html><body><div class="article"><p>第一段</p><p>第二段</p></div></body></html>"#;

const FEED: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0" xmlns:media="http://search.yahoo.com/mrss/">
<channel><title>测试源</title>
<item>
  <title>条目一</title>
  <link>https://feed.test/a1</link>
  <description><![CDATA[<p>摘要一</p><img src="https://feed.test/i1.jpg">]]></description>
  <pubDate>Mon, 01 Jan 2024 00:00:00 GMT</pubDate>
</item>
<item>
  <title>条目二</title>
  <link>https://feed.test/a2</link>
  <media:thumbnail url="https://feed.test/i2.jpg"/>
  <content:encoded xmlns:content="http://purl.org/rss/1.0/modules/content/"><![CDATA[<div>正文二</div>]]></content:encoded>
  <time>2024-01-02</time>
</item>
</channel></rss>"#;

const ENTITY_FEED: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0"><channel>
<item>
  <title>A &amp; B &#8212; C</title>
  <link>https://feed.test/a?x=1&amp;y=2</link>
</item>
</channel></rss>"#;

struct FakeFetcher;

impl legado_rust_engine::book::PageFetcher for FakeFetcher {
    fn fetch_rule<'a>(&'a self, rule: &'a UrlRule) -> BoxFuture<'a, Result<HttpResponse>> {
        Box::pin(async move {
            let url = rule.url();
            let text =
                page_for(&url).ok_or_else(|| Error::Other(format!("没有这个假页面: {url}")))?;
            Ok(HttpResponse {
                url,
                status: StatusCode::OK,
                headers: HeaderMap::new(),
                body: text.into(),
                text: text.to_string(),
            })
        })
    }
}

fn page_for(url: &str) -> Option<&'static str> {
    if url.contains("feed.test/rss") {
        Some(FEED)
    } else if url.contains("rss.test/list") {
        Some(LIST)
    } else if url.ends_with("rss.test/post/1") || url.ends_with("rss.test/post/2") {
        Some(ARTICLE)
    } else {
        None
    }
}

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
            self.failures
                .push(format!("{label}: 得到 {actual:?}，期望 {expected:?}"));
            println!("  FAIL {label}: 得到 {actual:?}，期望 {expected:?}");
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

fn rule_source() -> RssSource {
    serde_json::from_value(serde_json::json!({
        "sourceUrl": "https://rss.test/feed",
        "sourceName": "离线订阅源",
        "ruleArticles": "class.item",
        "ruleTitle": "h3@text",
        "rulePubDate": "span.date@text",
        "ruleDescription": "p.desc@text",
        "ruleImage": "img@src",
        "ruleLink": "a@href",
        "ruleNextPage": "a.next@href",
        "ruleContent": "div.article@html"
    }))
    .expect("测试订阅源 JSON")
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    let mut checks = Checks::default();
    default_checks(&mut checks);
    match rule_checks(&mut checks).await {
        Ok(()) => {}
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    }

    checks.guard(35);
    println!();
    println!("{} 项通过，{} 项失败", checks.passed, checks.failures.len());
    if checks.failures.is_empty() {
        println!("M5 RSS 离线自检通过");
        ExitCode::SUCCESS
    } else {
        for failure in &checks.failures {
            eprintln!("失败: {failure}");
        }
        ExitCode::FAILURE
    }
}

fn default_checks(checks: &mut Checks) {
    println!("== 默认 RSS 解析");
    let articles = parse_default("默认", FEED, "https://feed.test/rss");
    checks.eq("条目数", articles.len(), "2");
    checks.eq("第一条标题", &articles[0].title, "条目一");
    checks.eq("第一条链接", &articles[0].link, "https://feed.test/a1");
    checks.eq(
        "第一条摘要",
        articles[0].description.as_deref().unwrap_or("-"),
        r#"<p>摘要一</p><img src="https://feed.test/i1.jpg">"#,
    );
    checks.eq(
        "第一条配图（从 description 提）",
        articles[0].image.as_deref().unwrap_or("-"),
        "https://feed.test/i1.jpg",
    );
    checks.eq(
        "第一条时间（pubDate 原样）",
        articles[0].pub_date.as_deref().unwrap_or("-"),
        "Mon, 01 Jan 2024 00:00:00 GMT",
    );
    checks.eq(
        "第二条配图（media:thumbnail）",
        articles[1].image.as_deref().unwrap_or("-"),
        "https://feed.test/i2.jpg",
    );
    checks.eq(
        "第二条正文（content:encoded，无 CDATA 壳）",
        articles[1].content.as_deref().unwrap_or("-"),
        "<div>正文二</div>",
    );
    checks.eq(
        "第二条时间（time）",
        articles[1].pub_date.as_deref().unwrap_or("-"),
        "2024-01-02",
    );
    checks.eq("origin", &articles[0].origin, "https://feed.test/rss");
    checks.eq("sort", &articles[0].sort, "默认");

    let entity = parse_default("实体", ENTITY_FEED, "https://feed.test/rss");
    checks.eq(
        "实体还原（预定义 + 字符引用）",
        &entity[0].title,
        "A & B — C",
    );
    checks.eq(
        "链接里的实体",
        &entity[0].link,
        "https://feed.test/a?x=1&y=2",
    );

    checks.eq(
        "get_image_url 取第一个 img",
        get_image_url(r#"<p>x</p><img src="https://x/1.jpg"><img src="https://x/2.jpg">"#)
            .as_deref()
            .unwrap_or("-"),
        "https://x/1.jpg",
    );
    checks.eq(
        "get_image_url 认 `=` 两侧的空白",
        get_image_url(r#"<img src = "https://x/3.jpg">"#)
            .as_deref()
            .unwrap_or("-"),
        "https://x/3.jpg",
    );
    checks.eq(
        "get_image_url 没有 img 给 None",
        get_image_url("<p>没有图片</p>").as_deref().unwrap_or("-"),
        "-",
    );
}

async fn rule_checks(checks: &mut Checks) -> Result<()> {
    let rules = Arc::new(RuleEngine::new());
    let js = Arc::new(JsEngine::builder().rules(rules.clone()).build()?);
    rules.set_js(js.clone());
    let fetcher: Arc<dyn legado_rust_engine::book::PageFetcher> = Arc::new(FakeFetcher);
    let engine = RssEngine::new(rules, js, fetcher);

    println!();
    println!("== 规则解析");
    let source = rule_source();
    let (articles, next_url) = engine
        .articles(&source, "分类", "https://list.rss.test/list?page=1", 1)
        .await?;
    checks.eq("条目数", articles.len(), "2");
    checks.eq("第一条标题", &articles[0].title, "条目一");
    checks.eq(
        "第一条时间",
        articles[0].pub_date.as_deref().unwrap_or("-"),
        "2024-01-01",
    );
    checks.eq(
        "第一条摘要",
        articles[0].description.as_deref().unwrap_or("-"),
        "摘要一",
    );
    checks.eq(
        "第一条配图（相对 sortUrl 补全）",
        articles[0].image.as_deref().unwrap_or("-"),
        "https://list.rss.test/img/1.jpg",
    );
    checks.eq(
        "第一条链接（相对 sourceUrl 补全）",
        &articles[0].link,
        "https://rss.test/post/1",
    );
    checks.eq(
        "origin / sort",
        format!("{}|{}", articles[0].origin, articles[0].sort),
        "https://rss.test/feed|分类",
    );
    checks.eq(
        "下一页（相对 sortUrl 补全）",
        next_url.as_deref().unwrap_or("-"),
        "https://list.rss.test/list?page=2",
    );

    println!();
    println!("== RSS 变量（源 variable → 每篇文章，@get / @put / java.get / java.put）");
    {
        let variable_source: RssSource = serde_json::from_value(serde_json::json!({
            "sourceUrl": "https://rss.test/feed",
            "sourceName": "变量源",
            "variable": "{\"site\":\"离线订阅源\"}",
            "ruleArticles": "class.item",
            "ruleTitle": "@put:{seen:h3@text}@get:{site}-@get:{seen}",
            "ruleLink": "a@href",
            "ruleDescription": "@js:java.get('site') + '/' + java.put('tag', 'x') + java.get('tag')"
        }))
        .expect("变量订阅源 JSON");
        let (variable_articles, _) = engine
            .articles(
                &variable_source,
                "分类",
                "https://list.rss.test/list?page=1",
                1,
            )
            .await?;
        checks.eq(
            "源的 variable 读得到、同一条规则里的 @put 也读得到",
            &variable_articles[0].title,
            "离线订阅源-条目一",
        );
        checks.eq(
            "每篇文章一份变量（第二条自己那份）",
            &variable_articles[1].title,
            "离线订阅源-条目二",
        );
        checks.eq(
            "java.get 读源变量、java.put 后 java.get 读得到（java.log 返回实参）",
            variable_articles[0].description.as_deref().unwrap_or("-"),
            "离线订阅源/xx",
        );
        checks.eq(
            "文章变量落在模型上（@put 写回 article.variables）",
            variable_articles[0]
                .variables
                .get("seen")
                .map(String::as_str)
                .unwrap_or("-"),
            "条目一",
        );
    }

    println!();
    println!("== 文章正文");
    let content = engine
        .content(&source, &articles[0], "div.article@html")
        .await?;
    checks.eq(
        "正文（@html 整体 HTML，Jsoup prettyPrint 口径）",
        &content,
        "<div class=\"article\">\n <p>第一段</p>\n <p>第二段</p>\n</div>",
    );

    println!();
    println!("== ruleArticles 为空时回退默认解析");
    let default_source: RssSource = serde_json::from_value(serde_json::json!({
        "sourceUrl": "https://feed.test/rss",
        "sourceName": "默认解析源"
    }))
    .expect("默认解析源 JSON");
    let (articles, next) = engine
        .articles(&default_source, "默认", "https://feed.test/rss", 1)
        .await?;
    checks.eq("回退后条目数", articles.len(), "2");
    checks.eq("回退后标题", &articles[0].title, "条目一");
    checks.eq("回退后无下一页", next.is_none(), "true");

    println!();
    println!("== ruleArticles 的 - 反序");
    let reverse_source: RssSource = serde_json::from_value(serde_json::json!({
        "sourceUrl": "https://rss.test",
        "sourceName": "反序源",
        "ruleArticles": "-class.item",
        "ruleTitle": "h3@text",
        "ruleLink": "a@href"
    }))
    .expect("反序源 JSON");
    let (articles, _) = engine
        .articles(&reverse_source, "分类", "https://rss.test/list?page=1", 1)
        .await?;
    checks.eq("反序后条目数", articles.len(), "2");
    checks.eq("反序后第一条", &articles[0].title, "条目二");
    checks.eq("反序后第二条", &articles[1].title, "条目一");
    Ok(())
}
