use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use http::header::{CONTENT_TYPE, COOKIE};
use legado_core::{
    BoxFuture, CookieStore, Error, HttpFetcher, HttpRequest, HttpResponse, JsRequest, Result,
};
use reqwest::header::{HeaderMap as ReqHeaderMap, HeaderValue as ReqHeaderValue};

use crate::cookie::{MemoryCookieStore, merge_cookies};
use crate::encode;
use crate::rate::RateLimiter;
use crate::url::{DEFAULT_USER_AGENT, UrlContext, UrlRule};

#[derive(Clone)]
pub struct HttpClientBuilder {
    connect_timeout: Duration,
    read_timeout: Duration,
    total_timeout: Duration,
    proxy: Option<String>,
    accept_invalid_certs: bool,
    user_agent: String,
    cookies: Option<Arc<dyn CookieStore>>,
    max_redirects: usize,
}

impl Default for HttpClientBuilder {
    fn default() -> Self {
        Self {
            connect_timeout: Duration::from_secs(15),
            read_timeout: Duration::from_secs(15),
            total_timeout: Duration::from_secs(60),
            proxy: None,
            accept_invalid_certs: true,
            user_agent: DEFAULT_USER_AGENT.to_string(),
            cookies: None,
            max_redirects: 10,
        }
    }
}

impl HttpClientBuilder {
    pub fn connect_timeout(mut self, timeout: Duration) -> Self {
        self.connect_timeout = timeout;
        self
    }

    pub fn read_timeout(mut self, timeout: Duration) -> Self {
        self.read_timeout = timeout;
        self
    }

    pub fn total_timeout(mut self, timeout: Duration) -> Self {
        self.total_timeout = timeout;
        self
    }

    pub fn proxy(mut self, proxy: impl Into<String>) -> Self {
        self.proxy = Some(proxy.into());
        self
    }

    pub fn accept_invalid_certs(mut self, accept: bool) -> Self {
        self.accept_invalid_certs = accept;
        self
    }

    pub fn user_agent(mut self, user_agent: impl Into<String>) -> Self {
        self.user_agent = user_agent.into();
        self
    }

    pub fn cookies(mut self, cookies: Arc<dyn CookieStore>) -> Self {
        self.cookies = Some(cookies);
        self
    }

    pub fn build(self) -> Result<HttpClient> {
        let cookies = self
            .cookies
            .clone()
            .unwrap_or_else(|| Arc::new(MemoryCookieStore::new()));
        let base = self.build_pair(None)?;
        Ok(HttpClient {
            base,
            config: self,
            cookies,
            proxied: Mutex::new(HashMap::new()),
            rate: RateLimiter::new(),
        })
    }

    fn build_pair(&self, proxy: Option<&str>) -> Result<ClientPair> {
        let proxy = proxy.map(str::trim).filter(|text| !text.is_empty());
        Ok(ClientPair {
            plain: self.build_client(proxy, false)?,
            jar: self.build_client(proxy, true)?,
        })
    }

    fn build_client(&self, proxy: Option<&str>, cookie_jar: bool) -> Result<reqwest::Client> {
        let mut default_headers = ReqHeaderMap::new();
        let user_agent = ReqHeaderValue::from_str(&self.user_agent)
            .map_err(|error| Error::Http(format!("User-Agent 非法: {error}")))?;
        default_headers.insert(http::header::USER_AGENT, user_agent);
        default_headers.insert(
            http::header::CACHE_CONTROL,
            ReqHeaderValue::from_static("no-cache"),
        );

        let mut builder = reqwest::Client::builder()
            .connect_timeout(self.connect_timeout)
            .read_timeout(self.read_timeout)
            .timeout(self.total_timeout)
            .redirect(reqwest::redirect::Policy::limited(self.max_redirects))
            .danger_accept_invalid_certs(self.accept_invalid_certs)
            .danger_accept_invalid_hostnames(self.accept_invalid_certs)
            .default_headers(default_headers)
            .cookie_store(cookie_jar);

        let proxy = proxy.or(self.proxy.as_deref());
        if let Some(proxy) = proxy {
            let proxy = reqwest::Proxy::all(proxy)
                .map_err(|error| Error::Http(format!("代理 `{proxy}` 配不了: {error}")))?;
            builder = builder.proxy(proxy);
        }
        builder
            .build()
            .map_err(|error| Error::Http(format!("构造 HTTP 客户端失败: {error}")))
    }
}

#[derive(Clone)]
struct ClientPair {
    plain: reqwest::Client,
    jar: reqwest::Client,
}

pub struct HttpClient {
    base: ClientPair,
    config: HttpClientBuilder,
    cookies: Arc<dyn CookieStore>,
    proxied: Mutex<HashMap<String, ClientPair>>,
    rate: RateLimiter,
}

impl HttpClient {
    pub fn builder() -> HttpClientBuilder {
        HttpClientBuilder::default()
    }

    pub fn new() -> Result<Self> {
        HttpClient::builder().build()
    }

    pub fn cookies(&self) -> &dyn CookieStore {
        self.cookies.as_ref()
    }

    fn client_for(&self, proxy: Option<&str>, cookie_jar: bool) -> Result<reqwest::Client> {
        let proxy = proxy.map(str::trim).filter(|text| !text.is_empty());
        let Some(proxy) = proxy else {
            return Ok(pick_client(&self.base, cookie_jar));
        };
        {
            let cache = self
                .proxied
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if let Some(pair) = cache.get(proxy) {
                return Ok(pick_client(pair, cookie_jar));
            }
        }
        let pair = self.config.build_pair(Some(proxy))?;
        let picked = pick_client(&pair, cookie_jar);
        self.proxied
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(proxy.to_string(), pair);
        Ok(picked)
    }

    pub async fn send(&self, request: HttpRequest) -> Result<HttpResponse> {
        let client = self.client_for(None, false)?;
        self.send_with(&client, request).await
    }

    pub async fn send_retry(&self, request: HttpRequest, retry: u32) -> Result<HttpResponse> {
        let client = self.client_for(None, false)?;
        self.send_retry_with(&client, request, retry).await
    }

    pub async fn send_js(
        &self,
        request: HttpRequest,
        rate: Option<&str>,
        rate_key: &str,
    ) -> Result<HttpResponse> {
        let _guard = self.rate_guard(rate_key, rate).await;
        self.send(request).await
    }

    pub async fn send_rule(&self, rule: &UrlRule) -> Result<HttpResponse> {
        let rate_key = rule.rate_key().unwrap_or_default();
        let _guard = self.rate_guard(rate_key, rule.rate()).await;

        let mut request = rule.request();
        let cookie = self.cookies.get_cookie(rule.cookie_url());
        if !cookie.is_empty() {
            let existing = request
                .headers
                .get(COOKIE)
                .and_then(|value| value.to_str().ok())
                .unwrap_or_default()
                .to_string();
            let merged = merge_cookies(&[&existing, &cookie]);
            if let Ok(value) = http::HeaderValue::from_str(&merged) {
                request.headers.insert(COOKIE, value);
            }
        }

        let client = self.client_for(rule.proxy(), rule.cookie_jar())?;
        self.send_retry_with(&client, request, rule.retry()).await
    }

    async fn rate_guard<'a>(
        &'a self,
        key: &'a str,
        rate: Option<&str>,
    ) -> Option<crate::rate::RateGuard<'a>> {
        self.rate.acquire(key, rate).await
    }

    async fn send_retry_with(
        &self,
        client: &reqwest::Client,
        request: HttpRequest,
        retry: u32,
    ) -> Result<HttpResponse> {
        let mut last = None;
        for attempt in 0..=retry {
            if attempt > 0 {
                tracing::debug!("第 {attempt} 次重试 {}", request.url);
            }
            let response = self.send_with(client, request.clone()).await?;
            if response.status.is_success() {
                return Ok(response);
            }
            last = Some(response);
        }
        Ok(last.expect("至少发过一次请求"))
    }

    async fn send_with(
        &self,
        client: &reqwest::Client,
        request: HttpRequest,
    ) -> Result<HttpResponse> {
        let method = reqwest::Method::from_bytes(request.method.as_str().as_bytes())
            .map_err(|error| Error::Http(format!("请求方法非法: {error}")))?;
        let mut builder = client.request(method, &request.url);
        for (name, value) in request.headers.iter() {
            builder = builder.header(name, value);
        }
        if let Some(body) = request.body {
            builder = builder.body(body);
        }
        let response = builder
            .send()
            .await
            .map_err(|error| Error::Http(format!("{} 请求失败: {error}", request.url)))?;
        self.read_response(response, request.charset.as_deref())
            .await
    }

    async fn read_response(
        &self,
        response: reqwest::Response,
        charset: Option<&str>,
    ) -> Result<HttpResponse> {
        let url = response.url().to_string();
        let status = response.status();
        let headers = response.headers().clone();
        let bytes = response
            .bytes()
            .await
            .map_err(|error| Error::Http(format!("{url} 读响应体失败: {error}")))?;
        let content_type = headers
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .map(str::to_string);
        let encoding = encode::resolve_encoding(&bytes, content_type.as_deref(), charset);
        let text = encode::decode(&bytes, encoding);
        Ok(HttpResponse {
            url,
            status,
            headers,
            body: bytes,
            text,
        })
    }
}

fn pick_client(pair: &ClientPair, cookie_jar: bool) -> reqwest::Client {
    if cookie_jar {
        pair.jar.clone()
    } else {
        pair.plain.clone()
    }
}

impl HttpFetcher for HttpClient {
    fn fetch(&self, request: HttpRequest) -> BoxFuture<'_, Result<HttpResponse>> {
        Box::pin(async move { self.send(request).await })
    }

    fn fetch_limited(
        &self,
        request: HttpRequest,
        rate: Option<String>,
        rate_key: String,
    ) -> BoxFuture<'_, Result<HttpResponse>> {
        Box::pin(async move { self.send_js(request, rate.as_deref(), &rate_key).await })
    }
}

pub async fn check_login(
    js: &str,
    response: HttpResponse,
    ctx: &UrlContext<'_>,
) -> Result<HttpResponse> {
    if js.trim().is_empty() {
        return Ok(response);
    }
    let Some(evaluator) = ctx.js else {
        return Err(Error::Js("loginCheckJs 需要 JsEvaluator（M3 接上）".into()));
    };
    evaluator
        .eval_response(JsRequest {
            script: js,
            source: ctx.source,
            base_url: ctx.base_url,
            page: ctx.page,
            key: ctx.key,
            result: Some(&response.text),
            src: None,
            response: Some(&response),
            book: None,
            chapter: None,
            variables: None,
        })
        .await
}
