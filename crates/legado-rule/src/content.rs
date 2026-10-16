use html5ever::QualName;
use html5ever::driver::{self, ParseOpts};
use html5ever::tendril::TendrilSink;
use html5ever::tree_builder::TreeBuilderOpts;
use scraper::{Html, HtmlTreeSink};
use serde_json::Value as Json;

fn jsoup_head_noscript(source: &str) -> String {
    const ALLOWED: [&str; 7] = [
        "link", "style", "meta", "base", "title", "noscript", "script",
    ];
    let Some(head_start) = find_tag(source, 0, "head") else {
        return source.to_string();
    };
    let Some(head_end) = close_tag_end(source, head_start, "head") else {
        return source.to_string();
    };
    let Some(noscript_start) = find_tag(source, head_start, "noscript") else {
        return source.to_string();
    };
    if noscript_start >= head_end {
        return source.to_string();
    }
    let Some(noscript_end) = close_tag_end(source, noscript_start, "noscript") else {
        return source.to_string();
    };
    let body_start = source[noscript_start..]
        .find('>')
        .map(|end| noscript_start + end + 1);
    let Some(inner_start) = body_start else {
        return source.to_string();
    };
    let inner_end = source[..noscript_end]
        .rfind("</noscript")
        .filter(|end| *end >= inner_start)
        .unwrap_or(noscript_end);
    if inner_start >= inner_end {
        return source.to_string();
    }

    let mut out = String::with_capacity(source.len());
    out.push_str(&source[..inner_start]);
    let inner = &source[inner_start..inner_end];
    let mut index = 0;
    while index < inner.len() {
        if inner.as_bytes()[index] == b'<' {
            let name_end = inner[index + 1..]
                .find(|ch: char| ch.is_ascii_whitespace() || ch == '>' || ch == '/')
                .map(|end| index + 1 + end);
            let name = name_end
                .map(|end| inner[index + 1..end].to_ascii_lowercase())
                .unwrap_or_default();
            if name.is_empty() || name.starts_with('!') || ALLOWED.contains(&name.as_str()) {
                out.push('<');
            } else {
                out.push_str("&lt;");
            }
        } else {
            let ch = inner[index..].chars().next().unwrap_or('\0');
            out.push(ch);
            index += ch.len_utf8();
            continue;
        }
        index += 1;
    }
    out.push_str(&source[inner_end..]);
    out
}

fn find_tag(source: &str, from: usize, tag: &str) -> Option<usize> {
    let haystack = source[from..].to_ascii_lowercase();
    let needle = format!("<{tag}");
    let mut start = 0;
    while let Some(found) = haystack[start..].find(&needle) {
        let index = from + start + found;
        if starts_tag(source, index, tag) {
            return Some(index);
        }
        start += found + needle.len();
    }
    None
}

fn close_tag_end(source: &str, from: usize, tag: &str) -> Option<usize> {
    let haystack = source[from..].to_ascii_lowercase();
    let needle = format!("</{tag}");
    let found = haystack.find(&needle)? + from;
    source[found..].find('>').map(|end| found + end + 1)
}

fn jsoup_cdata(source: &str) -> String {
    const LITERAL_TAGS: [&str; 4] = ["title", "textarea", "script", "style"];
    if !source.contains("<![CDATA[") {
        return source.to_string();
    }
    let mut out = String::with_capacity(source.len());
    let mut index = 0;
    let mut literal: Option<&str> = None;
    while index < source.len() {
        if let Some(tag) = literal {
            match find_close_tag(source, index, tag) {
                Some(end) => {
                    out.push_str(&source[index..end]);
                    index = end;
                    literal = None;
                    continue;
                }
                None => {
                    out.push_str(&source[index..]);
                    break;
                }
            }
        }
        if source[index..].starts_with("<![CDATA[") {
            let body_start = index + "<![CDATA[".len();
            let (body, next) = match source[body_start..].find("]]>") {
                Some(end) => (&source[body_start..body_start + end], body_start + end + 3),
                None => (&source[body_start..], source.len()),
            };
            out.push_str(&escape_text(&crate::engine::unescape_html(body)));
            index = next;
            continue;
        }
        if source.as_bytes()[index] == b'<'
            && let Some(tag) = LITERAL_TAGS
                .iter()
                .find(|tag| starts_tag(source, index, tag))
        {
            literal = Some(tag);
        }
        let ch = source[index..].chars().next().unwrap_or('\0');
        out.push(ch);
        index += ch.len_utf8();
    }
    out
}

fn starts_tag(source: &str, index: usize, tag: &str) -> bool {
    let rest = source[index..]
        .strip_prefix('<')
        .unwrap_or(&source[index..]);
    let mut chars = rest.chars();
    for expected in tag.chars() {
        match chars.next() {
            Some(ch) if ch.eq_ignore_ascii_case(&expected) => {}
            _ => return false,
        }
    }
    matches!(
        chars.next(),
        None | Some(' ') | Some('\t') | Some('\n') | Some('\r') | Some('/') | Some('>')
    )
}

fn find_close_tag(source: &str, from: usize, tag: &str) -> Option<usize> {
    let needle = format!("</{tag}");
    let found = source[from..].to_ascii_lowercase().find(&needle)? + from;
    source[found..].find('>').map(|end| found + end + 1)
}

fn escape_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            other => out.push(other),
        }
    }
    out
}

fn parse_opts() -> ParseOpts {
    ParseOpts {
        tree_builder: TreeBuilderOpts {
            scripting_enabled: false,
            ..Default::default()
        },
        ..Default::default()
    }
}

fn jsoup_self_closing(source: &str) -> String {
    const VOID: [&str; 14] = [
        "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param",
        "source", "track", "wbr",
    ];
    const RAW: [&str; 4] = ["script", "style", "textarea", "title"];
    if !source.contains("/>") {
        return source.to_string();
    }
    let bytes = source.as_bytes();
    let mut out = String::with_capacity(source.len() + 64);
    let mut index = 0;
    while index < bytes.len() {
        let Some(offset) = source[index..].find('<') else {
            out.push_str(&source[index..]);
            break;
        };
        let start = index + offset;
        out.push_str(&source[index..start]);
        if source[start..].starts_with("<!--") {
            let end = source[start..]
                .find("-->")
                .map(|at| start + at + 3)
                .unwrap_or(bytes.len());
            out.push_str(&source[start..end]);
            index = end;
            continue;
        }
        if source[start..].starts_with("<![CDATA[") {
            let end = source[start..]
                .find("]]>")
                .map(|at| start + at + 3)
                .unwrap_or(bytes.len());
            out.push_str(&source[start..end]);
            index = end;
            continue;
        }
        if source[start + 1..].starts_with('!') || source[start + 1..].starts_with('/') {
            let end = tag_end(source, start);
            out.push_str(&source[start..end]);
            index = end;
            continue;
        }
        let name_start = start + 1;
        let name_end = source[name_start..]
            .find(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '-' || ch == ':'))
            .map(|at| name_start + at)
            .unwrap_or(bytes.len());
        let name = source[name_start..name_end].to_ascii_lowercase();
        let end = tag_end(source, start);
        let body = &source[start..end];
        let self_closing = body[..body.len().saturating_sub(1)]
            .trim_end()
            .ends_with('/');
        if !self_closing || VOID.contains(&name.as_str()) || name.is_empty() {
            out.push_str(body);
        } else {
            out.push_str(body[..body.len() - 1].trim_end());
            out.push('>');
            out.push_str("</");
            out.push_str(&name);
            out.push('>');
        }
        index = end;
        if RAW.contains(&name.as_str()) && !self_closing {
            let close = format!("</{name}");
            match source[index..].to_ascii_lowercase().find(&close) {
                Some(at) => {
                    let end = index + at;
                    out.push_str(&source[index..end]);
                    index = end;
                }
                None => {
                    out.push_str(&source[index..]);
                    break;
                }
            }
        }
    }
    out
}

fn tag_end(source: &str, start: usize) -> usize {
    let mut quote: Option<char> = None;
    for (offset, ch) in source[start..].char_indices() {
        match quote {
            Some(current) => {
                if ch == current {
                    quote = None;
                }
            }
            None => match ch {
                '"' | '\'' => quote = Some(ch),
                '>' => return start + offset + 1,
                _ => {}
            },
        }
    }
    source.len()
}

pub(crate) fn parse_document(source: &str) -> Html {
    let source = jsoup_head_noscript(&jsoup_cdata(&jsoup_self_closing(source)));
    driver::parse_document(HtmlTreeSink::new(Html::new_document()), parse_opts()).one(source)
}

pub(crate) fn parse_fragment(source: &str) -> Html {
    driver::parse_fragment(
        HtmlTreeSink::new(Html::new_fragment()),
        parse_opts(),
        QualName::new(None, "http://www.w3.org/1999/xhtml".into(), "body".into()),
        Vec::new(),
        false,
    )
    .one(jsoup_head_noscript(&jsoup_cdata(&jsoup_self_closing(
        source,
    ))))
}

#[derive(Debug, Clone)]
pub enum Content {
    Html { doc: Box<Html>, source: String },
    Json(Json),
    Text(String),
    Fragment(String),
    List(Vec<String>),
}

impl Content {
    pub fn html(source: impl AsRef<str>) -> Self {
        let source = source.as_ref();
        Self::Html {
            doc: Box::new(parse_document(source)),
            source: source.to_string(),
        }
    }

    pub fn fragment(source: impl Into<String>) -> Self {
        Self::Fragment(source.into())
    }

    pub fn text(source: impl Into<String>) -> Self {
        Self::Text(source.into())
    }

    pub fn to_source(&self) -> String {
        match self {
            Self::Html { source, .. } => source.clone(),
            Self::Json(value) => serde_json::to_string(value).unwrap_or_default(),
            Self::Text(text) | Self::Fragment(text) => text.clone(),
            Self::List(items) => items.join("\n"),
        }
    }

    pub fn json_input_error(&self) -> Option<&'static str> {
        let (empty, is_null) = match self {
            Self::Text(text) | Self::Fragment(text) => (text.is_empty(), text == "null"),
            Self::Html { source, .. } => (source.is_empty(), source == "null"),
            Self::Json(Json::Null) => (true, false),
            _ => return None,
        };
        if empty {
            Some("json string can not be null or empty")
        } else if is_null {
            Some("json can not be null")
        } else {
            None
        }
    }

    pub fn looks_like_json(&self) -> bool {
        match self {
            Self::Json(_) => true,
            Self::Text(text) => is_json_text(text),
            _ => false,
        }
    }

    pub fn with_html<R>(&self, scope: impl FnOnce(&Html) -> R) -> R {
        match self {
            Self::Html { doc, .. } => scope(doc),
            Self::Fragment(text) => scope(&parse_fragment(text)),
            other => scope(&parse_document(&other.to_source())),
        }
    }

    pub fn with_json<R>(&self, scope: impl FnOnce(&Json) -> R) -> Result<R, String> {
        match self {
            Self::Json(value) => Ok(scope(value)),
            other => {
                let value = parse_json(&other.to_source())?;
                Ok(scope(&value))
            }
        }
    }

    pub fn as_items(&self) -> Option<&[String]> {
        match self {
            Self::List(items) => Some(items),
            _ => None,
        }
    }

    pub fn to_items(&self) -> Vec<String> {
        match self {
            Self::List(items) => items.clone(),
            Self::Text(text) | Self::Fragment(text) => {
                if text.is_empty() {
                    Vec::new()
                } else {
                    vec![text.clone()]
                }
            }
            Self::Json(Json::Array(items)) => items
                .iter()
                .map(|item| match item {
                    Json::String(text) => text.clone(),
                    other => other.to_string(),
                })
                .collect(),
            Self::Json(other) => vec![java_to_string(other)],
            Self::Html { .. } => {
                let source = self.to_source();
                if source.is_empty() {
                    Vec::new()
                } else {
                    vec![source]
                }
            }
        }
    }
}

fn is_json_text(text: &str) -> bool {
    let text = text.trim();
    (text.starts_with('{') && text.ends_with('}')) || (text.starts_with('[') && text.ends_with(']'))
}

fn parse_json(source: &str) -> Result<Json, String> {
    match serde_json::from_str(source) {
        Ok(value) => Ok(value),
        Err(error) => {
            if !source.contains('\'') {
                return Err(format!("内容不是 JSON：{error}"));
            }
            match serde_json::from_str(&single_to_double_quotes(source)) {
                Ok(value) => Ok(value),
                Err(_) => Err(format!("内容不是 JSON：{error}")),
            }
        }
    }
}

fn single_to_double_quotes(source: &str) -> String {
    let mut out = String::with_capacity(source.len());
    let mut chars = source.chars();
    let mut in_double = false;
    while let Some(ch) = chars.next() {
        match ch {
            '\\' if in_double => {
                out.push(ch);
                if let Some(next) = chars.next() {
                    out.push(next);
                }
            }
            '"' => {
                in_double = !in_double;
                out.push(ch);
            }
            '\'' if !in_double => {
                out.push('"');
                while let Some(inner) = chars.next() {
                    match inner {
                        '\\' => match chars.next() {
                            Some('\'') => out.push('\''),
                            Some(other) => {
                                out.push('\\');
                                out.push(other);
                            }
                            None => {}
                        },
                        '"' => out.push_str("\\\""),
                        '\'' => break,
                        other => out.push(other),
                    }
                }
                out.push('"');
            }
            _ => out.push(ch),
        }
    }
    out
}

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    None,
    Text(String),
    Json(Json),
    List(Vec<String>),
    Elements(Vec<String>),
    Rows(Vec<Vec<String>>),
}

impl Value {
    pub fn is_empty(&self) -> bool {
        match self {
            Self::None => true,
            Self::Text(text) => text.is_empty(),
            Self::Json(Json::Null) => true,
            Self::Json(_) => false,
            Self::List(items) | Self::Elements(items) => items.is_empty(),
            Self::Rows(rows) => rows.is_empty(),
        }
    }

    pub fn to_text(&self) -> String {
        match self {
            Self::None => String::new(),
            Self::Text(text) => text.clone(),
            Self::Json(Json::String(text)) => text.clone(),
            Self::Json(value) => java_to_string(value),
            Self::List(items) | Self::Elements(items) => items.join("\n"),
            Self::Rows(rows) => rows
                .iter()
                .map(|row| row.first().cloned().unwrap_or_default())
                .collect::<Vec<_>>()
                .join("\n"),
        }
    }

    pub fn into_text(self) -> String {
        match self {
            Self::Text(text) => text,
            other => other.to_text(),
        }
    }

    pub fn into_list(self) -> Vec<String> {
        match self {
            Self::None => Vec::new(),
            Self::Text(text) => {
                if text.is_empty() {
                    Vec::new()
                } else {
                    text.split('\n').map(str::to_string).collect()
                }
            }
            Self::Json(Json::Array(items)) => items
                .into_iter()
                .map(|item| match item {
                    Json::String(text) => text,
                    other => other.to_string(),
                })
                .collect(),
            Self::Json(value) => vec![value.to_string()],
            Self::List(items) | Self::Elements(items) => items,
            Self::Rows(rows) => rows
                .into_iter()
                .map(|row| row.into_iter().next().unwrap_or_default())
                .collect(),
        }
    }

    pub fn into_content(self) -> Content {
        match self {
            Self::None => Content::Text(String::new()),
            Self::Text(text) => Content::Text(text),
            Self::Json(value) => Content::Json(value),
            Self::List(items) | Self::Elements(items) => Content::List(items),
            Self::Rows(mut rows) => {
                if rows.len() == 1 {
                    Content::List(rows.remove(0))
                } else {
                    Content::List(
                        rows.into_iter()
                            .map(|row| row.into_iter().next().unwrap_or_default())
                            .collect(),
                    )
                }
            }
        }
    }
}

fn jsonsmart_json(value: &Json) -> String {
    let mut out = String::new();
    write_jsonsmart_json(value, &mut out);
    out
}

fn write_jsonsmart_json(value: &Json, out: &mut String) {
    match value {
        Json::Null => out.push_str("null"),
        Json::Bool(flag) => out.push_str(if *flag { "true" } else { "false" }),
        Json::Number(number) => out.push_str(&number.to_string()),
        Json::String(text) => write_jsonsmart_string(text, out),
        Json::Array(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write_jsonsmart_json(item, out);
            }
            out.push(']');
        }
        Json::Object(map) => {
            out.push('{');
            for (index, (key, item)) in map.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write_jsonsmart_string(key, out);
                out.push(':');
                write_jsonsmart_json(item, out);
            }
            out.push('}');
        }
    }
}

fn write_jsonsmart_string(text: &str, out: &mut String) {
    out.push('"');
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '/' => out.push_str("\\/"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            ch if is_jsonsmart_escaped(ch) => {
                out.push_str(&format!("\\u{:04X}", ch as u32));
            }
            ch => out.push(ch),
        }
    }
    out.push('"');
}

fn is_jsonsmart_escaped(ch: char) -> bool {
    let code = ch as u32;
    code < 0x20 || (0x80..0xA0).contains(&code) || (0x2000..0x2100).contains(&code)
}

pub(crate) fn java_to_string(value: &Json) -> String {
    match value {
        Json::Null => "null".to_string(),
        Json::Bool(flag) => flag.to_string(),
        Json::Number(number) => number.to_string(),
        Json::String(text) => text.clone(),
        Json::Array(_) => jsonsmart_json(value),
        Json::Object(map) => format!(
            "{{{}}}",
            map.iter()
                .map(|(key, item)| format!("{key}={}", java_to_string(item)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}
