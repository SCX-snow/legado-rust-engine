use std::process::ExitCode;
use std::time::{Duration, Instant};

use legado_core::{
    BookSource, BoxFuture, Bytes, CookieStore, HeaderMap, HttpResponse, JsEvaluator, JsRequest,
    Result, StatusCode,
};
use legado_js::JsEngine;
use legado_net::encode::{decode, encoding_for};
use legado_net::{
    HttpClient, MemoryCookieStore, UrlContext, UrlRule, base_url, check_login,
    content_type_charset, detect_encoding, escape, has_url_encoded, parse_lenient_json,
    resolve_encoding, sub_domain, url_encode,
};

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

    fn truthy(&mut self, label: &str, condition: bool, detail: impl std::fmt::Display) {
        if condition {
            self.passed += 1;
            println!("  ✓ {label}");
        } else {
            self.failures.push(format!("{label}: {detail}"));
            println!("  ✗ {label}: {detail}");
        }
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

struct FakeJs;

impl JsEvaluator for FakeJs {
    fn eval<'a>(&'a self, request: JsRequest<'a>) -> BoxFuture<'a, Result<String>> {
        let script = request.script.trim().to_string();
        Box::pin(async move {
            if script.starts_with("http") {
                Ok(script)
            } else {
                Ok(format!("JS[{script}]"))
            }
        })
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    let mut checks = Checks::default();
    text_checks(&mut checks);
    json_checks(&mut checks);
    domain_checks(&mut checks);
    cookie_checks(&mut checks);
    encoding_checks(&mut checks);
    url_checks(&mut checks).await;
    option_js_checks(&mut checks).await;
    rate_checks(&mut checks).await;
    login_check_checks(&mut checks).await;
    checks.guard(109);

    println!(
        "\n{} 项通过，{} 项失败",
        checks.passed,
        checks.failures.len()
    );
    if checks.failures.is_empty() {
        println!("M2 离线自检通过");
        ExitCode::SUCCESS
    } else {
        for failure in &checks.failures {
            eprintln!("失败: {failure}");
        }
        ExitCode::FAILURE
    }
}

fn text_checks(checks: &mut Checks) {
    checks.section("URL 转义");
    checks.eq(
        "escape 中文与符号",
        escape("斗破 abc/1"),
        "%u6597%u7834%20abc%2f1",
    );
    checks.eq("escape 控制字符补零", escape("\u{a}"), "%0a");
    checks.eq("escape 代理对", escape("😀"), "%ud83d%ude00");
    checks.eq(
        "url_encode GBK",
        url_encode("斗破 1", encoding_for("gbk").unwrap()),
        "%B6%B7%C6%C6+1",
    );
    checks.eq(
        "url_encode UTF-8",
        url_encode("斗破", encoding_for("utf-8").unwrap()),
        "%E6%96%97%E7%A0%B4",
    );
    checks.eq(
        "url_encode 保留 -_.*",
        url_encode("a-b_c.d*e", encoding_for("utf-8").unwrap()),
        "a-b_c.d*e",
    );
    checks.eq("hasUrlEncoded 已编码", has_url_encoded("%E6%96%97"), "true");
    checks.eq("hasUrlEncoded 中文", has_url_encoded("斗破"), "false");
    checks.eq(
        "hasUrlEncoded 斜杠不在白名单",
        has_url_encoded("a/b"),
        "false",
    );
    checks.eq(
        "hasUrlEncoded 等号不在白名单",
        has_url_encoded("a=b"),
        "false",
    );
    checks.eq("hasUrlEncoded 空串", has_url_encoded(""), "true");
    checks.eq("hasUrlEncoded 半截转义", has_url_encoded("%E6%9"), "false");
}

fn json_checks(checks: &mut Checks) {
    checks.section("Gson 宽松 JSON（URL 选项 / header 的写法）");
    let option = parse_lenient_json("{'method':'post','body':'s={{key}}','retry':3,'n':null}")
        .expect("单引号写法应该能解析");
    checks.eq(
        "单引号 method",
        option["method"].as_str().unwrap_or("<缺失>"),
        "post",
    );
    checks.eq(
        "单引号 body",
        option["body"].as_str().unwrap_or("<缺失>"),
        "s={{key}}",
    );
    checks.eq(
        "数字 retry",
        option["retry"].as_i64().unwrap_or(-1).to_string(),
        "3",
    );
    checks.eq("裸 null", option["n"].is_null().to_string(), "true");

    let nested = parse_lenient_json(
        r#"{'headers':{'Referer':'https://a.com/x, y:z'},'webView':true,'bare':1}"#,
    )
    .expect("嵌套对象应该能解析");
    checks.eq(
        "头里的逗号冒号不切分",
        nested["headers"]["Referer"].as_str().unwrap_or("<缺失>"),
        "https://a.com/x, y:z",
    );
    checks.eq(
        "裸布尔",
        nested["webView"].as_bool().unwrap_or(false).to_string(),
        "true",
    );
    checks.eq(
        "裸键",
        nested["bare"].as_i64().unwrap_or(-1).to_string(),
        "1",
    );

    let sloppy = parse_lenient_json("{a: 1, b: [1,2,],}").expect("裸键 + 尾逗号应该能解析");
    checks.eq(
        "尾逗号数组",
        sloppy["b"][1].as_i64().unwrap_or(-1).to_string(),
        "2",
    );
    checks.eq(
        "转义",
        parse_lenient_json(r#"'a\nb'"#)
            .unwrap()
            .as_str()
            .unwrap_or(""),
        "a\nb",
    );
    checks.eq(
        "\\u 转义",
        parse_lenient_json(r#"'中\u6587'"#)
            .unwrap()
            .as_str()
            .unwrap_or(""),
        "中文",
    );

    checks.truthy(
        "缺值要报错",
        parse_lenient_json("{'a':}").is_err(),
        "却解析成功了",
    );
    checks.truthy(
        "空串要报错",
        parse_lenient_json("").is_err(),
        "却解析成功了",
    );
    checks.truthy(
        "多余内容要报错",
        parse_lenient_json("{'a':1} 尾巴").is_err(),
        "却解析成功了",
    );
}

fn domain_checks(checks: &mut Checks) {
    checks.section("eTLD+1 与 baseUrl");
    checks.eq(
        "多段后缀 .com.cn",
        sub_domain("https://www.a.com.cn/x"),
        "a.com.cn",
    );
    checks.eq(
        "多段后缀 .co.uk",
        sub_domain("https://news.bbc.co.uk/"),
        "bbc.co.uk",
    );
    checks.eq("IP 不做 eTLD+1", sub_domain("http://1.2.3.4/x"), "1.2.3.4");
    checks.eq(
        "带端口的 host",
        sub_domain("https://localhost:8080/x"),
        "localhost",
    );
    checks.eq(
        "普通子域",
        sub_domain("https://www.novelhall.com/a?b=1"),
        "novelhall.com",
    );
    checks.eq("非 http 原样返回", sub_domain("/search/x"), "/search/x");
    checks.eq(
        "baseUrl 保留端口",
        base_url("https://a.com:8080/x?y=1").unwrap_or_default(),
        "https://a.com:8080",
    );
    checks.eq(
        "baseUrl 无路径",
        base_url("https://a.com").unwrap_or_default(),
        "https://a.com",
    );
    checks.eq(
        "baseUrl 非 http 为 None",
        base_url("/x").is_none().to_string(),
        "true",
    );
}

fn cookie_checks(checks: &mut Checks) {
    checks.section("cookie 容器");
    let store = MemoryCookieStore::new();
    store.set_cookie("https://www.a.com/x", "a=1; b=2");
    checks.eq(
        "按域名读",
        store.get_cookie("https://m.a.com/y"),
        "a=1; b=2",
    );
    checks.eq(
        "不同 eTLD+1 读不到",
        store.get_cookie("https://a.com.evil.com/"),
        "",
    );
    checks.eq("别的域名读不到", store.get_cookie("https://b.com/"), "");
    store.replace_cookie("https://m.a.com/", "b=3; c=4");
    checks.eq(
        "replace 合并覆盖",
        store.get_cookie("https://a.com/"),
        "a=1; b=3; c=4",
    );
    store.remove_cookie("https://a.com/");
    checks.eq("删除后为空", store.get_cookie("https://m.a.com/"), "");
    checks.eq(
        "空值丢弃、null 保留",
        legado_net::cookie_to_map("a=; b=null; c=3")
            .len()
            .to_string(),
        "2",
    );
    let ip_store = MemoryCookieStore::new();
    ip_store.set_cookie("http://1.2.3.4/a", "s=1");
    checks.eq("不同 IP 不共享", ip_store.get_cookie("http://9.8.3.4/"), "");
}

fn encoding_checks(checks: &mut Checks) {
    checks.section("charset 判定与解码");
    let gbk = encoding_for("gbk").unwrap();
    let sample = "斗破苍穹是一本很好看的网络小说，讲的是萧炎的成长故事，很长很长很长的一段中文用来让编码检测有足够样本。";
    let gbk_bytes = gbk.encode(sample).0.into_owned();

    checks.eq(
        "Content-Type 里的 charset",
        content_type_charset("text/html; charset=GBK")
            .map(|encoding| encoding.name())
            .unwrap_or("<没认出来>"),
        "GBK",
    );
    checks.eq(
        "Content-Type 带引号",
        content_type_charset("text/html; charset=\"gb2312\"")
            .map(|encoding| encoding.name())
            .unwrap_or("<没认出来>"),
        "GBK",
    );
    checks.eq("GBK 往返", decode(&gbk_bytes, gbk) == sample, "true");
    checks.eq(
        "没有线索时 chardetng 认出 GBK 系",
        matches!(
            detect_encoding(&gbk_bytes).map(|encoding| encoding.name()),
            Some("GBK" | "GB18030")
        )
        .to_string(),
        "true",
    );
    checks.eq(
        "chardetng 的结果能解回原文",
        decode(&gbk_bytes, detect_encoding(&gbk_bytes).unwrap()) == sample,
        "true",
    );

    let html =
        String::from("<html><head><meta charset=\"gbk\"></head><body>") + sample + "</body></html>";
    let html_bytes = gbk.encode(&html).0.into_owned();
    checks.eq(
        "正文 <meta charset>",
        resolve_encoding(&html_bytes, None, None).name(),
        "GBK",
    );
    checks.eq(
        "http-equiv 写法",
        resolve_encoding(
            &gbk.encode("<meta http-equiv=\"Content-Type\" content=\"text/html; charset=gbk\">")
                .0,
            None,
            None,
        )
        .name(),
        "GBK",
    );
    checks.eq(
        "选项优先于 Content-Type",
        resolve_encoding(&gbk_bytes, Some("text/html; charset=gbk"), Some("utf-8")).name(),
        "UTF-8",
    );
    checks.eq(
        "Content-Type 优先于正文探测",
        resolve_encoding(&html_bytes, Some("text/html; charset=utf-8"), None).name(),
        "UTF-8",
    );

    let mut bom = vec![0xEF, 0xBB, 0xBF];
    bom.extend_from_slice("中文".as_bytes());
    checks.eq(
        "BOM 判定为 UTF-8",
        resolve_encoding(&bom, None, None).name(),
        "UTF-8",
    );
    checks.eq(
        "BOM 被吃掉",
        decode(&bom, resolve_encoding(&bom, None, None)),
        "中文",
    );
    checks.eq(
        "Content-Type 优先于 BOM",
        resolve_encoding(&bom, Some("text/html; charset=gbk"), None).name(),
        "GBK",
    );
    let mut bom_meta = vec![0xEF, 0xBB, 0xBF];
    bom_meta.extend_from_slice("<meta charset=\"gbk\">中文".as_bytes());
    checks.eq(
        "BOM 优先于正文 <meta>",
        resolve_encoding(&bom_meta, None, None).name(),
        "UTF-8",
    );

    checks.eq(
        "认不出的 charset 退回 UTF-8",
        resolve_encoding(sample.as_bytes(), None, Some("不是编码")).name(),
        "UTF-8",
    );
}

async fn url_checks(checks: &mut Checks) {
    checks.section("URL 模板");
    let base = "https://site.com";

    let ctx = UrlContext::new(base)
        .with_key("斗破")
        .with_page(2)
        .with_js(&FakeJs);
    let rule = UrlRule::parse("/s?q={{key}}&p={{page}}&b={{baseUrl}}", &ctx)
        .await
        .expect("绑定表达式应该能解析");
    checks.eq(
        "{{key}}/{{page}}/{{baseUrl}}",
        rule.request().url,
        "https://site.com/s?q=%E6%96%97%E7%A0%B4&p=2&b=https%3A%2F%2Fsite.com",
    );

    let rule = UrlRule::parse("/s?q=%E5%B7%B2%E7%BC%96%E7%A0%81", &UrlContext::new(base))
        .await
        .unwrap();
    checks.eq(
        "已编码的原样保留",
        rule.request().url,
        "https://site.com/s?q=%E5%B7%B2%E7%BC%96%E7%A0%81",
    );

    let rule = UrlRule::parse(
        "/s?q={{key}}&p={{page}},{'charset':'escape'}",
        &UrlContext::new(base).with_key("斗破").with_page(2),
    )
    .await
    .unwrap();
    checks.eq(
        "charset=escape 编原值",
        rule.request().url,
        "https://site.com/s?q=%u6597%u7834&p=2",
    );

    let rule = UrlRule::parse(
        "/s?q={{key}},{'charset':'gbk'}",
        &UrlContext::new(base).with_key("斗破"),
    )
    .await
    .unwrap();
    checks.eq(
        "charset=gbk 编原值",
        rule.request().url,
        "https://site.com/s?q=%B6%B7%C6%C6",
    );

    for (page, expected) in [
        (1, "https://site.com/list/"),
        (2, "https://site.com/list/page/2/"),
    ] {
        let rule = UrlRule::parse(
            "/list/<,page/{{page}}/>",
            &UrlContext::new(base).with_page(page),
        )
        .await
        .unwrap();
        checks.eq(&format!("<a,b> page={page}"), rule.request().url, expected);
    }
    let rule = UrlRule::parse("/list/<a,b>", &UrlContext::new(base).with_page(9))
        .await
        .unwrap();
    checks.eq(
        "<a,b> 越界取最后一个",
        rule.request().url,
        "https://site.com/list/b",
    );
    let rule = UrlRule::parse("/list/<a,b>", &UrlContext::new(base))
        .await
        .unwrap();
    checks.eq(
        "没有页码就留着",
        rule.request().url,
        "https://site.com/list/%3Ca,b%3E",
    );

    let rule = UrlRule::parse(
        "/search.html,{'method':'POST','body':'s={{key}}&t=1','retry':3,'serverID':42}",
        &UrlContext::new(base).with_key("斗破"),
    )
    .await
    .unwrap();
    checks.eq("POST method()", rule.method().as_str(), "POST");
    let request = rule.request();
    checks.eq(
        "POST 地址不带 query",
        request.url,
        "https://site.com/search.html",
    );
    checks.eq(
        "POST form body",
        String::from_utf8(request.body.clone().unwrap().to_vec()).unwrap(),
        "s=%E6%96%97%E7%A0%B4&t=1",
    );
    checks.eq(
        "POST form Content-Type",
        request
            .headers
            .get("content-type")
            .unwrap()
            .to_str()
            .unwrap(),
        "application/x-www-form-urlencoded",
    );
    checks.eq("retry 选项", rule.retry().to_string(), "3");
    checks.eq(
        "serverID 选项",
        rule.server_id().unwrap_or(-1).to_string(),
        "42",
    );
    checks.eq(
        "没写 charset 就是 None",
        rule.charset().is_none().to_string(),
        "true",
    );

    let rule = UrlRule::parse("/s,{'charset':'gbk'}", &UrlContext::new(base))
        .await
        .unwrap();
    checks.eq(
        "charset 选项读出来",
        rule.charset().unwrap_or("<没读到>"),
        "gbk",
    );

    let rule = UrlRule::parse(
        "/api,{'method':'POST','body':'{\"a\":1}','headers':{'X-T':'1'}}",
        &UrlContext::new(base),
    )
    .await
    .unwrap();
    let request = rule.request();
    checks.eq(
        "JSON body 原样",
        String::from_utf8(request.body.clone().unwrap().to_vec()).unwrap(),
        "{\"a\":1}",
    );
    checks.eq(
        "JSON Content-Type",
        request
            .headers
            .get("content-type")
            .unwrap()
            .to_str()
            .unwrap(),
        "application/json; charset=UTF-8",
    );
    checks.eq(
        "选项自定义头",
        request.headers.get("x-t").unwrap().to_str().unwrap(),
        "1",
    );

    let rule = UrlRule::parse(
        "/api,{'method':'POST','body':{'b':1,'a':2}}",
        &UrlContext::new(base),
    )
    .await
    .unwrap();
    checks.eq(
        "JSON 对象 body 保键序",
        String::from_utf8(rule.request().body.clone().unwrap().to_vec()).unwrap(),
        "{\"b\":1,\"a\":2}",
    );

    let rule = UrlRule::parse(
        r#"/api,{'method':'POST','body':'{"a":1}','headers':{'content-type':'text/plain'}}"#,
        &UrlContext::new(base),
    )
    .await
    .unwrap();
    let request = rule.request();
    checks.eq(
        "POST 小写 content-type 认出来",
        request
            .headers
            .get("content-type")
            .and_then(|value| value.to_str().ok())
            .unwrap_or("-"),
        "text/plain",
    );

    let rule = UrlRule::parse("//other.com/x", &UrlContext::new("https://site.com/a/b"))
        .await
        .unwrap();
    checks.eq("协议相对", rule.request().url, "https://other.com/x");
    let rule = UrlRule::parse("c.html", &UrlContext::new("https://site.com/a/"))
        .await
        .unwrap();
    checks.eq("相对路径", rule.request().url, "https://site.com/a/c.html");
    let rule = UrlRule::parse(
        "https://other.com/x",
        &UrlContext::new("https://site.com/a/"),
    )
    .await
    .unwrap();
    checks.eq("绝对地址不动", rule.request().url, "https://other.com/x");
    let rule = UrlRule::parse("javascript:void(0)", &UrlContext::new(base))
        .await
        .unwrap();
    checks.eq("javascript: 丢弃", rule.request().url, "");
    let rule = UrlRule::parse("data:text/html,<b>hi</b>", &UrlContext::new(base))
        .await
        .unwrap();
    checks.eq(
        "data: 原样留",
        rule.request().url,
        "data:text/html,<b>hi</b>",
    );

    let rule = UrlRule::parse("<js>A</js>tail/@result", &ctx)
        .await
        .unwrap();
    checks.eq(
        "@result 串尾段",
        rule.request().url,
        "https://site.com/tail/JS[A]",
    );
    let rule = UrlRule::parse("@js:B", &ctx).await.unwrap();
    checks.eq(
        "@js: 相对结果按 baseUrl 补全",
        rule.request().url,
        "https://site.com/JS[B]",
    );
    let rule = UrlRule::parse("@js:https://other.com/x", &ctx)
        .await
        .unwrap();
    checks.eq("@js: 绝对结果", rule.request().url, "https://other.com/x");

    let error = UrlRule::parse("{{page-1}}", &UrlContext::new(base).with_page(2))
        .await
        .err()
        .map(|error| error.to_string())
        .unwrap_or_default();
    checks.truthy(
        "没有 JsEvaluator 时报错并点名",
        error.contains("JsEvaluator"),
        format!("报错是 {error:?}"),
    );
    match UrlRule::parse("{{page}}", &UrlContext::new(base).with_page(2)).await {
        Ok(rule) => checks.eq(
            "没有 JsEvaluator 时纯绑定仍可用",
            rule.request().url,
            "https://site.com/2",
        ),
        Err(error) => checks.truthy(
            "没有 JsEvaluator 时纯绑定仍可用",
            false,
            format!("却报错了：{error}"),
        ),
    }

    checks.truthy(
        "客户端构造",
        HttpClient::builder()
            .total_timeout(Duration::from_secs(5))
            .build()
            .is_ok(),
        "构造失败",
    );
    checks.truthy(
        "非法代理串报错不 panic",
        HttpClient::builder().proxy("http://").build().is_err(),
        "非法代理竟然构造成功了",
    );

    let source: BookSource = serde_json::from_value(serde_json::json!({
        "bookSourceUrl": "https://site.com",
        "bookSourceName": "选项源",
        "concurrentRate": "3/300",
        "enabledCookieJar": true,
        "header": "{\"proxy\":\"http://127.0.0.1:7890\",\"Referer\":\"https://site.com\"}"
    }))
    .expect("选项书源 JSON");
    let rule = UrlRule::parse(
        "/list",
        &UrlContext::new("https://site.com").with_source(&source),
    )
    .await
    .unwrap();
    checks.eq(
        "concurrentRate → rate()",
        rule.rate().unwrap_or("-"),
        "3/300",
    );
    checks.eq(
        "rate_key 用书源 key",
        rule.rate_key().unwrap_or("-"),
        "https://site.com",
    );
    checks.eq("enabledCookieJar", rule.cookie_jar().to_string(), "true");
    checks.eq("cookie_url", rule.cookie_url(), "https://site.com");
    checks.eq(
        "header 里的 proxy",
        rule.proxy().unwrap_or("-"),
        "http://127.0.0.1:7890",
    );
    checks.eq(
        "header 非 proxy 键变成请求头",
        rule.headers()
            .get("referer")
            .and_then(|value| value.to_str().ok())
            .unwrap_or("-"),
        "https://site.com",
    );
}

async fn option_js_checks(checks: &mut Checks) {
    checks.section("URL 选项 js");
    let js = match JsEngine::builder().build() {
        Ok(js) => js,
        Err(error) => {
            checks.truthy("建 JsEngine（js 选项用）", false, error);
            return;
        }
    };
    let ctx = UrlContext::new("https://site.com").with_js(&js);
    let url = match UrlRule::parse(
        r#"/search?q=1,{'js':'result.replace("/search","/find") + "&from=js"'}"#,
        &ctx,
    )
    .await
    {
        Ok(rule) => rule.request().url,
        Err(error) => {
            checks.truthy("选项 js 改地址", false, format!("解析失败：{error}"));
            return;
        }
    };
    checks.eq("选项 js 改地址", url, "https://site.com/find?q=1&from=js");
}

async fn rate_checks(checks: &mut Checks) {
    checks.section("concurrentRate 限速");

    let limiter = legado_net::RateLimiter::new();
    let started = Instant::now();
    for _ in 0..3 {
        let _guard = limiter.acquire("k", Some("3/300")).await;
    }
    let first_three = started.elapsed();
    checks.truthy(
        "时间窗内不额外等待",
        first_three < Duration::from_millis(150),
        format!("前三次用了 {}ms", first_three.as_millis()),
    );
    let _guard = limiter.acquire("k", Some("3/300")).await;
    let four_requests = started.elapsed();
    checks.truthy(
        "第 4 次等到窗口结束",
        four_requests >= Duration::from_millis(300),
        format!("四次一共只用了 {}ms", four_requests.as_millis()),
    );

    let limiter = legado_net::RateLimiter::new();
    let started = Instant::now();
    {
        let _guard = limiter.acquire("k", Some("400")).await;
    }
    let _guard = limiter.acquire("k", Some("400")).await;
    let gap = started.elapsed();
    checks.truthy(
        "纯数字间隔",
        gap >= Duration::from_millis(400),
        format!("间隔只有 {}ms", gap.as_millis()),
    );

    let limiter = legado_net::RateLimiter::new();
    let started = Instant::now();
    for _ in 0..5 {
        let _guard = limiter.acquire("k", Some("0")).await;
    }
    checks.truthy(
        "rate=0 不限速",
        started.elapsed() < Duration::from_millis(100),
        "却等了",
    );
    checks.truthy(
        "没配 rate 时返回 None",
        limiter.acquire("k", None).await.is_none(),
        "却拿到了 guard",
    );

    let limiter = legado_net::RateLimiter::new();
    let _first = limiter.acquire("a", Some("500")).await;
    let started = Instant::now();
    let _second = limiter.acquire("b", Some("500")).await;
    checks.truthy(
        "不同源互不影响",
        started.elapsed() < Duration::from_millis(100),
        "却互相等了",
    );
}

async fn login_check_checks(checks: &mut Checks) {
    checks.section("loginCheckJs 响应对象");
    let js = match JsEngine::builder().build() {
        Ok(js) => js,
        Err(error) => {
            checks.truthy("建 JsEngine", false, error);
            return;
        }
    };
    let ctx = UrlContext::new("https://site.com").with_js(&js);
    let mut headers = HeaderMap::new();
    headers.insert("content-type", "text/html".parse().expect("头"));
    let response = || HttpResponse {
        url: "https://site.com/login".to_string(),
        status: StatusCode::OK,
        headers: headers.clone(),
        body: Bytes::from("<html>login page</html>"),
        text: "<html>login page</html>".to_string(),
    };

    let replaced = check_login(
        r#"var out = result;
           if (result.url.indexOf('/login') > 0) {
             out = { url: result.url + '?need=1', code: 403,
                     body: function(){ return 'need login' },
                     headers: function(){ return 'x-check: 1' } };
           }
           out"#,
        response(),
        &ctx,
    )
    .await
    .expect("脚本返回响应对象");
    checks.eq(
        "返回新响应对象的 url",
        replaced.url,
        "https://site.com/login?need=1",
    );
    checks.eq("返回新响应对象的 code", replaced.status.as_u16(), "403");
    checks.eq("返回新响应对象的正文", replaced.text, "need login");
    checks.eq(
        "返回新响应对象的响应头",
        replaced
            .headers
            .get("x-check")
            .and_then(|value| value.to_str().ok())
            .unwrap_or("-"),
        "1",
    );

    let text_only = check_login(
        "result.body().replace('login page', 'ok')",
        response(),
        &ctx,
    )
    .await
    .expect("脚本返回正文");
    checks.eq("返回字符串换正文", text_only.text, "<html>ok</html>");
    checks.eq("换正文时 url 不变", text_only.url, "https://site.com/login");

    let unchanged = check_login("var x = 1;", response(), &ctx)
        .await
        .expect("脚本没有返回值");
    checks.eq(
        "没有返回值时保留原响应",
        unchanged.text,
        "<html>login page</html>",
    );

    let headers_seen = check_login(
        "result.headers().indexOf('text/html') >= 0 ? 'headers-ok' : 'headers-missing'",
        response(),
        &ctx,
    )
    .await
    .expect("脚本读响应头");
    checks.eq("result.headers() 可读", headers_seen.text, "headers-ok");
}
