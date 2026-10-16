pub mod css;
pub mod index;
pub mod json;
pub mod xpath;

use legado_regex::Regex;

use crate::content::{Content, Value};
use crate::selector::css::{CssSelector, RuleOutput};
pub(crate) mod pretty;

use crate::selector::json::JsonSelector;

#[derive(Debug, Clone)]
pub enum Selector {
    Css(CssSelector),
    XPath {
        text: String,
        error: Option<String>,
    },
    Json(JsonSelector),
    Regex(RegexPipeline),
    Auto {
        json: JsonSelector,
        other: Box<Selector>,
    },
}

impl Selector {
    pub fn eval(&self, content: &Content, output: RuleOutput) -> Result<Value, String> {
        match self {
            Self::Css(css) => css.eval(content, output),
            Self::XPath { text, error } => match error {
                Some(_) => Ok(Value::None),
                None => xpath::eval(content, text, output),
            },
            Self::Json(json) => json.eval(content, output),
            Self::Regex(pipeline) => pipeline.eval(content),
            Self::Auto { json, other } => {
                if content.looks_like_json() {
                    json.eval(content, output)
                } else {
                    other.eval(content, output)
                }
            }
        }
    }
}

pub type DeferredRegex = Result<Regex, String>;

#[derive(Debug, Clone)]
pub struct RegexPipeline {
    stages: Vec<DeferredRegex>,
}

impl RegexPipeline {
    pub fn parse(text: &str) -> Self {
        Self {
            stages: text
                .split("&&")
                .map(str::trim)
                .filter(|stage| !stage.is_empty())
                .map(compile)
                .collect(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.stages.is_empty()
    }

    fn eval(&self, content: &Content) -> Result<Value, String> {
        let mut text = content.to_source();
        for (index, stage) in self.stages.iter().enumerate() {
            let regex = stage.as_ref().map_err(|error| error.clone())?;
            if index + 1 == self.stages.len() {
                return collect(regex, &text);
            }
            let mut joined = String::new();
            for caps in regex
                .captures_iter(&text)
                .map_err(|error| error.to_string())?
            {
                joined.push_str(caps.get(0).unwrap_or_default());
            }
            if joined.is_empty() {
                return Ok(Value::None);
            }
            text = joined;
        }
        Ok(Value::None)
    }
}

fn collect(regex: &Regex, text: &str) -> Result<Value, String> {
    let mut rows: Vec<Vec<String>> = Vec::new();
    for captures in regex
        .captures_iter(text)
        .map_err(|error| error.to_string())?
    {
        let row = (0..captures.len())
            .map(|group| captures.get(group).unwrap_or_default().to_string())
            .collect();
        rows.push(row);
    }
    Ok(if rows.is_empty() {
        Value::None
    } else {
        Value::Rows(rows)
    })
}

pub fn compile(pattern: &str) -> DeferredRegex {
    Regex::new(pattern).map_err(|error| error.to_string())
}
