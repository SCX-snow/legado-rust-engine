use std::net::IpAddr;

pub fn base_url(url: &str) -> Option<String> {
    let lower = url.to_ascii_lowercase();
    if !lower.starts_with("http://") && !lower.starts_with("https://") {
        return None;
    }
    match url.get(9..).and_then(|rest| rest.find('/')) {
        Some(index) => Some(url[..9 + index].to_string()),
        None => Some(url.to_string()),
    }
}

pub fn host(url: &str) -> Option<String> {
    let parsed = url::Url::parse(url).ok()?;
    Some(parsed.host_str()?.to_ascii_lowercase())
}

pub fn sub_domain(url: &str) -> String {
    let Some(base) = base_url(url) else {
        return url.to_string();
    };
    let Some(host) = host(&base) else {
        return base;
    };
    if host.parse::<IpAddr>().is_ok() {
        return host;
    }
    psl::domain_str(&host).map(str::to_string).unwrap_or(host)
}
