use std::collections::HashMap;

use bytes::Bytes;
use http::{HeaderMap, Method, StatusCode};

use crate::model::{Book, BookChapter, Source};
use crate::{BoxFuture, LocalBoxFuture, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JsValue {
    Text(String),
    List(Vec<String>),
    Null,
    Other(String),
}

impl JsValue {
    pub fn into_text(self) -> String {
        match self {
            JsValue::Text(text) => text,
            JsValue::List(items) => java_list_to_string(&items),
            JsValue::Null => String::new(),
            JsValue::Other(text) => text,
        }
    }
}

pub fn java_list_to_string(items: &[String]) -> String {
    format!("[{}]", items.join(", "))
}

pub trait JsEvaluator: Send + Sync {
    fn eval<'a>(&'a self, request: JsRequest<'a>) -> BoxFuture<'a, Result<String>>;

    fn eval_value<'a>(
        &'a self,
        request: JsRequest<'a>,
        result_items: Option<&'a [String]>,
    ) -> BoxFuture<'a, Result<JsValue>> {
        let _ = result_items;
        Box::pin(async move { self.eval(request).await.map(JsValue::Text) })
    }

    fn eval_response<'a>(&'a self, request: JsRequest<'a>) -> BoxFuture<'a, Result<HttpResponse>> {
        let _ = request;
        Box::pin(async move {
            Err(crate::Error::Js(
                "这个 JsEvaluator 不支持响应对象（loginCheckJs 要 legado-js）".into(),
            ))
        })
    }
}

pub trait RuleRunner: Send + Sync {
    fn get_string<'a>(&'a self, request: RuleRequest<'a>) -> LocalBoxFuture<'a, Result<String>>;
    fn get_elements<'a>(
        &'a self,
        request: RuleRequest<'a>,
    ) -> LocalBoxFuture<'a, Result<Vec<String>>>;

    fn get_string_list<'a>(
        &'a self,
        request: RuleRequest<'a>,
    ) -> LocalBoxFuture<'a, Result<Vec<String>>> {
        Box::pin(async move {
            let text = self.get_string(request).await?;
            Ok(if text.is_empty() {
                Vec::new()
            } else {
                text.split('\n').map(str::to_string).collect()
            })
        })
    }
}

#[derive(Debug)]
pub struct RuleRequest<'a> {
    pub rule: &'a str,
    pub content: &'a str,
    pub base_url: &'a str,
    pub source: Option<&'a dyn Source>,
    pub book: Option<&'a Book>,
    pub chapter: Option<&'a BookChapter>,
    pub page: Option<i32>,
    pub key: Option<&'a str>,
    pub variables: Option<&'a mut JsVariables>,
    pub unescape: bool,
}

#[derive(Debug)]
pub struct JsRequest<'a> {
    pub script: &'a str,
    pub source: Option<&'a dyn Source>,
    pub base_url: &'a str,
    pub page: Option<i32>,
    pub key: Option<&'a str>,
    pub result: Option<&'a str>,
    pub src: Option<&'a str>,
    pub response: Option<&'a HttpResponse>,
    pub book: Option<&'a Book>,
    pub chapter: Option<&'a BookChapter>,
    pub variables: Option<&'a mut JsVariables>,
}

#[derive(Debug, Clone, Default)]
pub struct JsVariables {
    chapter: Option<HashMap<String, String>>,
    book: Option<HashMap<String, String>>,
    source: Option<HashMap<String, String>>,
}

impl JsVariables {
    pub fn new(
        chapter: Option<HashMap<String, String>>,
        book: Option<HashMap<String, String>>,
        source: Option<HashMap<String, String>>,
    ) -> Self {
        Self {
            chapter,
            book,
            source,
        }
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        [
            self.chapter.as_ref(),
            self.book.as_ref(),
            self.source.as_ref(),
        ]
        .into_iter()
        .flatten()
        .find_map(|side| side.get(key).filter(|value| !value.is_empty()))
        .map(String::as_str)
    }

    pub fn put(&mut self, key: &str, value: Option<&str>) {
        let Some(target) = self
            .chapter
            .as_mut()
            .or(self.book.as_mut())
            .or(self.source.as_mut())
        else {
            return;
        };
        match value {
            Some(value) => {
                target.insert(key.to_string(), value.to_string());
            }
            None => {
                target.remove(key);
            }
        }
    }

    pub fn put_chapter(&mut self, key: &str, value: Option<&str>) {
        let target = self.chapter.get_or_insert_with(HashMap::new);
        match value {
            Some(value) => {
                target.insert(key.to_string(), value.to_string());
            }
            None => {
                target.remove(key);
            }
        }
    }

    pub fn chapter(&self) -> Option<&HashMap<String, String>> {
        self.chapter.as_ref()
    }

    pub fn book(&self) -> Option<&HashMap<String, String>> {
        self.book.as_ref()
    }

    pub fn source(&self) -> Option<&HashMap<String, String>> {
        self.source.as_ref()
    }
}

pub trait HttpFetcher: Send + Sync {
    fn fetch(&self, request: HttpRequest) -> BoxFuture<'_, Result<HttpResponse>>;

    fn fetch_limited(
        &self,
        request: HttpRequest,
        rate: Option<String>,
        rate_key: String,
    ) -> BoxFuture<'_, Result<HttpResponse>> {
        let _ = (rate, rate_key);
        self.fetch(request)
    }
}

#[derive(Debug, Clone)]
pub struct HttpRequest {
    pub url: String,
    pub method: Method,
    pub headers: HeaderMap,
    pub body: Option<Bytes>,
    pub charset: Option<String>,
}

impl HttpRequest {
    pub fn get(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            method: Method::GET,
            headers: HeaderMap::new(),
            body: None,
            charset: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct HttpResponse {
    pub url: String,
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub body: Bytes,
    pub text: String,
}

pub trait CookieStore: Send + Sync {
    fn get_cookie(&self, url: &str) -> String;
    fn set_cookie(&self, url: &str, cookie: &str);
    fn replace_cookie(&self, url: &str, cookie: &str);
    fn remove_cookie(&self, url: &str);
}
