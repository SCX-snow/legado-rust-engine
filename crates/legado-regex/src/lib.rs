#![allow(non_camel_case_types)]

use std::ffi::{CStr, c_char, c_void};
use std::ptr::NonNull;
use std::sync::Mutex;

#[repr(C)]
struct URegularExpression {
    _private: [u8; 0],
}

type UErrorCode = i32;
const U_ZERO_ERROR: UErrorCode = 0;

unsafe extern "C" {
    fn uregex_open(
        pattern: *const u16,
        length: i32,
        flags: u32,
        pe: *mut c_void,
        status: *mut UErrorCode,
    ) -> *mut URegularExpression;
    fn uregex_close(regexp: *mut URegularExpression);
    fn uregex_clone(
        regexp: *const URegularExpression,
        status: *mut UErrorCode,
    ) -> *mut URegularExpression;
    fn uregex_setText(
        regexp: *mut URegularExpression,
        text: *const u16,
        len: i32,
        status: *mut UErrorCode,
    );
    fn uregex_findNext(regexp: *mut URegularExpression, status: *mut UErrorCode) -> i8;
    fn uregex_start(regexp: *mut URegularExpression, group: i32, status: *mut UErrorCode) -> i32;
    fn uregex_end(regexp: *mut URegularExpression, group: i32, status: *mut UErrorCode) -> i32;
    fn uregex_group(
        regexp: *mut URegularExpression,
        group: i32,
        dest: *mut u16,
        capacity: i32,
        status: *mut UErrorCode,
    ) -> i32;
    fn uregex_groupCount(regexp: *mut URegularExpression, status: *mut UErrorCode) -> i32;
    fn uregex_setTimeLimit(regexp: *mut URegularExpression, millis: i32, status: *mut UErrorCode);
    fn u_errorName(code: UErrorCode) -> *const c_char;
}

fn error_name(code: UErrorCode) -> String {
    if code == U_ZERO_ERROR {
        return "U_ZERO_ERROR".into();
    }
    let p = unsafe { u_errorName(code) };
    if p.is_null() {
        format!("U_ERROR_{code}")
    } else {
        unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
    }
}

#[derive(Debug, Clone)]
pub enum Error {
    Syntax(String),
    Runtime(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Syntax(m) => write!(f, "正则编译失败：{m}"),
            Error::Runtime(m) => write!(f, "正则求值失败：{m}"),
        }
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

struct Owned(NonNull<URegularExpression>);

unsafe impl Send for Owned {}

impl Drop for Owned {
    fn drop(&mut self) {
        unsafe { uregex_close(self.0.as_ptr()) }
    }
}

struct Matcher(NonNull<URegularExpression>);

impl Drop for Matcher {
    fn drop(&mut self) {
        unsafe { uregex_close(self.0.as_ptr()) }
    }
}

impl Matcher {
    fn set_text(&mut self, text: &[u16]) -> Result<()> {
        let mut st = U_ZERO_ERROR;
        unsafe { uregex_setText(self.0.as_ptr(), text.as_ptr(), text.len() as i32, &mut st) };
        if st != U_ZERO_ERROR {
            return Err(Error::Runtime(error_name(st)));
        }
        Ok(())
    }

    fn find_next(&mut self) -> Result<bool> {
        let mut st = U_ZERO_ERROR;
        let ok = unsafe { uregex_findNext(self.0.as_ptr(), &mut st) };
        if st != U_ZERO_ERROR {
            return Err(Error::Runtime(error_name(st)));
        }
        Ok(ok != 0)
    }

    fn group_count(&self) -> usize {
        let mut st = U_ZERO_ERROR;
        let n = unsafe { uregex_groupCount(self.0.as_ptr(), &mut st) };
        if st != U_ZERO_ERROR || n < 0 {
            0
        } else {
            n as usize
        }
    }

    fn start(&self, group: i32) -> i32 {
        let mut st = U_ZERO_ERROR;
        let v = unsafe { uregex_start(self.0.as_ptr(), group, &mut st) };
        if st != U_ZERO_ERROR { -1 } else { v }
    }

    fn end(&self, group: i32) -> i32 {
        let mut st = U_ZERO_ERROR;
        let v = unsafe { uregex_end(self.0.as_ptr(), group, &mut st) };
        if st != U_ZERO_ERROR { -1 } else { v }
    }

    fn group(&self, group: i32) -> Option<String> {
        let start = self.start(group);
        if start < 0 {
            return None;
        }
        let len = self.end(group) - start;
        if len < 0 {
            return None;
        }
        let mut buf = vec![0u16; len as usize + 1];
        let mut st = U_ZERO_ERROR;
        let n = unsafe {
            uregex_group(
                self.0.as_ptr(),
                group,
                buf.as_mut_ptr(),
                buf.len() as i32,
                &mut st,
            )
        };
        if st != U_ZERO_ERROR || n < 0 {
            return None;
        }
        buf.truncate(n as usize);
        Some(String::from_utf16_lossy(&buf))
    }
}

#[derive(Debug, Clone)]
pub struct Captures {
    groups: Vec<Option<String>>,
    start16: usize,
    end16: usize,
}

impl Captures {
    pub fn len(&self) -> usize {
        self.groups.len()
    }

    pub fn is_empty(&self) -> bool {
        self.groups.is_empty()
    }

    pub fn get(&self, i: usize) -> Option<&str> {
        self.groups.get(i).and_then(|g| g.as_deref())
    }

    pub fn byte_span(&self, text: &str) -> (usize, usize) {
        (
            utf16_to_byte(text, self.start16),
            utf16_to_byte(text, self.end16),
        )
    }
}

fn utf16_to_byte(text: &str, index16: usize) -> usize {
    let mut u16_pos = 0usize;
    for (byte_pos, ch) in text.char_indices() {
        if u16_pos >= index16 {
            return byte_pos;
        }
        u16_pos += ch.len_utf16();
    }
    text.len()
}

pub struct Regex {
    pattern: String,
    flags: u32,
    mastery: Mutex<Owned>,
    time_limit_ms: i32,
}

impl Clone for Regex {
    fn clone(&self) -> Self {
        let guard = self.mastery.lock().unwrap_or_else(|e| e.into_inner());
        let mut st = U_ZERO_ERROR;
        let raw = unsafe { uregex_clone(guard.0.as_ptr(), &mut st) };
        match NonNull::new(raw) {
            Some(cloned) => Self {
                pattern: self.pattern.clone(),
                flags: self.flags,
                mastery: Mutex::new(Owned(cloned)),
                time_limit_ms: self.time_limit_ms,
            },
            None => Self::with_flags(&self.pattern, self.flags)
                .expect("已编译过的正则克隆失败")
                .with_time_limit(self.time_limit_ms.max(0) as u64),
        }
    }
}

impl std::fmt::Debug for Regex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Regex({:?})", self.pattern)
    }
}

impl Regex {
    pub fn new(pattern: &str) -> Result<Self> {
        Self::with_flags(pattern, 0)
    }

    pub fn with_flags(pattern: &str, flags: u32) -> Result<Self> {
        let units: Vec<u16> = pattern.encode_utf16().collect();
        let mut st = U_ZERO_ERROR;
        let raw = unsafe {
            uregex_open(
                units.as_ptr(),
                units.len() as i32,
                flags,
                std::ptr::null_mut(),
                &mut st,
            )
        };
        let Some(owned) = NonNull::new(raw) else {
            return Err(Error::Syntax(error_name(st)));
        };
        Ok(Self {
            pattern: pattern.to_string(),
            flags,
            mastery: Mutex::new(Owned(owned)),
            time_limit_ms: 0,
        })
    }

    pub fn with_time_limit(mut self, millis: u64) -> Self {
        self.time_limit_ms = millis.min(i32::MAX as u64) as i32;
        self
    }

    pub fn as_str(&self) -> &str {
        &self.pattern
    }

    fn matcher(&self) -> Result<Matcher> {
        let guard = self.mastery.lock().unwrap_or_else(|e| e.into_inner());
        let mut st = U_ZERO_ERROR;
        let raw = unsafe { uregex_clone(guard.0.as_ptr(), &mut st) };
        let Some(clone) = NonNull::new(raw) else {
            return Err(Error::Runtime(error_name(st)));
        };
        let matcher = Matcher(clone);
        if self.time_limit_ms > 0 {
            let mut st = U_ZERO_ERROR;
            unsafe { uregex_setTimeLimit(matcher.0.as_ptr(), self.time_limit_ms, &mut st) };
        }
        Ok(matcher)
    }

    fn with_text<T>(
        &self,
        text: &str,
        f: impl FnOnce(&mut Matcher, &[u16]) -> Result<T>,
    ) -> Result<T> {
        let units: Vec<u16> = text.encode_utf16().collect();
        let mut m = self.matcher()?;
        m.set_text(&units)?;
        f(&mut m, &units)
    }

    pub fn is_match(&self, text: &str) -> Result<bool> {
        self.with_text(text, |m, _| m.find_next())
    }

    pub fn find(&self, text: &str) -> Result<Option<(usize, usize)>> {
        let caps = self.captures(text)?;
        Ok(caps.map(|c| c.byte_span(text)))
    }

    pub fn captures(&self, text: &str) -> Result<Option<Captures>> {
        self.with_text(text, |m, _| {
            if !m.find_next()? {
                return Ok(None);
            }
            Ok(Some(collect_captures(m)))
        })
    }

    pub fn captures_iter(&self, text: &str) -> Result<Vec<Captures>> {
        self.with_text(text, |m, _| {
            let mut out = Vec::new();
            while m.find_next()? {
                out.push(collect_captures(m));
            }
            Ok(out)
        })
    }

    pub fn replace(&self, text: &str, replacement: &str) -> Result<String> {
        self.replacen(text, 1, replacement)
    }

    pub fn replace_all(&self, text: &str, replacement: &str) -> Result<String> {
        self.replacen(text, 0, replacement)
    }

    pub fn replacen(&self, text: &str, limit: usize, replacement: &str) -> Result<String> {
        self.replace_with(text, limit, |caps| {
            Ok(expand_replacement(replacement, caps))
        })
    }

    pub fn replace_with<F>(&self, text: &str, limit: usize, mut replacement: F) -> Result<String>
    where
        F: FnMut(&Captures) -> Result<String>,
    {
        let units: Vec<u16> = text.encode_utf16().collect();
        let mut m = self.matcher()?;
        m.set_text(&units)?;
        let mut out: Vec<u16> = Vec::with_capacity(units.len());
        let mut last = 0usize;
        let mut done = 0usize;
        while m.find_next()? {
            if limit != 0 && done >= limit {
                break;
            }
            let caps = collect_captures(&m);
            out.extend_from_slice(&units[last..caps.start16]);
            let piece = replacement(&caps)?;
            out.extend(piece.encode_utf16());
            last = caps.end16;
            done += 1;
        }
        out.extend_from_slice(&units[last..]);
        Ok(String::from_utf16_lossy(&out))
    }
}

fn collect_captures(m: &Matcher) -> Captures {
    let count = m.group_count() + 1;
    let mut groups = Vec::with_capacity(count);
    for i in 0..count {
        groups.push(m.group(i as i32));
    }
    Captures {
        groups,
        start16: m.start(0).max(0) as usize,
        end16: m.end(0).max(0) as usize,
    }
}

fn expand_replacement(replacement: &str, caps: &Captures) -> String {
    let chars: Vec<char> = replacement.chars().collect();
    let mut out = String::with_capacity(replacement.len());
    let mut i = 0usize;
    while i < chars.len() {
        let c = chars[i];
        i += 1;
        match c {
            '\\' => {
                if i < chars.len() {
                    out.push(chars[i]);
                    i += 1;
                } else {
                    out.push('\\');
                }
            }
            '$' => {
                if i < chars.len() && chars[i] == '{' {
                    let mut j = i + 1;
                    while j < chars.len() && chars[j] != '}' {
                        j += 1;
                    }
                    let name: String = chars[i + 1..j].iter().collect();
                    if let Ok(idx) = name.parse::<usize>() {
                        out.push_str(caps.get(idx).unwrap_or("null"));
                    }
                    i = (j + 1).min(chars.len());
                } else {
                    let mut idx: Option<usize> = None;
                    while i < chars.len() && chars[i].is_ascii_digit() {
                        let d = chars[i] as usize - '0' as usize;
                        idx = Some(idx.unwrap_or(0) * 10 + d);
                        i += 1;
                    }
                    match idx {
                        Some(n) if n < caps.len() => out.push_str(caps.get(n).unwrap_or("null")),
                        Some(n) => {
                            out.push('$');
                            out.push_str(&n.to_string());
                        }
                        None => out.push('$'),
                    }
                }
            }
            other => out.push(other),
        }
    }
    out
}
