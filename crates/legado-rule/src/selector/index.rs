#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum IndexMode {
    #[default]
    All,
    Pick,
    Exclude,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IndexPart {
    Single(i64),
    Range {
        start: Option<i64>,
        end: Option<i64>,
        step: i64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct IndexFilter {
    pub mode: IndexMode,
    pub parts: Vec<IndexPart>,
}

impl IndexFilter {
    pub fn parse(text: &str) -> (String, Self) {
        let chars: Vec<char> = text.chars().collect();
        if chars.last() == Some(&']')
            && let Some(open) = find_open_bracket(&chars)
            && let Some(filter) = parse_bracket(&chars[open + 1..chars.len() - 1])
        {
            return (chars[..open].iter().collect(), filter);
        }
        parse_legacy(&chars)
    }

    pub fn apply(&self, len: usize) -> Vec<usize> {
        if self.mode == IndexMode::All || len == 0 {
            return (0..len).collect();
        }
        let mut picked: Vec<usize> = Vec::new();
        for part in &self.parts {
            match part {
                IndexPart::Single(value) => {
                    if let Some(index) = resolve(*value, len) {
                        push_unique(&mut picked, index);
                    }
                }
                IndexPart::Range { start, end, step } => {
                    for index in expand(*start, *end, *step, len) {
                        push_unique(&mut picked, index);
                    }
                }
            }
        }
        match self.mode {
            IndexMode::Pick => picked,
            IndexMode::Exclude => (0..len).filter(|index| !picked.contains(index)).collect(),
            IndexMode::All => (0..len).collect(),
        }
    }
}

fn push_unique(target: &mut Vec<usize>, value: usize) {
    if !target.contains(&value) {
        target.push(value);
    }
}

fn resolve(value: i64, len: usize) -> Option<usize> {
    if value >= 0 {
        let index = value as usize;
        (index < len).then_some(index)
    } else {
        let offset = value.unsigned_abs() as usize;
        (offset <= len).then(|| len - offset)
    }
}

fn find_open_bracket(chars: &[char]) -> Option<usize> {
    let mut depth = 0;
    for (index, &ch) in chars.iter().enumerate().rev() {
        match ch {
            ']' => depth += 1,
            '[' => {
                depth -= 1;
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
    }
    None
}

fn parse_bracket(chars: &[char]) -> Option<IndexFilter> {
    let mut body = chars;
    let mut mode = IndexMode::Pick;
    if let Some((&first, rest)) = body.split_first()
        && first == '!'
    {
        mode = IndexMode::Exclude;
        body = rest;
    }

    let text: String = body.iter().collect();
    if text.trim().is_empty() {
        return None;
    }

    let mut parts = Vec::new();
    for item in text.split(',') {
        let item = item.trim();
        if item.is_empty() {
            continue;
        }
        let fields: Vec<&str> = item.split(':').map(str::trim).collect();
        match fields.len() {
            1 => parts.push(IndexPart::Single(fields[0].parse().ok()?)),
            2 | 3 => {
                let start = parse_optional(fields[0])?;
                let end = parse_optional(fields[1])?;
                let step = if fields.len() == 3 {
                    parse_optional(fields[2])?.unwrap_or(1)
                } else {
                    1
                };
                parts.push(IndexPart::Range { start, end, step });
            }
            _ => return None,
        }
    }

    (!parts.is_empty()).then_some(IndexFilter { mode, parts })
}

fn parse_optional(text: &str) -> Option<Option<i64>> {
    if text.is_empty() {
        Some(None)
    } else {
        text.parse().ok().map(Some)
    }
}

fn parse_legacy(chars: &[char]) -> (String, IndexFilter) {
    let whole = || chars.iter().collect::<String>();
    let mut numbers: Vec<i64> = Vec::new();
    let mut digits = String::new();
    let mut negative = false;
    let mut index = chars.len();

    while index > 0 {
        index -= 1;
        let ch = chars[index];
        if ch == ' ' {
            continue;
        }
        if ch.is_ascii_digit() {
            digits.insert(0, ch);
            continue;
        }
        if ch == '-' {
            negative = true;
            continue;
        }
        if ch == ':' || ch == '.' || ch == '!' {
            if digits.is_empty() {
                return (whole(), IndexFilter::default());
            }
            let value: i64 = digits.parse().unwrap_or_default();
            numbers.push(if negative { -value } else { value });
            digits.clear();
            negative = false;

            if ch == ':' {
                continue;
            }
            let mode = if ch == '!' {
                IndexMode::Exclude
            } else {
                IndexMode::Pick
            };
            let parts = numbers
                .iter()
                .rev()
                .map(|value| IndexPart::Single(*value))
                .collect();
            return (chars[..index].iter().collect(), IndexFilter { mode, parts });
        }
        return (whole(), IndexFilter::default());
    }

    (whole(), IndexFilter::default())
}

fn expand(start: Option<i64>, end: Option<i64>, step: i64, len: usize) -> Vec<usize> {
    if len == 0 {
        return Vec::new();
    }
    let len_i = len as i64;
    let start = match start {
        None => 0,
        Some(value) => clamp(value, len_i),
    };
    let end = match end {
        None => len_i - 1,
        Some(value) => clamp(value, len_i),
    };

    if start == end || step >= len_i {
        return vec![start as usize];
    }
    let step = if step > 0 {
        step
    } else if -step < len_i {
        step + len_i
    } else {
        1
    };

    let mut out = Vec::new();
    if end > start {
        let mut current = start;
        while current <= end {
            out.push(current as usize);
            current += step;
        }
    } else {
        let mut current = start;
        while current >= end {
            out.push(current as usize);
            current -= step;
        }
    }
    out
}

fn clamp(value: i64, len: i64) -> i64 {
    if value >= 0 {
        if value < len { value } else { len - 1 }
    } else if -value <= len {
        len + value
    } else {
        0
    }
}
