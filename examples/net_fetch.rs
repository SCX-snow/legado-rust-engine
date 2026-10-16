use std::process::ExitCode;
use std::sync::Arc;
use std::time::{Duration, Instant};

use legado_rust_engine::core::{BookSource, CookieStore, Error, HttpRequest, JsEvaluator, Source};
use legado_rust_engine::js::JsEngine;
use legado_rust_engine::net::{
    HttpClient, MemoryCookieStore, UrlContext, UrlRule, detect_encoding, resolve_encoding,
};
use legado_rust_engine::rule::RuleEngine;

fn search_cases(
    index: usize,
    source: &BookSource,
    content: &str,
    base_url: &str,
) -> Vec<serde_json::Value> {
    let mut cases = Vec::new();
    let Some(search) = source.rule_search.as_ref() else {
        return cases;
    };
    let source_json = serde_json::to_value(source).unwrap_or_default();
    let mut push = |id: String, mode: &str, rule: &str| {
        if rule.trim().is_empty() {
            return;
        }
        cases.push(serde_json::json!({
            "id": id,
            "mode": mode,
            "rule": rule,
            "content": content,
            "baseUrl": base_url,
            "source": source_json,
        }));
    };
    if let Some(rule) = search.list.book_list.as_deref() {
        push(format!("{index}:list:string"), "string", rule);
        push(format!("{index}:list:list"), "list", rule);
    }
    for (field, rule) in [
        ("name", &search.list.name),
        ("author", &search.list.author),
        ("intro", &search.list.intro),
        ("kind", &search.list.kind),
        ("lastChapter", &search.list.last_chapter),
        ("updateTime", &search.list.update_time),
        ("bookUrl", &search.list.book_url),
        ("coverUrl", &search.list.cover_url),
        ("wordCount", &search.list.word_count),
    ] {
        if let Some(rule) = rule.as_deref() {
            push(format!("{index}:0:search:{field}:string"), "string", rule);
        }
    }
    cases
}

const REDIRECT_PROBES: [&str; 2] = ["http://69shuba.cx/", "http://appi.kuwo.cn/"];

fn needs_js(rule: &str) -> bool {
    if rule.contains("@js:")
        || rule.contains("@JS:")
        || rule.contains("<js>")
        || rule.contains("<JS>")
    {
        return true;
    }
    let mut rest = rule;
    while let Some(start) = rest.find("{{") {
        let Some(end) = rest[start + 2..].find("}}") else {
            break;
        };
        let inner = rest[start + 2..start + 2 + end].trim();
        if !matches!(inner, "key" | "page" | "baseUrl") {
            return true;
        }
        rest = &rest[start + 2 + end + 2..];
    }
    false
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    let mut limit: usize = usize::MAX;
    let mut keyword = "斗破苍穹".to_string();
    let mut sources_path = "fixtures/book_sources.json".to_string();
    let mut emit_cases: Option<String> = None;
    let mut positional = 0;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--sources" => sources_path = args.next().unwrap_or(sources_path),
            "--emit-cases" => emit_cases = args.next(),
            other => {
                if positional == 0 {
                    limit = other.parse().unwrap_or(limit);
                } else if positional == 1 {
                    keyword = other.to_string();
                }
                positional += 1;
            }
        }
    }

    match run(limit, &keyword, &sources_path, emit_cases.as_deref()).await {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

async fn run(
    limit: usize,
    keyword: &str,
    sources_path: &str,
    emit_cases: Option<&str>,
) -> legado_rust_engine::core::Result<bool> {
    let text = std::fs::read_to_string(sources_path)?;
    let sources: Vec<BookSource> = serde_json::from_str(&text)?;
    let mut case_writer = match emit_cases {
        Some(path) => {
            if let Some(parent) = std::path::Path::new(path).parent() {
                std::fs::create_dir_all(parent)?;
            }
            Some(std::io::BufWriter::new(std::fs::File::create(path)?))
        }
        None => None,
    };
    let mut case_count = 0usize;
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

    let (
        mut ok,
        mut http_error,
        mut request_error,
        mut parse_error,
        mut script_error,
        mut skipped,
        mut js_sources,
        mut rate_checked,
        mut decode_fail,
    ) = (0, 0, 0, 0, 0, 0, 0, 0, 0);
    let mut attempted = 0;
    let mut rate_candidates = 0;
    let mut rate_fail = Vec::new();
    let mut page_fail = Vec::new();
    let mut encoding_report = Vec::new();
    let mut reached: Vec<(usize, String)> = Vec::new();

    for (index, source) in sources
        .iter()
        .enumerate()
        .filter(|(_, source)| source.enabled)
    {
        let Some(rule_text) = source.search_url.clone() else {
            continue;
        };
        if rule_text.trim().is_empty() {
            skipped += 1;
            continue;
        }
        if needs_js(&rule_text) || source.header().is_some_and(needs_js) {
            js_sources += 1;
        }
        if attempted >= limit {
            break;
        }
        attempted += 1;

        let ctx = UrlContext::new(source.key())
            .with_source(source)
            .with_key(keyword)
            .with_page(1)
            .with_js(js.as_ref());
        let rule = match UrlRule::parse(&rule_text, &ctx).await {
            Ok(rule) => rule,
            Err(Error::Js(message)) => {
                script_error += 1;
                println!(
                    "✗ {:<22} JS 求值失败: {}",
                    source.tag(),
                    short(&message, 90)
                );
                continue;
            }
            Err(error) => {
                parse_error += 1;
                println!("✗ {:<22} URL 解析失败: {error}", source.tag());
                continue;
            }
        };
        let url = rule.request().url;
        let rule_rate = rule.rate().map(str::to_string);
        let numeric_rate = rule_rate.as_deref().and_then(plain_rate_millis);
        if numeric_rate.is_some() {
            rate_candidates += 1;
        }

        let started = Instant::now();
        match client.send_rule(&rule).await {
            Ok(response) => {
                let elapsed = started.elapsed();
                let encoding = resolve_encoding(&response.body, None, None);
                let content_type = response
                    .headers
                    .get("content-type")
                    .and_then(|value| value.to_str().ok())
                    .unwrap_or("-")
                    .to_string();
                let detected = detect_encoding(&response.body);
                if !response.body.is_empty() && response.text.is_empty() {
                    decode_fail += 1;
                }
                if response.status.is_success() {
                    ok += 1;
                    reached.push((index, url.clone()));
                    if let Some(writer) = case_writer.as_mut() {
                        for case in search_cases(index, source, &response.text, &url) {
                            use std::io::Write as _;
                            writeln!(writer, "{case}")?;
                            case_count += 1;
                        }
                    }
                } else {
                    http_error += 1;
                }
                encoding_report.push((
                    source.tag().to_string(),
                    content_type.clone(),
                    encoding.name().to_string(),
                    detected.map(|encoding| encoding.name().to_string()),
                ));
                println!(
                    "{} {:<22} {:>3} {:>7}B {:>6}字 {:>6}ms  {} → {}",
                    if response.status.is_success() {
                        "✓"
                    } else {
                        "·"
                    },
                    source.tag(),
                    response.status.as_u16(),
                    response.body.len(),
                    response.text.chars().count(),
                    elapsed.as_millis(),
                    short(&url, 58),
                    encoding.name(),
                );

                if let Some(rate) = rule_rate.clone()
                    && let Some(expected) = numeric_rate
                {
                    let second = client.send_rule(&rule).await;
                    let gap = Instant::now().duration_since(started);
                    rate_checked += 1;
                    let passed = gap >= Duration::from_millis(expected);
                    println!(
                        "    ↳ concurrentRate={rate} 第一次开始→第二次结束 {}ms（要求 ≥{expected}ms）{}  第二次请求：{}",
                        gap.as_millis(),
                        if passed { "✓" } else { "✗" },
                        match &second {
                            Ok(response) => format!(
                                "{} {}字",
                                response.status.as_u16(),
                                response.text.chars().count()
                            ),
                            Err(error) => format!("失败 {}", short(&error.to_string(), 40)),
                        }
                    );
                    if !passed {
                        rate_fail.push(source.tag().to_string());
                    }
                }
            }
            Err(error) => {
                request_error += 1;
                println!(
                    "· {:<22} 请求失败: {}",
                    source.tag(),
                    short(&error.to_string(), 90)
                );
            }
        }
    }

    println!("\n== 汇总 ==");
    println!(
        "尝试 {attempted} 个书源（含 JS 的 {js_sources} 个已接 JS 引擎，跳过空规则 {skipped} 个）"
    );
    println!(
        "请求成功 {ok}，非 2xx {http_error}，请求失败 {request_error}，URL 解析失败 {parse_error}，JS 求值失败 {script_error}"
    );
    println!("限速校验 {rate_checked} 次，不达标 {rate_fail:?}");

    if let Some(path) = emit_cases {
        println!("\n对拍请求行: {case_count} 条 → {path}");
        return Ok(case_count > 0);
    }

    check_pages(
        &client,
        &sources,
        &reached,
        keyword,
        js.as_ref(),
        &mut page_fail,
    )
    .await;
    let redirect_fail = check_redirects(&client).await;

    println!("\n== 编码 ==");
    for (tag, content_type, encoding, detected) in encoding_report.iter().take(12) {
        println!(
            "  {tag:<22} Content-Type={:<34} 采用 {}  chardetng={}",
            short(content_type, 34),
            encoding,
            detected.as_deref().unwrap_or("-")
        );
    }

    println!("\n== cookie 域名隔离（真实域名）==");
    let store = MemoryCookieStore::new();
    store.set_cookie("https://www.novelhall.com/index.php", "s=1; t=2");
    store.set_cookie("http://1.2.3.4/a", "ip=1");
    let mut domain_fail = Vec::new();
    for (label, actual, expected) in [
        (
            "同 eTLD+1 的子域共享",
            store.get_cookie("https://m.novelhall.com/x"),
            "s=1; t=2",
        ),
        (
            "别的域读不到",
            store.get_cookie("https://aitbooks.com/"),
            "",
        ),
        (
            "同一 IP 当同一个域",
            store.get_cookie("http://1.2.3.4/b"),
            "ip=1",
        ),
        ("不同 IP 不共享", store.get_cookie("http://9.8.3.4/"), ""),
    ] {
        let mark = if actual == expected { "✓" } else { "✗" };
        println!("  {mark} {label:<22} 得到 {actual:?} 期望 {expected:?}");
        if actual != expected {
            domain_fail.push(format!("{label}: 得到 {actual:?}，期望 {expected:?}"));
        }
    }

    if parse_error > 0 {
        println!("\nURL 解析失败 {parse_error} 个，M2 的模板覆盖不完整");
        return Ok(false);
    }
    if ok == 0 {
        println!("\n没有任何站点请求成功，可能是网络不通");
        return Ok(false);
    }
    if decode_fail > 0 {
        println!("\n有 {decode_fail} 个响应体非空却解不出文本，charset 判定 / 解码有问题");
        return Ok(false);
    }
    if !domain_fail.is_empty() {
        println!("\ncookie 域名隔离不达标：{domain_fail:?}");
        return Ok(false);
    }
    if !rate_fail.is_empty() {
        println!("\n限速不达标：{rate_fail:?}");
        return Ok(false);
    }
    if rate_candidates > 0 && rate_checked == 0 {
        println!(
            "\n有 {rate_candidates} 个纯数字 concurrentRate 源，但第一次请求都没成，限速没验证到"
        );
        return Ok(false);
    }
    if rate_candidates == 0 {
        println!("\n⚠ 语料里没有纯数字 concurrentRate，限速检查本次空转（只报警不判失败）");
    }
    if !page_fail.is_empty() {
        println!("\n翻页不达标：{page_fail:?}");
        return Ok(false);
    }
    if !redirect_fail.is_empty() {
        println!("\n重定向没跟上：{redirect_fail:?}");
        return Ok(false);
    }
    println!("\nM2 请求层验收通过");
    Ok(true)
}

async fn check_pages(
    client: &HttpClient,
    sources: &[BookSource],
    reached: &[(usize, String)],
    keyword: &str,
    js: &dyn JsEvaluator,
    page_fail: &mut Vec<String>,
) {
    println!("\n== 翻页（page=2）==");
    let mut checked = 0;
    for (index, first_url) in reached {
        if checked >= 3 {
            break;
        }
        let source = &sources[*index];
        let Some(rule_text) = source.search_url.as_deref() else {
            continue;
        };
        let paginated = rule_text.contains("{{page}}") || rule_text.contains('<');
        if !paginated {
            continue;
        }
        checked += 1;

        let ctx = UrlContext::new(source.key())
            .with_source(source)
            .with_key(keyword)
            .with_page(2)
            .with_js(js);
        let rule = match UrlRule::parse(rule_text, &ctx).await {
            Ok(rule) => rule,
            Err(error) => {
                page_fail.push(format!("{}: {error}", source.tag()));
                continue;
            }
        };
        let second = rule.request().url;
        let outcome = match client.send_rule(&rule).await {
            Ok(response) => format!(
                "{} {}字",
                response.status.as_u16(),
                response.text.chars().count()
            ),
            Err(error) => short(&error.to_string(), 60),
        };
        let inside_brackets = rule_text
            .find('<')
            .zip(rule_text.rfind('>'))
            .is_some_and(|(start, end)| rule_text[start..end].contains("{{page}}"));
        let must_differ = rule_text.contains("{{page}}") && !inside_brackets;
        let differs = second != *first_url;
        if must_differ && !differs {
            page_fail.push(source.tag().to_string());
        }
        println!(
            "  {:<22} 第一页 {}\n  {:<22} 第二页 {}  ← {outcome}",
            source.tag(),
            first_url,
            "",
            second,
        );
    }
    if checked == 0 {
        println!("  没有可翻页的书源（第一页成功的里面没有带 {{page}} / <> 的）");
        if !reached.is_empty() {
            page_fail.push(format!(
                "{} 个第一页成功的源里没有带 {{{{page}}}} / <> 的，翻页未验证",
                reached.len()
            ));
        }
    }
}

async fn check_redirects(client: &HttpClient) -> Vec<String> {
    println!("\n== 重定向 ==");
    let mut failed = Vec::new();
    for probe in REDIRECT_PROBES {
        match client.send(HttpRequest::get(probe)).await {
            Ok(response) => {
                let followed = response.url != probe;
                if !followed {
                    failed.push(probe.to_string());
                }
                println!(
                    "  {probe:<28} → {} {} {}",
                    response.status.as_u16(),
                    short(&response.url, 46),
                    if followed { "✓" } else { "✗ 没跟随" }
                );
            }
            Err(error) => {
                failed.push(format!("{probe}: {}", short(&error.to_string(), 60)));
                println!("  {probe:<28} → 失败: {}", short(&error.to_string(), 60));
            }
        }
    }
    failed
}

fn plain_rate_millis(rate: &str) -> Option<u64> {
    if rate.contains('/') {
        return None;
    }
    rate.trim().parse().ok()
}

fn short(text: &str, limit: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= limit {
        return text.to_string();
    }
    format!("{}…", chars[..limit].iter().collect::<String>())
}
