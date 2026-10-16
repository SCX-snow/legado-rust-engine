use legado_core::{Error, Result};
use serde_json::{Map, Number, Value};

pub fn parse(text: &str) -> Result<Value> {
    let mut parser = Parser::new(text);
    parser.skip_ws();
    let value = parser.parse_value()?;
    parser.skip_ws();
    if parser.peek().is_some() {
        return Err(parser.error("JSON 结束后还有多余内容"));
    }
    Ok(value)
}

struct Parser {
    chars: Vec<char>,
    pos: usize,
}

fn is_terminator(ch: char) -> bool {
    ch.is_whitespace() || matches!(ch, ',' | ':' | '{' | '}' | '[' | ']' | ';' | '#')
}

impl Parser {
    fn new(text: &str) -> Self {
        Self {
            chars: text.chars().collect(),
            pos: 0,
        }
    }

    fn error(&self, message: impl Into<String>) -> Error {
        Error::Other(format!(
            "宽松 JSON 解析失败（第 {} 个字符）：{}",
            self.pos + 1,
            message.into()
        ))
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn next(&mut self) -> Option<char> {
        let ch = self.peek();
        if ch.is_some() {
            self.pos += 1;
        }
        ch
    }

    fn skip_ws(&mut self) {
        while matches!(self.peek(), Some(ch) if ch.is_whitespace() || ch == '\u{feff}') {
            self.pos += 1;
        }
    }

    fn parse_value(&mut self) -> Result<Value> {
        self.skip_ws();
        match self.peek() {
            None => Err(self.error("内容为空")),
            Some('{') => self.parse_object(),
            Some('[') => self.parse_array(),
            Some('"' | '\'') => Ok(Value::String(self.parse_string()?)),
            _ => self.parse_bare(),
        }
    }

    fn parse_object(&mut self) -> Result<Value> {
        self.next();
        let mut map = Map::new();
        loop {
            self.skip_ws();
            match self.peek() {
                Some('}') => {
                    self.next();
                    return Ok(Value::Object(map));
                }
                None => return Err(self.error("对象没有闭合")),
                _ => {}
            }
            let key = match self.peek() {
                Some('"' | '\'') => self.parse_string()?,
                _ => self.parse_bare_text()?,
            };
            self.skip_ws();
            if self.next() != Some(':') {
                return Err(self.error(format!("键 `{key}` 后面缺少 `:`")));
            }
            let value = self.parse_value()?;
            map.insert(key, value);
            self.skip_ws();
            match self.next() {
                Some(',') => continue,
                Some('}') => return Ok(Value::Object(map)),
                _ => return Err(self.error("对象里缺少 `,` 或 `}`")),
            }
        }
    }

    fn parse_array(&mut self) -> Result<Value> {
        self.next();
        let mut items = Vec::new();
        loop {
            self.skip_ws();
            match self.peek() {
                Some(']') => {
                    self.next();
                    return Ok(Value::Array(items));
                }
                None => return Err(self.error("数组没有闭合")),
                _ => {}
            }
            items.push(self.parse_value()?);
            self.skip_ws();
            match self.next() {
                Some(',') => continue,
                Some(']') => return Ok(Value::Array(items)),
                _ => return Err(self.error("数组里缺少 `,` 或 `]`")),
            }
        }
    }

    fn parse_string(&mut self) -> Result<String> {
        let quote = self.next().expect("调用前已确认是引号");
        let mut out = String::new();
        loop {
            let Some(ch) = self.next() else {
                return Err(self.error("字符串没有闭合"));
            };
            if ch == quote {
                return Ok(out);
            }
            if ch != '\\' {
                out.push(ch);
                continue;
            }
            let Some(escape) = self.next() else {
                return Err(self.error("字符串以 `\\` 结尾"));
            };
            match escape {
                'b' => out.push('\u{8}'),
                'f' => out.push('\u{c}'),
                'n' => out.push('\n'),
                'r' => out.push('\r'),
                't' => out.push('\t'),
                'u' => out.push(self.parse_unicode_escape()?),
                other => out.push(other),
            }
        }
    }

    fn parse_unicode_escape(&mut self) -> Result<char> {
        let high = self.parse_hex4()?;
        if (0xD800..0xDC00).contains(&high) {
            let checkpoint = self.pos;
            if self.next() == Some('\\') && self.next() == Some('u') {
                let low = self.parse_hex4()?;
                if (0xDC00..0xE000).contains(&low) {
                    let code = 0x10000 + ((high - 0xD800) << 10) + (low - 0xDC00);
                    return char::from_u32(code).ok_or_else(|| self.error("代理对越界"));
                }
            }
            self.pos = checkpoint;
        }
        char::from_u32(high).ok_or_else(|| self.error("`\\u` 转义不是合法字符"))
    }

    fn parse_hex4(&mut self) -> Result<u32> {
        let mut code = 0;
        for _ in 0..4 {
            let Some(ch) = self.next() else {
                return Err(self.error("`\\u` 转义少了 4 位十六进制"));
            };
            let Some(digit) = ch.to_digit(16) else {
                return Err(self.error("`\\u` 转义不是十六进制"));
            };
            code = code * 16 + digit;
        }
        Ok(code)
    }

    fn parse_bare(&mut self) -> Result<Value> {
        let text = self.parse_bare_text()?;
        match text.as_str() {
            "true" => Ok(Value::Bool(true)),
            "false" => Ok(Value::Bool(false)),
            "null" => Ok(Value::Null),
            _ => match text.parse::<Number>() {
                Ok(number) => Ok(Value::Number(number)),
                Err(_) => Ok(Value::String(text)),
            },
        }
    }

    fn parse_bare_text(&mut self) -> Result<String> {
        let start = self.pos;
        while matches!(self.peek(), Some(ch) if !is_terminator(ch)) {
            self.pos += 1;
        }
        if self.pos == start {
            return Err(self.error("缺少值"));
        }
        Ok(self.chars[start..self.pos].iter().collect())
    }
}
