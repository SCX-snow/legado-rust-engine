use legado_core::{BoxFuture, HttpResponse, Result};
use legado_net::{HttpClient, UrlRule};

pub trait PageFetcher: Send + Sync {
    fn fetch_rule<'a>(&'a self, rule: &'a UrlRule) -> BoxFuture<'a, Result<HttpResponse>>;
}

impl PageFetcher for HttpClient {
    fn fetch_rule<'a>(&'a self, rule: &'a UrlRule) -> BoxFuture<'a, Result<HttpResponse>> {
        Box::pin(async move { self.send_rule(rule).await })
    }
}
