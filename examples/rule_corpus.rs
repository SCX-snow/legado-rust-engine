use std::collections::{BTreeMap, BTreeSet};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use legado_rust_engine::core::{BookSource, Error, JsEvaluator, Result};
use legado_rust_engine::js::JsEngine;
use legado_rust_engine::rule::{Content, EvalContext, RuleEngine};
use serde_json::Value;

const RULE_KEYS: [&str; 6] = [
    "ruleSearch",
    "ruleExplore",
    "ruleBookInfo",
    "ruleToc",
    "ruleContent",
    "ruleReview",
];

const KNOWN_FAKE_CONTENT_ERRORS: [&str; 3] =
    ["missing-url-context", "fake-script-error", "script-throw"];

const KNOWN_ENGINE_DIVERGENCES: [&str; 1] = ["host-type-conversion"];

const KNOWN_SOURCE_RULE_ERRORS: [&str; 1] = ["source-bad-selector"];

fn main() -> ExitCode {
    let path = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("fixtures/book_sources.json"));

    match run(&path) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(error) => {
            eprintln!("{}: {error}", path.display());
            ExitCode::FAILURE
        }
    }
}

fn run(path: &Path) -> Result<bool> {
    let text = std::fs::read_to_string(path)?;
    let value: Value = serde_json::from_str(&text)?;
    let sources = value
        .as_array()
        .filter(|items| !items.is_empty())
        .ok_or_else(|| Error::Other(format!("{} 的顶层必须是非空 JSON 数组", path.display())))?;

    println!("{}：{} 个书源", path.display(), sources.len());

    let engine = Arc::new(RuleEngine::new());
    let js = Arc::new(
        JsEngine::builder()
            .rules(engine.clone())
            .timeout(Duration::from_secs(2))
            .build()?,
    );
    engine.set_js(js.clone());

    let mut compile_failed = 0usize;
    let mut parse_failed = 0usize;
    let mut checked = 0usize;
    let mut with_js = 0usize;
    let mut eval_failed = 0usize;
    let mut html_failed = 0usize;
    let mut json_failed = 0usize;
    let mut categories: BTreeMap<&'static str, usize> = BTreeMap::new();
    let mut samples: Vec<String> = Vec::new();
    let mut unknown_samples: Vec<String> = Vec::new();

    for source in sources {
        let Ok(book) = serde_json::from_value::<BookSource>(source.clone()) else {
            parse_failed += 1;
            continue;
        };
        let mut rules: BTreeSet<String> = BTreeSet::new();
        if let Some(object) = source.as_object() {
            for key in RULE_KEYS {
                collect_rules(object.get(key), &mut rules);
            }
        }
        for rule in &rules {
            if let Err(error) = engine.compile(rule) {
                compile_failed += 1;
                push_sample(
                    &mut samples,
                    format!("编译失败 {:?}\n    {error}", brief(rule)),
                );
                continue;
            }
            checked += 1;
            if has_js(rule) {
                with_js += 1;
            }
            let failure = eval_rule(&engine, js.as_ref(), rule, &book);
            if failure.html.is_none() && failure.json.is_none() {
                continue;
            }
            eval_failed += 1;
            html_failed += usize::from(failure.html.is_some());
            json_failed += usize::from(failure.json.is_some());
            for (side, error) in [("html", &failure.html), ("json", &failure.json)] {
                let Some(error) = error else { continue };
                let category = categorize(error);
                *categories.entry(category).or_default() += 1;
                let line = format!(
                    "[{side}/{category}] 求值报错 {:?}\n    {error}",
                    brief(rule)
                );
                if !is_known(category) && unknown_samples.len() < 10 {
                    unknown_samples.push(line.clone());
                }
                push_sample(&mut samples, line);
            }
        }
    }

    println!(
        "  编译失败 {compile_failed} 条；{checked} 条跑了假内容求值（含 js 的 {with_js} 条，用真 JsEngine），报错 {eval_failed} 条（HTML 侧 {html_failed} / JSON 侧 {json_failed}）"
    );
    println!("  求值报错类别：");
    if categories.is_empty() {
        println!("    （无）");
    }
    let mut unexplained = 0usize;
    let mut divergences = 0usize;
    for (category, count) in &categories {
        if KNOWN_FAKE_CONTENT_ERRORS.contains(category) {
            println!("    · {category}: {count}");
        } else if KNOWN_ENGINE_DIVERGENCES.contains(category) {
            println!("    ⚠已知差异 {category}: {count}");
            divergences += count;
        } else if KNOWN_SOURCE_RULE_ERRORS.contains(category) {
            println!("    · 源侧坏规则 {category}: {count}（样例见下，确认没有引擎 bug 混进来）");
        } else {
            println!("    ❗未知 {category}: {count}");
            unexplained += count;
        }
    }
    if !unknown_samples.is_empty() {
        println!("  未知类别样例（判定失败，得逐个看）：");
        for sample in &unknown_samples {
            println!("  - {sample}");
        }
    }
    if !samples.is_empty() {
        println!("  前 {} 条问题：", samples.len().min(15));
        for sample in samples.iter().take(15) {
            println!("  - {sample}");
        }
    }

    if parse_failed > 0 {
        println!("\n有 {parse_failed} 个书源解析不了");
        return Ok(false);
    }
    if compile_failed > 0 {
        println!("\n有规则编译不过，得看看切分器");
        return Ok(false);
    }
    if checked == 0 {
        println!("\n一条规则都没抽到，fixtures 或字段名不对");
        return Ok(false);
    }
    if unexplained > 0 {
        println!("\n有 {unexplained} 条求值报错不在已知类别里，逐个看过再决定是回归还是假象");
        return Ok(false);
    }
    if divergences > 0 {
        println!(
            "\n规则全部编译通过；已知假内容报错之外还有 {divergences} 条**已知引擎差异**（见 KNOWN_ENGINE_DIVERGENCES）"
        );
    } else {
        println!("\n规则全部编译通过，求值报错都在已知（假内容）类别里");
    }
    Ok(true)
}

#[derive(Default)]
struct EvalFailure {
    html: Option<String>,
    json: Option<String>,
}

fn eval_rule(
    engine: &RuleEngine,
    js: &dyn JsEvaluator,
    rule: &str,
    source: &BookSource,
) -> EvalFailure {
    let html = r#"<html><body>
        <div class="book"><h3 class="title">斗破苍穹</h3><a href="/book/1">详情</a>
        <span class="author">天蚕土豆</span><p class="intro">简介</p></div>
        <ul class="list"><li><a href="/c/1">第一章</a></li><li><a href="/c/2">第二章</a></li></ul>
    </body></html>"#;
    let json = r#"{"code":0,"data":{"name":"斗破苍穹","items":[{"id":1,"name":"a"},{"id":2,"name":"b"}]}}"#;

    let mut ctx = EvalContext::new(Content::html(html))
        .with_source(source)
        .with_js(js);
    let html = block_on(engine.eval(rule, &mut ctx))
        .err()
        .map(|error| format!("{error}"));
    let mut ctx = EvalContext::new(Content::text(json))
        .with_source(source)
        .with_js(js);
    let json = block_on(engine.eval(rule, &mut ctx))
        .err()
        .map(|error| format!("{error}"));
    EvalFailure { html, json }
}

fn categorize(error: &str) -> &'static str {
    let lower = error.to_lowercase();
    if lower.contains("redeclaration of") {
        return "redeclaration";
    }
    if lower.contains("missing url context") {
        return "missing-url-context";
    }
    if lower.contains("into type '") {
        return "host-type-conversion";
    }
    for shape in [
        "cannot read propert",
        "not a function",
        "unexpected end of json input",
        "is not a constructor",
    ] {
        if lower.contains(shape) {
            return "fake-script-error";
        }
    }
    if lower.contains("执行失败: error: ") {
        return "script-throw";
    }
    if lower.contains("css 选择器解析失败") || lower.contains("xpath 解析失败") {
        return "source-bad-selector";
    }
    "unknown"
}

fn is_known(category: &str) -> bool {
    KNOWN_FAKE_CONTENT_ERRORS.contains(&category)
        || KNOWN_ENGINE_DIVERGENCES.contains(&category)
        || KNOWN_SOURCE_RULE_ERRORS.contains(&category)
}

fn has_js(rule: &str) -> bool {
    rule.contains("@js:")
        || rule.contains("@JS:")
        || rule.contains("<js")
        || rule.contains("<JS")
        || rule.contains("{{")
}

fn brief(text: &str) -> String {
    let mut brief: String = text.chars().take(120).collect();
    if text.chars().count() > 120 {
        brief.push('…');
    }
    brief
}

fn push_sample(samples: &mut Vec<String>, sample: String) {
    if samples.len() < 15 {
        samples.push(sample);
    }
}

fn collect_rules(value: Option<&Value>, out: &mut BTreeSet<String>) {
    match value {
        Some(Value::String(text)) => {
            let trimmed = text.trim();
            if (trimmed.starts_with('{') || trimmed.starts_with('['))
                && let Ok(nested) = serde_json::from_str::<Value>(trimmed)
            {
                collect_rules(Some(&nested), out);
                return;
            }
            if !trimmed.is_empty() {
                out.insert(trimmed.to_string());
            }
        }
        Some(Value::Object(map)) => {
            for value in map.values() {
                collect_rules(Some(value), out);
            }
        }
        Some(Value::Array(items)) => {
            for item in items {
                collect_rules(Some(item), out);
            }
        }
        _ => {}
    }
}

fn block_on<F: Future>(future: F) -> F::Output {
    static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    let runtime = RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("建 tokio 运行时")
    });
    runtime.block_on(future)
}
