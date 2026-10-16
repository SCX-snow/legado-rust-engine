use encoding_rs::UTF_8;
use http::header::{CONTENT_TYPE, HeaderName, HeaderValue, USER_AGENT};
use http::{HeaderMap, Method};
use legado_core::{Error, HttpRequest, JsEvaluator, JsRequest, Result, Source};

use crate::encode::{self, ESCAPE};
use crate::json;

pub const DEFAULT_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) \
     AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";

#[derive(Clone, Copy)]
pub struct UrlContext<'a> {
    pub base_url: &'a str,
    pub source: Option<&'a dyn Source>,
    pub key: Option<&'a str>,
    pub page: Option<i32>,
    pub js: Option<&'a dyn JsEvaluator>,
}

impl<'a> UrlContext<'a> {
    pub fn new(base_url: &'a str) -> Self {
        Self {
            base_url,
            source: None,
            key: None,
            page: None,
            js: None,
        }
    }

    pub fn with_source(mut self, source: &'a dyn Source) -> Self {
        self.source = Some(source);
        self
    }

    pub fn with_key(mut self, key: &'a str) -> Self {
        self.key = Some(key);
        self
    }

    pub fn with_page(mut self, page: i32) -> Self {
        self.page = Some(page);
        self
    }

    pub fn with_js(mut self, js: &'a dyn JsEvaluator) -> Self {
        self.js = Some(js);
        self
    }
}

#[derive(Debug, Clone)]
pub struct UrlRule {
    url: String,
    query: String,
    fields: Vec<(String, String)>,
    method: Method,
    headers: HeaderMap,
    body: Option<String>,
    charset: Option<String>,
    retry: u32,
    server_id: Option<i64>,
    proxy: Option<String>,
    rate: Option<String>,
    rate_key: Option<String>,
    cookie_jar: bool,
    cookie_url: String,
}

impl UrlRule {
    pub async fn parse(rule: &str, ctx: &UrlContext<'_>) -> Result<Self> {
        let base = strip_options(ctx.base_url);
        let ctx = UrlContext {
            base_url: &base,
            ..*ctx
        };

        let (mut headers, proxy) = source_headers(&ctx).await?;

        let after_js = analyze_js(rule, &ctx).await?;
        let after_pages = replace_key_page(&after_js, &ctx).await?;
        let (url_no_option, option_text) = split_options(&after_pages);
        let (path_text, mut query) = split_query(&url_no_option);
        let mut url = absolute_url(ctx.base_url, &path_text);

        let mut method = Method::GET;
        let mut body = None;
        let mut charset = None;
        let mut retry = 0;
        let mut server_id = None;

        if let Some(text) = option_text {
            let option = json::parse(&text)?;
            if option
                .get("method")
                .and_then(|value| value.as_str())
                .is_some_and(|value| value.eq_ignore_ascii_case("POST"))
            {
                method = Method::POST;
            }
            if let Some(serde_json::Value::Object(map)) = option.get("headers") {
                for (key, value) in map {
                    insert_header(&mut headers, key, &value_to_text(value));
                }
            }
            body = option.get("body").and_then(body_text);
            charset = option
                .get("charset")
                .and_then(|value| value.as_str())
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string);
            retry = option
                .get("retry")
                .and_then(value_as_i64)
                .unwrap_or(0)
                .clamp(0, 100) as u32;
            server_id = option.get("serverID").and_then(value_as_i64);
            if let Some(js) = option
                .get("js")
                .and_then(|value| value.as_str())
                .filter(|value| !value.trim().is_empty())
            {
                let current = join_query(&url, &query);
                let js_base = crate::domain::base_url(&current).unwrap_or_else(|| base.clone());
                let js_ctx = UrlContext {
                    base_url: &js_base,
                    ..ctx
                };
                let result = eval_js(js, Some(&current), &js_ctx).await?;
                (url, query) = split_query(&result);
            }
        }

        let mut fields = Vec::new();
        if method == Method::GET {
            fields = analyze_fields(&query, charset.as_deref());
        } else if let Some(text) = body.as_deref()
            && !is_json(text)
            && !is_xml(text)
            && !headers.contains_key(CONTENT_TYPE)
        {
            fields = analyze_fields(text, charset.as_deref());
        }

        let (rate, rate_key, cookie_jar, cookie_url) = match ctx.source {
            Some(source) => (
                source.concurrent_rate().map(str::to_string),
                Some(source.key().to_string()),
                source.cookie_jar(),
                source.key().to_string(),
            ),
            None => (None, None, false, url.clone()),
        };

        Ok(Self {
            url,
            query,
            fields,
            method,
            headers,
            body,
            charset,
            retry,
            server_id,
            proxy,
            rate,
            rate_key,
            cookie_jar,
            cookie_url,
        })
    }

    pub fn url(&self) -> String {
        if self.method == Method::GET {
            if self.fields.is_empty() {
                self.url.clone()
            } else {
                format!("{}?{}", self.url, join_fields(&self.fields))
            }
        } else {
            join_query(&self.url, &self.query)
        }
    }

    pub fn method(&self) -> &Method {
        &self.method
    }

    pub fn headers(&self) -> &HeaderMap {
        &self.headers
    }

    pub fn charset(&self) -> Option<&str> {
        self.charset.as_deref()
    }

    pub fn retry(&self) -> u32 {
        self.retry
    }

    pub fn server_id(&self) -> Option<i64> {
        self.server_id
    }

    pub fn proxy(&self) -> Option<&str> {
        self.proxy.as_deref()
    }

    pub fn rate(&self) -> Option<&str> {
        self.rate.as_deref()
    }

    pub fn rate_key(&self) -> Option<&str> {
        self.rate_key.as_deref()
    }

    pub fn cookie_jar(&self) -> bool {
        self.cookie_jar
    }

    pub fn cookie_url(&self) -> &str {
        &self.cookie_url
    }

    pub fn request(&self) -> HttpRequest {
        let mut headers = self.headers.clone();
        let url = self.url();
        let body = if self.method == Method::POST {
            let raw = self.body.clone().unwrap_or_default();
            if !self.fields.is_empty() || raw.trim().is_empty() {
                headers
                    .entry(CONTENT_TYPE)
                    .or_insert(HeaderValue::from_static(
                        "application/x-www-form-urlencoded",
                    ));
                Some(join_fields(&self.fields).into())
            } else if headers.contains_key(CONTENT_TYPE) {
                Some(raw.into())
            } else {
                headers.insert(
                    CONTENT_TYPE,
                    HeaderValue::from_static("application/json; charset=UTF-8"),
                );
                Some(raw.into())
            }
        } else {
            None
        };

        HttpRequest {
            url,
            method: self.method.clone(),
            headers,
            body,
            charset: self.charset.clone(),
        }
    }
}

async fn source_headers(ctx: &UrlContext<'_>) -> Result<(HeaderMap, Option<String>)> {
    let mut headers = HeaderMap::new();
    let mut proxy = None;

    let raw = match ctx.source.and_then(|source| source.header()) {
        Some(text) if !text.trim().is_empty() => match js_rule_body(text) {
            Some(code) => eval_js(code, None, ctx).await?,
            None => text.to_string(),
        },
        _ => String::new(),
    };

    if !raw.trim().is_empty()
        && let Ok(serde_json::Value::Object(map)) = json::parse(&raw)
    {
        for (key, value) in map {
            let value = value_to_text(&value);
            if key.eq_ignore_ascii_case("proxy") {
                proxy = Some(value);
                continue;
            }
            insert_header(&mut headers, &key, &value);
        }
    }

    if !headers.contains_key(USER_AGENT) {
        insert_header(&mut headers, "User-Agent", DEFAULT_USER_AGENT);
    }
    Ok((headers, proxy))
}

fn js_rule_body(text: &str) -> Option<&str> {
    let trimmed = text.trim_start();
    if trimmed.len() >= 4 && trimmed[..4].eq_ignore_ascii_case("@js:") {
        return Some(&trimmed[4..]);
    }
    if trimmed.len() >= 4 && trimmed[..4].eq_ignore_ascii_case("<js>") {
        let code = &trimmed[4..];
        return Some(code.rfind('<').map_or(code, |index| &code[..index]));
    }
    None
}

async fn analyze_js(rule: &str, ctx: &UrlContext<'_>) -> Result<String> {
    let chars: Vec<char> = rule.chars().collect();
    let mut result: Option<String> = None;
    let mut cursor = 0;
    let mut start = 0;

    while cursor < chars.len() {
        if starts_with_ci(&chars, cursor, "@js:") {
            push_prefix(&chars[start..cursor], &mut result);
            let code: String = chars[cursor + 4..].iter().collect();
            result = Some(eval_js(&code, result.as_deref(), ctx).await?);
            start = chars.len();
            break;
        }
        if starts_with_ci(&chars, cursor, "<js>")
            && let Some(end) = find_ci(&chars, cursor + 4, "</js>")
        {
            push_prefix(&chars[start..cursor], &mut result);
            let code: String = chars[cursor + 4..end].iter().collect();
            result = Some(eval_js(&code, result.as_deref(), ctx).await?);
            cursor = end + 5;
            start = cursor;
            continue;
        }
        cursor += 1;
    }

    if start < chars.len() {
        push_prefix(&chars[start..], &mut result);
    }
    Ok(result.unwrap_or_else(|| rule.to_string()))
}

fn push_prefix(prefix: &[char], result: &mut Option<String>) {
    let text: String = prefix.iter().collect();
    let text = text.trim();
    if text.is_empty() {
        return;
    }
    let current = result.clone().unwrap_or_default();
    *result = Some(text.replace("@result", &current));
}

async fn replace_key_page(rule: &str, ctx: &UrlContext<'_>) -> Result<String> {
    let text = if rule.contains("{{") && rule.contains("}}") {
        replace_expressions(rule, ctx).await?
    } else {
        rule.to_string()
    };
    Ok(replace_pages(&text, ctx.page))
}

async fn replace_expressions(rule: &str, ctx: &UrlContext<'_>) -> Result<String> {
    let chars: Vec<char> = rule.chars().collect();
    let mut out = String::with_capacity(rule.len());
    let mut cursor = 0;
    while cursor < chars.len() {
        if chars[cursor] == '{'
            && chars.get(cursor + 1) == Some(&'{')
            && let Some(end) = find_seq(&chars, cursor + 2, &['}', '}'])
        {
            let inner: String = chars[cursor + 2..end].iter().collect();
            out.push_str(&resolve_expression(&inner, ctx).await?);
            cursor = end + 2;
            continue;
        }
        out.push(chars[cursor]);
        cursor += 1;
    }
    Ok(out)
}

async fn resolve_expression(inner: &str, ctx: &UrlContext<'_>) -> Result<String> {
    match inner.trim() {
        "key" => return Ok(ctx.key.unwrap_or_default().to_string()),
        "page" => return Ok(ctx.page.map(|page| page.to_string()).unwrap_or_default()),
        "baseUrl" => return Ok(ctx.base_url.to_string()),
        _ => {}
    }
    eval_js(inner.trim(), None, ctx).await
}

fn replace_pages(rule: &str, page: Option<i32>) -> String {
    let Some(page) = page else {
        return rule.to_string();
    };
    let chars: Vec<char> = rule.chars().collect();
    let mut out = String::with_capacity(rule.len());
    let mut cursor = 0;
    while cursor < chars.len() {
        if chars[cursor] == '<'
            && let Some(offset) = chars[cursor + 1..].iter().position(|ch| *ch == '>')
        {
            let end = cursor + 1 + offset;
            let inner: String = chars[cursor + 1..end].iter().collect();
            let pages: Vec<&str> = inner.split(',').map(str::trim).collect();
            let index = page.max(1) as usize;
            let chosen = if index < pages.len() {
                pages[index - 1]
            } else {
                pages.last().copied().unwrap_or_default()
            };
            out.push_str(chosen);
            cursor = end + 1;
            continue;
        }
        out.push(chars[cursor]);
        cursor += 1;
    }
    out
}

fn split_options(rule: &str) -> (String, Option<String>) {
    let chars: Vec<char> = rule.chars().collect();
    for index in 0..chars.len() {
        if chars[index] != ',' {
            continue;
        }
        let mut start = index;
        while start > 0 && chars[start - 1].is_whitespace() {
            start -= 1;
        }
        let mut end = index + 1;
        while end < chars.len() && chars[end].is_whitespace() {
            end += 1;
        }
        if chars.get(end) == Some(&'{') {
            let before: String = chars[..start].iter().collect();
            let after: String = chars[end..].iter().collect();
            return (before, Some(after));
        }
    }
    (rule.to_string(), None)
}

fn strip_options(text: &str) -> String {
    split_options(text).0
}

pub fn absolute_url(base: &str, path: &str) -> String {
    let path = path.trim();
    if base.is_empty() {
        return path.to_string();
    }
    let lower = path.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") || lower.starts_with("data:") {
        return path.to_string();
    }
    if lower.starts_with("javascript") {
        return String::new();
    }
    match url::Url::parse(base).and_then(|base| base.join(path)) {
        Ok(url) => url.to_string(),
        Err(_) => path.to_string(),
    }
}

fn analyze_fields(text: &str, charset: Option<&str>) -> Vec<(String, String)> {
    let mut fields = Vec::new();
    for field in text.split('&') {
        let field = field.trim();
        if field.is_empty() {
            continue;
        }
        let (key, value) = match field.split_once('=') {
            Some((key, value)) => (key, value),
            None => (field, ""),
        };
        let value = match charset {
            Some(label) if label.eq_ignore_ascii_case(ESCAPE) => encode::escape(value),
            Some(label) => encode::url_encode(value, encode::encoding_for(label).unwrap_or(UTF_8)),
            None => {
                if encode::has_url_encoded(value) {
                    value.to_string()
                } else {
                    encode::url_encode(value, UTF_8)
                }
            }
        };
        fields.push((key.to_string(), value));
    }
    fields
}

fn join_fields(fields: &[(String, String)]) -> String {
    fields
        .iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect::<Vec<_>>()
        .join("&")
}

fn split_query(text: &str) -> (String, String) {
    match text.find('?') {
        Some(position) => (
            text[..position].to_string(),
            text[position + 1..].to_string(),
        ),
        None => (text.to_string(), String::new()),
    }
}

fn join_query(url: &str, query: &str) -> String {
    if query.is_empty() {
        url.to_string()
    } else {
        format!("{url}?{query}")
    }
}

async fn eval_js(code: &str, result: Option<&str>, ctx: &UrlContext<'_>) -> Result<String> {
    if code.trim().is_empty() {
        return Ok(String::new());
    }
    let Some(js) = ctx.js else {
        return Err(Error::Js(format!(
            "URL 规则里的 JS 需要 JsEvaluator（M3 接上）：{}",
            shorten(code)
        )));
    };
    js.eval(JsRequest {
        script: code,
        source: ctx.source,
        base_url: ctx.base_url,
        page: ctx.page,
        key: ctx.key,
        result,
        src: None,
        response: None,
        book: None,
        chapter: None,
        variables: None,
    })
    .await
}

fn shorten(code: &str) -> String {
    let text: String = code.chars().take(60).collect();
    if text.chars().count() < code.chars().count() {
        format!("{text}…")
    } else {
        text
    }
}

fn insert_header(headers: &mut HeaderMap, name: &str, value: &str) {
    let name = name.trim();
    let Ok(name) = HeaderName::from_bytes(name.as_bytes()) else {
        tracing::warn!("请求头名字非法，已跳过: {name}");
        return;
    };
    let Ok(value) = HeaderValue::from_str(value.trim()) else {
        tracing::warn!("请求头 {name} 的值非法，已跳过");
        return;
    };
    headers.insert(name, value);
}

fn value_to_text(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Null => "null".to_string(),
        serde_json::Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

fn body_text(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::Null => None,
        serde_json::Value::String(text) if text.trim().is_empty() => None,
        serde_json::Value::String(text) => Some(text.clone()),
        other => Some(other.to_string()),
    }
}

fn value_as_i64(value: &serde_json::Value) -> Option<i64> {
    match value {
        serde_json::Value::Number(number) => number.as_i64(),
        serde_json::Value::String(text) => text.trim().parse().ok(),
        _ => None,
    }
}

fn is_json(text: &str) -> bool {
    let text = text.trim();
    (text.starts_with('{') && text.ends_with('}')) || (text.starts_with('[') && text.ends_with(']'))
}

fn is_xml(text: &str) -> bool {
    let text = text.trim();
    text.starts_with('<') && text.ends_with('>')
}

fn starts_with_ci(chars: &[char], at: usize, needle: &str) -> bool {
    let needle: Vec<char> = needle.chars().collect();
    chars
        .get(at..at + needle.len())
        .is_some_and(|slice| eq_ignore_ascii_case(slice, &needle))
}

fn find_ci(chars: &[char], from: usize, needle: &str) -> Option<usize> {
    let needle: Vec<char> = needle.chars().collect();
    (from..chars.len().saturating_sub(needle.len() - 1))
        .find(|index| eq_ignore_ascii_case(&chars[*index..*index + needle.len()], &needle))
}

fn eq_ignore_ascii_case(left: &[char], right: &[char]) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .zip(right)
            .all(|(left, right)| left.eq_ignore_ascii_case(right))
}

fn find_seq(chars: &[char], from: usize, needle: &[char]) -> Option<usize> {
    (from..=chars.len().saturating_sub(needle.len()))
        .find(|index| chars[*index..*index + needle.len()] == *needle)
}
