pub mod client;
pub mod cookie;
pub mod domain;
pub mod encode;
pub mod json;
pub mod rate;
pub mod url;

pub use client::{HttpClient, HttpClientBuilder, check_login};
pub use cookie::{MemoryCookieStore, cookie_to_map, map_to_cookie, merge_cookies};
pub use domain::{base_url, sub_domain};
pub use encode::{
    content_type_charset, decode, detect_encoding, escape, has_url_encoded, resolve_encoding,
    url_encode,
};
pub use json::parse as parse_lenient_json;
pub use rate::{RateGuard, RateLimiter};
pub use url::{DEFAULT_USER_AGENT, UrlContext, UrlRule, absolute_url};
