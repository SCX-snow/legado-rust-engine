use std::sync::LazyLock;

use regex::Regex;

static NBSP: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(&nbsp;)+").unwrap());
static ESP: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(&ensp;|&emsp;)").unwrap());
static NO_PRINT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new("(&thinsp;|&zwnj;|&zwj;|\u{2009}|\u{200C}|\u{200D})").unwrap());
static WRAP_HTML: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"</?(?:div|p|br|hr|h\d|article|dd|dl)[^>]*>").unwrap());
static COMMENT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<!--[^>]*-->").unwrap());
static NOT_IMG: LazyLock<legado_regex::Regex> = LazyLock::new(|| {
    legado_regex::Regex::new(r"</?(?!img)[a-zA-Z]+(?=[ >])[^<>]*>").expect("内置正则")
});
static INDENT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s*\n+\s*").unwrap());
static LEAD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[\n\s]+").unwrap());
static TAIL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[\n\s]+$").unwrap());
static IMG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)<img[^>]*>").unwrap());
static IMG_SRC: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?i)\ssrc\s*=\s*"([^"]*)""#).unwrap());
static IMG_DATA: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?i)\sdata-[^=>]*=\s*"([^"]*)""#).unwrap());

pub fn format_keep_img(html: &str, base_url: Option<&str>) -> String {
    let kept = format_text(html);
    IMG.replace_all(&kept, |caps: &regex::Captures<'_>| {
        let tag = caps
            .get(0)
            .map(|matched| matched.as_str())
            .unwrap_or_default();
        match image_src(tag) {
            Some(src) => {
                let src = match base_url {
                    Some(base) if !base.is_empty() => resolve(base, &src),
                    _ => src.trim().to_string(),
                };
                format!("<img src=\"{src}\">")
            }
            None => tag.to_string(),
        }
    })
    .into_owned()
}

pub fn format_text(html: &str) -> String {
    let text = NBSP.replace_all(html, " ");
    let text = ESP.replace_all(&text, " ");
    let text = NO_PRINT.replace_all(&text, "");
    let text = WRAP_HTML.replace_all(&text, "\n");
    let text = COMMENT.replace_all(&text, "");
    let text = strip_tags_except_img(&text);
    let text = INDENT.replace_all(&text, "\n　　");
    let text = LEAD.replace_all(&text, "　　");
    TAIL.replace_all(&text, "").into_owned()
}

fn strip_tags_except_img(text: &str) -> String {
    NOT_IMG
        .replace_all(text, "")
        .unwrap_or_else(|_| text.to_string())
}

fn image_src(tag: &str) -> Option<String> {
    IMG_SRC
        .captures(tag)
        .or_else(|| IMG_DATA.captures(tag))
        .and_then(|caps| caps.get(1).map(|matched| matched.as_str().to_string()))
}

fn resolve(base: &str, path: &str) -> String {
    let path = path.trim();
    if path.is_empty() {
        return String::new();
    }
    let lower = path.to_ascii_lowercase();
    if lower.starts_with("data:") || lower.starts_with("http://") || lower.starts_with("https://") {
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
