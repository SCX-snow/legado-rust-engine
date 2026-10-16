#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Slice {
    pub text: String,
    pub offset: usize,
}

pub const PUT_MASK: char = '\u{0}';

fn is_filler(ch: char) -> bool {
    ch.is_whitespace() || ch == PUT_MASK
}

pub fn strip_put_mask(text: &str) -> String {
    text.chars().filter(|ch| *ch != PUT_MASK).collect()
}

impl Slice {
    pub fn new(chars: &[char], offset: usize) -> Self {
        Self {
            text: chars.iter().collect(),
            offset,
        }
    }

    pub fn trimmed(&self) -> Self {
        let chars: Vec<char> = self.text.chars().collect();
        let start = chars.iter().take_while(|ch| is_filler(**ch)).count();
        let end = chars
            .iter()
            .rposition(|ch| !is_filler(*ch))
            .map_or(start, |index| index + 1);
        Self {
            text: chars[start..end].iter().collect(),
            offset: self.offset + start,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.text.trim_matches(is_filler).is_empty()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Combinator {
    Concat,
    Fallback,
    Interleave,
}

impl Combinator {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Concat => "&&",
            Self::Fallback => "||",
            Self::Interleave => "%%",
        }
    }
}

const COMBINATORS: [(Combinator, [char; 2]); 3] = [
    (Combinator::Concat, ['&', '&']),
    (Combinator::Fallback, ['|', '|']),
    (Combinator::Interleave, ['%', '%']),
];

#[derive(Debug, Default, Clone)]
pub(crate) struct Balance {
    brackets: i32,
    parens: i32,
    braces: i32,
    quote: Option<char>,
    escaped: bool,
    regex: bool,
    regex_class: bool,
    prev: Option<char>,
}

impl Balance {
    pub(crate) fn step(&mut self, ch: char) -> bool {
        self.step_at(std::slice::from_ref(&ch), 0)
    }

    pub(crate) fn step_at(&mut self, chars: &[char], index: usize) -> bool {
        let ch = chars[index];
        let top = self.quote.is_none()
            && !self.regex
            && self.brackets == 0
            && self.parens == 0
            && self.braces == 0;
        if self.regex {
            if self.escaped {
                self.escaped = false;
            } else if ch == '\\' {
                self.escaped = true;
            } else if ch == '[' {
                self.regex_class = true;
            } else if ch == ']' {
                self.regex_class = false;
            } else if ch == '/' && !self.regex_class {
                self.regex = false;
                self.prev = Some('/');
            }
            return top;
        }
        if self.escaped {
            self.escaped = false;
            self.prev = Some(ch);
            return top;
        }
        if let Some(quote) = self.quote {
            if ch == '\\' {
                self.escaped = true;
            } else if ch == quote {
                self.quote = None;
                self.prev = Some(quote);
            }
            return top;
        }
        if ch == '/'
            && regex_can_start(self.prev, self.braces > 0)
            && regex_literal_end(chars, index).is_some()
        {
            self.regex = true;
            self.regex_class = false;
            self.prev = Some('/');
            return top;
        }
        match ch {
            '\\' => self.escaped = true,
            '\'' | '"' => self.quote = Some(ch),
            '[' => self.brackets += 1,
            ']' => self.brackets = (self.brackets - 1).max(0),
            '(' => self.parens += 1,
            ')' => self.parens = (self.parens - 1).max(0),
            '{' => self.braces += 1,
            '}' => self.braces = (self.braces - 1).max(0),
            _ => {}
        }
        if !ch.is_whitespace() {
            self.prev = Some(ch);
        }
        top
    }
}

fn starts_with(chars: &[char], at: usize, needle: &str) -> bool {
    needle.chars().enumerate().all(|(index, ch)| {
        chars
            .get(at + index)
            .is_some_and(|value| value.eq_ignore_ascii_case(&ch))
    })
}

pub fn regex_mode(text: &[char]) -> bool {
    let mut index = 0;
    let mut chunk_start = 0;
    let mut first_placeholder: Option<usize> = None;

    while index < text.len() {
        let Some(end) = placeholder_end(text, index) else {
            index += 1;
            continue;
        };
        if has_dollar_ref(&text[chunk_start..index]) {
            return true;
        }
        first_placeholder.get_or_insert(index);
        index = end;
        chunk_start = end;
    }
    if has_dollar_ref(&text[chunk_start..]) {
        return true;
    }

    match first_placeholder {
        Some(at) => at == 0 || !text[..at].iter().collect::<String>().contains("##"),
        None => false,
    }
}

fn placeholder_end(text: &[char], at: usize) -> Option<usize> {
    if text[at] == '{'
        && text.get(at + 1) == Some(&'{')
        && let Some(end) = find_matching(text, at + 1, '{', '}')
    {
        return Some(end + 2);
    }
    if starts_with(text, at, "@get:")
        && text.get(at + 5) == Some(&'{')
        && let Some(end) = find_matching(text, at + 5, '{', '}')
    {
        return Some(end + 1);
    }
    None
}

fn has_dollar_ref(chunk: &[char]) -> bool {
    let end = chunk
        .windows(2)
        .position(|pair| pair == ['#', '#'])
        .unwrap_or(chunk.len());
    chunk[..end]
        .windows(2)
        .any(|pair| pair[0] == '$' && pair[1].is_ascii_digit())
}

fn find_matching(chars: &[char], open_at: usize, open: char, close: char) -> Option<usize> {
    let mut depth = 0;
    let mut quote: Option<char> = None;
    let mut escaped = false;
    let mut prev: Option<char> = None;
    let mut index = open_at;
    while index < chars.len() {
        let ch = chars[index];
        if escaped {
            escaped = false;
        } else if let Some(current) = quote {
            if ch == '\\' {
                escaped = true;
            } else if ch == current {
                quote = None;
            }
        } else if ch == '/' && regex_can_start(prev, true) {
            match regex_literal_end(chars, index) {
                Some(end) => {
                    prev = Some('/');
                    index = end;
                }
                None => prev = Some(ch),
            }
        } else {
            match ch {
                '\\' => escaped = true,
                '\'' | '"' => quote = Some(ch),
                ch if ch == open => depth += 1,
                ch if ch == close => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(index);
                    }
                }
                _ => {}
            }
            if !ch.is_whitespace() {
                prev = Some(ch);
            }
        }
        index += 1;
    }
    None
}

fn regex_can_start(prev: Option<char>, in_template: bool) -> bool {
    match prev {
        None => in_template,
        Some(ch) => matches!(
            ch,
            '(' | ','
                | '='
                | ':'
                | '['
                | '!'
                | '&'
                | '|'
                | '?'
                | '{'
                | ';'
                | '+'
                | '-'
                | '*'
                | '%'
                | '<'
                | '>'
                | '^'
                | '~'
        ),
    }
}

fn regex_literal_end(chars: &[char], start: usize) -> Option<usize> {
    let mut index = start + 1;
    let mut in_class = false;
    while index < chars.len() {
        match chars[index] {
            '\\' => index += 2,
            '\n' | '\r' => return None,
            '[' => {
                in_class = true;
                index += 1;
            }
            ']' => {
                in_class = false;
                index += 1;
            }
            '/' if !in_class => return Some(index),
            _ => index += 1,
        }
    }
    None
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JsSegmentKind {
    Text,
    Js,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JsSegment {
    pub kind: JsSegmentKind,
    pub text: String,
    pub offset: usize,
}

pub fn split_js(rule: &str) -> Vec<JsSegment> {
    let chars: Vec<char> = rule.chars().collect();
    let mut segments = Vec::new();
    let mut start = 0;
    let mut pos = 0;

    while pos < chars.len() {
        let (body_start, body_end, next) = if starts_with(&chars, pos, "<js>") {
            let body_start = pos + 4;
            match (body_start..chars.len()).find(|&i| starts_with(&chars, i, "</js>")) {
                Some(close) => (body_start, close, close + 5),
                None => (body_start, chars.len(), chars.len()),
            }
        } else if starts_with(&chars, pos, "@js:") {
            (pos + 4, chars.len(), chars.len())
        } else {
            pos += 1;
            continue;
        };

        if pos > start {
            push_text_segment(&mut segments, &chars, start, pos);
        }
        segments.push(JsSegment {
            kind: JsSegmentKind::Js,
            text: chars[body_start..body_end].iter().collect(),
            offset: body_start,
        });

        start = next;
        pos = next;
    }

    if start < chars.len() {
        push_text_segment(&mut segments, &chars, start, chars.len());
    }
    segments
}

fn push_text_segment(segments: &mut Vec<JsSegment>, chars: &[char], start: usize, end: usize) {
    if chars[start..end].iter().all(|ch| *ch <= ' ') {
        return;
    }
    let mut first = start;
    while first < end && chars[first] <= ' ' {
        first += 1;
    }
    let mut last = end;
    while last > first && chars[last - 1] <= ' ' {
        last -= 1;
    }
    segments.push(JsSegment {
        kind: JsSegmentKind::Text,
        text: chars[first..last].iter().collect(),
        offset: first,
    });
}

pub fn split_top_level(text: &str, separator: char) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut balance = Balance::default();
    let mut start = 0;
    for (index, ch) in text.char_indices() {
        let top = balance.step(ch);
        if top && ch == separator {
            parts.push(&text[start..index]);
            start = index + ch.len_utf8();
        }
    }
    parts.push(&text[start..]);
    parts
}

pub fn split_combinator(text: &[char], offset: usize) -> (Vec<Slice>, Option<Combinator>) {
    let mut balance = Balance::default();
    let mut found: Option<Combinator> = None;

    for (index, &ch) in text.iter().enumerate() {
        let top = balance.step_at(text, index);
        if !top || found.is_some() {
            continue;
        }
        for (combinator, needle) in COMBINATORS {
            if ch == needle[0] && text.get(index + 1) == Some(&needle[1]) {
                found = Some(combinator);
                break;
            }
        }
    }

    let Some(combinator) = found else {
        return (vec![Slice::new(text, offset)], None);
    };

    let needle = combinator.as_str();
    let mut balance = Balance::default();
    let mut parts = Vec::new();
    let mut start = 0;
    let mut index = 0;
    while index < text.len() {
        let top = balance.step_at(text, index);
        if top && starts_with(text, index, needle) {
            parts.push(Slice::new(&text[start..index], offset + start));
            index += 2;
            start = index;
            continue;
        }
        index += 1;
    }
    parts.push(Slice::new(&text[start..], offset + start));
    (parts, Some(combinator))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplaceParts {
    pub pattern: String,
    pub replacement: String,
    pub first: bool,
}

pub fn split_replace(text: &[char], offset: usize) -> (Slice, Option<ReplaceParts>) {
    let mut cuts = Vec::new();
    let mut index = 0;
    while index + 1 < text.len() {
        if text[index] == '{'
            && text.get(index + 1) == Some(&'{')
            && let Some(end) = find_matching(text, index + 1, '{', '}')
        {
            index = end + 2;
            continue;
        }
        if text[index] == '#' && text[index + 1] == '#' {
            cuts.push(index);
            index += 2;
            continue;
        }
        index += 1;
    }

    let Some(&first_cut) = cuts.first() else {
        return (Slice::new(text, offset), None);
    };

    let mut parts = Vec::new();
    let mut start = 0;
    for cut in &cuts {
        parts.push(text[start..*cut].iter().collect::<String>());
        start = cut + 2;
    }
    parts.push(text[start..].iter().collect::<String>());

    (
        Slice::new(&text[..first_cut], offset),
        Some(ReplaceParts {
            pattern: parts.get(1).cloned().unwrap_or_default(),
            replacement: parts.get(2).cloned().unwrap_or_default(),
            first: parts.len() > 3,
        }),
    )
}

pub fn parse_puts(json: &str) -> Vec<(String, String)> {
    let inner = json.trim();
    let inner = inner
        .strip_prefix('{')
        .and_then(|text| text.strip_suffix('}'))
        .unwrap_or(inner);

    let mut puts = Vec::new();
    for item in split_top_level(inner, ',') {
        let Some(colon) = find_top_level(item, ':') else {
            continue;
        };
        let key = unquote(item[..colon].trim());
        let value = unquote(item[colon + 1..].trim());
        if !key.is_empty() {
            puts.push((key, value));
        }
    }
    puts
}

fn unquote(text: &str) -> String {
    if let Ok(value) = serde_json::from_str::<String>(text) {
        return value;
    }
    let bytes = text.as_bytes();
    if text.len() >= 2
        && ((bytes[0] == b'"' && bytes[text.len() - 1] == b'"')
            || (bytes[0] == b'\'' && bytes[text.len() - 1] == b'\''))
    {
        return text[1..text.len() - 1].to_string();
    }
    text.to_string()
}

pub fn find_top_level(text: &str, needle: char) -> Option<usize> {
    let mut balance = Balance::default();
    for (index, ch) in text.char_indices() {
        let top = balance.step(ch);
        if top && ch == needle {
            return Some(index);
        }
    }
    None
}

pub fn take_puts(text: &[char]) -> (String, Vec<(String, String)>) {
    let mut masked = text.to_vec();
    let mut puts = Vec::new();
    let mut index = 0;
    while index < text.len() {
        if !starts_with(text, index, "@put:") || text.get(index + 5) != Some(&'{') {
            index += 1;
            continue;
        }
        let brace = index + 5;
        let Some(end) = find_matching(text, brace, '{', '}') else {
            index += 1;
            continue;
        };
        let json: String = text[brace..=end].iter().collect();
        puts.extend(parse_puts(&json));
        for slot in masked.iter_mut().take(end + 1).skip(index) {
            *slot = PUT_MASK;
        }
        index = end + 1;
    }
    (masked.iter().collect(), puts)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Piece {
    Literal(String),
    Expr(String),
    Get(String),
    Ref(usize),
}

pub fn has_placeholders(text: &[char]) -> bool {
    split_pieces(text)
        .iter()
        .any(|piece| !matches!(piece, Piece::Literal(_)))
}

pub fn split_pieces(text: &[char]) -> Vec<Piece> {
    let mut pieces = Vec::new();
    let mut literal = String::new();
    let mut index = 0;

    while index < text.len() {
        if text[index] == PUT_MASK {
            index += 1;
            continue;
        }
        if text[index] == '{'
            && text.get(index + 1) == Some(&'{')
            && let Some(end) = find_matching(text, index + 1, '{', '}')
            && end > index + 2
        {
            flush(&mut literal, &mut pieces);
            pieces.push(Piece::Expr(text[index + 2..end].iter().collect()));
            index = end + 2;
            continue;
        }
        if starts_with(text, index, "@get:")
            && text.get(index + 5) == Some(&'{')
            && let Some(end) = find_matching(text, index + 5, '{', '}')
        {
            flush(&mut literal, &mut pieces);
            pieces.push(Piece::Get(text[index + 6..end].iter().collect()));
            index = end + 1;
            continue;
        }
        if text[index] == '$'
            && let Some(first) = text.get(index + 1).and_then(|ch| ch.to_digit(10))
            && first > 0
        {
            let mut digits = first.to_string();
            if let Some(second) = text.get(index + 2).and_then(|ch| ch.to_digit(10)) {
                digits.push(char::from_digit(second, 10).unwrap_or('0'));
            }
            flush(&mut literal, &mut pieces);
            pieces.push(Piece::Ref(digits.parse().unwrap_or(1)));
            index += 1 + digits.len();
            continue;
        }
        literal.push(text[index]);
        index += 1;
    }
    flush(&mut literal, &mut pieces);
    pieces
}

fn flush(literal: &mut String, pieces: &mut Vec<Piece>) {
    if !literal.is_empty() {
        pieces.push(Piece::Literal(std::mem::take(literal)));
    }
}

pub fn literals_look_like_rule(pieces: &[Piece]) -> bool {
    pieces.iter().any(|piece| match piece {
        Piece::Literal(text) => {
            text.contains('@')
                || text.contains("&&")
                || text.contains("||")
                || text.contains("%%")
                || text.contains("##")
        }
        _ => false,
    })
}
