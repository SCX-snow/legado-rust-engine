use std::sync::Arc;

use legado_core::{Result, RuleError};

use crate::engine::RuleEngine;
use crate::selector::css::CssSelector;
use crate::selector::{RegexPipeline, Selector, compile as compile_regex};
use crate::split::{
    Combinator, JsSegment, JsSegmentKind, Piece, ReplaceParts, Slice, has_placeholders,
    literals_look_like_rule, regex_mode as legado_regex_mode, split_combinator, split_js,
    split_pieces, split_replace, strip_put_mask, take_puts,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    pub fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }

    pub fn range(&self) -> std::ops::Range<usize> {
        self.start..self.end
    }

    pub fn slice<'a>(&self, text: &'a str) -> &'a str {
        let start = char_offset(text, self.start);
        let end = char_offset(text, self.end);
        &text[start..end]
    }
}

#[derive(Debug, Clone)]
pub struct CompiledRule {
    pub text: String,
    pub steps: Vec<Step>,
}

#[derive(Debug, Clone)]
pub struct Step {
    pub span: Span,
    pub puts: Vec<(String, Arc<CompiledRule>)>,
    pub body: Body,
    pub replace: Option<ReplaceSpec>,
}

#[derive(Debug, Clone)]
pub enum Body {
    Rule(Rule),
    Script(Vec<Piece>),
    Identity,
}

#[derive(Debug, Clone)]
pub enum Rule {
    Concat(Vec<Rule>),
    Fallback(Vec<Rule>),
    Interleave(Vec<Rule>),
    Template {
        pieces: Vec<Piece>,
        span: Span,
    },
    Interpolate {
        pieces: Vec<Piece>,
        span: Span,
    },
    Literal {
        pieces: Vec<Piece>,
        span: Span,
        selector: Box<Rule>,
    },
    Leaf {
        selector: Selector,
        span: Span,
    },
}

#[derive(Debug, Clone)]
pub struct ReplaceSpec {
    pub regex: Option<Result<legado_regex::Regex, String>>,
    pub pieces: Option<Vec<Piece>>,
    pub pattern: String,
    pub replacement: String,
    pub first: bool,
    pub span: Span,
}

fn build_replace(parts: ReplaceParts, span: Span) -> ReplaceSpec {
    let pattern = strip_put_mask(&parts.pattern);
    let chars: Vec<char> = pattern.chars().collect();
    let (regex, pieces) = if has_placeholders(&chars) {
        (None, Some(split_pieces(&chars)))
    } else {
        (Some(compile_regex(&pattern)), None)
    };
    ReplaceSpec {
        regex,
        pieces,
        pattern,
        replacement: strip_put_mask(&parts.replacement),
        first: parts.first,
        span,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Css,
    XPath,
    AutoXPath,
    Json,
    Regex,
    Auto,
}

pub(crate) fn compile(engine: &RuleEngine, rule: &str) -> Result<CompiledRule> {
    let mut steps = Vec::new();
    for segment in split_js(rule) {
        steps.push(compile_segment(engine, &segment)?);
    }
    Ok(CompiledRule {
        text: rule.to_string(),
        steps,
    })
}

fn compile_segment(engine: &RuleEngine, segment: &JsSegment) -> Result<Step> {
    let chars: Vec<char> = segment.text.chars().collect();
    let span = Span::new(segment.offset, segment.offset + chars.len());

    let (masked, puts) = take_puts(&chars);
    let mut compiled_puts = Vec::new();
    for (key, value) in puts {
        compiled_puts.push((key, engine.compile(&value)?));
    }

    if segment.kind == JsSegmentKind::Js {
        let code_chars: Vec<char> = masked.chars().collect();
        let (code, replace) = split_replace(&code_chars, segment.offset);
        let code: Vec<char> = code.text.chars().collect();
        return Ok(Step {
            span,
            puts: compiled_puts,
            body: Body::Script(split_pieces(&code)),
            replace: replace.map(|parts| build_replace(parts, span)),
        });
    }

    let chars: Vec<char> = masked.chars().collect();
    let regex_mode = legado_regex_mode(&chars);
    let (body, replace) = split_replace(&chars, segment.offset);
    let (kind, rest) = split_prefix(&body);
    let body = if replace.is_some() && rest.trimmed().is_empty() {
        Body::Identity
    } else if regex_mode && kind != Some(Kind::Regex) {
        let literal = if kind == Some(Kind::Css) && starts_with_css_prefix(&body) {
            body.trimmed()
        } else {
            rest.clone()
        };
        let literal_chars: Vec<char> = literal.text.chars().collect();
        Body::Rule(Rule::Literal {
            pieces: split_pieces(&literal_chars),
            span: Span::new(literal.offset, literal.offset + literal_chars.len()),
            selector: Box::new(compile_rule(&rest, kind)?),
        })
    } else {
        Body::Rule(compile_rule(&rest, kind)?)
    };

    Ok(Step {
        span,
        puts: compiled_puts,
        body,
        replace: replace.map(|parts| build_replace(parts, span)),
    })
}

fn split_prefix(slice: &Slice) -> (Option<Kind>, Slice) {
    let trimmed = slice.trimmed();
    let text = trimmed.text.as_str();

    let (kind, rest) = if let Some(rest) = strip_ci(text, "@css:") {
        (Some(Kind::Css), rest)
    } else if let Some(rest) = text.strip_prefix("@@") {
        (Some(Kind::Css), rest)
    } else if let Some(rest) = strip_ci(text, "@xpath:") {
        (Some(Kind::XPath), rest)
    } else if let Some(rest) = strip_ci(text, "@json:") {
        (Some(Kind::Json), rest)
    } else if text.starts_with("$.") || text.starts_with("$[") {
        (Some(Kind::Json), text)
    } else if text.starts_with('/') {
        (Some(Kind::AutoXPath), text)
    } else {
        (None, text)
    };

    let consumed = trimmed.text.len() - rest.len();
    (
        kind,
        Slice {
            text: rest.to_string(),
            offset: trimmed.offset + consumed,
        },
    )
}

fn strip_ci<'a>(text: &'a str, prefix: &str) -> Option<&'a str> {
    let head = text.get(..prefix.len())?;
    head.eq_ignore_ascii_case(prefix)
        .then(|| &text[prefix.len()..])
}

fn starts_with_css_prefix(slice: &Slice) -> bool {
    strip_ci(slice.trimmed().text.as_str(), "@css:").is_some()
}

fn empty_template(offset: usize) -> Rule {
    Rule::Template {
        pieces: Vec::new(),
        span: Span::new(offset, offset),
    }
}

fn compile_rule(slice: &Slice, kind: Option<Kind>) -> Result<Rule> {
    let trimmed = slice.trimmed();
    if trimmed.is_empty() {
        return Ok(empty_template(trimmed.offset));
    }

    if kind == Some(Kind::Regex) {
        let text = strip_put_mask(&trimmed.text);
        let chars: Vec<char> = text.chars().collect();
        return Ok(Rule::Leaf {
            selector: Selector::Regex(RegexPipeline::parse(&text)),
            span: Span::new(trimmed.offset, trimmed.offset + chars.len()),
        });
    }

    let chars: Vec<char> = trimmed.text.chars().collect();
    let (parts, combinator) = split_combinator(&chars, trimmed.offset);
    let mut rules = Vec::new();
    for part in &parts {
        rules.push(compile_part(part, kind));
    }

    Ok(match (combinator, rules.len()) {
        (None, _) | (_, 1) => rules
            .pop()
            .unwrap_or_else(|| empty_template(trimmed.offset)),
        (Some(Combinator::Concat), _) => Rule::Concat(rules),
        (Some(Combinator::Fallback), _) => Rule::Fallback(rules),
        (Some(Combinator::Interleave), _) => Rule::Interleave(rules),
    })
}

fn compile_part(part: &Slice, kind: Option<Kind>) -> Rule {
    let trimmed = part.trimmed();
    let chars: Vec<char> = trimmed.text.chars().collect();
    let span = Span::new(trimmed.offset, trimmed.offset + chars.len());

    if has_placeholders(&chars) {
        let pieces = split_pieces(&chars);
        return if literals_look_like_rule(&pieces) {
            Rule::Interpolate { pieces, span }
        } else {
            Rule::Template { pieces, span }
        };
    }

    let text = strip_put_mask(&trimmed.text);
    if is_dollar_literal(&text) {
        return Rule::Template {
            pieces: vec![Piece::Literal(text)],
            span,
        };
    }

    Rule::Leaf {
        selector: build_selector(&text, kind.unwrap_or_else(|| dispatch(&text))),
        span,
    }
}

fn is_dollar_literal(text: &str) -> bool {
    text.strip_prefix('$')
        .is_some_and(|digits| !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()))
}

fn dispatch(text: &str) -> Kind {
    let text = text.trim();
    if text.starts_with("$.") || text.starts_with("$[") {
        Kind::Json
    } else if text.starts_with('/') {
        Kind::XPath
    } else {
        Kind::Auto
    }
}

fn strip_xpath_suffix(text: &str) -> String {
    let trimmed = text.trim_end();
    let Some(at) = trimmed.rfind('@') else {
        return trimmed.to_string();
    };
    let name = &trimmed[at + 1..];
    let is_attr_step = trimmed[..at].ends_with('/');
    let is_word = !name.is_empty()
        && name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-');
    if is_attr_step || !is_word {
        trimmed.to_string()
    } else {
        trimmed[..at].to_string()
    }
}

fn xpath_selector(text: &str) -> Selector {
    crate::selector::xpath::silence_errors();
    let text = strip_xpath_suffix(text);
    Selector::XPath {
        text: text.clone(),
        error: (!libxml::xpath::is_well_formed_xpath(&text))
            .then(|| format!("XPath 编译失败：{text}")),
    }
}

fn build_selector(text: &str, kind: Kind) -> Selector {
    match kind {
        Kind::Css => Selector::Css(CssSelector::parse(text, false)),
        Kind::XPath => xpath_selector(text),
        Kind::AutoXPath => Selector::Auto {
            json: crate::selector::json::JsonSelector::parse(text),
            other: Box::new(xpath_selector(text)),
        },
        Kind::Json => Selector::Json(crate::selector::json::JsonSelector::parse(text)),
        Kind::Regex => Selector::Regex(RegexPipeline::parse(text)),
        Kind::Auto => Selector::Auto {
            json: crate::selector::json::JsonSelector::parse(text),
            other: Box::new(Selector::Css(CssSelector::parse(text, false))),
        },
    }
}

fn char_offset(text: &str, chars: usize) -> usize {
    text.char_indices()
        .nth(chars)
        .map(|(index, _)| index)
        .unwrap_or(text.len())
}

pub(crate) fn rule_error(text: &str, span: Span, message: impl Into<String>) -> RuleError {
    RuleError::new(text, span.range(), message)
}
