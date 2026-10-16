use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io::{BufRead, Write};
use std::path::Path;
use std::process::ExitCode;
use std::sync::Arc;

use legado_rust_engine::core::{Book, BookChapter, BookSource, JsEvaluator, ReplaceRule};
use legado_rust_engine::js::JsEngine;
use legado_rust_engine::replace::{apply_to_content, apply_to_title};
use legado_rust_engine::rule::{Content, EvalContext, RuleEngine};
use serde::{Deserialize, Serialize};
use serde_json::{Value as Json, json};

const CORPUS_DIR: &str = "fixtures/legado_ab";
const REPLACE_CORPUS_DIR: &str = "fixtures/replace_ab";
const REAL_CORPUS_DIR: &str = "fixtures/legado_ab_real";

#[derive(Deserialize, Clone)]
struct Case {
    id: String,
    #[serde(default)]
    kind: String,
    #[serde(default = "default_mode")]
    mode: String,
    #[serde(default)]
    rule: String,
    #[serde(default)]
    content: String,
    #[serde(default)]
    text: Option<String>,
    #[serde(default, rename = "baseUrl")]
    base_url: Option<String>,
    #[serde(default)]
    source: Option<Json>,
    #[serde(default)]
    book: Option<Json>,
    #[serde(default)]
    chapter: Option<Json>,
    #[serde(default)]
    target: Option<String>,
    #[serde(default, rename = "bookName")]
    book_name: Option<String>,
    #[serde(default, rename = "bookOrigin")]
    book_origin: Option<String>,
    #[serde(default)]
    rules: Option<Vec<Json>>,
}

fn default_mode() -> String {
    "string".to_string()
}

#[derive(Serialize, Deserialize, Clone, Default)]
struct Outcome {
    id: String,
    ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    result: Option<Json>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut corpus: Option<String> = None;
    let mut emit: Option<String> = None;
    let mut stdio = false;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--emit" => {
                index += 1;
                emit = args.get(index).cloned();
            }
            "--stdio" => stdio = true,
            "--corpus" => {
                index += 1;
                corpus = Some(
                    args.get(index)
                        .cloned()
                        .unwrap_or_else(|| CORPUS_DIR.to_string()),
                );
            }
            other => {
                eprintln!(
                    "用法：legado_ab [--emit <out.jsonl>] [--stdio] [--corpus <dir>]（不认识 {other}）"
                );
                return ExitCode::FAILURE;
            }
        }
        index += 1;
    }

    if stdio {
        return run_stdio().await;
    }
    let dirs: Vec<String> = match &corpus {
        Some(dir) => vec![dir.clone()],
        None => vec![
            CORPUS_DIR.to_string(),
            REAL_CORPUS_DIR.to_string(),
            REPLACE_CORPUS_DIR.to_string(),
        ],
    };
    if let Some(path) = &emit
        && let Some(message) = emit_guard(path, &dirs)
    {
        eprintln!("{message}");
        return ExitCode::FAILURE;
    }
    let mut ok = true;
    for dir in &dirs {
        if dirs.len() > 1 {
            println!("\n########## {dir} ##########");
        }
        match run_corpus(dir, emit.as_deref()).await {
            Ok(true) => {}
            Ok(false) => ok = false,
            Err(error) => {
                eprintln!("{error}");
                ok = false;
            }
        }
    }
    if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

struct Driver {
    rules: Arc<RuleEngine>,
    js: Arc<JsEngine>,
    book_vars: RefCell<HashMap<String, HashMap<String, String>>>,
}

impl Driver {
    fn new() -> Result<Self, String> {
        let rules = Arc::new(RuleEngine::new());
        let mut builder = JsEngine::builder().rules(rules.clone());
        if std::env::var("AB_HTTP").is_ok() {
            match legado_rust_engine::net::HttpClient::builder().build() {
                Ok(client) => builder = builder.http(Arc::new(client)),
                Err(error) => eprintln!("[ab] HttpClient 不可用: {error}"),
            }
        }
        let js = Arc::new(builder.build().map_err(|error| error.to_string())?);
        rules.set_js(js.clone());
        Ok(Self {
            rules,
            js,
            book_vars: RefCell::new(HashMap::new()),
        })
    }

    async fn run(&self, case: &Case) -> Outcome {
        match self.evaluate(case).await {
            Ok(result) => Outcome {
                id: case.id.clone(),
                ok: true,
                result: Some(result),
                error: None,
            },
            Err(error) => Outcome {
                id: case.id.clone(),
                ok: false,
                result: None,
                error: Some(error.chars().take(500).collect()),
            },
        }
    }

    async fn evaluate(&self, case: &Case) -> Result<Json, String> {
        if case.kind == "replace" {
            return self.evaluate_replace(case).await;
        }
        let source: Option<BookSource> = match &case.source {
            Some(value) => {
                Some(serde_json::from_value(value.clone()).map_err(|error| error.to_string())?)
            }
            None => None,
        };
        let mut book: Option<Book> = match &case.book {
            Some(value) => {
                Some(serde_json::from_value(value.clone()).map_err(|error| error.to_string())?)
            }
            None => None,
        };
        let mut chapter: Option<BookChapter> = match &case.chapter {
            Some(value) => {
                Some(serde_json::from_value(value.clone()).map_err(|error| error.to_string())?)
            }
            None => None,
        };

        if let Some(book) = book.as_mut()
            && !book.book_url.is_empty()
            && let Some(stored) = self.book_vars.borrow().get(&book.book_url)
        {
            for (key, value) in stored {
                book.variables
                    .entry(key.clone())
                    .or_insert_with(|| value.clone());
            }
        }

        let mut ctx = EvalContext::new(content_for(&case.content));
        if let Some(url) = &case.base_url {
            ctx = ctx.with_base_url(url.clone());
        }
        if let Some(source) = source.as_ref() {
            ctx = ctx.with_source(source);
        }
        if let Some(book) = book.as_mut() {
            ctx = ctx.with_book(book);
        }
        if let Some(chapter) = chapter.as_mut() {
            ctx = ctx.with_chapter(chapter);
        }
        ctx = ctx.with_js(self.js.as_ref() as &dyn JsEvaluator);

        match case.mode.as_str() {
            "list" => self
                .rules
                .eval_list(&case.rule, &mut ctx)
                .await
                .map(|list| json!(list))
                .map_err(|error| error.to_string()),
            "elements" => self
                .rules
                .eval(&case.rule, &mut ctx)
                .await
                .map(|value| json!(value.into_list()))
                .map_err(|error| error.to_string()),
            "both" => {
                let text = self
                    .rules
                    .eval_string(&case.rule, &mut ctx)
                    .await
                    .map_err(|error| error.to_string())?;
                let list = self
                    .rules
                    .eval_list(&case.rule, &mut ctx)
                    .await
                    .map_err(|error| error.to_string())?;
                Ok(json!({ "string": text, "list": list }))
            }
            _ => self
                .rules
                .eval_string(&case.rule, &mut ctx)
                .await
                .map(|text| json!(text))
                .map_err(|error| error.to_string()),
        }
        .inspect(|_| {
            if let Some(book) = book.as_ref()
                && !book.book_url.is_empty()
            {
                self.book_vars
                    .borrow_mut()
                    .entry(book.book_url.clone())
                    .or_default()
                    .extend(book.variables.clone());
            }
        })
    }

    async fn evaluate_replace(&self, case: &Case) -> Result<Json, String> {
        let raw = case.rules.clone().unwrap_or_default();
        let mut rules = Vec::with_capacity(raw.len());
        for value in raw {
            rules.push(
                serde_json::from_value::<ReplaceRule>(value).map_err(|error| error.to_string())?,
            );
        }
        let text = case.text.clone().unwrap_or_else(|| case.content.clone());
        let book_name = case.book_name.clone().unwrap_or_default();
        let origin = case.book_origin.clone().unwrap_or_default();
        let js = self.js.as_ref() as &dyn JsEvaluator;
        let outcome = if case.target.as_deref() == Some("title") {
            apply_to_title(&rules, &book_name, &origin, &text, Some(js)).await
        } else {
            apply_to_content(&rules, &book_name, &origin, &text, Some(js)).await
        };
        Ok(json!(outcome.text))
    }
}

fn looks_json(text: &str) -> bool {
    let text = text.trim();
    (text.starts_with('{') && text.ends_with('}')) || (text.starts_with('[') && text.ends_with(']'))
}

fn content_for(text: &str) -> Content {
    if looks_json(text) {
        Content::text(text)
    } else {
        Content::html(text)
    }
}

fn canonical(result: Option<&Json>) -> String {
    match result {
        None | Some(Json::Null) => String::new(),
        Some(Json::String(text)) => text.clone(),
        Some(Json::Array(items)) => items
            .iter()
            .map(|item| canonical(Some(item)))
            .collect::<Vec<_>>()
            .join("\u{1f}"),
        Some(other) => other.to_string(),
    }
}

fn status(legado: &Outcome, rust: &Outcome) -> &'static str {
    if legado.ok && rust.ok {
        if canonical(legado.result.as_ref()) == canonical(rust.result.as_ref()) {
            "SAME"
        } else {
            "DIFF_OK"
        }
    } else if legado.ok {
        "LEGADO_OK_RUST_ERR"
    } else if rust.ok {
        "RUST_OK_LEGADO_ERR"
    } else {
        "BOTH_ERR"
    }
}

fn same_output(a: &Outcome, b: &Outcome) -> bool {
    a.ok == b.ok && canonical(a.result.as_ref()) == canonical(b.result.as_ref())
}

const EXPECTED_CASES: &[(&str, usize)] = &[
    (CORPUS_DIR, 1156),
    (REAL_CORPUS_DIR, 112),
    (REPLACE_CORPUS_DIR, 48),
];

const VOLATILE_CASES: &[&str] = &[
    "1874:detail:string",
    "1874:list:string",
    "2691:detail:list",
    "2691:detail:string",
    "2691:list:list",
    "2691:list:string",
    "2691:toc:list",
    "178:0:search:bookUrl:string",
    "185:0:search:bookUrl:string",
    "9885:content:list",
    "9885:content:string",
    "9885:detail:list",
    "9885:detail:string",
    "9885:fragment:list",
    "9885:fragment:string",
    "9885:list:list",
    "9885:list:string",
    "9885:toc:list",
    "9885:toc:string",
    "9898:detail:string",
    "9898:list:string",
    "9901:detail:list",
    "9901:detail:string",
    "9901:list:list",
    "9901:list:string",
    "20:0:search:bookUrl:string",
    "568:0:search:bookUrl:string",
];

fn volatile_cases(dir: &str) -> &'static [&'static str] {
    if path_eq(dir, REPLACE_CORPUS_DIR) || path_eq(dir, REAL_CORPUS_DIR) {
        &[]
    } else {
        VOLATILE_CASES
    }
}

fn emit_guard(path: &str, dirs: &[String]) -> Option<String> {
    for dir in [CORPUS_DIR, REAL_CORPUS_DIR, REPLACE_CORPUS_DIR] {
        if !path_eq(path, &format!("{dir}/rust.jsonl")) {
            continue;
        }
        if !dirs.iter().any(|run| path_eq(run, dir)) {
            return Some(format!(
                "拒绝 --emit {path}：这次跑的是 {dirs:?}，却要覆盖 {dir}/rust.jsonl；要重建它请加 `--corpus {dir}`"
            ));
        }
        if dirs.len() > 1 {
            return Some(format!(
                "拒绝 --emit {path}：一次跑两个语料会把两份结果写进同一个文件（后跑的覆盖先跑的）；加 `--corpus <dir>` 只跑一个"
            ));
        }
    }
    None
}

fn cases_digest(dir: &str) -> Result<String, String> {
    use md5::{Digest as _, Md5};
    let bytes = std::fs::read(format!("{dir}/cases.jsonl")).map_err(|error| error.to_string())?;
    let mut hasher = Md5::new();
    hasher.update(&bytes);
    Ok(format!("{:x}", hasher.finalize()))
}

async fn run_corpus(dir: &str, emit: Option<&str>) -> Result<bool, String> {
    let cases = read_jsonl::<Case>(&format!("{dir}/cases.jsonl"))?;
    if cases.is_empty() {
        return Err(format!("{dir}/cases.jsonl 是空的"));
    }
    let mut case_ids: BTreeSet<&str> = BTreeSet::new();
    for case in &cases {
        if !case_ids.insert(case.id.as_str()) {
            return Err(format!(
                "{dir}/cases.jsonl 里 id `{}` 重复：判定按 id 建索引，重复的那条会逃出判定",
                case.id
            ));
        }
    }

    let legado = read_indexed(&format!("{dir}/legado.jsonl"))?;
    if legado.is_empty() {
        return Err(format!(
            "{dir}/legado.jsonl 是空的（legado 的实际行为记录）"
        ));
    }
    let uncovered: Vec<&str> = cases
        .iter()
        .filter(|case| !legado.contains_key(&case.id))
        .map(|case| case.id.as_str())
        .collect();
    if !uncovered.is_empty() {
        return Err(format!(
            "{dir}/legado.jsonl 缺少 {}/{} 条 case 的 oracle 记录（例：{:?}）；\
             新增 case 必须先用 JVM harness 录 legado 结果才能进门禁，不能只加 case",
            uncovered.len(),
            cases.len(),
            uncovered.iter().take(5).collect::<Vec<_>>()
        ));
    }
    let orphan: Vec<&str> = legado
        .keys()
        .map(String::as_str)
        .filter(|id| !case_ids.contains(id))
        .collect();
    if !orphan.is_empty() {
        return Err(format!(
            "{dir}/legado.jsonl 里有语料里没有的 id（{:?}）：语料删过 case？删条就是静默削覆盖率，必须重新 review",
            orphan.iter().take(5).collect::<Vec<_>>()
        ));
    }
    if let Some((_, expected)) = EXPECTED_CASES.iter().find(|(path, _)| path_eq(path, dir))
        && cases.len() != *expected
    {
        return Err(format!(
            "{dir}/cases.jsonl 有 {} 条，预期 {expected} 条：语料增删必须显式改 EXPECTED_CASES 并 review",
            cases.len()
        ));
    }

    let digest = cases_digest(dir)?;
    let sidecar = format!("{dir}/cases.md5");
    if emit.is_none() {
        match std::fs::read_to_string(&sidecar) {
            Ok(recorded) => {
                if recorded.trim() != digest {
                    return Err(format!(
                        "{sidecar} 与 cases.jsonl 不一致：改过输入就必须重录 oracle 并重新基线（输出可能一个字没变）"
                    ));
                }
            }
            Err(_) if EXPECTED_CASES.iter().any(|(path, _)| path_eq(path, dir)) => {
                return Err(format!(
                    "{sidecar} 不存在：仓库里的语料必须记录输入摘要（跑一次 --emit 会连它一起写）"
                ));
            }
            Err(_) => {}
        }
    }

    if path_eq(dir, CORPUS_DIR) {
        let stale: Vec<&str> = volatile_cases(dir)
            .iter()
            .copied()
            .filter(|id| !case_ids.contains(id))
            .collect();
        if !stale.is_empty() {
            return Err(format!(
                "VOLATILE_CASES 里有语料里不存在的 id（{stale:?}）：语料换过就必须重新 review 这份不确定清单"
            ));
        }
    }
    let volatile: BTreeSet<&str> = volatile_cases(dir)
        .iter()
        .copied()
        .filter(|id| case_ids.contains(id))
        .collect();

    let driver = Driver::new()?;
    let mut current: Vec<Outcome> = Vec::with_capacity(cases.len());
    for case in &cases {
        current.push(driver.run(case).await);
    }

    if let Some(path) = emit {
        let mut file = std::fs::File::create(path).map_err(|error| error.to_string())?;
        for outcome in &current {
            writeln!(file, "{}", serde_json::to_string(outcome).unwrap())
                .map_err(|e| e.to_string())?;
        }
        print_report(&cases, &legado, &current, None, &volatile);
        std::fs::write(&sidecar, format!("{digest}\n")).map_err(|error| error.to_string())?;
        println!("\n当前结果写到 {path}");
        println!("输入摘要写到 {sidecar}（门禁每次都会比对，改输入就必须重录）");
        println!(
            "⚠️ --emit 只记录、不判定：退出码 0 不代表验收通过；判定的命令是不带 --emit 的\n   cargo run -q --example legado_ab -- --corpus {dir}"
        );
        return Ok(true);
    }

    let baseline_path = format!("{dir}/rust.jsonl");
    let baseline = read_indexed(&baseline_path)?;
    if baseline.is_empty() {
        return Err(format!(
            "{baseline_path} 不存在或为空；先跑 `--emit {baseline_path}` 建立基线"
        ));
    }
    let extra: Vec<&str> = baseline
        .keys()
        .map(String::as_str)
        .filter(|id| !case_ids.contains(id))
        .collect();
    if !extra.is_empty() {
        return Err(format!(
            "{baseline_path} 里有语料里没有的 id（{:?}）：语料删过 case？重新基线并 review",
            extra.iter().take(5).collect::<Vec<_>>()
        ));
    }
    let without_baseline: Vec<&str> = cases
        .iter()
        .filter(|case| !baseline.contains_key(&case.id))
        .map(|case| case.id.as_str())
        .collect();
    if !without_baseline.is_empty() {
        return Err(format!(
            "{baseline_path} 缺少 {}/{} 条 case 的基线（例：{:?}）；先跑 `--emit {baseline_path}` 建立基线",
            without_baseline.len(),
            cases.len(),
            without_baseline.iter().take(5).collect::<Vec<_>>()
        ));
    }

    let ok = print_report(&cases, &legado, &current, Some(&baseline), &volatile);
    Ok(ok)
}

fn print_report(
    cases: &[Case],
    legado: &BTreeMap<String, Outcome>,
    current: &[Outcome],
    baseline: Option<&BTreeMap<String, Outcome>>,
    volatile: &BTreeSet<&str>,
) -> bool {
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    let mut base_same = 0usize;
    let mut cur_same = 0usize;
    let mut problems: Vec<String> = Vec::new();
    let mut missing = 0usize;
    let mut exempt = 0usize;

    for (case, outcome) in cases.iter().zip(current) {
        let Some(expect) = legado.get(&case.id) else {
            missing += 1;
            problems.push(format!(
                "{}: legado.jsonl 里没有这条 case 的 oracle 记录",
                case.id
            ));
            continue;
        };
        let cur = status(expect, outcome);
        *counts.entry(cur).or_default() += 1;
        if cur == "SAME" {
            cur_same += 1;
        }

        if let Some(baseline) = baseline {
            let Some(base) = baseline.get(&case.id) else {
                problems.push(format!("{}: 基线里没有这条", case.id));
                continue;
            };
            let base_status = status(expect, base);
            if base_status == "SAME" {
                base_same += 1;
            }
            if same_output(base, outcome) {
                continue;
            }
            if base_status != "SAME" && cur == "SAME" {
                continue;
            }
            if volatile.contains(case.id.as_str()) && base_status != "SAME" {
                exempt += 1;
                continue;
            }
            problems.push(format!(
                "{} [{} → {}] 规则 {:?}\n     legado {:?}\n     基线   {:?}\n     当前   {:?}",
                case.id,
                base_status,
                cur,
                brief(&case.rule),
                display(expect),
                display(base),
                display(outcome),
            ));
        }
    }

    let total = current.len() - missing;
    println!("语料 {} 条（legado 记录缺失 {missing} 条）", cases.len());
    println!("注：语料是刻意偏向分歧的回归样本，下面的分布**不代表全量一致率**");
    println!("与 legado 的一致率（样本内）：");
    for key in [
        "SAME",
        "DIFF_OK",
        "LEGADO_OK_RUST_ERR",
        "RUST_OK_LEGADO_ERR",
        "BOTH_ERR",
    ] {
        let count = counts.get(key).copied().unwrap_or(0);
        let ratio = if total == 0 {
            0.0
        } else {
            100.0 * count as f64 / total as f64
        };
        println!("  {key:20} {count:5}  ({ratio:5.1}%)");
    }
    if baseline.is_some() {
        println!(
            "  基线 SAME {base_same} → 当前 SAME {cur_same}（已知不确定 case {} 条，本轮值不逐字比、豁免 {exempt} 条）",
            volatile.len()
        );
        if cur_same < base_same {
            problems.push(format!(
                "SAME 数下降：基线 {base_same} → 当前 {cur_same}（任何不是「更接近 legado」的变化都判失败）"
            ));
        }
    }
    if missing > 0 {
        println!("  ⚠ {missing} 条没有 legado 记录，没参与判定（已判失败）");
    }

    if baseline.is_none() {
        return true;
    }
    if problems.is_empty() {
        println!("\n✅ 与基线一致，且没有偏离 legado 的行为变化");
        return true;
    }
    println!("\n❌ {} 条行为变化不是「更接近 legado」：", problems.len());
    for problem in problems.iter().take(25) {
        println!("  - {problem}");
    }
    if problems.len() > 25 {
        println!("  …… 还有 {} 条", problems.len() - 25);
    }
    println!(
        "\n如果这些变化是**有意**的：跑 `cargo run -q --example legado_ab -- --corpus <语料目录> --emit <语料目录>/rust.jsonl` 更新基线并 review；\n如果不是，就是回归，回去查。"
    );
    false
}

async fn run_stdio() -> ExitCode {
    let driver = match Driver::new() {
        Ok(driver) => driver,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let case: Case = match serde_json::from_str(&line) {
            Ok(case) => case,
            Err(error) => {
                let outcome = Outcome {
                    id: String::new(),
                    ok: false,
                    result: None,
                    error: Some(format!("请求行不是合法 JSON: {error}")),
                };
                let _ = writeln!(out, "{}", serde_json::to_string(&outcome).unwrap());
                let _ = out.flush();
                continue;
            }
        };
        let outcome = driver.run(&case).await;
        let _ = writeln!(out, "{}", serde_json::to_string(&outcome).unwrap());
        let _ = out.flush();
    }
    ExitCode::SUCCESS
}

fn read_jsonl<T: for<'de> Deserialize<'de>>(path: &str) -> Result<Vec<T>, String> {
    let text = std::fs::read_to_string(path).map_err(|error| format!("{path}: {error}"))?;
    let mut items = Vec::new();
    for (number, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let item = serde_json::from_str(line)
            .map_err(|error| format!("{path} 第 {} 行: {error}", number + 1))?;
        items.push(item);
    }
    Ok(items)
}

fn read_indexed(path: &str) -> Result<BTreeMap<String, Outcome>, String> {
    let mut index: BTreeMap<String, Outcome> = BTreeMap::new();
    for item in read_jsonl::<Outcome>(path)? {
        let id = item.id.clone();
        if index.insert(id.clone(), item).is_some() {
            return Err(format!(
                "{path}: id `{id}` 重复，判定按 id 索引，重复的会有一条逃出判定"
            ));
        }
    }
    Ok(index)
}

fn path_eq(a: &str, b: &str) -> bool {
    Path::new(a).components().eq(Path::new(b).components())
}

fn brief(rule: &str) -> String {
    let mut brief: String = rule.chars().take(90).collect();
    if rule.chars().count() > 90 {
        brief.push('…');
    }
    brief
}

fn display(outcome: &Outcome) -> String {
    if outcome.ok {
        let text = canonical(outcome.result.as_ref()).replace('\u{1f}', " | ");
        let mut short: String = text.chars().take(80).collect();
        if text.chars().count() > 80 {
            short.push('…');
        }
        format!("ok {short:?}")
    } else {
        format!(
            "err {:?}",
            outcome
                .error
                .as_deref()
                .unwrap_or("")
                .chars()
                .take(80)
                .collect::<String>()
        )
    }
}
