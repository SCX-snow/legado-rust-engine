use legado_core::{JsEvaluator, JsRequest, ReplaceRule};
use legado_regex::Regex;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplaceTarget {
    Title,
    Content,
}

#[derive(Debug, Clone)]
pub struct ReplaceFailure {
    pub name: String,
    pub error: String,
}

#[derive(Debug, Clone, Default)]
pub struct ReplaceOutcome {
    pub text: String,
    pub applied: Vec<String>,
    pub failed: Vec<ReplaceFailure>,
}

pub fn select<'a>(
    rules: &'a [ReplaceRule],
    target: ReplaceTarget,
    book_name: &str,
    origin: &str,
) -> Vec<&'a ReplaceRule> {
    let mut picked: Vec<&ReplaceRule> = rules
        .iter()
        .filter(|rule| {
            rule.is_enabled
                && !rule.pattern.is_empty()
                && match target {
                    ReplaceTarget::Title => rule.scope_title,
                    ReplaceTarget::Content => rule.scope_content,
                }
                && scope_match(rule, book_name, origin)
        })
        .collect();
    picked.sort_by_key(|rule| rule.order);
    picked
}

fn scope_match(rule: &ReplaceRule, book_name: &str, origin: &str) -> bool {
    if let Some(scope) = rule.scope.as_deref().filter(|scope| !scope.is_empty())
        && !(scope.contains(book_name) || scope.contains(origin))
    {
        return false;
    }
    if let Some(exclude) = rule
        .exclude_scope
        .as_deref()
        .filter(|scope| !scope.is_empty())
        && (exclude.contains(book_name) || exclude.contains(origin))
    {
        return false;
    }
    true
}

pub async fn apply_to_content(
    rules: &[ReplaceRule],
    book_name: &str,
    origin: &str,
    text: &str,
    js: Option<&dyn JsEvaluator>,
) -> ReplaceOutcome {
    let selected = select(rules, ReplaceTarget::Content, book_name, origin);
    run(&selected, text, js, false).await
}

pub async fn apply_to_title(
    rules: &[ReplaceRule],
    book_name: &str,
    origin: &str,
    text: &str,
    js: Option<&dyn JsEvaluator>,
) -> ReplaceOutcome {
    let selected = select(rules, ReplaceTarget::Title, book_name, origin);
    run(&selected, text, js, true).await
}

async fn run(
    selected: &[&ReplaceRule],
    text: &str,
    js: Option<&dyn JsEvaluator>,
    title: bool,
) -> ReplaceOutcome {
    let mut current = text.to_string();
    let mut applied = Vec::new();
    let mut failed = Vec::new();
    for rule in selected {
        match apply_one(rule, &current, js).await {
            Ok(next) => {
                if title && next.trim().is_empty() {
                    continue;
                }
                if next != current {
                    applied.push(rule.name.clone());
                }
                current = next;
            }
            Err(error) => failed.push(ReplaceFailure {
                name: rule.name.clone(),
                error,
            }),
        }
    }
    ReplaceOutcome {
        text: current,
        applied,
        failed,
    }
}

async fn apply_one(
    rule: &ReplaceRule,
    text: &str,
    js: Option<&dyn JsEvaluator>,
) -> Result<String, String> {
    if !rule.is_regex {
        return Ok(text.replace(&rule.pattern, &rule.replacement));
    }
    let regex = build_regex(rule)?;
    if let Some(script) = rule.replacement.strip_prefix("@js:") {
        return replace_with_js(&regex, script, rule, text, js).await;
    }
    match regex.replacen(text, 0, &rule.replacement) {
        Ok(replaced) => Ok(replaced),
        Err(error) => Err(error.to_string()),
    }
}

fn build_regex(rule: &ReplaceRule) -> Result<Regex, String> {
    let timeout = if rule.timeout_millisecond > 0 {
        rule.timeout_millisecond as u64
    } else {
        3000
    };
    Regex::new(&rule.pattern)
        .map(|regex| regex.with_time_limit(timeout))
        .map_err(|error| error.to_string())
}

async fn replace_with_js(
    regex: &Regex,
    script: &str,
    rule: &ReplaceRule,
    text: &str,
    js: Option<&dyn JsEvaluator>,
) -> Result<String, String> {
    let Some(js) = js else {
        return Err("替换值用了 @js:，但没有 JsEvaluator".into());
    };
    let timeout = std::time::Duration::from_millis(if rule.timeout_millisecond > 0 {
        rule.timeout_millisecond as u64
    } else {
        3000
    });
    let started = std::time::Instant::now();
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    for captures in regex
        .captures_iter(text)
        .map_err(|error| error.to_string())?
    {
        if started.elapsed() > timeout {
            return Err("替换超时".into());
        }
        let Some(matched) = captures.get(0) else {
            continue;
        };
        let (start, end) = captures.byte_span(text);
        out.push_str(&text[last..start]);
        let value = js
            .eval(JsRequest {
                script,
                source: None,
                base_url: "",
                page: None,
                key: None,
                result: Some(matched),
                src: None,
                response: None,
                book: None,
                chapter: None,
                variables: None,
            })
            .await
            .map_err(|error| format!("替换值 @js 求值失败：{error}"))?;
        out.push_str(&value);
        last = end;
    }
    out.push_str(&text[last..]);
    Ok(out)
}
