use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use legado_rust_engine::core::{BookSource, Result, RssSource};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

const RULE_KEYS: [&str; 6] = [
    "ruleSearch",
    "ruleExplore",
    "ruleBookInfo",
    "ruleToc",
    "ruleContent",
    "ruleReview",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Book,
    Rss,
}

trait Labelled {
    fn label(&self) -> String;
}

impl Labelled for BookSource {
    fn label(&self) -> String {
        format!("{} <{}>", self.book_source_name, self.book_source_url)
    }
}

impl Labelled for RssSource {
    fn label(&self) -> String {
        format!("{} <{}>", self.source_name, self.source_url)
    }
}

#[derive(Default)]
struct Checks {
    expected: usize,
    ran: usize,
    failures: Vec<String>,
}

impl Checks {
    fn expect(&mut self, count: usize) {
        self.expected += count;
    }

    fn check(&mut self, label: &str, ok: bool, detail: impl FnOnce() -> String) {
        self.ran += 1;
        if ok {
            println!("  ok   {label}");
        } else {
            let message = format!("{label}: {}", detail());
            println!("  FAIL {message}");
            self.failures.push(message);
        }
    }

    fn check_each(&mut self, label: &str, ok: bool, detail: impl FnOnce() -> String) {
        self.ran += 1;
        if !ok {
            let message = format!("{label}: {}", detail());
            println!("  FAIL {message}");
            self.failures.push(message);
        }
    }

    fn guard(&mut self) {
        let (ran, expected) = (self.ran, self.expected);
        if ran != expected {
            let message = format!("断言执行数 {ran}，预期 {expected}（有分支被跳过？）");
            println!("  FAIL {message}");
            self.failures.push(message);
        }
    }
}

fn detect_kind(value: &Value) -> Kind {
    let first = match value {
        Value::Array(items) => items.first(),
        other => Some(other),
    };
    match first.and_then(|item| item.get("sourceUrl")) {
        Some(_) => Kind::Rss,
        None => Kind::Book,
    }
}

fn main() -> ExitCode {
    let paths: Vec<PathBuf> = match std::env::args().skip(1).collect::<Vec<_>>() {
        args if args.is_empty() => vec![
            PathBuf::from("fixtures/book_sources.json"),
            PathBuf::from("fixtures/rss_sources.json"),
        ],
        args => args.into_iter().map(PathBuf::from).collect(),
    };

    let mut checks = Checks::default();
    for path in paths {
        println!("== {}", path.display());
        if let Err(error) = check_file(&path, &mut checks) {
            println!("  失败: {error}");
            checks.failures.push(format!("{}: {error}", path.display()));
        }
    }

    checks.guard();
    println!();
    println!(
        "{} 项检查执行，{} 项失败",
        checks.ran,
        checks.failures.len()
    );
    if checks.failures.is_empty() {
        println!("全部往返一致");
        ExitCode::SUCCESS
    } else {
        for failure in &checks.failures {
            println!("  ! {failure}");
        }
        ExitCode::FAILURE
    }
}

fn check_file(path: &Path, checks: &mut Checks) -> Result<()> {
    let text = std::fs::read_to_string(path)?;
    let value: Value = serde_json::from_str(&text)?;
    match detect_kind(&value) {
        Kind::Book => check::<BookSource>(&text, &["bookSourceUrl", "bookSourceName"], checks),
        Kind::Rss => check::<RssSource>(&text, &["sourceUrl", "sourceName"], checks),
    }
}

fn check<T>(text: &str, identity: &[&str], checks: &mut Checks) -> Result<()>
where
    T: DeserializeOwned + Serialize + PartialEq + Labelled,
{
    let parsed: Vec<T> = serde_json::from_str(text)?;
    let original: Vec<Value> = serde_json::from_str(text)?;
    let exported = serde_json::to_string(&parsed)?;
    let exported_values: Vec<Value> = serde_json::from_str(&exported)?;
    let reparsed: Vec<T> = serde_json::from_str(&exported)?;

    checks.expect(original.len() + 4);
    println!("  {} 条", parsed.len());

    checks.check(
        "每条都带标识字段",
        exported_values.iter().all(|item| {
            identity
                .iter()
                .all(|key| item.get(*key).is_some_and(|value| !value.is_null()))
        }),
        || format!("导出里缺少 {identity:?}"),
    );

    checks.check(
        "条数一致",
        parsed.len() == original.len()
            && parsed.len() == exported_values.len()
            && parsed.len() == reparsed.len(),
        || {
            format!(
                "原始 {} / 模型 {} / 导出 {} / 回读 {}",
                original.len(),
                parsed.len(),
                exported_values.len(),
                reparsed.len()
            )
        },
    );

    let mismatched: Vec<String> = parsed
        .iter()
        .zip(&reparsed)
        .filter(|(before, after)| before != after)
        .map(|(before, _)| before.label())
        .collect();
    checks.check(
        "serde 往返（模型自反性）",
        mismatched.is_empty(),
        || {
            format!(
                "{} 条不一致: {}",
                mismatched.len(),
                mismatched
                    .iter()
                    .take(5)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        },
    );

    for (index, (original_item, exported_item)) in original.iter().zip(&exported_values).enumerate()
    {
        let problems = compare_item(index, original_item, exported_item, &exported_values);
        checks.check_each(
            &format!("第 {index} 条与原始 JSON 逐字段一致"),
            problems.is_empty(),
            || problems.join("; "),
        );
    }

    let shape = legado_shape_problems(&exported);
    checks.check(
        "导出形态是 legado 读得回的",
        shape.is_empty(),
        || shape.join("; "),
    );

    if let Some(keys) = exported_keys(&exported) {
        println!("  导出字段: {keys}");
    }
    Ok(())
}

fn modeled_keys(all_exported: &[Value], path: &[String]) -> BTreeSet<String> {
    let mut keys = BTreeSet::new();
    for item in all_exported {
        let mut node = item;
        let mut found = true;
        for key in path {
            match node.get(key) {
                Some(value) => node = value,
                None => {
                    found = false;
                    break;
                }
            }
        }
        if found && let Some(object) = node.as_object() {
            keys.extend(object.keys().cloned());
        }
    }
    keys
}

fn compare_item(
    index: usize,
    original: &Value,
    exported: &Value,
    all_exported: &[Value],
) -> Vec<String> {
    let mut problems = Vec::new();
    match (original.as_object(), exported.as_object()) {
        (Some(original), Some(exported)) => {
            compare_object(original, exported, all_exported, &[], &mut problems);
        }
        _ => problems.push(format!("第 {index} 条不是 JSON 对象")),
    }
    problems
}

fn compare_object(
    original: &serde_json::Map<String, Value>,
    exported: &serde_json::Map<String, Value>,
    all_exported: &[Value],
    path: &[String],
    problems: &mut Vec<String>,
) {
    let modeled = modeled_keys(all_exported, path);
    for (key, value) in original {
        let mut child = path.to_vec();
        child.push(key.clone());
        match exported.get(key) {
            Some(exported_value) => {
                compare_value(value, exported_value, all_exported, &child, problems);
            }
            None => {
                if value.is_null() || (RULE_KEYS.contains(&key.as_str()) && value.is_array()) {
                    continue;
                }
                if modeled.contains(key) {
                    problems.push(format!("{} 在导出里丢了（原始 {value}）", child.join(".")));
                }
            }
        }
    }
}

fn compare_value(
    original: &Value,
    exported: &Value,
    all_exported: &[Value],
    path: &[String],
    problems: &mut Vec<String>,
) {
    if let Value::String(text) = original
        && !scalar_eq(original, exported)
        && let Ok(parsed) = serde_json::from_str::<Value>(text.trim())
    {
        compare_value(&parsed, exported, all_exported, path, problems);
        return;
    }
    match (original, exported) {
        (Value::Object(a), Value::Object(b)) => {
            compare_object(a, b, all_exported, path, problems);
        }
        (Value::Array(a), Value::Array(b)) => {
            if a.len() != b.len() {
                problems.push(format!(
                    "{} 长度变了 {} → {}",
                    path.join("."),
                    a.len(),
                    b.len()
                ));
                return;
            }
            for (x, y) in a.iter().zip(b) {
                compare_value(x, y, all_exported, path, problems);
            }
        }
        _ => {
            if !scalar_eq(original, exported) {
                problems.push(format!(
                    "{} 值变了: 原始 {original} → 导出 {exported}",
                    path.join(".")
                ));
            }
        }
    }
}

fn scalar_eq(original: &Value, exported: &Value) -> bool {
    if original == exported {
        return true;
    }
    match (original, exported) {
        (Value::Number(a), Value::Number(b)) => a.as_f64() == b.as_f64(),
        (Value::Number(a), Value::String(b)) => number_text_matches(a, b),
        (Value::String(a), Value::Number(b)) => number_text_matches(b, a),
        (Value::Bool(a), Value::String(b)) => bool_text_matches(*a, b),
        (Value::String(a), Value::Bool(b)) => bool_text_matches(*b, a),
        _ => false,
    }
}

fn number_text_matches(number: &serde_json::Number, text: &str) -> bool {
    match text.trim().parse::<f64>() {
        Ok(value) => number.as_f64() == Some(value),
        Err(_) => number.as_f64() == Some(0.0),
    }
}

fn bool_text_matches(value: bool, text: &str) -> bool {
    matches!(
        text.trim().to_ascii_lowercase().as_str(),
        "true" | "1" | "yes"
    ) == value
}

fn exported_keys(exported: &str) -> Option<String> {
    let value: Value = serde_json::from_str(exported).ok()?;
    let first = match &value {
        Value::Array(items) => items.first()?,
        other => other,
    };
    let keys: Vec<&str> = first.as_object()?.keys().map(String::as_str).collect();
    Some(keys.join(", "))
}

fn legado_shape_problems(exported: &str) -> Vec<String> {
    let items: Vec<Value> = match serde_json::from_str(exported) {
        Ok(items) => items,
        Err(error) => return vec![format!("导出结果不是 JSON 数组: {error}")],
    };

    let mut problems = Vec::new();
    for (index, item) in items.iter().enumerate() {
        let Some(object) = item.as_object() else {
            problems.push(format!("第 {index} 条不是对象"));
            continue;
        };
        for key in RULE_KEYS {
            if let Some(rule) = object.get(key)
                && !rule.is_object()
            {
                problems.push(format!("第 {index} 条的 {key} 不是对象"));
            }
        }
    }
    problems
}
