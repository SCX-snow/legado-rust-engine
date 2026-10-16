use ego_tree::NodeRef;
use legado_regex::Regex;
use scraper::{ElementRef, Html, Node, Selector as StandardSelector};

use crate::content::{Content, Value};
use crate::selector::index::IndexFilter;
use crate::split::{Balance, split_top_level};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleOutput {
    Elements,
    Text,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Extract {
    Element,
    Text,
    TextNodes,
    OwnText,
    Html,
    All,
    InnerHtml,
    Attr(String),
}

impl Extract {
    fn parse(text: &str) -> Self {
        let text = text.trim();
        match text {
            "text" => Self::Text,
            "textNodes" => Self::TextNodes,
            "ownText" => Self::OwnText,
            "html" => Self::Html,
            "all" => Self::All,
            "__innerHtml" => Self::InnerHtml,
            other => Self::Attr(other.to_string()),
        }
    }
}

fn first_element(html: &Html) -> Option<ElementRef<'_>> {
    let root = html
        .tree
        .root()
        .children()
        .find(|child| child.value().is_element())
        .and_then(ElementRef::wrap)?;
    if matches!(html.tree.root().value(), Node::Fragment) && root.value().name() == "html" {
        return root.child_elements().next();
    }
    Some(root)
}

fn parse_steps(selection: &str) -> Vec<CssStep> {
    let steps: Vec<CssStep> = split_top_level(selection, '@')
        .into_iter()
        .filter(|part| !part.trim().is_empty())
        .map(CssStep::parse)
        .collect();
    if !steps.is_empty() || selection.trim().is_empty() {
        return steps;
    }
    vec![CssStep::parse(selection)]
}

fn run_steps<'a>(html: &'a Html, steps: &[CssStep]) -> Result<Vec<ElementRef<'a>>, String> {
    let mut elements = vec![html.root_element()];
    let mut document = true;
    for step in steps {
        elements = step.apply(html, &elements, document)?;
        document = false;
    }
    Ok(elements)
}

#[derive(Debug, Clone)]
struct CssStep {
    select: CssSelect,
    filters: Vec<Filter>,
    index: IndexFilter,
    child: bool,
}

#[derive(Debug, Clone)]
enum CssSelect {
    Children,
    Class(String),
    Tag(String),
    Id(String),
    OwnText(String),
    Standard(Result<StandardSelector, String>),
    Union(Vec<CssStep>),
    Chain {
        left: Box<CssStep>,
        combinator: char,
        right: Box<CssStep>,
        tail: Option<Box<CssStep>>,
    },
}

#[derive(Debug, Clone)]
enum Filter {
    Eq(i64),
    Lt(i64),
    Gt(i64),
    Contains(String),
    ContainsOwn(String),
    Matches(Result<Regex, String>),
    MatchesOwn(Result<Regex, String>),
    AttrMatches(String, Result<Regex, String>),
    Has(Box<CssSelector>),
    Not(Vec<CssSelector>),
    Invalid(String),
}

#[derive(Debug, Clone)]
pub struct CssSelector {
    element_steps: Vec<CssStep>,
    text_steps: Vec<CssStep>,
    text_extract: Extract,
    bare: Option<String>,
}

impl CssSelector {
    pub fn parse(raw: &str, pure: bool) -> Self {
        let raw = raw.trim();
        let parts = split_top_level(raw, '@');
        let bare = (!pure && parts.len() == 1).then(|| raw.to_string());

        let element_steps = parse_steps(raw);

        let (text_selection, text_extract) = if pure {
            match raw.rfind('@') {
                Some(at) => (raw[..at].to_string(), Extract::parse(&raw[at + 1..])),
                None => (raw.to_string(), Extract::Element),
            }
        } else if parts.len() > 1 {
            let last = parts.last().copied().unwrap_or_default();
            (parts[..parts.len() - 1].join("@"), Extract::parse(last))
        } else {
            (raw.to_string(), Extract::Element)
        };
        let text_steps = parse_steps(&text_selection);

        Self {
            element_steps,
            text_steps,
            text_extract,
            bare,
        }
    }

    pub fn eval(&self, content: &Content, output: RuleOutput) -> Result<Value, String> {
        content.with_html(|html| self.eval_html(html, output))
    }

    fn parse_self(text: &str) -> Self {
        let (standard, attr_filters) = extract_attr_regex(text);
        let mut steps = parse_steps(&standard);
        if steps.is_empty() {
            steps.push(CssStep {
                select: CssSelect::Children,
                filters: Vec::new(),
                index: IndexFilter::default(),
                child: false,
            });
        }
        if !attr_filters.is_empty()
            && let Some(step) = steps.first_mut()
        {
            step.filters.extend(attr_filters);
        }
        Self {
            element_steps: steps.clone(),
            text_steps: steps,
            text_extract: Extract::Element,
            bare: None,
        }
    }

    fn matches_element(&self, element: ElementRef<'_>) -> Result<bool, String> {
        match self.element_steps.as_slice() {
            [step] => step.matches_element(element),
            _ => Err("`:not(...)` 里只支持单段选择器".to_string()),
        }
    }

    fn eval_html(&self, html: &Html, output: RuleOutput) -> Result<Value, String> {
        if output == RuleOutput::Text {
            if let Some(token) = &self.bare {
                let root: Vec<ElementRef<'_>> = first_element(html).into_iter().collect();
                return Ok(self.extract_from(&root, &Extract::parse(token)));
            }
            let elements = run_steps(html, &self.text_steps)?;
            return Ok(self.extract_from(&elements, &self.text_extract));
        }

        let elements = run_steps(html, &self.element_steps)?;
        Ok(if elements.is_empty() {
            Value::None
        } else {
            Value::Elements(
                elements
                    .iter()
                    .map(|element| crate::selector::pretty::outer_html(*element, false))
                    .collect(),
            )
        })
    }

    fn select_in<'a>(&self, element: ElementRef<'a>) -> Result<Vec<ElementRef<'a>>, String> {
        let mut elements = vec![element];
        for step in &self.element_steps {
            elements = step.apply_element(&elements)?;
        }
        Ok(elements)
    }

    fn extract_from(&self, elements: &[ElementRef<'_>], extract: &Extract) -> Value {
        let mut out: Vec<String> = Vec::new();
        match extract {
            Extract::Element => {
                for element in elements {
                    let html = escape_nbsp(&element.html());
                    if !html.is_empty() {
                        out.push(html);
                    }
                }
            }
            Extract::Text => {
                for element in elements {
                    let text = element_text(*element);
                    if !text.is_empty() {
                        out.push(text);
                    }
                }
            }
            Extract::TextNodes => {
                for element in elements {
                    let nodes = own_text_nodes(*element);
                    if !nodes.is_empty() {
                        out.push(nodes.join("\n"));
                    }
                }
            }
            Extract::OwnText => {
                for element in elements {
                    let text = element_own_text(*element);
                    if !text.is_empty() {
                        out.push(text);
                    }
                }
            }
            Extract::Html => {
                let html = elements_html(elements, true);
                if !html.is_empty() {
                    out.push(html);
                }
            }
            Extract::All => {
                let html = elements_html(elements, false);
                if !html.is_empty() {
                    out.push(html);
                }
            }
            Extract::InnerHtml => {
                let html = elements
                    .iter()
                    .map(|element| crate::selector::pretty::inner_html(*element))
                    .collect::<Vec<_>>()
                    .join("");
                if !html.is_empty() {
                    out.push(html);
                }
            }
            Extract::Attr(name) => {
                for element in elements {
                    if let Some(value) = element.attr(name)
                        && !value.trim().is_empty()
                        && !out.iter().any(|item| item == value)
                    {
                        out.push(value.to_string());
                    }
                }
            }
        }

        match out.len() {
            0 => Value::None,
            1 => Value::Text(out.remove(0)),
            _ => Value::List(out),
        }
    }
}

impl CssStep {
    fn parse(raw: &str) -> Self {
        let trimmed = raw.trim();
        let branches = split_top_level(trimmed, ',');
        if branches.len() > 1 {
            let steps: Vec<CssStep> = branches
                .iter()
                .map(|branch| CssStep::parse(branch))
                .collect();
            return Self {
                select: CssSelect::Union(steps),
                filters: Vec::new(),
                index: IndexFilter::default(),
                child: false,
            };
        }
        if !split_filters(trimmed).1.is_empty()
            && let Some((left, combinator, right)) = split_top_combinator(trimmed)
        {
            let (first, tail) = split_first_compound(&right);
            return Self {
                select: CssSelect::Chain {
                    left: Box::new(CssStep::parse(&left)),
                    combinator,
                    right: Box::new(CssStep::parse(&first)),
                    tail: (!tail.is_empty()).then(|| Box::new(CssStep::parse(&tail))),
                },
                filters: Vec::new(),
                index: IndexFilter::default(),
                child: false,
            };
        }
        let (child, trimmed) = match trimmed.strip_prefix('>') {
            Some(rest) => (true, rest.trim()),
            None => (false, trimmed),
        };
        let (without_index, index) = IndexFilter::parse(trimmed);
        let (core, filters) = split_filters(without_index.trim());
        let select = match core.trim() {
            "" | "children" => CssSelect::Children,
            other => match other.split_once('.') {
                Some(("class", name)) => CssSelect::Class(name.to_string()),
                Some(("tag", name)) => CssSelect::Tag(name.to_string()),
                Some(("id", name)) => CssSelect::Id(name.to_string()),
                Some(("text", name)) => CssSelect::OwnText(name.to_string()),
                _ => CssSelect::Standard(parse_standard(other)),
            },
        };
        Self {
            select,
            filters,
            index,
            child,
        }
    }

    fn matches_element(&self, element: ElementRef<'_>) -> Result<bool, String> {
        if !self.select.matches_self(element)? {
            return Ok(false);
        }
        let mut found = vec![element];
        apply_filters(&mut found, &self.filters)?;
        Ok(!found.is_empty())
    }

    fn apply<'a>(
        &self,
        html: &'a Html,
        inputs: &[ElementRef<'a>],
        document: bool,
    ) -> Result<Vec<ElementRef<'a>>, String> {
        self.gather(inputs, Some((html, document)))
    }

    fn apply_element<'a>(&self, inputs: &[ElementRef<'a>]) -> Result<Vec<ElementRef<'a>>, String> {
        self.gather(inputs, None)
    }

    fn gather<'a>(
        &self,
        inputs: &[ElementRef<'a>],
        document: Option<(&'a Html, bool)>,
    ) -> Result<Vec<ElementRef<'a>>, String> {
        let mut out = Vec::new();
        for input in inputs {
            let mut found = match &self.select {
                CssSelect::Union(branches) => union_branches(branches, *input, document)?,
                _ => self.candidates(*input, document)?,
            };
            self.finish(&mut found, &mut out)?;
        }
        Ok(out)
    }

    fn candidates<'a>(
        &self,
        input: ElementRef<'a>,
        document: Option<(&'a Html, bool)>,
    ) -> Result<Vec<ElementRef<'a>>, String> {
        if self.child {
            let mut found = Vec::new();
            for child in input.child_elements() {
                if self.matches_element(child)? {
                    found.push(child);
                }
            }
            return Ok(found);
        }
        Ok(match &self.select {
            CssSelect::Standard(selector) => {
                let selector = selector.as_ref().map_err(|error| error.clone())?;
                match document {
                    Some((html, true)) => html.select(selector).collect(),
                    _ => {
                        let mut found: Vec<ElementRef<'a>> = Vec::new();
                        if selector.matches(&input) {
                            found.push(input);
                        }
                        found.extend(input.select(selector));
                        found
                    }
                }
            }
            other => other.select(input),
        })
    }

    fn finish<'a>(
        &self,
        found: &mut Vec<ElementRef<'a>>,
        out: &mut Vec<ElementRef<'a>>,
    ) -> Result<(), String> {
        apply_filters(found, &self.filters)?;
        let picked = self.index.apply(found.len());
        out.extend(picked.into_iter().map(|index| found[index]));
        Ok(())
    }
}

fn union_branches<'a>(
    branches: &[CssStep],
    input: ElementRef<'a>,
    document: Option<(&'a Html, bool)>,
) -> Result<Vec<ElementRef<'a>>, String> {
    let mut hits: Vec<ElementRef<'a>> = Vec::new();
    for branch in branches {
        let mut found = branch.candidates(input, document)?;
        branch.finish(&mut found, &mut hits)?;
    }
    if hits.is_empty() {
        return Ok(hits);
    }
    let mut out = Vec::new();
    match document {
        Some((html, true)) => {
            for node in html.tree.nodes() {
                if let Some(element) = ElementRef::wrap(node)
                    && hits.contains(&element)
                {
                    out.push(element);
                }
            }
        }
        _ => {
            if hits.contains(&input) {
                out.push(input);
            }
            for element in input.descendent_elements() {
                if hits.contains(&element) {
                    out.push(element);
                }
            }
        }
    }
    Ok(out)
}

impl CssSelect {
    fn matches_self(&self, element: ElementRef<'_>) -> Result<bool, String> {
        Ok(match self {
            Self::Children => true,
            Self::Class(name) => jsoup_has_class(element, name),
            Self::Tag(name) => element.value().name().eq_ignore_ascii_case(name),
            Self::Id(name) => element.value().id() == Some(name.as_str()),
            Self::OwnText(text) => element_own_text(element)
                .to_lowercase()
                .contains(&text.to_lowercase()),
            Self::Standard(selector) => selector
                .as_ref()
                .map_err(|error| error.clone())?
                .matches(&element),
            Self::Chain {
                left,
                combinator,
                right,
                tail,
            } => match tail {
                None => chain_matches(left, *combinator, right, element)?,
                Some(_) => {
                    let root = element.ancestors().last().and_then(ElementRef::wrap);
                    match root {
                        Some(root) => self.select(root).contains(&element),
                        None => false,
                    }
                }
            },
            Self::Union(branches) => {
                let mut hit = false;
                for branch in branches {
                    if branch.matches_element(element)? {
                        hit = true;
                        break;
                    }
                }
                hit
            }
        })
    }

    fn select<'a>(&self, element: ElementRef<'a>) -> Vec<ElementRef<'a>> {
        match self {
            Self::Children => element.child_elements().collect(),
            Self::Chain {
                left,
                combinator,
                right,
                tail,
            } => {
                let hits: Vec<ElementRef<'a>> = element
                    .descendent_elements()
                    .filter(|child| {
                        chain_matches(left, *combinator, right, *child).unwrap_or(false)
                    })
                    .collect();
                match tail {
                    None => hits,
                    Some(tail) => {
                        let mut out = Vec::new();
                        for hit in hits {
                            out.extend(tail.apply_element(&[hit]).unwrap_or_default());
                        }
                        out
                    }
                }
            }
            Self::Class(name) => element
                .descendent_elements()
                .filter(|child| jsoup_has_class(*child, name))
                .collect(),
            Self::Tag(name) => element
                .descendent_elements()
                .filter(|child| child.value().name().eq_ignore_ascii_case(name))
                .collect(),
            Self::Id(name) => element
                .descendent_elements()
                .filter(|child| child.value().id() == Some(name.as_str()))
                .collect(),
            Self::OwnText(text) => {
                let needle = text.to_lowercase();
                element
                    .descendent_elements()
                    .filter(|child| element_own_text(*child).to_lowercase().contains(&needle))
                    .collect()
            }
            Self::Standard(_) => Vec::new(),
            Self::Union(_) => Vec::new(),
        }
    }
}

fn element_sibling_index(element: ElementRef<'_>) -> i64 {
    element
        .prev_siblings()
        .filter(|node| matches!(node.value(), Node::Element(_)))
        .count() as i64
}

fn apply_filters(elements: &mut Vec<ElementRef<'_>>, filters: &[Filter]) -> Result<(), String> {
    for filter in filters {
        match filter {
            Filter::Eq(index) => {
                elements.retain(|element| element_sibling_index(*element) == *index)
            }
            Filter::Lt(index) => {
                elements.retain(|element| element_sibling_index(*element) < *index)
            }
            Filter::Gt(index) => {
                elements.retain(|element| element_sibling_index(*element) > *index)
            }
            Filter::Contains(text) => {
                let needle = text.to_lowercase();
                elements.retain(|element| element_text(*element).to_lowercase().contains(&needle));
            }
            Filter::ContainsOwn(text) => {
                let needle = text.to_lowercase();
                elements
                    .retain(|element| element_own_text(*element).to_lowercase().contains(&needle));
            }
            Filter::Matches(regex) => {
                let regex = regex.as_ref().map_err(|error| error.clone())?;
                elements.retain(|element| regex.is_match(&element_text(*element)).unwrap_or(false));
            }
            Filter::MatchesOwn(regex) => {
                let regex = regex.as_ref().map_err(|error| error.clone())?;
                elements
                    .retain(|element| regex.is_match(&element_own_text(*element)).unwrap_or(false));
            }
            Filter::AttrMatches(name, regex) => {
                let regex = regex.as_ref().map_err(|error| error.clone())?;
                elements.retain(|element| {
                    regex
                        .is_match(element.attr(name).unwrap_or_default())
                        .unwrap_or(false)
                });
            }
            Filter::Has(selector) => {
                let mut keep = Vec::new();
                for element in elements.iter() {
                    if !selector.select_in(*element)?.is_empty() {
                        keep.push(*element);
                    }
                }
                *elements = keep;
            }
            Filter::Not(selectors) => {
                let mut keep = Vec::new();
                for element in elements.iter() {
                    let mut matched = false;
                    for selector in selectors {
                        if selector.matches_element(*element)? {
                            matched = true;
                            break;
                        }
                    }
                    if !matched {
                        keep.push(*element);
                    }
                }
                *elements = keep;
            }
            Filter::Invalid(message) => return Err(message.clone()),
        }
    }
    Ok(())
}

fn parse_standard(text: &str) -> Result<StandardSelector, String> {
    let text = text.trim();
    let text = if text.is_empty() { "*" } else { text };
    match StandardSelector::parse(text) {
        Ok(selector) => Ok(selector),
        Err(error) => {
            let mut candidates = Vec::new();
            let quoted = quote_attribute_values(text);
            if let Some(escaped) = escape_leading_digit_idents(&quoted) {
                candidates.push(escaped);
            }
            if quoted != text {
                candidates.push(quoted);
            }
            for candidate in candidates {
                if let Ok(selector) = StandardSelector::parse(&candidate) {
                    return Ok(selector);
                }
            }
            Err(format!("CSS 选择器解析失败：{error}"))
        }
    }
}

fn escape_leading_digit_idents(text: &str) -> Option<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len() + 8);
    let mut changed = false;
    let mut brackets = 0i32;
    let mut quote: Option<char> = None;
    let mut index = 0;

    while index < chars.len() {
        let ch = chars[index];
        if let Some(current) = quote {
            out.push(ch);
            if ch == '\\' {
                if let Some(&next) = chars.get(index + 1) {
                    out.push(next);
                    index += 2;
                    continue;
                }
            } else if ch == current {
                quote = None;
            }
            index += 1;
            continue;
        }
        match ch {
            '\\' => {
                out.push(ch);
                if let Some(&next) = chars.get(index + 1) {
                    out.push(next);
                }
                index += 2;
                continue;
            }
            '\'' | '"' => {
                quote = Some(ch);
                out.push(ch);
                index += 1;
                continue;
            }
            '[' => {
                brackets += 1;
                out.push(ch);
                index += 1;
                continue;
            }
            ']' => {
                brackets -= 1;
                out.push(ch);
                index += 1;
                continue;
            }
            _ => {}
        }
        if brackets == 0
            && (ch == '.' || ch == '#')
            && let Some(escaped) =
                escape_ident_start(chars.get(index + 1).copied(), chars.get(index + 2).copied())
        {
            out.push(ch);
            out.push_str(&escaped);
            changed = true;
            index += 2;
            continue;
        }
        out.push(ch);
        index += 1;
    }

    changed.then_some(out)
}

fn escape_ident_start(first: Option<char>, second: Option<char>) -> Option<String> {
    match first? {
        digit if digit.is_ascii_digit() => Some(format!("\\{:x} ", digit as u32)),
        '-' if second.is_some_and(|ch| ch.is_ascii_digit()) => Some("\\-".to_string()),
        _ => None,
    }
}

fn quote_attribute_values(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let single = char::from_u32(39).unwrap();
    let mut out = String::with_capacity(text.len() + 8);
    let mut i = 0;
    while i < chars.len() {
        if chars[i] != '[' {
            out.push(chars[i]);
            i += 1;
            continue;
        }
        let mut j = i + 1;
        let mut depth = 1;
        let mut quote: Option<char> = None;
        while j < chars.len() {
            let c = chars[j];
            match quote {
                Some(q) if c == q => quote = None,
                Some(_) => {}
                None if c == '"' || c == single => quote = Some(c),
                None if c == '[' => depth += 1,
                None if c == ']' => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                None => {}
            }
            j += 1;
        }
        if j >= chars.len() {
            out.extend(chars[i..].iter());
            break;
        }
        let inner: String = chars[i + 1..j].iter().collect();
        out.push('[');
        out.push_str(&quote_inner(&inner));
        out.push(']');
        i = j + 1;
    }
    out
}

fn quote_inner(inner: &str) -> String {
    let trimmed = inner.trim();
    let single = char::from_u32(39).unwrap();
    let chars: Vec<char> = trimmed.chars().collect();
    let mut idx = 0;
    while idx < chars.len() && !chars[idx].is_whitespace() && !"~|^$*=".contains(chars[idx]) {
        idx += 1;
    }
    let attr: String = chars[..idx].iter().collect();
    let rest: String = chars[idx..].iter().collect();
    let rest = rest.trim_start();
    let operators = ["~=", "|=", "^=", "$=", "*=", "="];
    let Some((op, after)) = operators
        .iter()
        .find_map(|op| rest.strip_prefix(op).map(|after| (*op, after)))
    else {
        return trimmed.to_string();
    };
    let value = after.trim();
    if value.is_empty() || value.starts_with('"') || value.starts_with(single) {
        return trimmed.to_string();
    }
    let needs_quote = value.chars().any(|ch| {
        ch.is_whitespace()
            || (ch.is_ascii() && !(ch.is_ascii_alphanumeric() || ch == '-' || ch == '_'))
    });
    if !needs_quote {
        return trimmed.to_string();
    }
    let mut out = String::with_capacity(trimmed.len() + 2);
    out.push_str(&attr);
    out.push_str(op);
    out.push('"');
    out.push_str(value);
    out.push('"');
    out
}

fn split_filters(text: &str) -> (String, Vec<Filter>) {
    const NAMES: [&str; 6] = ["eq", "lt", "gt", "contains", "containsown", "matches"];
    const NAMES_OWN: [&str; 2] = ["matchesown", "has"];

    let chars: Vec<char> = text.chars().collect();
    let mut standard = String::new();
    let mut filters = Vec::new();
    let mut balance = Balance::default();
    let mut index = 0;

    while index < chars.len() {
        let top = balance.step(chars[index]);
        if top && chars[index] == ':' {
            let name_start = index + 1;
            let mut name_end = name_start;
            while name_end < chars.len() && chars[name_end].is_ascii_alphabetic() {
                name_end += 1;
            }
            let name: String = chars[name_start..name_end].iter().collect();
            let lowered = name.to_lowercase();
            let known = NAMES.contains(&lowered.as_str()) || NAMES_OWN.contains(&lowered.as_str());
            if known
                && chars.get(name_end) == Some(&'(')
                && let Some(end) = find_paren(&chars, name_end)
            {
                let argument: String = chars[name_end + 1..end].iter().collect();
                filters.push(parse_filter(&lowered, &argument));
                index = end + 1;
                continue;
            }
            if lowered == "not"
                && chars.get(name_end) == Some(&'(')
                && let Some(end) = find_paren(&chars, name_end)
            {
                let argument: String = chars[name_end + 1..end].iter().collect();
                if has_jsoup_pseudo(&argument)
                    && let Some(selectors) = parse_not(&argument)
                {
                    filters.push(Filter::Not(selectors));
                    index = end + 1;
                    continue;
                }
            }
        }
        standard.push(chars[index]);
        index += 1;
    }
    (standard, filters)
}

const JSOUP_PSEUDO: [&str; 7] = [
    "eq",
    "lt",
    "gt",
    "contains",
    "containsown",
    "matches",
    "matchesown",
];

fn has_jsoup_pseudo(text: &str) -> bool {
    let chars: Vec<char> = text.chars().collect();
    let mut index = 0;
    while index < chars.len() {
        if chars[index] == ':' {
            let start = index + 1;
            let mut end = start;
            while end < chars.len() && chars[end].is_ascii_alphabetic() {
                end += 1;
            }
            let name: String = chars[start..end].iter().collect::<String>().to_lowercase();
            if chars.get(end) == Some(&'(') && JSOUP_PSEUDO.contains(&name.as_str()) {
                return true;
            }
        }
        index += 1;
    }
    false
}

fn parse_not(argument: &str) -> Option<Vec<CssSelector>> {
    if !has_jsoup_pseudo(argument) {
        return None;
    }
    let mut selectors = Vec::new();
    for item in split_top_level(argument, ',') {
        let item = item.trim();
        if item.is_empty() {
            return None;
        }
        let selector = CssSelector::parse_self(item);
        if selector.element_steps.len() != 1 {
            return None;
        }
        selectors.push(selector);
    }
    (!selectors.is_empty()).then_some(selectors)
}

fn find_paren(chars: &[char], open_at: usize) -> Option<usize> {
    let mut depth = 0;
    let mut quote: Option<char> = None;
    let mut escaped = false;
    for (index, &ch) in chars.iter().enumerate().skip(open_at) {
        if escaped {
            escaped = false;
            continue;
        }
        if let Some(current) = quote {
            if ch == '\\' {
                escaped = true;
            } else if ch == current {
                quote = None;
            }
            continue;
        }
        match ch {
            '\\' => escaped = true,
            '\'' | '"' => quote = Some(ch),
            '(' => depth += 1,
            ')' => {
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

fn extract_attr_regex(text: &str) -> (String, Vec<Filter>) {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut filters = Vec::new();
    let mut index = 0;
    while index < chars.len() {
        if chars[index] != '[' {
            out.push(chars[index]);
            index += 1;
            continue;
        }
        let Some(end) = find_bracket(&chars, index) else {
            out.extend(chars[index..].iter());
            break;
        };
        let inner: String = chars[index + 1..end].iter().collect();
        match split_attr_op(&inner) {
            Some((name, op, value)) if op == "~=" => {
                filters.push(Filter::AttrMatches(
                    name.trim().to_string(),
                    compile_regex(&unquote(&value)),
                ));
            }
            _ => out.extend(chars[index..=end].iter()),
        }
        index = end + 1;
    }
    (out, filters)
}

fn find_bracket(chars: &[char], open: usize) -> Option<usize> {
    let mut quote: Option<char> = None;
    let mut depth = 0i32;
    let mut index = open;
    while index < chars.len() {
        let ch = chars[index];
        match quote {
            Some(active) => {
                if ch == active {
                    quote = None;
                }
            }
            None => match ch {
                '"' | '\'' => quote = Some(ch),
                '[' => depth += 1,
                ']' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(index);
                    }
                }
                _ => {}
            },
        }
        index += 1;
    }
    None
}

fn split_attr_op(inner: &str) -> Option<(String, String, String)> {
    for op in ["~=", "*=", "^=", "$=", "|=", "="] {
        if let Some((name, value)) = inner.split_once(op) {
            return Some((name.to_string(), op.to_string(), value.to_string()));
        }
    }
    None
}

fn parse_filter(name: &str, argument: &str) -> Filter {
    let argument = argument.trim();
    match name {
        "eq" | "lt" | "gt" => match argument.parse::<i64>() {
            Ok(value) => match name {
                "eq" => Filter::Eq(value),
                "lt" => Filter::Lt(value),
                _ => Filter::Gt(value),
            },
            Err(_) => Filter::Invalid(format!(":{name}({argument}) 参数不是整数")),
        },
        "contains" => Filter::Contains(unquote(argument)),
        "containsown" => Filter::ContainsOwn(unquote(argument)),
        "matches" => Filter::Matches(compile_regex(&unquote(argument))),
        "matchesown" => Filter::MatchesOwn(compile_regex(&unquote(argument))),
        "has" => Filter::Has(Box::new(CssSelector::parse(argument, true))),
        _ => Filter::Invalid(format!("不认识的伪类 :{name}")),
    }
}

fn unquote(text: &str) -> String {
    let bytes = text.as_bytes();
    if text.len() >= 2
        && ((bytes[0] == b'"' && bytes[text.len() - 1] == b'"')
            || (bytes[0] == b'\'' && bytes[text.len() - 1] == b'\''))
    {
        text[1..text.len() - 1].to_string()
    } else {
        text.to_string()
    }
}

fn compile_regex(pattern: &str) -> Result<Regex, String> {
    Regex::new(pattern).map_err(|error| error.to_string())
}

pub(crate) fn normalised_text(text: &str) -> String {
    let mut accum = String::new();
    append_normalised_whitespace(&mut accum, text);
    accum
        .trim_matches(|c: char| c.is_ascii_whitespace())
        .to_string()
}

pub fn element_text(element: ElementRef<'_>) -> String {
    let mut accum = String::new();
    append_element_text(element, &mut accum);
    accum
        .trim_matches(|c: char| c.is_ascii_whitespace())
        .to_string()
}

fn append_node_text(child: NodeRef<'_, Node>, accum: &mut String) {
    match child.value() {
        Node::Text(text) => append_normalised_whitespace(accum, text),
        Node::Fragment => {
            for inner in child.children() {
                append_node_text(inner, accum);
            }
        }
        _ => {
            if let Some(child_element) = ElementRef::wrap(child) {
                append_element_text(child_element, accum);
            }
        }
    }
}

fn append_element_text(element: ElementRef<'_>, accum: &mut String) {
    let name = element.value().name();
    push_block_space(name, accum);
    if name == "script" || name == "style" {
        return;
    }
    for child in element.children() {
        append_node_text(child, accum);
    }
    if is_block_tag(name)
        && next_sibling_formats_inline(element)
        && !accum
            .chars()
            .next_back()
            .is_some_and(|c: char| c.is_ascii_whitespace())
    {
        accum.push(' ');
    }
}

fn push_block_space(name: &str, accum: &mut String) {
    if !accum.is_empty()
        && (is_block_tag(name) || name == "br")
        && !accum
            .chars()
            .next_back()
            .is_some_and(|c: char| c.is_ascii_whitespace())
    {
        accum.push(' ');
    }
}

fn is_invisible_char(ch: char) -> bool {
    ch == '\u{200b}' || ch == '\u{ad}'
}

fn append_normalised_whitespace(accum: &mut String, text: &str) {
    let strip_leading = accum
        .chars()
        .next_back()
        .is_some_and(is_actually_whitespace);
    let mut last_was_white = false;
    let mut reached_non_white = false;
    for ch in text.chars() {
        if is_actually_whitespace(ch) {
            if (strip_leading && !reached_non_white) || last_was_white {
                continue;
            }
            accum.push(' ');
            last_was_white = true;
        } else if !is_invisible_char(ch) {
            accum.push(ch);
            last_was_white = false;
            reached_non_white = true;
        }
    }
}

fn chain_matches(
    left: &CssStep,
    combinator: char,
    right: &CssStep,
    element: ElementRef<'_>,
) -> Result<bool, String> {
    if !right.matches_element(element)? {
        return Ok(false);
    }
    match combinator {
        '+' => match previous_element_sibling(element) {
            Some(sibling) => left.matches_element(sibling),
            None => Ok(false),
        },
        '~' => {
            for sibling in element.prev_siblings().filter_map(ElementRef::wrap) {
                if left.matches_element(sibling)? {
                    return Ok(true);
                }
            }
            Ok(false)
        }
        '>' => match element.parent().and_then(ElementRef::wrap) {
            Some(parent) => left.matches_element(parent),
            None => Ok(false),
        },
        _ => {
            for ancestor in element.ancestors().filter_map(ElementRef::wrap) {
                if left.matches_element(ancestor)? {
                    return Ok(true);
                }
            }
            Ok(false)
        }
    }
}

fn previous_element_sibling(element: ElementRef<'_>) -> Option<ElementRef<'_>> {
    element.prev_siblings().filter_map(ElementRef::wrap).next()
}

fn split_top_combinator(text: &str) -> Option<(String, char, String)> {
    let chars: Vec<char> = text.chars().collect();
    let mut balance = Balance::default();
    for (index, &ch) in chars.iter().enumerate() {
        let top = balance.step_at(&chars, index);
        if !top || !(ch == '+' || ch == '>' || ch == '~' || ch.is_whitespace()) {
            continue;
        }
        let left: String = chars[..index].iter().collect();
        let mut start = index + 1;
        while start < chars.len() && chars[start].is_whitespace() {
            start += 1;
        }
        let mut combinator = ch;
        if ch.is_whitespace()
            && let Some(&next) = chars.get(start)
            && matches!(next, '+' | '>' | '~')
        {
            combinator = next;
            start += 1;
            while start < chars.len() && chars[start].is_whitespace() {
                start += 1;
            }
        }
        let right: String = chars[start..].iter().collect();
        let (left, right) = (left.trim().to_string(), right.trim().to_string());
        if !left.is_empty() && !right.is_empty() {
            return Some((left, combinator, right));
        }
    }
    None
}

fn split_first_compound(text: &str) -> (String, String) {
    let chars: Vec<char> = text.chars().collect();
    let mut balance = Balance::default();
    for (index, &ch) in chars.iter().enumerate() {
        let top = balance.step_at(&chars, index);
        if !top || !(ch == '+' || ch == '>' || ch == '~' || ch.is_whitespace()) {
            continue;
        }
        let first: String = chars[..index].iter().collect();
        let mut start = index;
        while start < chars.len() && chars[start].is_whitespace() {
            start += 1;
        }
        if let Some(&next) = chars.get(start)
            && matches!(next, '+' | '>' | '~')
        {
            start += 1;
        }
        let rest: String = chars[start..].iter().collect();
        let (first, rest) = (first.trim().to_string(), rest.trim().to_string());
        if !first.is_empty() && !rest.is_empty() {
            return (first, rest);
        }
    }
    (text.trim().to_string(), String::new())
}

fn jsoup_has_class(element: ElementRef<'_>, name: &str) -> bool {
    let Some(class_attr) = element.value().attr("class") else {
        return false;
    };
    if class_attr.is_empty() || class_attr.len() < name.len() {
        return false;
    }
    let wanted = name.to_lowercase();
    if class_attr.len() == name.len() {
        return class_attr.to_lowercase() == wanted;
    }
    class_attr
        .split_whitespace()
        .any(|token| token.to_lowercase() == wanted)
}

fn is_actually_whitespace(ch: char) -> bool {
    ch.is_ascii_whitespace() || ch == '\u{a0}'
}

fn next_sibling_formats_inline(element: ElementRef<'_>) -> bool {
    match element.next_sibling().as_ref().map(|node| node.value()) {
        Some(Node::Text(_)) => true,
        Some(Node::Element(next)) => formats_as_inline(next.name()),
        _ => false,
    }
}

fn fragment_children<'a>(
    children: impl Iterator<Item = NodeRef<'a, Node>>,
) -> impl Iterator<Item = NodeRef<'a, Node>> {
    children.flat_map(|child| {
        if matches!(child.value(), Node::Fragment) {
            child.children().collect::<Vec<_>>()
        } else {
            vec![child]
        }
    })
}

pub fn element_own_text(element: ElementRef<'_>) -> String {
    let mut accum = String::new();
    for child in fragment_children(element.children()) {
        match child.value() {
            Node::Text(text) => append_normalised_whitespace(&mut accum, text),
            _ => {
                if let Some(child_element) = ElementRef::wrap(child)
                    && child_element.value().name() == "br"
                    && !accum
                        .chars()
                        .next_back()
                        .is_some_and(|c: char| c.is_ascii_whitespace())
                {
                    accum.push(' ');
                }
            }
        }
    }
    accum
        .trim_matches(|c: char| c.is_ascii_whitespace())
        .to_string()
}

pub fn own_text_nodes(element: ElementRef<'_>) -> Vec<String> {
    fragment_children(element.children())
        .filter_map(|node| match node.value() {
            Node::Text(text) => {
                let text = normalised_text(text);
                (!text.is_empty()).then_some(text)
            }
            _ => None,
        })
        .collect()
}

pub(crate) fn escape_nbsp(html: &str) -> String {
    if html.contains('\u{a0}') {
        html.replace('\u{a0}', "&nbsp;")
    } else {
        html.to_string()
    }
}

pub fn collapse_whitespace(text: &str) -> String {
    let mut accum = String::new();
    append_normalised_whitespace(&mut accum, text);
    accum
        .trim_matches(|c: char| c.is_ascii_whitespace())
        .to_string()
}

fn elements_html(elements: &[ElementRef<'_>], strip_script_style: bool) -> String {
    crate::selector::pretty::elements_outer_html(elements, strip_script_style).join("\n")
}

pub(crate) use crate::selector::pretty::formats_as_inline;

fn is_block_tag(name: &str) -> bool {
    crate::selector::pretty::is_block(name)
}
