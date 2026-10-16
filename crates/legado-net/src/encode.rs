use encoding_rs::{Encoding, UTF_8};

pub const ESCAPE: &str = "escape";

pub fn encoding_for(label: &str) -> Option<&'static Encoding> {
    let label = label.trim();
    if label.is_empty() || label.eq_ignore_ascii_case(ESCAPE) {
        return None;
    }
    Encoding::for_label(label.as_bytes())
}

fn encode_bytes(text: &str, encoding: &'static Encoding) -> Vec<u8> {
    let (bytes, _, had_errors) = encoding.encode(text);
    if !had_errors {
        return bytes.into_owned();
    }
    let mut out = Vec::with_capacity(text.len());
    for ch in text.chars() {
        let single = ch.to_string();
        let (bytes, _, bad) = encoding.encode(&single);
        if bad {
            out.push(b'?');
        } else {
            out.extend_from_slice(&bytes);
        }
    }
    out
}

pub fn url_encode(text: &str, encoding: &'static Encoding) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in encode_bytes(text, encoding) {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'.' | b'-' | b'*' | b'_' => {
                out.push(char::from(byte));
            }
            b' ' => out.push('+'),
            _ => {
                out.push('%');
                out.push(
                    char::from_digit(u32::from(byte >> 4), 16)
                        .unwrap_or('0')
                        .to_ascii_uppercase(),
                );
                out.push(
                    char::from_digit(u32::from(byte & 0xF), 16)
                        .unwrap_or('0')
                        .to_ascii_uppercase(),
                );
            }
        }
    }
    out
}

pub fn has_url_encoded(text: &str) -> bool {
    let chars: Vec<char> = text.chars().collect();
    let mut index = 0;
    while index < chars.len() {
        let ch = chars[index];
        if ch.is_ascii_alphanumeric() || "+-_.$:()!*@&#,[]".contains(ch) {
            index += 1;
            continue;
        }
        if ch == '%' && index + 2 < chars.len() {
            let (high, low) = (chars[index + 1], chars[index + 2]);
            if high.is_ascii_hexdigit() && low.is_ascii_hexdigit() {
                index += 3;
                continue;
            }
        }
        return false;
    }
    true
}

pub fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for unit in text.encode_utf16() {
        let code = u32::from(unit);
        if unit < 128 && (unit as u8 as char).is_ascii_alphanumeric() {
            out.push(char::from(unit as u8));
            continue;
        }
        out.push('%');
        if code < 0x10 {
            out.push('0');
        } else if code >= 0x100 {
            out.push('u');
        }
        out.push_str(&format!("{code:x}"));
    }
    out
}

pub fn resolve_encoding(
    bytes: &[u8],
    content_type: Option<&str>,
    charset_override: Option<&str>,
) -> &'static Encoding {
    if let Some(label) = charset_override
        && let Some(encoding) = encoding_for(label)
    {
        return encoding;
    }
    if let Some(content_type) = content_type
        && let Some(encoding) = content_type_charset(content_type)
    {
        return encoding;
    }
    if let Some((encoding, _)) = Encoding::for_bom(bytes) {
        return encoding;
    }
    if let Some(encoding) = html_meta_charset(bytes) {
        return encoding;
    }
    detect_encoding(bytes).unwrap_or(UTF_8)
}

pub fn content_type_charset(content_type: &str) -> Option<&'static Encoding> {
    let lower = content_type.to_ascii_lowercase();
    let start = lower.find("charset")? + "charset".len();
    let tail = lower[start..].trim_start();
    let tail = tail.strip_prefix('=')?.trim_start();
    let tail = tail.trim_start_matches(['"', '\'']);
    let end = tail
        .find(|ch: char| ch == ';' || ch == '"' || ch == '\'' || ch.is_whitespace())
        .unwrap_or(tail.len());
    encoding_for(&tail[..end])
}

pub fn decode(bytes: &[u8], encoding: &'static Encoding) -> String {
    encoding.decode(bytes).0.into_owned()
}

pub fn detect_encoding(bytes: &[u8]) -> Option<&'static Encoding> {
    if bytes.is_empty() {
        return None;
    }
    let mut detector = chardetng::EncodingDetector::new();
    detector.feed(bytes, true);
    Some(detector.guess(None, true))
}

pub fn html_meta_charset(bytes: &[u8]) -> Option<&'static Encoding> {
    const SNIFF_LIMIT: usize = 4096;
    let head = &bytes[..bytes.len().min(SNIFF_LIMIT)];
    let lower = String::from_utf8_lossy(head).to_ascii_lowercase();
    let mut rest = lower.as_str();
    while let Some(start) = rest.find("<meta") {
        let tail = &rest[start..];
        let end = tail.find('>').unwrap_or(tail.len());
        if let Some(encoding) = meta_tag_charset(&tail[..end]) {
            return Some(encoding);
        }
        if end >= tail.len() {
            break;
        }
        rest = &tail[end + 1..];
    }
    None
}

fn meta_tag_charset(tag: &str) -> Option<&'static Encoding> {
    let start = tag.find("charset")? + "charset".len();
    let tail = tag[start..].trim_start();
    let tail = tail.strip_prefix('=')?.trim_start();
    let tail = tail.trim_start_matches(['"', '\'']);
    let end = tail
        .find(|ch: char| ch == '"' || ch == '\'' || ch == ';' || ch == '>' || ch.is_whitespace())
        .unwrap_or(tail.len());
    encoding_for(&tail[..end])
}
