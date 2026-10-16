use std::sync::LazyLock;

use legado_regex::Regex;

static NAME: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\s+作\s*者.*|\s+\S+\s+著").expect("内置正则"));
static AUTHOR: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s*作\s*者[:：\s]+|\s+著").expect("内置正则"));

pub(crate) fn format_book_name(name: &str) -> String {
    NAME.replace_all(name, "")
        .unwrap_or_else(|_| name.to_string())
        .trim()
        .to_string()
}

pub(crate) fn format_book_author(author: &str) -> String {
    AUTHOR
        .replace_all(author, "")
        .unwrap_or_else(|_| author.to_string())
        .trim()
        .to_string()
}

pub(crate) fn word_count_format(word_count: &str) -> String {
    let word_count = word_count.trim();
    if word_count.is_empty() {
        return String::new();
    }
    let Ok(words) = word_count.parse::<i64>() else {
        return word_count.to_string();
    };
    if words <= 0 {
        return String::new();
    }
    if words > 10000 {
        let wan = words as f64 / 10000.0;
        let text = format!("{wan:.1}");
        let text = text.trim_end_matches('0').trim_end_matches('.');
        format!("{text}万字")
    } else {
        format!("{words}字")
    }
}

pub(crate) fn is_true(text: &str) -> bool {
    let text = text.trim();
    if text.is_empty() || text == "null" {
        return false;
    }
    !matches!(
        text.to_ascii_lowercase().as_str(),
        "false" | "no" | "not" | "0"
    )
}

pub(crate) fn regex_full_match(pattern: &str, text: &str) -> bool {
    match Regex::new(pattern) {
        Ok(regex) => regex
            .find(text)
            .ok()
            .flatten()
            .is_some_and(|(start, end)| start == 0 && end == text.len()),
        Err(_) => false,
    }
}
