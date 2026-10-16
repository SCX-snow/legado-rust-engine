use std::process::ExitCode;
use std::sync::Arc;

use legado_rust_engine::book::{BookEngine, PageFetcher, search_book_to_book};
use legado_rust_engine::core::{
    BookSource, BoxFuture, Error, HeaderMap, HttpResponse, ReplaceRule, Result, StatusCode,
};
use legado_rust_engine::js::JsEngine;
use legado_rust_engine::net::UrlRule;
use legado_rust_engine::rule::RuleEngine;

const SEARCH: &str = r#"<html><body><!--NEED_LOGIN-->
<div class="book"><h3>【斗破苍穹】</h3><span class="author">作者：天蚕土豆</span>
  <span class="kind">玄幻</span><span class="kind">热血</span>
  <a class="url" href="/book/1.html">详情</a><img class="cover" src="/img/1.jpg">
  <p class="intro">简介一</p><span class="last">最新章节一</span><span class="wc">123456</span></div>
<div class="book"><h3>【斗破苍穹】</h3><span class="author">作者：天蚕土豆</span>
  <span class="kind">玄幻</span><span class="kind">热血</span>
  <a class="url" href="/book/1.html">详情</a><img class="cover" src="/img/1.jpg">
  <p class="intro">简介一</p><span class="last">最新章节一</span><span class="wc">123456</span></div>
<div class="book"><h3>【完美世界】</h3><span class="author">作者：辰东</span>
  <a class="url" href="/book/2.html">详情</a></div>
</body></html>"#;

const INFO: &str = r#"<html><body>
<h1 class="name">斗破苍穹</h1><span class="author">天蚕土豆</span>
<div class="intro">这是简介</div>
<div class="kind">玄幻</div><div class="kind">热血</div>
<img class="cover" src="/img/1.jpg"><span class="last">第100章</span><span class="wc">123456</span>
<a class="toc" href="/book/1/toc.html">目录</a>
</body></html>"#;

const TOC1: &str = r#"<html><body>
<div class="chapter" data-vip="true" data-pay="true"><a href="c1.html">第1章</a></div>
<div class="chapter" data-vip="true"><a href="c2.html">第2章</a></div>
<div class="chapter" data-vip="true"><a href="c2.html">第2章</a></div>
<a class="next" href="/book/1/toc2.html">下一页</a>
</body></html>"#;

const TOC2: &str = r#"<html><body>
<div class="chapter"><a href="c3.html">第3章</a></div>
<div class="chapter" data-volume="true"><a>第三卷</a></div>
</body></html>"#;

const CONTENT1: &str = r#"<html><body><h1>第1章</h1>
<div class="content">&nbsp;<p>正文第一段</p><p>正文第二段</p><img src="/img/pic.jpg"></div>
<a class="next" href="/book/1/c1_2.html">下一页</a>
</body></html>"#;

const CONTENT2: &str = r#"<html><body><div class="content"><p>正文第三段</p></div></body></html>"#;

const CONTENT_C2: &str = r#"<html><body><div class="content"><p>第二章正文</p></div>
<a class="next" href="/book/1/c3.html">下一页</a>
</body></html>"#;

const CONTENT_C3: &str =
    r#"<html><body><div class="content"><p>第三章正文</p></div></body></html>"#;

const DETAIL: &str = r#"<html><body><h1 class="name">详情页命中</h1><span class="author">某作者</span>
<a class="toc" href="/book/9/toc.html">目录</a></body></html>"#;

struct FakeFetcher;

impl PageFetcher for FakeFetcher {
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
    if url.contains("book.test/search") {
        Some(SEARCH)
    } else if url.contains("detail.test/s") {
        Some(DETAIL)
    } else if url.contains("explore.test/list") {
        Some(TOC1)
    } else if url.ends_with("book.test/book/1.html") {
        Some(INFO)
    } else if url.ends_with("book.test/book/1/toc2.html") {
        Some(TOC2)
    } else if url.ends_with("book.test/book/1/toc.html") {
        Some(TOC1)
    } else if url.ends_with("book.test/book/1/c1_2.html") {
        Some(CONTENT2)
    } else if url.ends_with("book.test/book/1/c3.html") {
        Some(CONTENT_C3)
    } else if url.ends_with("book.test/book/1/c2.html") {
        Some(CONTENT_C2)
    } else if url.ends_with("book.test/book/1/c1.html") {
        Some(CONTENT1)
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
            println!("  ✓ {label}");
        } else {
            self.failures
                .push(format!("{label}: 得到 {actual:?}，期望 {expected:?}"));
            println!("  ✗ {label}: 得到 {actual:?}，期望 {expected:?}");
        }
    }

    fn contains(&mut self, label: &str, haystack: &str, needle: &str) {
        if haystack.contains(needle) {
            self.passed += 1;
            println!("  ✓ {label}");
        } else {
            self.failures
                .push(format!("{label}: {haystack:?} 不含 {needle:?}"));
            println!("  ✗ {label}: 不含 {needle:?}");
        }
    }

    fn truthy(&mut self, label: &str, condition: bool, detail: impl std::fmt::Display) {
        if condition {
            self.passed += 1;
            println!("  ✓ {label}");
        } else {
            self.failures.push(format!("{label}: {detail}"));
            println!("  ✗ {label}: {detail}");
        }
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

fn source() -> BookSource {
    serde_json::from_value(serde_json::json!({
        "bookSourceUrl": "https://book.test",
        "bookSourceName": "离线测试源",
        "bookSourceType": 0,
        "searchUrl": "/search?q={{key}}&page={{page}}",
        "loginCheckJs": "result.body().includes('NEED_LOGIN') ? result.body().replace('</body>', '<div class=\"book\"><h3>登录后书</h3><span class=\"author\">作者：匿名</span><a class=\"url\" href=\"/book/9.html\">详情</a></div></body>') : result",
        "ruleSearch": {
            "bookList": "class.book",
            "name": "@js:java.getString('h3@text').replace(/[【】]/g,'')",
            "author": "span.author@text",
            "kind": "span.kind@text",
            "intro": "p.intro@text",
            "bookUrl": "a.url@href",
            "coverUrl": "img.cover@src",
            "lastChapter": "span.last@text",
            "wordCount": "span.wc@text"
        },
        "ruleBookInfo": {
            "name": "@js:java.put('infoToken','abc'); java.getString('h1.name@text')",
            "author": "span.author@text",
            "kind": "div.kind@text",
            "intro": "div.intro@text",
            "coverUrl": "img.cover@src",
            "lastChapter": "span.last@text",
            "wordCount": "span.wc@text",
            "tocUrl": "a.toc@href"
        },
        "ruleToc": {
            "chapterList": "div.chapter",
            "chapterName": "@put:{cname:a@text}a@text",
            "chapterUrl": "a@href",
            "isVip": "div.chapter@data-vip",
            "isPay": "div.chapter@data-pay",
            "isVolume": "div.chapter@data-volume",
            "nextTocUrl": "a.next@href"
        },
        "ruleContent": {
            "title": "h1@text",
            "content": "div.content@html",
            "nextContentUrl": "a.next@href",
            "replaceRegex": "##第二段##第二段（改）"
        }
    }))
    .expect("测试书源 JSON")
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    match run().await {
        Ok(checks) => {
            println!(
                "\n{} 项通过，{} 项失败",
                checks.passed,
                checks.failures.len()
            );
            if checks.failures.is_empty() {
                println!("M4 离线自检通过");
                ExitCode::SUCCESS
            } else {
                for failure in &checks.failures {
                    eprintln!("失败: {failure}");
                }
                ExitCode::FAILURE
            }
        }
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<Checks> {
    let rules = Arc::new(RuleEngine::new());
    let js = Arc::new(JsEngine::builder().rules(rules.clone()).build()?);
    rules.set_js(js.clone());
    let fetcher: Arc<dyn PageFetcher> = Arc::new(FakeFetcher);
    let engine = BookEngine::new(rules, js, fetcher);

    let source = source();
    let mut checks = Checks::default();

    println!("== 搜索（列表页 + 去重 + 相对地址 + loginCheckJs）");
    let results = engine.search(&source, "斗破", 1).await?;
    checks.eq("结果数量（去重后）", results.len(), "3");
    let first = &results[0];
    checks.eq("书名（@js 回头调规则）", &first.name, "斗破苍穹");
    checks.eq("作者格式化", &first.author, "天蚕土豆");
    checks.eq("分类", first.kind.as_deref().unwrap_or("-"), "玄幻,热血");
    checks.eq(
        "字数格式化",
        first.word_count.as_deref().unwrap_or("-"),
        "12.3万字",
    );
    checks.eq("简介", first.intro.as_deref().unwrap_or("-"), "简介一");
    checks.eq(
        "封面补全",
        first.cover_url.as_deref().unwrap_or("-"),
        "https://book.test/img/1.jpg",
    );
    checks.eq(
        "详情页补全",
        &first.book_url,
        "https://book.test/book/1.html",
    );
    checks.eq(
        "最新章节",
        first.latest_chapter_title.as_deref().unwrap_or("-"),
        "最新章节一",
    );
    checks.eq("第二本", &results[1].name, "完美世界");
    checks.eq("loginCheckJs 追加的书", &results[2].name, "登录后书");

    println!("\n== loginCheckJs 的另两种返回值（保持原响应）");
    let mut object_source = source.clone();
    object_source.base.login_check_js = Some("result".into());
    let via_object = engine.search(&object_source, "斗破", 1).await?;
    checks.eq("返回响应对象时正文不变", via_object.len(), "2");
    let mut other_source = source.clone();
    other_source.base.login_check_js = Some("undefined".into());
    let via_other = engine.search(&other_source, "斗破", 1).await?;
    checks.eq("返回其它值时保留原响应", via_other.len(), "2");

    println!("\n== 详情（java.put 书变量）");
    let mut book = search_book_to_book(first);
    engine.book_info(&source, &mut book).await?;
    checks.eq("书名", &book.name, "斗破苍穹");
    checks.eq("作者", &book.author, "天蚕土豆");
    checks.eq("分类", book.kind.as_deref().unwrap_or("-"), "玄幻,热血");
    checks.eq("简介", book.intro.as_deref().unwrap_or("-"), "这是简介");
    checks.eq(
        "封面补全",
        book.cover_url.as_deref().unwrap_or("-"),
        "https://book.test/img/1.jpg",
    );
    checks.eq(
        "目录补全",
        &book.toc_url,
        "https://book.test/book/1/toc.html",
    );
    checks.eq(
        "最新章节",
        book.latest_chapter_title.as_deref().unwrap_or("-"),
        "第100章",
    );
    checks.eq(
        "字数",
        book.word_count.as_deref().unwrap_or("-"),
        "12.3万字",
    );
    checks.eq(
        "java.put 书变量",
        book.variables
            .get("infoToken")
            .map(String::as_str)
            .unwrap_or("-"),
        "abc",
    );

    println!("\n== 目录（翻页 + 去重 + 卷 / VIP / 已购 + 章变量）");
    let mut chapters = engine.chapters(&source, &mut book).await?;
    checks.eq("章节数", chapters.len(), "4");
    checks.eq("第1章标题", &chapters[0].title, "第1章");
    checks.eq("第3章标题", &chapters[2].title, "第3章");
    checks.eq("第1章链接", &chapters[0].url, "c1.html");
    checks.eq(
        "章节 base_url",
        &chapters[0].base_url,
        "https://book.test/book/1/toc.html",
    );
    checks.eq("章节序号", chapters[2].index, "2");
    checks.truthy("第1章 VIP", chapters[0].is_vip, "应标记 VIP");
    checks.truthy("第3章非 VIP", !chapters[2].is_vip, "不应标记 VIP");
    checks.truthy("第1章 已购", chapters[0].is_pay, "应标记已购");
    checks.truthy("第3章非已购", !chapters[2].is_pay, "不应标记已购");
    checks.truthy("第4条是卷", chapters[3].is_volume, "应标记卷");
    checks.eq("卷的 URL 用标题+序号", &chapters[3].url, "第三卷1");
    checks.truthy("第1章非卷", !chapters[0].is_volume, "不应标记卷");
    checks.eq("book 目录总数", book.total_chapter_num, "4");
    checks.eq(
        "book 最新章节",
        book.latest_chapter_title.as_deref().unwrap_or("-"),
        "第三卷",
    );
    checks.eq(
        "@put 章变量",
        chapters[0]
            .variables
            .get("cname")
            .map(String::as_str)
            .unwrap_or("-"),
        "第1章",
    );

    println!("\n== 正文（翻页 + 图片绝对化 + replaceRegex）");
    let next = chapters.get(1).map(|chapter| chapter.url.clone());
    let text = engine
        .content(&source, &mut book, &mut chapters[0], next.as_deref())
        .await?;
    checks.eq("正文标题回写", &chapters[0].title, "第1章");
    checks.contains("第一页正文", &text, "正文第一段");
    checks.contains("第二页正文", &text, "正文第三段");
    checks.contains("replaceRegex", &text, "正文第二段（改）");
    checks.contains(
        "图片补全",
        &text,
        r#"<img src="https://book.test/img/pic.jpg">"#,
    );
    checks.truthy(
        "formatKeepImg 去掉段落标签",
        !text.contains("<p>") && text.contains("正文第一段"),
        &text,
    );

    println!("\n== 正文分页遇到下一章就停（nextContentUrl == 下一章）");
    let next = chapters.get(2).map(|chapter| chapter.url.clone());
    let second = engine
        .content(&source, &mut book, &mut chapters[1], next.as_deref())
        .await?;
    checks.truthy(
        "下一章不当作本章分页",
        second.contains("第二章正文") && !second.contains("第三章正文"),
        &second,
    );

    println!();
    println!("== 正文替换净化（content_with_replace / apply_title_replace）");
    let replace_rules: Vec<ReplaceRule> = serde_json::from_value(serde_json::json!([
        {"name":"回退（改）","pattern":"（改）","replacement":"","order":1}
    ]))
    .expect("替换规则 JSON");
    let replaced = engine
        .content_with_replace(&source, &mut book, &mut chapters[0], None, &replace_rules)
        .await?;
    checks.truthy(
        "替换规则去掉（改）",
        replaced.contains("正文第一段") && !replaced.contains("（改）"),
        &replaced,
    );
    let title_rules: Vec<ReplaceRule> = serde_json::from_value(serde_json::json!([
        {"name":"标题","pattern":"1","replacement":"一","scopeTitle":true,"scopeContent":false,"order":0}
    ]))
    .expect("标题规则 JSON");
    let outcome = engine
        .apply_title_replace(&book, &mut chapters[0], &title_rules)
        .await;
    checks.eq("标题替换", &chapters[0].title, "第一章");
    checks.eq("标题替换记录", outcome.applied.join(","), "标题");

    println!("\n== bookUrlPattern 命中详情页");
    let detail_source: BookSource = serde_json::from_value(serde_json::json!({
        "bookSourceUrl": "https://detail.test",
        "bookSourceName": "详情页源",
        "searchUrl": "/s?q={{key}}",
        "bookUrlPattern": "https://detail\\.test/s.*",
        "ruleBookInfo": {
            "name": "h1.name@text",
            "author": "span.author@text",
            "tocUrl": "a.toc@href"
        }
    }))
    .expect("详情页测试书源");
    let detail = engine.search(&detail_source, "x", 1).await?;
    checks.eq("详情页结果数", detail.len(), "1");
    checks.eq("详情页书名", &detail[0].name, "详情页命中");
    checks.eq(
        "详情页 bookUrl 退回 baseUrl",
        &detail[0].book_url,
        "https://detail.test/s?q=x",
    );

    println!("\n== 列表为空 + bookUrlPattern 是空串 → 退回详情页解析");
    let fallback_source: BookSource = serde_json::from_value(serde_json::json!({
        "bookSourceUrl": "https://detail.test",
        "bookSourceName": "空串详情页源",
        "searchUrl": "/s?q={{key}}",
        "bookUrlPattern": "",
        "ruleSearch": { "bookList": "div.no-such", "name": "h1@text" },
        "ruleBookInfo": { "name": "h1.name@text", "author": "span.author@text" }
    }))
    .expect("空串 bookUrlPattern 测试书源");
    let fallback = engine.search(&fallback_source, "x", 1).await?;
    checks.eq("空串 bookUrlPattern 且列表选空 → 1 条", fallback.len(), "1");
    checks.eq("退回详情页取到的书名", &fallback[0].name, "详情页命中");

    println!("\n== 发现（ruleExplore）");
    let explore_source: BookSource = serde_json::from_value(serde_json::json!({
        "bookSourceUrl": "https://explore.test",
        "bookSourceName": "发现源",
        "ruleExplore": {
            "bookList": "div.chapter@tag.a",
            "name": "text",
            "bookUrl": "href"
        }
    }))
    .expect("发现测试书源");
    let explored = engine.explore(&explore_source, "/list?page=1", 1).await?;
    checks.eq("发现结果数", explored.len(), "2");
    checks.eq("发现首条（裸 text 规则）", &explored[0].name, "第1章");
    checks.truthy(
        "发现链接（裸 href 规则）",
        explored[0].book_url.ends_with("c1.html"),
        &explored[0].book_url,
    );

    println!("\n== 精确搜索（按书名 + 作者逐源找）");
    {
        let mut broken = source.clone();
        broken.search_url = Some("https://book.test/nope?key={{key}}".into());
        let (book, index) = engine
            .precise_search(&[broken, source.clone()], "斗破苍穹", "天蚕土豆")
            .await?;
        checks.eq("命中书名", &book.name, "斗破苍穹");
        checks.eq("命中作者", &book.author, "天蚕土豆");
        checks.eq("跳过报错的那个源", index.to_string(), "1");
        checks.truthy(
            "详情页地址已就位",
            !book.toc_url.trim().is_empty(),
            &book.toc_url,
        );
        let error = engine
            .precise_search(std::slice::from_ref(&source), "斗破苍穹", "别的作者")
            .await
            .expect_err("作者对不上应该报错");
        checks.truthy(
            "没命中时报「未搜索到<书名>作者」",
            error.to_string().contains("未搜索到<斗破苍穹>别的作者"),
            error.to_string(),
        );
    }

    println!("\n== formatJs / preUpdateJs（TocRule 上的两个脚本钩子）");
    {
        let mut formatted_source = source.clone();
        if let Some(toc) = formatted_source.rule_toc.as_mut() {
            toc.format_js = Some("index + '.' + title".into());
        }
        let mut formatted_book = book.clone();
        let formatted = engine
            .chapters(&formatted_source, &mut formatted_book)
            .await?;
        checks.eq(
            "formatJs 改标题（index 从 1 起）",
            &formatted[0].title,
            "1.第1章",
        );
        checks.eq("formatJs 第 3 章", &formatted[2].title, "3.第3章");
        let mut broken_source = source.clone();
        if let Some(toc) = broken_source.rule_toc.as_mut() {
            toc.format_js = Some("没有这个变量.boom".into());
        }
        let mut broken_book = book.clone();
        let broken = engine.chapters(&broken_source, &mut broken_book).await?;
        checks.eq("formatJs 报错时保留原标题", &broken[0].title, "第1章");
        let mut pre_source = source.clone();
        if let Some(toc) = pre_source.rule_toc.as_mut() {
            toc.pre_update_js = Some("book.bookUrl = book.bookUrl".into());
        }
        let mut pre_book = book.clone();
        let pre = engine.chapters(&pre_source, &mut pre_book).await?;
        checks.eq("preUpdateJs 正常跑完不影响目录", pre.len(), "4");
        let mut bad_source = source.clone();
        if let Some(toc) = bad_source.rule_toc.as_mut() {
            toc.pre_update_js = Some("没有这个变量.boom".into());
        }
        let mut bad_book = book.clone();
        let failed = engine.chapters(&bad_source, &mut bad_book).await.is_err();
        checks.truthy(
            "preUpdateJs 报错则整条链路失败",
            failed,
            "chapters() 应该返回错误",
        );
    }

    checks.guard(66);
    Ok(checks)
}
