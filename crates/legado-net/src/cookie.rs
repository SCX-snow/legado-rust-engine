use std::collections::BTreeMap;
use std::sync::Mutex;

use legado_core::CookieStore;

use crate::domain::sub_domain;

pub fn cookie_to_map(cookie: &str) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    if cookie.trim().is_empty() {
        return map;
    }
    for pair in cookie.split(';') {
        let Some((key, value)) = pair.split_once('=') else {
            continue;
        };
        let key = key.trim();
        let value = value.trim();
        if key.is_empty() {
            continue;
        }
        if !value.is_empty() || value == "null" {
            map.insert(key.to_string(), value.to_string());
        }
    }
    map
}

pub fn map_to_cookie(map: &BTreeMap<String, String>) -> String {
    map.iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect::<Vec<_>>()
        .join("; ")
}

pub fn merge_cookies(cookies: &[&str]) -> String {
    let mut map = BTreeMap::new();
    for cookie in cookies {
        map.extend(cookie_to_map(cookie));
    }
    map_to_cookie(&map)
}

#[derive(Default)]
pub struct MemoryCookieStore {
    cookies: Mutex<BTreeMap<String, String>>,
}

impl MemoryCookieStore {
    pub fn new() -> Self {
        Self::default()
    }

    fn with<T>(&self, action: impl FnOnce(&mut BTreeMap<String, String>) -> T) -> T {
        let mut cookies = self
            .cookies
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        action(&mut cookies)
    }
}

impl CookieStore for MemoryCookieStore {
    fn get_cookie(&self, url: &str) -> String {
        let domain = sub_domain(url);
        self.with(|cookies| cookies.get(&domain).cloned().unwrap_or_default())
    }

    fn set_cookie(&self, url: &str, cookie: &str) {
        let domain = sub_domain(url);
        self.with(|cookies| {
            cookies.insert(domain, cookie.to_string());
        });
    }

    fn replace_cookie(&self, url: &str, cookie: &str) {
        let domain = sub_domain(url);
        self.with(|cookies| {
            let merged = match cookies.get(&domain) {
                Some(existing) if !existing.is_empty() => merge_cookies(&[existing, cookie]),
                _ => cookie.to_string(),
            };
            cookies.insert(domain, merged);
        });
    }

    fn remove_cookie(&self, url: &str) {
        let domain = sub_domain(url);
        self.with(|cookies| {
            cookies.remove(&domain);
        });
    }
}
