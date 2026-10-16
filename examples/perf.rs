use std::process::ExitCode;
use std::sync::Arc;
use std::time::Instant;

use legado_rust_engine::core::BookSource;
use legado_rust_engine::js::JsEngine;
use legado_rust_engine::rule::{Content, EvalContext, RuleEngine};

const CORPUS: &str = "fixtures/legado_ab/cases.jsonl";

#[derive(Default)]
struct Checks {
    passed: usize,
    failures: Vec<String>,
}

impl Checks {
    fn truthy(&mut self, label: &str, ok: bool, detail: &str) {
        if ok {
            self.passed += 1;
            println!("  ok   {label}");
        } else {
            self.failures.push(format!("{label}: {detail}"));
            println!("  FAIL {label}: {detail}");
        }
    }

    fn guard(&mut self, expected: usize) {
        let ran = self.passed + self.failures.len();
        if ran == expected {
            println!("  ok   断言执行数 {expected}");
        } else {
            let message = format!("断言执行数 {ran}，预期 {expected}");
            self.failures.push(message.clone());
            println!("  FAIL {message}");
        }
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    let limit: usize = std::env::args()
        .nth(1)
        .and_then(|text| text.parse().ok())
        .unwrap_or(200);
    match run(limit).await {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

async fn run(limit: usize) -> legado_rust_engine::core::Result<bool> {
    let rules = Arc::new(RuleEngine::new());
    let js = Arc::new(JsEngine::builder().rules(rules.clone()).build()?);
    rules.set_js(js.clone());
    let source: BookSource = serde_json::from_str(r#"{"bookSourceUrl": "https://example.com"}"#)?;

    let text = std::fs::read_to_string(CORPUS)?;
    let mut pages = Vec::new();
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        let case: serde_json::Value = match serde_json::from_str(line) {
            Ok(case) => case,
            Err(_) => continue,
        };
        if case.get("kind").and_then(|kind| kind.as_str()) == Some("replace") {
            continue;
        }
        let content = case
            .get("content")
            .and_then(|content| content.as_str())
            .unwrap_or_default();
        let rule = case
            .get("rule")
            .and_then(|rule| rule.as_str())
            .unwrap_or_default();
        if content.contains("<html")
            && !rule.trim().is_empty()
            && case.get("mode").and_then(|mode| mode.as_str()) != Some("list")
        {
            pages.push((rule.to_string(), content.to_string()));
        }
    }
    if pages.len() > limit {
        pages.truncate(limit);
    }

    let mut checks = Checks::default();
    let page = pages
        .first()
        .map(|(_, content)| content.clone())
        .unwrap_or_default();

    println!("== 微基准（同一个真实页面，取最好一轮）");
    let parse = bench(5, 50, || {
        let _ = Content::html(&page);
    });
    println!(
        "  HTML 解析（{} 字节）      {:>9.1} µs/次",
        page.len(),
        parse
    );
    checks.truthy("微基准页面非空", !page.is_empty(), "语料里没有真实页面");

    let mut css_ctx = EvalContext::new(Content::html(&page));
    let css = bench(20, 200, || {
        let _ = block_on(rules.eval_string("tag.div@html", &mut css_ctx));
    });
    println!("  CSS 链 `tag.div@html`      {:>9.1} µs/次", css);

    let json_page = r#"{"code":0,"data":{"books":[{"title":"斗破苍穹","price":10},{"title":"武动乾坤","price":20}]}}"#;
    let mut json_ctx = EvalContext::new(Content::text(json_page));
    let json = bench(20, 200, || {
        let _ = block_on(rules.eval_string("$.data.books[*].title", &mut json_ctx));
    });
    println!("  JsonPath `$.data.books[*]` {:>9.1} µs/次", json);

    let mut chain_ctx = EvalContext::new(Content::html(&page));
    let chain = bench(20, 200, || {
        let _ = block_on(rules.eval_string("tag.div.0@tag.p.0@text", &mut chain_ctx));
    });
    println!("  多段 CSS 链（含索引）      {:>9.1} µs/次", chain);

    println!(
        "\n== 真实页面语料上的规则求值（{} 条，规则就是书源原规则）",
        pages.len()
    );
    let started = Instant::now();
    let mut ok = 0usize;
    let mut errors = 0usize;
    for (rule, content) in &pages {
        let mut ctx = EvalContext::new(Content::html(content)).with_source(&source);
        ctx = ctx.with_js(js.as_ref());
        match rules.eval_string(rule, &mut ctx).await {
            Ok(_) => ok += 1,
            Err(_) => errors += 1,
        }
    }
    let elapsed = started.elapsed();
    let per_case = elapsed.as_secs_f64() * 1_000_000.0 / pages.len().max(1) as f64;
    println!(
        "  合计 {:.0?}（{:.1} µs/条，{:.0} 条/秒）：成功 {}、报错 {}",
        elapsed,
        per_case,
        pages.len() as f64 / elapsed.as_secs_f64().max(f64::EPSILON),
        ok,
        errors
    );
    println!(
        "统计: perf 页面 {} 条 / 解析 {:.1}µs / CSS {:.1}µs / JsonPath {:.1}µs / 链 {:.1}µs / 语料 {:.1}µs 每条",
        pages.len(),
        parse,
        css,
        json,
        chain,
        per_case
    );

    checks.truthy("语料条数 > 0", !pages.is_empty(), "没有真实页面");
    checks.truthy(
        "微基准都跑出结果",
        parse > 0.0 && css > 0.0 && json > 0.0 && chain > 0.0,
        "有 0",
    );
    checks.truthy(
        "规则求值至少成功一半（真实页面 + 原规则）",
        ok * 2 >= pages.len(),
        &format!("成功 {ok} / 报错 {errors}"),
    );
    checks.guard(4);

    println!(
        "\n{} 项通过，{} 项失败",
        checks.passed,
        checks.failures.len()
    );
    for failure in &checks.failures {
        println!("  失败: {failure}");
    }
    Ok(checks.failures.is_empty())
}

fn bench(rounds: usize, iters: usize, mut body: impl FnMut()) -> f64 {
    let mut best = f64::MAX;
    for _ in 0..rounds {
        let started = Instant::now();
        for _ in 0..iters {
            body();
        }
        let per = started.elapsed().as_secs_f64() * 1_000_000.0 / iters as f64;
        best = best.min(per);
    }
    best
}

fn block_on<F: std::future::Future>(future: F) -> F::Output {
    use std::task::{Context, Poll, Waker};
    let mut context = Context::from_waker(Waker::noop());
    let mut future = Box::pin(future);
    loop {
        match future.as_mut().poll(&mut context) {
            Poll::Ready(output) => return output,
            Poll::Pending => std::thread::yield_now(),
        }
    }
}
