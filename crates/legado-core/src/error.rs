use std::fmt;
use std::ops::Range;

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Rule(#[from] RuleError),
    #[error("JSON 解析失败: {0}")]
    Json(#[from] serde_json::Error),
    #[error("IO 错误: {0}")]
    Io(#[from] std::io::Error),
    #[error("HTTP 请求失败: {0}")]
    Http(String),
    #[error("JS 执行失败: {0}")]
    Js(String),
    #[error("缓存读写失败: {0}")]
    Cache(String),
    #[error("{0}")]
    Other(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Position {
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleError {
    pub rule: String,
    pub span: Range<usize>,
    pub message: String,
}

impl RuleError {
    pub fn new(rule: impl Into<String>, span: Range<usize>, message: impl Into<String>) -> Self {
        Self {
            rule: rule.into(),
            span,
            message: message.into(),
        }
    }

    pub fn at(rule: impl Into<String>, offset: usize, message: impl Into<String>) -> Self {
        Self::new(rule, offset..offset, message)
    }

    pub fn position(&self) -> Position {
        let (mut line, mut column) = (1, 1);
        for ch in self.rule.chars().take(self.span.start) {
            if ch == '\n' {
                line += 1;
                column = 1;
            } else {
                column += 1;
            }
        }
        Position { line, column }
    }
}

impl fmt::Display for RuleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Position { line, column } = self.position();
        write!(f, "规则第 {line} 行第 {column} 列: {}", self.message)?;
        if let Some(text) = self.rule.lines().nth(line - 1) {
            write!(f, "\n  {text}\n  {}^", " ".repeat(column - 1))?;
        }
        Ok(())
    }
}

impl std::error::Error for RuleError {}
