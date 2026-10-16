use std::sync::Arc;

use serde_json::Value as Json;
use serde_json_path::JsonPath;

use crate::content::{Content, Value, java_to_string};
use crate::selector::css::RuleOutput;

#[derive(Debug, Clone)]
pub struct JsonSelector {
    path: Option<Arc<JsonPath>>,
    template: Option<String>,
    definite: bool,
    error: Option<String>,
}

impl JsonSelector {
    pub fn parse(text: &str) -> Self {
        let text = text.trim();
        let definite = !(text.contains("..") || text.contains('*') || text.contains('?'));
        if text.contains("{$.") {
            return Self {
                path: None,
                template: Some(text.to_string()),
                definite,
                error: None,
            };
        }
        if let Ok(path) = JsonPath::parse(text) {
            return Self::ok(path, definite);
        }
        let rooted = if text.starts_with('$') {
            text.to_string()
        } else {
            format!("$.{text}")
        };
        if rooted != text
            && let Ok(path) = JsonPath::parse(&rooted)
        {
            return Self::ok(path, definite);
        }
        let bracketed = bracket_hyphen_members(&rooted);
        if bracketed != rooted
            && let Ok(path) = JsonPath::parse(&bracketed)
        {
            return Self::ok(path, definite);
        }
        if let Some(dotted) = drop_dot_before_bracket(&rooted)
            && let Ok(path) = JsonPath::parse(&dotted)
        {
            return Self::ok(path, definite);
        }
        match JsonPath::parse(&rooted) {
            Ok(path) => Self::ok(path, definite),
            Err(error) => Self::err(error, definite),
        }
    }

    fn ok(path: JsonPath, definite: bool) -> Self {
        Self {
            path: Some(Arc::new(path)),
            template: None,
            definite,
            error: None,
        }
    }

    fn err(error: serde_json_path::ParseError, definite: bool) -> Self {
        Self {
            path: None,
            template: None,
            definite,
            error: Some(format!("JsonPath 解析失败：{error}")),
        }
    }

    pub fn eval(&self, content: &Content, output: RuleOutput) -> Result<Value, String> {
        if let Some(message) = content.json_input_error() {
            return Err(message.to_string());
        }
        if let Some(template) = &self.template {
            let Some(substituted) = substitute_inner(template, content, 0) else {
                return Ok(empty_json_list());
            };
            return Ok(if substituted.is_empty() {
                empty_json_list()
            } else {
                Value::List(vec![substituted])
            });
        }
        let Some(path) = &self.path else {
            return Ok(empty_json_list());
        };
        match content.with_json(|json| query(path, json, output, self.definite)) {
            Ok(value) => Ok(value),
            Err(_) => Ok(if path.to_string() == "$" {
                match output {
                    RuleOutput::Elements => Value::Json(Json::String(content.to_source())),
                    RuleOutput::Text => Value::Text(content.to_source()),
                }
            } else {
                empty_json_list()
            }),
        }
    }

    pub fn parse_error(&self) -> Option<&str> {
        self.error.as_deref()
    }
}

fn empty_json_list() -> Value {
    Value::List(Vec::new())
}

const MAX_INNER_DEPTH: usize = 8;

fn substitute_inner(template: &str, content: &Content, depth: usize) -> Option<String> {
    if depth > MAX_INNER_DEPTH {
        return None;
    }
    if !template.contains("{$") {
        return Some(template.to_string());
    }
    let chars: Vec<char> = template.chars().collect();
    let mut out = String::new();
    let mut index = 0;
    let mut emitted_to: Option<usize> = None;
    while index < chars.len() {
        if chars[index] == '{'
            && chars.get(index + 1) == Some(&'$')
            && chars.get(index + 2) == Some(&'.')
            && let Some(end) = matching_brace(&chars, index)
        {
            let inner: String = chars[index + 1..end].iter().collect();
            let value = eval_inner_path(&inner, content, depth + 1);
            if !value.is_empty() {
                out.extend(chars[emitted_to.unwrap_or(0)..index].iter().copied());
                out.push_str(&value);
                emitted_to = Some(end + 1);
                index = end + 1;
                continue;
            }
        }
        index += 1;
    }
    let emitted_to = emitted_to?;
    out.extend(chars[emitted_to..].iter().copied());
    Some(out)
}

fn matching_brace(chars: &[char], start: usize) -> Option<usize> {
    let mut level = 0usize;
    for (offset, ch) in chars[start..].iter().enumerate() {
        match ch {
            '{' => level += 1,
            '}' => {
                level -= 1;
                if level == 0 {
                    return Some(start + offset);
                }
            }
            _ => {}
        }
    }
    None
}

fn eval_inner_path(path: &str, content: &Content, depth: usize) -> String {
    let Some(resolved) = substitute_inner(path, content, depth) else {
        return String::new();
    };
    if resolved.contains("{$") {
        return String::new();
    }
    match JsonSelector::parse(&resolved).eval(content, RuleOutput::Text) {
        Ok(value) => value.to_text(),
        Err(_) => String::new(),
    }
}

fn bracket_hyphen_members(path: &str) -> String {
    let chars: Vec<char> = path.chars().collect();
    let mut out = String::with_capacity(path.len() + 8);
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '.' {
            let recursive = i + 1 < chars.len() && chars[i + 1] == '.';
            let name_start = if recursive { i + 2 } else { i + 1 };
            let mut j = name_start;
            while j < chars.len()
                && (chars[j].is_ascii_alphanumeric() || chars[j] == '_' || chars[j] == '-')
            {
                j += 1;
            }
            let name: String = chars[name_start..j].iter().collect();
            let starts_name = name
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_alphabetic() || c == '_');
            if starts_name && name.contains('-') {
                if recursive {
                    out.push_str("..");
                }
                out.push_str("['");
                out.push_str(&name);
                out.push_str("']");
                i = j;
                continue;
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

fn drop_dot_before_bracket(path: &str) -> Option<String> {
    let chars: Vec<char> = path.chars().collect();
    let mut out = String::with_capacity(path.len());
    let mut quote: Option<char> = None;
    let mut changed = false;
    let mut i = 0;
    while i < chars.len() {
        let ch = chars[i];
        if let Some(current) = quote {
            out.push(ch);
            if ch == '\\' {
                if let Some(&next) = chars.get(i + 1) {
                    out.push(next);
                    i += 2;
                    continue;
                }
            } else if ch == current {
                quote = None;
            }
            i += 1;
            continue;
        }
        match ch {
            '\'' | '"' => {
                quote = Some(ch);
                out.push(ch);
            }
            '.' if chars.get(i + 1) == Some(&'[')
                && i.checked_sub(1).and_then(|at| chars.get(at)) != Some(&'.') =>
            {
                changed = true;
            }
            _ => out.push(ch),
        }
        i += 1;
    }
    changed.then_some(out)
}

fn query(path: &JsonPath, json: &Json, output: RuleOutput, definite: bool) -> Value {
    let nodes: Vec<&Json> = path.query(json).into_iter().collect();
    match output {
        RuleOutput::Elements => match nodes.as_slice() {
            [] => Value::None,
            [single] => Value::Json((*single).clone()),
            many => Value::Json(Json::Array(
                many.iter().map(|node| (*node).clone()).collect(),
            )),
        },
        RuleOutput::Text => {
            let items: Vec<String> = match nodes.as_slice() {
                [Json::Array(elements)] if definite => {
                    elements.iter().map(java_to_string).collect()
                }
                _ => nodes.iter().map(|node| java_to_string(node)).collect(),
            };
            match items.len() {
                0 => Value::List(Vec::new()),
                1 => Value::Text(items.into_iter().next().unwrap_or_default()),
                _ => Value::List(items),
            }
        }
    }
}
