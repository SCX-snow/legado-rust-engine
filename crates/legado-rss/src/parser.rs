use std::sync::LazyLock;

use legado_core::RssArticle;
use legado_regex::Regex;
use quick_xml::events::{BytesStart, Event};
use quick_xml::reader::Reader;

pub fn parse_default(sort_name: &str, xml: &str, source_url: &str) -> Vec<RssArticle> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(false);
    let mut articles = Vec::new();
    let mut current: Option<RssArticle> = None;
    let mut inside_item = false;
    while let Ok(event) = reader.read_event() {
        match event {
            Event::Start(start) => {
                let name = tag_name(start.name().as_ref());
                if name.eq_ignore_ascii_case("item") || name.eq_ignore_ascii_case("entry") {
                    inside_item = true;
                    current = Some(RssArticle::default());
                } else if inside_item {
                    handle_start(&mut reader, &start, &name, current.as_mut());
                }
            }
            Event::Empty(start) => {
                if inside_item {
                    handle_empty(&start, current.as_mut());
                }
            }
            Event::End(end) => {
                let name = tag_name(end.name().as_ref());
                if name.eq_ignore_ascii_case("item") || name.eq_ignore_ascii_case("entry") {
                    inside_item = false;
                    if let Some(mut article) = current.take() {
                        article.origin = source_url.to_string();
                        article.sort = sort_name.to_string();
                        articles.push(article);
                    }
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    articles
}

fn handle_start(
    reader: &mut Reader<&[u8]>,
    start: &BytesStart<'_>,
    name: &str,
    article: Option<&mut RssArticle>,
) {
    let Some(article) = article else {
        return;
    };
    match name.to_ascii_lowercase().as_str() {
        "title" => article.title = read_text(reader, start),
        "link" => {
            let text = read_text(reader, start);
            if !text.is_empty() {
                article.link = text;
            }
        }
        "description" | "summary" => {
            let text = read_text(reader, start);
            if article.image.is_none() {
                article.image = get_image_url(&text);
            }
            article.description = Some(text);
        }
        "content:encoded" | "content" => {
            let text = read_text(reader, start);
            if article.image.is_none() {
                article.image = get_image_url(&text);
            }
            article.content = Some(text);
        }
        "pubdate" | "published" | "updated" | "time" => {
            article.pub_date = Some(read_text(reader, start));
        }
        _ => {}
    }
}

fn handle_empty(start: &BytesStart<'_>, article: Option<&mut RssArticle>) {
    let Some(article) = article else {
        return;
    };
    let name = tag_name(start.name().as_ref());
    match name.to_ascii_lowercase().as_str() {
        "media:thumbnail" | "media:content" => {
            if let Some(url) = attribute(start, "url") {
                article.image = Some(url);
            }
        }
        "enclosure" => {
            let kind = attribute(start, "type").unwrap_or_default();
            if kind.contains("image/")
                && let Some(url) = attribute(start, "url")
            {
                article.image = Some(url);
            }
        }
        _ => {}
    }
}

fn read_text(reader: &mut Reader<&[u8]>, start: &BytesStart<'_>) -> String {
    let end_name = start.name().as_ref().to_vec();
    let mut text = String::new();
    loop {
        match reader.read_event() {
            Ok(Event::Text(raw)) => {
                if let Ok(value) = raw.decode() {
                    match quick_xml::escape::unescape(&value) {
                        Ok(unescaped) => text.push_str(&unescaped),
                        Err(_) => text.push_str(&value),
                    }
                }
            }
            Ok(Event::CData(raw)) => {
                if let Ok(value) = std::str::from_utf8(raw.as_ref()) {
                    text.push_str(value);
                }
            }
            Ok(Event::GeneralRef(raw)) => {
                if let Ok(name) = raw.decode() {
                    if let Some(value) = quick_xml::escape::resolve_predefined_entity(&name) {
                        text.push_str(value);
                    } else if let Ok(Some(ch)) = raw.resolve_char_ref() {
                        text.push(ch);
                    }
                }
            }
            Ok(Event::End(end)) => {
                if end.name().as_ref() == end_name.as_slice() {
                    break;
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    text.trim().to_string()
}

fn attribute(start: &BytesStart<'_>, key: &str) -> Option<String> {
    for attribute in start.attributes().flatten() {
        if attribute.key.as_ref().eq_ignore_ascii_case(key.as_bytes())
            && let Ok(value) = attribute.unescape_value()
        {
            return Some(value.trim().to_string());
        }
    }
    None
}

fn tag_name(raw: &[u8]) -> String {
    String::from_utf8_lossy(raw).to_string()
}

pub fn get_image_url(html: &str) -> Option<String> {
    static IMG: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r#"(?i)<img [^>]*>"#).expect("内置正则"));
    static SRC: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r#"(?i)src\s*=\s*"([^"]+)""#).expect("内置正则"));
    let (start, end) = IMG.find(html).ok().flatten()?;
    let captures = SRC.captures(&html[start..end]).ok().flatten()?;
    captures.get(1).map(|value| value.trim().to_string())
}
