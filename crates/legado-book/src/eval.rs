use legado_core::{Book, BookChapter, BookSource, HttpResponse, JsEvaluator, Result, Source};
use legado_net::{UrlContext, UrlRule, absolute_url, check_login};
use legado_rule::{Content, EvalContext, RuleEngine, Value};
use serde_json::Value as Json;

use crate::fetch::PageFetcher;

pub struct PageEval<'a, S: Source> {
    pub rules: &'a RuleEngine,
    pub js: &'a dyn JsEvaluator,
    pub source: &'a S,
    pub fetcher: &'a dyn PageFetcher,
    pub base_url: String,
    pub redirect_url: String,
    pub page: Option<i32>,
    pub key: Option<String>,
}

pub(crate) type Eval<'a> = PageEval<'a, BookSource>;

impl<'a, S: Source> PageEval<'a, S> {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        rules: &'a RuleEngine,
        js: &'a dyn JsEvaluator,
        source: &'a S,
        fetcher: &'a dyn PageFetcher,
        base_url: impl Into<String>,
        redirect_url: impl Into<String>,
        page: Option<i32>,
        key: Option<String>,
    ) -> Self {
        Self {
            rules,
            js,
            source,
            fetcher,
            base_url: base_url.into(),
            redirect_url: redirect_url.into(),
            page,
            key,
        }
    }

    pub fn with_urls(&self, base_url: impl Into<String>, redirect_url: impl Into<String>) -> Self {
        Self {
            rules: self.rules,
            js: self.js,
            source: self.source,
            fetcher: self.fetcher,
            base_url: base_url.into(),
            redirect_url: redirect_url.into(),
            page: self.page,
            key: self.key.clone(),
        }
    }

    pub async fn fetch_page(
        &self,
        rule: &str,
        base_url: &str,
        key: Option<&str>,
        page: Option<i32>,
    ) -> Result<HttpResponse> {
        fetch_page(
            self.js,
            self.source,
            self.fetcher,
            rule,
            base_url,
            key,
            page,
        )
        .await
    }

    fn make_ctx<'c>(
        &self,
        content: Content,
        book: Option<&'c mut Book>,
        chapter: Option<&'c mut BookChapter>,
    ) -> EvalContext<'c>
    where
        'a: 'c,
    {
        let mut ctx = EvalContext::new(content)
            .with_base_url(self.base_url.as_str())
            .with_source(self.source)
            .with_js(self.js);
        if let Some(page) = self.page {
            ctx = ctx.with_page(page);
        }
        if let Some(key) = &self.key {
            ctx = ctx.with_key(key.clone());
        }
        if let Some(book) = book {
            ctx = ctx.with_book(book);
        }
        if let Some(chapter) = chapter {
            ctx = ctx.with_chapter(chapter);
        }
        ctx
    }

    pub async fn string(
        &self,
        rule: Option<&str>,
        content: &Content,
        is_url: bool,
        unescape: bool,
        book: Option<&mut Book>,
        chapter: Option<&mut BookChapter>,
    ) -> Result<String> {
        let Some(rule) = rule.filter(|rule| !rule.trim().is_empty()) else {
            return Ok(if is_url {
                self.base_url.clone()
            } else {
                String::new()
            });
        };
        let mut ctx = self.make_ctx(content.clone(), book, chapter);
        let text = if unescape {
            self.rules.eval_string(rule, &mut ctx).await?
        } else {
            self.rules.eval_string_raw(rule, &mut ctx).await?
        };
        if is_url {
            let value = text.trim();
            if value.is_empty() {
                return Ok(self.base_url.clone());
            }
            return Ok(absolute_url(&self.redirect_url, value));
        }
        Ok(text)
    }

    pub async fn list(
        &self,
        rule: Option<&str>,
        content: &Content,
        is_url: bool,
        book: Option<&mut Book>,
        chapter: Option<&mut BookChapter>,
    ) -> Result<Vec<String>> {
        let Some(rule) = rule.filter(|rule| !rule.trim().is_empty()) else {
            return Ok(Vec::new());
        };
        let mut ctx = self.make_ctx(content.clone(), book, chapter);
        let items = self.rules.eval_list(rule, &mut ctx).await?;
        if !is_url {
            return Ok(items);
        }
        let mut urls = Vec::new();
        for item in items {
            let absolute = absolute_url(&self.redirect_url, item.trim());
            if !absolute.is_empty() && !urls.contains(&absolute) {
                urls.push(absolute);
            }
        }
        Ok(urls)
    }

    pub async fn value(
        &self,
        rule: &str,
        content: &Content,
        book: Option<&mut Book>,
        chapter: Option<&mut BookChapter>,
    ) -> Result<Value> {
        let mut ctx = self.make_ctx(content.clone(), book, chapter);
        self.rules.eval(rule, &mut ctx).await
    }
}

pub async fn fetch_page(
    js: &dyn JsEvaluator,
    source: &dyn Source,
    fetcher: &dyn PageFetcher,
    rule: &str,
    base_url: &str,
    key: Option<&str>,
    page: Option<i32>,
) -> Result<HttpResponse> {
    let ctx = UrlContext::new(base_url).with_source(source).with_js(js);
    let ctx = match key {
        Some(key) => ctx.with_key(key),
        None => ctx,
    };
    let ctx = match page {
        Some(page) => ctx.with_page(page),
        None => ctx,
    };
    let url_rule = UrlRule::parse(rule, &ctx).await?;
    let response = fetcher.fetch_rule(&url_rule).await?;
    match source
        .login_check_js()
        .filter(|rule| !rule.trim().is_empty())
    {
        Some(check) => check_login(check, response, &ctx).await,
        None => Ok(response),
    }
}

pub fn content_for(body: &str) -> Content {
    let trimmed = body.trim_start();
    if (trimmed.starts_with('{') || trimmed.starts_with('['))
        && let Ok(value) = serde_json::from_str::<Json>(body)
    {
        return Content::Json(value);
    }
    Content::html(body)
}

pub fn split_value(value: Value) -> Vec<Content> {
    match value {
        Value::Elements(items) => items.into_iter().map(Content::fragment).collect(),
        Value::List(items) => items.into_iter().map(Content::text).collect(),
        Value::Rows(rows) => rows.into_iter().map(Content::List).collect(),
        Value::Json(Json::Array(items)) => items.into_iter().map(Content::Json).collect(),
        Value::Json(Json::Null) | Value::None => Vec::new(),
        Value::Json(value) => vec![Content::Json(value)],
        Value::Text(text) => {
            if text.is_empty() {
                Vec::new()
            } else {
                vec![Content::text(text)]
            }
        }
    }
}

pub fn strip_reverse(rule: &str) -> (&str, bool) {
    if let Some(rest) = rule.strip_prefix('-') {
        (rest, true)
    } else if let Some(rest) = rule.strip_prefix('+') {
        (rest, false)
    } else {
        (rule, false)
    }
}
