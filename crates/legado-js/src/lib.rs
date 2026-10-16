pub mod asymmetric;
pub mod crypto;
pub(crate) mod date_format;
pub mod host;
pub mod html_format;
pub mod java;
pub mod javax;
pub mod jsoup;
pub mod packages;
pub mod rules;
pub(crate) mod ttf;
pub mod worker;

use std::sync::Arc;
use std::time::Duration;

use host::Binding;
use legado_core::{
    BoxFuture, CacheStore, CookieStore, HttpFetcher, HttpResponse, JsEvaluator, JsRequest, JsValue,
    Result, RuleRunner,
};
use worker::{Worker, WorkerConfig};

#[derive(Clone, Default)]
pub struct JsEngineBuilder {
    http: Option<Arc<dyn HttpFetcher>>,
    cache: Option<Arc<dyn CacheStore>>,
    cookies: Option<Arc<dyn CookieStore>>,
    rules: Option<Arc<dyn RuleRunner>>,
    config: WorkerConfig,
}

impl JsEngineBuilder {
    pub fn http(mut self, http: Arc<dyn HttpFetcher>) -> Self {
        self.http = Some(http);
        self
    }

    pub fn cache(mut self, cache: Arc<dyn CacheStore>) -> Self {
        self.cache = Some(cache);
        self
    }

    pub fn cookies(mut self, cookies: Arc<dyn CookieStore>) -> Self {
        self.cookies = Some(cookies);
        self
    }

    pub fn rules(mut self, rules: Arc<dyn RuleRunner>) -> Self {
        self.rules = Some(rules);
        self
    }

    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.config.timeout = timeout;
        self
    }

    pub fn memory_limit(mut self, bytes: usize) -> Self {
        self.config.memory_limit = bytes;
        self
    }

    pub fn android_id(mut self, id: impl Into<String>) -> Self {
        self.config.android_id = id.into();
        self
    }

    pub fn build(self) -> Result<JsEngine> {
        let config = self.config.clone();
        Ok(JsEngine {
            worker: Worker::spawn(
                config.clone(),
                worker::Deps {
                    http: self.http,
                    cache: self.cache,
                    cookies: self.cookies,
                    rules: self.rules,
                },
            )?,
            timeout: config.timeout,
        })
    }
}

pub struct JsEngine {
    worker: Worker,
    timeout: Duration,
}

impl JsEngine {
    pub fn builder() -> JsEngineBuilder {
        JsEngineBuilder::default()
    }

    pub fn new() -> Result<Self> {
        JsEngineBuilder::default().build()
    }

    async fn run(
        &self,
        request: JsRequest<'_>,
        result_items: Option<&[String]>,
    ) -> Result<worker::Output> {
        let mut request = request;
        let lib = request
            .source
            .and_then(|source| source.js_lib())
            .filter(|lib| !lib.trim().is_empty())
            .map(str::to_string);
        let lib_key = lib_key(
            request
                .source
                .map(|source| source.key())
                .unwrap_or_default(),
            lib.as_deref(),
        );
        let binding = Binding::from_request(&request, result_items);
        let output = if worker::on_js_thread() {
            worker::eval_inline(binding, request.script.to_string())?
        } else {
            self.worker
                .eval(
                    binding,
                    request.script.to_string(),
                    lib_key,
                    lib,
                    self.timeout,
                )
                .await?
        };
        if let Some(target) = request.variables.as_deref_mut() {
            *target = output.variables.clone();
        }
        Ok(output)
    }
}

impl JsEvaluator for JsEngine {
    fn eval<'a>(&'a self, request: JsRequest<'a>) -> BoxFuture<'a, Result<String>> {
        Box::pin(async move { Ok(self.run(request, None).await?.value) })
    }

    fn eval_value<'a>(
        &'a self,
        request: JsRequest<'a>,
        result_items: Option<&'a [String]>,
    ) -> BoxFuture<'a, Result<JsValue>> {
        Box::pin(async move {
            use crate::worker::JsKind;
            let output = self.run(request, result_items).await?;
            Ok(match output.items {
                Some(items) => JsValue::List(items),
                None => match output.kind {
                    JsKind::Text => JsValue::Text(output.value),
                    JsKind::Null => JsValue::Null,
                    JsKind::Other => JsValue::Other(output.value),
                },
            })
        })
    }

    fn eval_response<'a>(&'a self, request: JsRequest<'a>) -> BoxFuture<'a, Result<HttpResponse>> {
        Box::pin(async move {
            let original = request.response.cloned();
            let output = self.run(request, None).await?;
            if let Some(response) = output.response {
                return Ok(response);
            }
            match original {
                Some(mut response) if !output.value.is_empty() => {
                    response.body = output.value.clone().into();
                    response.text = output.value;
                    Ok(response)
                }
                Some(response) => Ok(response),
                None => Err(legado_core::Error::Js(
                    "loginCheckJs 没有输入响应，脚本也没返回响应对象".into(),
                )),
            }
        })
    }
}

fn lib_key(source_key: &str, lib: Option<&str>) -> u64 {
    use std::hash::{Hash as _, Hasher as _};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    source_key.hash(&mut hasher);
    lib.unwrap_or_default().hash(&mut hasher);
    hasher.finish()
}
