use std::collections::HashMap;
use std::rc::Rc;

use base64::Engine as _;
use legado_core::{Error, HttpRequest, HttpResponse, Result, java_list_to_string};
use rquickjs::function::Opt;
use rquickjs::{Array, Ctx, Function, Object, Value as JsValue};

use crate::host::Host;

pub(crate) fn js_err(error: Error) -> rquickjs::Error {
    rquickjs::Error::new_from_js_message("宿主", "java", error.to_string())
}

fn js_err_message(message: String) -> rquickjs::Error {
    rquickjs::Error::new_from_js_message("宿主", "java", message)
}

pub(crate) fn install<'js>(ctx: &Ctx<'js>, host: &Rc<Host>) -> rquickjs::Result<Object<'js>> {
    let java = Object::new(ctx.clone())?;

    java.set("ajax", ajax(ctx, host)?)?;
    java.set("ajaxAll", ajax_all(ctx, host)?)?;
    java.set("connect", connect(ctx, host)?)?;
    java.set("get", get(ctx, host)?)?;
    java.set("post", post(ctx, host)?)?;
    java.set("head", head(ctx, host)?)?;

    java.set("getString", get_string(ctx, host)?)?;
    java.set("getStringList", get_string_list(ctx, host)?)?;
    java.set("getElement", get_elements(ctx, host)?)?;
    java.set("getElements", get_elements(ctx, host)?)?;
    java.set("setContent", set_content(ctx, host, &java)?)?;

    java.set("base64Encode", base64_encode(ctx)?)?;
    java.set("base64Decode", base64_decode(ctx)?)?;
    java.set("base64DecodeToByteArray", base64_decode_bytes(ctx)?)?;
    java.set("hexEncodeToString", hex_encode(ctx)?)?;
    java.set("hexDecodeToString", hex_decode(ctx)?)?;
    java.set("hexDecodeToByteArray", hex_decode_bytes(ctx)?)?;
    java.set("md5Encode", md5_encode(ctx)?)?;
    java.set("digestBase64Str", digest_base64_str(ctx)?)?;
    java.set("encodeURI", encode_uri(ctx)?)?;
    java.set("strToBytes", str_to_bytes(ctx)?)?;
    java.set("bytesToStr", bytes_to_str(ctx)?)?;
    java.set("htmlFormat", html_format(ctx)?)?;
    java.set("t2s", t2s(ctx)?)?;
    java.set("s2t", s2t(ctx)?)?;

    java.set("createSymmetricCrypto", create_symmetric_crypto(ctx)?)?;
    java.set("getCookie", get_cookie(ctx, host)?)?;
    java.set("aesBase64DecodeToString", symmetric_decrypt_str(ctx)?)?;
    java.set("desBase64DecodeToString", symmetric_decrypt_str(ctx)?)?;
    java.set("createAsymmetricCrypto", create_asymmetric_crypto(ctx)?)?;
    java.set("createSign", create_sign(ctx)?)?;
    java.set("digestHex", digest_hex(ctx)?)?;
    java.set("HMacHex", hmac(ctx, true)?)?;
    java.set("HMacBase64", hmac(ctx, false)?)?;
    java.set("md5Encode16", md5_encode16(ctx)?)?;
    java.set("timeFormat", time_format(ctx)?)?;
    java.set("timeFormatUTC", time_format_utc(ctx)?)?;
    java.set("toNumChapter", to_num_chapter(ctx)?)?;
    java.set("randomUUID", random_uuid(ctx)?)?;
    java.set("androidId", android_id(ctx, host)?)?;

    java.set("log", log(ctx)?)?;
    java.set("toast", toast(ctx, "toast")?)?;
    java.set("longToast", toast(ctx, "longToast")?)?;

    java.set("getUrl", get_url(ctx, host)?)?;
    java.set("put", put(ctx, host)?)?;
    java.set("cacheFile", cache_file(ctx, host)?)?;
    java.set("importScript", import_script(ctx, host)?)?;

    java.set("queryTTF", crate::ttf::query_ttf(ctx, host)?)?;
    java.set("queryBase64TTF", crate::ttf::query_ttf(ctx, host)?)?;
    java.set("replaceFont", crate::ttf::replace_font_fn(ctx, host)?)?;
    Ok(java)
}

fn ajax<'js>(ctx: &Ctx<'js>, host: &Rc<Host>) -> rquickjs::Result<Function<'js>> {
    let host = host.clone();
    Function::new(
        ctx.clone(),
        move |value: JsValue<'js>| -> rquickjs::Result<String> {
            let url = ajax_url(&value);
            Ok(match send(&host, &url, None) {
                Ok(response) => response.text,
                Err(error) => format!("{error}\n{error:?}"),
            })
        },
    )
}

fn ajax_url(value: &JsValue<'_>) -> String {
    if let Some(items) = java_list_items(value) {
        return items.first().cloned().unwrap_or_else(|| "null".to_string());
    }
    if value.is_undefined() {
        return "undefined".to_string();
    }
    if value.is_null() {
        return "null".to_string();
    }
    js_text(value)
}

fn connect<'js>(ctx: &Ctx<'js>, host: &Rc<Host>) -> rquickjs::Result<Function<'js>> {
    let host = host.clone();
    Function::new(
        ctx.clone(),
        move |ctx: Ctx<'js>, url: String, header: Opt<JsValue<'js>>| {
            let header = header.0.as_ref().map(string_map).unwrap_or_default();
            response_object(&ctx, &send(&host, &url, Some(&header)).map_err(js_err)?)
        },
    )
}

fn ajax_all<'js>(ctx: &Ctx<'js>, host: &Rc<Host>) -> rquickjs::Result<Function<'js>> {
    let host = host.clone();
    Function::new(
        ctx.clone(),
        move |ctx: Ctx<'js>, urls: Array<'js>| -> rquickjs::Result<JsValue<'js>> {
            let urls: Vec<String> = urls
                .iter::<String>()
                .map(|url| url.unwrap_or_default())
                .collect();
            let responses = host.runtime.block_on(async {
                let futures = urls.iter().map(|url| send_async(&host, url, None));
                futures::future::join_all(futures).await
            });
            let out = Array::new(ctx.clone())?;
            for (index, response) in responses.into_iter().enumerate() {
                match response {
                    Ok(response) => out.set(index, response_object(&ctx, &response)?)?,
                    Err(_) => out.set(index, JsValue::new_null(ctx.clone()))?,
                }
            }
            Ok(out.into_value())
        },
    )
}

async fn send_async(
    host: &Rc<Host>,
    url: &str,
    headers: Option<&HashMap<String, String>>,
) -> Result<HttpResponse> {
    let Some(fetcher) = host.http.as_ref() else {
        return Err(Error::Js("没有接 HttpFetcher，java.ajax 用不了".into()));
    };
    let (url, option_method, option_body, option_headers) = split_option(url);
    let mut request = HttpRequest::get(&url);
    request.method = http::Method::from_bytes(option_method.as_bytes())
        .map_err(|error| Error::Js(format!("请求方法非法: {error}")))?;
    request.body = option_body.map(|body| body.into());
    for (name, value) in option_headers.iter().chain(headers.into_iter().flatten()) {
        if let (Ok(name), Ok(value)) = (
            http::HeaderName::from_bytes(name.as_bytes()),
            http::HeaderValue::from_str(value),
        ) {
            request.headers.insert(name, value);
        }
    }
    let (rate, rate_key) = host.with_binding(|binding| {
        binding
            .and_then(|binding| binding.source.as_ref())
            .map(|source| {
                (
                    source.concurrent_rate().map(str::to_string),
                    source.key().to_string(),
                )
            })
            .unwrap_or_default()
    });
    fetcher.fetch_limited(request, rate, rate_key).await
}

fn head<'js>(ctx: &Ctx<'js>, host: &Rc<Host>) -> rquickjs::Result<Function<'js>> {
    let host = host.clone();
    Function::new(
        ctx.clone(),
        move |ctx: Ctx<'js>, url: String, headers: Opt<JsValue<'js>>| {
            let headers = headers.0.as_ref().map(string_map).unwrap_or_default();
            let response = send_with(&host, &url, Some("HEAD"), None, &headers).map_err(js_err)?;
            response_object(&ctx, &response)
        },
    )
}

fn get<'js>(ctx: &Ctx<'js>, host: &Rc<Host>) -> rquickjs::Result<Function<'js>> {
    let host = host.clone();
    Function::new(
        ctx.clone(),
        move |ctx: Ctx<'js>,
              args: rquickjs::function::Rest<JsValue<'js>>|
              -> rquickjs::Result<JsValue<'js>> {
            if args.0.len() > 1 {
                let url = args.0[0]
                    .as_string()
                    .and_then(|v| v.to_string().ok())
                    .unwrap_or_default();
                let header = string_map(&args.0[1]);
                return response_object(&ctx, &send(&host, &url, Some(&header)).map_err(js_err)?);
            }
            let key = args
                .0
                .first()
                .and_then(|value| value.as_string())
                .and_then(|value| value.to_string().ok())
                .unwrap_or_default();
            let value = host.with_binding(|binding| match binding {
                Some(binding) => match key.as_str() {
                    "bookName" => text_field(binding.book.as_ref().map(|book| book.name.as_str())),
                    "title" => text_field(
                        binding
                            .chapter
                            .as_ref()
                            .map(|chapter| chapter.title.as_str()),
                    ),
                    _ => binding.variables.get(&key).unwrap_or_default().to_string(),
                },
                None => String::new(),
            });
            Ok(rquickjs::String::from_str(ctx, &value)?.into_value())
        },
    )
}

fn text_field(value: Option<&str>) -> String {
    value.unwrap_or_default().to_string()
}

fn post<'js>(ctx: &Ctx<'js>, host: &Rc<Host>) -> rquickjs::Result<Function<'js>> {
    let host = host.clone();
    Function::new(
        ctx.clone(),
        move |ctx: Ctx<'js>, url: String, body: String, headers: Opt<JsValue<'js>>| {
            let header = headers.0.as_ref().map(string_map).unwrap_or_default();
            let response = send_body(&host, &url, Some(body), &header).map_err(js_err)?;
            response_object(&ctx, &response)
        },
    )
}

fn put<'js>(ctx: &Ctx<'js>, host: &Rc<Host>) -> rquickjs::Result<Function<'js>> {
    let host = host.clone();
    Function::new(
        ctx.clone(),
        move |key: String, value: JsValue<'js>| -> rquickjs::Result<String> {
            let text = js_text(&value);
            host.with_binding_mut(|binding| {
                if let Some(binding) = binding {
                    binding.variables.put(&key, Some(&text));
                }
            });
            Ok(text)
        },
    )
}

fn get_url<'js>(ctx: &Ctx<'js>, host: &Rc<Host>) -> rquickjs::Result<Function<'js>> {
    let host = host.clone();
    Function::new(ctx.clone(), move || -> String {
        host.with_binding(|binding| binding.map(|b| b.base_url.clone()).unwrap_or_default())
    })
}

pub(crate) fn send(
    host: &Rc<Host>,
    url: &str,
    headers: Option<&HashMap<String, String>>,
) -> Result<HttpResponse> {
    let (url, method, body, extra) = split_option(url);
    let mut all = extra;
    if let Some(headers) = headers {
        all.extend(headers.clone());
    }
    send_with(host, &url, Some(&method), body, &all).map(|mut response| {
        response.url = url;
        response
    })
}

pub(crate) fn send_body(
    host: &Rc<Host>,
    url: &str,
    body: Option<String>,
    headers: &HashMap<String, String>,
) -> Result<HttpResponse> {
    send_with(host, url, Some("POST"), body, headers)
}

fn send_with(
    host: &Rc<Host>,
    url: &str,
    method: Option<&str>,
    body: Option<String>,
    headers: &HashMap<String, String>,
) -> Result<HttpResponse> {
    let Some(fetcher) = host.http.as_ref() else {
        return Err(Error::Js("没有接 HttpFetcher，java.ajax 用不了".into()));
    };
    let (url, option_method, option_body, option_headers) = split_option(url);
    let method = method.unwrap_or(&option_method);
    let mut request = HttpRequest::get(&url);
    request.method = http::Method::from_bytes(method.as_bytes())
        .map_err(|error| Error::Js(format!("请求方法非法: {error}")))?;
    request.body = body.or(option_body).map(|body| body.into());
    for (name, value) in option_headers.iter().chain(headers.iter()) {
        if let (Ok(name), Ok(value)) = (
            http::HeaderName::from_bytes(name.as_bytes()),
            http::HeaderValue::from_str(value),
        ) {
            request.headers.insert(name, value);
        }
    }
    let (rate, rate_key) = host.with_binding(|binding| {
        binding
            .and_then(|binding| binding.source.as_ref())
            .map(|source| {
                (
                    source.concurrent_rate().map(str::to_string),
                    source.key().to_string(),
                )
            })
            .unwrap_or_default()
    });
    host.runtime
        .block_on(fetcher.fetch_limited(request, rate, rate_key))
}

fn split_option(url: &str) -> (String, String, Option<String>, HashMap<String, String>) {
    let mut headers = HashMap::new();
    let (base, option) = match find_options(url) {
        Some((base, text)) => (base, serde_json::from_str::<serde_json::Value>(&text).ok()),
        None => (url.to_string(), None),
    };
    let mut method = "GET".to_string();
    let mut body = None;
    if let Some(serde_json::Value::Object(fields)) = option {
        for (key, value) in fields {
            match key.as_str() {
                "method" => {
                    if let Some(text) = value.as_str() {
                        method = text.to_ascii_uppercase();
                    }
                }
                "body" => {
                    body = Some(match &value {
                        serde_json::Value::String(text) => text.clone(),
                        other => other.to_string(),
                    });
                }
                "headers" => {
                    if let serde_json::Value::Object(fields) = value {
                        for (name, value) in fields {
                            headers.insert(name, json_text(&value));
                        }
                    }
                }
                _ => {}
            }
        }
    }
    (base, method, body, headers)
}

fn find_options(url: &str) -> Option<(String, String)> {
    let chars: Vec<char> = url.chars().collect();
    for index in 0..chars.len() {
        if chars[index] != ',' {
            continue;
        }
        let mut start = index;
        while start > 0 && chars[start - 1].is_whitespace() {
            start -= 1;
        }
        let mut end = index + 1;
        while end < chars.len() && chars[end].is_whitespace() {
            end += 1;
        }
        if chars.get(end) == Some(&'{') {
            return Some((
                chars[..start].iter().collect(),
                chars[end..].iter().collect(),
            ));
        }
    }
    None
}

fn json_text(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(text) => text.clone(),
        serde_json::Value::Null => "null".to_string(),
        other => other.to_string(),
    }
}

pub(crate) fn response_object<'js>(
    ctx: &Ctx<'js>,
    response: &HttpResponse,
) -> rquickjs::Result<JsValue<'js>> {
    let object = Object::new(ctx.clone())?;
    object.set("url", response.url.as_str())?;
    object.set("code", response.status.as_u16())?;
    object.set(
        "body",
        Function::new(ctx.clone(), {
            let text = response.text.clone();
            move || -> String { text.clone() }
        })?,
    )?;
    let headers: HashMap<String, String> = response
        .headers
        .iter()
        .filter_map(|(name, value)| Some((name.to_string(), value.to_str().ok()?.to_string())))
        .collect();
    let headers_text = headers
        .iter()
        .map(|(name, value)| format!("{name}: {value}"))
        .collect::<Vec<_>>()
        .join("\r\n");
    object.set(
        "headers",
        Function::new(ctx.clone(), move || -> String { headers_text.clone() })?,
    )?;
    object.set("header", crate::host::map_object(ctx, &headers)?)?;
    object.set("statusCode", {
        let code = response.status.as_u16();
        Function::new(ctx.clone(), move || -> u16 { code })?
    })?;
    Ok(object.into_value())
}

pub(crate) fn response_from_js(value: &JsValue<'_>) -> Option<HttpResponse> {
    let object = value.as_object()?;
    let url: String = object.get("url").unwrap_or_default();
    let code: u16 = object.get("code").unwrap_or(200);
    let text: String = object.get::<_, Function>("body").ok()?.call(()).ok()?;
    let headers_text: String = object
        .get::<_, Function>("headers")
        .ok()
        .and_then(|headers| headers.call(()).ok())
        .unwrap_or_default();
    let mut headers = http::HeaderMap::new();
    for line in headers_text.lines() {
        if let Some((name, value)) = line.split_once(':')
            && let (Ok(name), Ok(value)) = (
                http::HeaderName::from_bytes(name.trim().as_bytes()),
                http::HeaderValue::from_str(value.trim()),
            )
        {
            headers.insert(name, value);
        }
    }
    Some(HttpResponse {
        url,
        status: http::StatusCode::from_u16(code).unwrap_or(http::StatusCode::OK),
        headers,
        body: text.clone().into(),
        text,
    })
}

fn get_string<'js>(ctx: &Ctx<'js>, host: &Rc<Host>) -> rquickjs::Result<Function<'js>> {
    let host = host.clone();
    Function::new(
        ctx.clone(),
        move |ctx: Ctx<'js>,
              args: rquickjs::function::Rest<JsValue<'js>>|
              -> rquickjs::Result<JsValue<'js>> {
            let rule = args.0.first().map(js_text).unwrap_or_default();
            if rule.is_empty() {
                return Ok(rquickjs::String::from_str(ctx, "")?.into_value());
            }
            let (content, unescape) = crate::rules::content_arg(args.0.get(1));
            let text = crate::rules::run_string(&host, &rule, content, unescape).map_err(js_err)?;
            Ok(rquickjs::String::from_str(ctx, &text)?.into_value())
        },
    )
}

fn get_elements<'js>(ctx: &Ctx<'js>, host: &Rc<Host>) -> rquickjs::Result<Function<'js>> {
    let host = host.clone();
    Function::new(
        ctx.clone(),
        move |ctx: Ctx<'js>, rule: String| -> rquickjs::Result<JsValue<'js>> {
            let items = crate::rules::run_elements(&host, &rule, None).map_err(js_err)?;
            let content = host
                .with_binding(|binding| binding.map(|b| b.content.clone()))
                .unwrap_or_default();
            Ok(
                crate::jsoup::elements(&ctx, &host, crate::jsoup::shared_document(content), items)
                    .into_value(),
            )
        },
    )
}

fn get_string_list<'js>(ctx: &Ctx<'js>, host: &Rc<Host>) -> rquickjs::Result<Function<'js>> {
    let host = host.clone();
    Function::new(
        ctx.clone(),
        move |ctx: Ctx<'js>,
              args: rquickjs::function::Rest<JsValue<'js>>|
              -> rquickjs::Result<JsValue<'js>> {
            let rule = match args.0.first() {
                Some(value) if !value.is_null() && !value.is_undefined() => js_text(value),
                _ => String::new(),
            };
            if rule.is_empty() {
                return Ok(JsValue::new_null(ctx));
            }
            let content = match args.0.get(1) {
                Some(value) if !value.is_null() && !value.is_undefined() => Some(js_text(value)),
                _ => None,
            };
            let is_url = args.0.get(2).map(js_truthy).unwrap_or(false);
            let items = crate::rules::run_string_list(&host, &rule, content).map_err(js_err)?;
            let items = if is_url {
                resolve_url_list(&host, items)
            } else {
                items
            };
            Ok(string_list(&ctx, items)?.into_value())
        },
    )
}

fn resolve_url_list(host: &Rc<Host>, items: Vec<String>) -> Vec<String> {
    let base = host.with_binding(|binding| binding.map(|b| b.base_url.clone()).unwrap_or_default());
    let mut out: Vec<String> = Vec::new();
    for item in items {
        let url = absolute_url(&base, &item);
        if !url.is_empty() && !out.contains(&url) {
            out.push(url);
        }
    }
    out
}

fn absolute_url(base: &str, path: &str) -> String {
    let path = path.trim();
    if base.is_empty() {
        return path.to_string();
    }
    let lower = path.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") {
        return path.to_string();
    }
    if is_base64_data_url(path) {
        return path.to_string();
    }
    if path.starts_with("javascript") {
        return String::new();
    }
    match url::Url::parse(base).and_then(|url| url.join(path)) {
        Ok(url) => url.to_string(),
        Err(_) => path.to_string(),
    }
}

fn is_base64_data_url(path: &str) -> bool {
    path.starts_with("data:") && path.contains(";base64,")
}

pub(crate) fn string_list<'js>(ctx: &Ctx<'js>, items: Vec<String>) -> rquickjs::Result<Array<'js>> {
    let array = Array::new(ctx.clone())?;
    for (index, text) in items.iter().enumerate() {
        array.set(index, text.as_str())?;
    }
    let items = Rc::new(items);
    let props = array.as_object();
    props.prop(
        "size",
        Function::new(ctx.clone(), {
            let items = items.clone();
            move || -> usize { items.len() }
        })?,
    )?;
    props.prop(
        "isEmpty",
        Function::new(ctx.clone(), {
            let items = items.clone();
            move || -> bool { items.is_empty() }
        })?,
    )?;
    props.prop(
        "get",
        Function::new(ctx.clone(), {
            let items = items.clone();
            move |ctx: Ctx<'js>, index: JsValue<'js>| -> rquickjs::Result<String> {
                let raw = js_to_f64(&index);
                let index = if raw.is_finite() {
                    raw.trunc() as i64
                } else {
                    0
                };
                if index < 0 || index as usize >= items.len() {
                    return Err(rquickjs::Exception::throw_message(
                        &ctx,
                        &format!("Index {index} out of bounds for length {}", items.len()),
                    ));
                }
                Ok(items[index as usize].clone())
            }
        })?,
    )?;
    props.prop(
        "toArray",
        Function::new(ctx.clone(), {
            let array = array.clone();
            move || -> Array<'js> { array.clone() }
        })?,
    )?;
    let to_text = Function::new(ctx.clone(), {
        let items = items.clone();
        move || -> String { java_list_to_string(&items) }
    })?;
    props.prop("toString", to_text)?;
    props.prop(JAVA_LIST_MARK, true)?;
    Ok(array)
}

const JAVA_LIST_MARK: &str = "__legadoJavaList";

fn is_java_list(value: &JsValue<'_>) -> bool {
    value
        .as_object()
        .and_then(|object| object.get::<_, bool>(JAVA_LIST_MARK).ok())
        .unwrap_or(false)
}

pub(crate) fn java_list_items(value: &JsValue<'_>) -> Option<Vec<String>> {
    if !is_java_list(value) {
        return None;
    }
    let array = value.as_array()?;
    Some(
        (0..array.len())
            .filter_map(|index| array.get::<JsValue<'_>>(index).ok())
            .map(|item| js_text(&item))
            .collect(),
    )
}

fn set_content<'js>(
    ctx: &Ctx<'js>,
    host: &Rc<Host>,
    java: &Object<'js>,
) -> rquickjs::Result<Function<'js>> {
    let host = host.clone();
    let java = java.clone();
    Function::new(ctx.clone(), move |value: JsValue<'js>| -> Object<'js> {
        let text = js_text(&value);
        host.with_binding_mut(|binding| {
            if let Some(binding) = binding {
                binding.content = text;
            }
        });
        java.clone()
    })
}

fn base64_encode<'js>(ctx: &Ctx<'js>) -> rquickjs::Result<Function<'js>> {
    Function::new(ctx.clone(), |text: String| -> String {
        base64::engine::general_purpose::STANDARD.encode(text.as_bytes())
    })
}

fn base64_decode<'js>(ctx: &Ctx<'js>) -> rquickjs::Result<Function<'js>> {
    Function::new(
        ctx.clone(),
        |_ctx: Ctx<'js>, text: String, option: Opt<JsValue<'js>>| -> String {
            let charset = option
                .0
                .as_ref()
                .and_then(|value| value.as_string())
                .and_then(|value| value.to_string().ok());
            crate::crypto::decode_charset(
                &decode_base64(&text).unwrap_or_default(),
                charset.as_deref(),
            )
        },
    )
}

fn base64_decode_bytes<'js>(ctx: &Ctx<'js>) -> rquickjs::Result<Function<'js>> {
    Function::new(
        ctx.clone(),
        |ctx: Ctx<'js>,
         text: String,
         _flags: Opt<JsValue<'js>>|
         -> rquickjs::Result<JsValue<'js>> {
            Ok(bytes_to_js(&ctx, &decode_base64(&text).unwrap_or_default()))
        },
    )
}

pub(crate) fn decode_base64(text: &str) -> Option<Vec<u8>> {
    let cleaned: String = text.chars().filter(|ch| !ch.is_whitespace()).collect();
    base64::engine::general_purpose::STANDARD
        .decode(&cleaned)
        .or_else(|_| base64::engine::general_purpose::URL_SAFE.decode(&cleaned))
        .ok()
}

fn digest_base64_str<'js>(ctx: &Ctx<'js>) -> rquickjs::Result<Function<'js>> {
    Function::new(
        ctx.clone(),
        |data: String, algorithm: Opt<String>| -> rquickjs::Result<String> {
            let algorithm = algorithm.0.unwrap_or_else(|| "MD5".to_string());
            let digest =
                crate::crypto::digest_bytes(&algorithm, data.as_bytes()).map_err(js_err_message)?;
            Ok(base64::engine::general_purpose::STANDARD.encode(digest))
        },
    )
}

fn create_symmetric_crypto<'js>(ctx: &Ctx<'js>) -> rquickjs::Result<Function<'js>> {
    Function::new(
        ctx.clone(),
        |ctx: Ctx<'js>,
         transformation: String,
         key: JsValue<'js>,
         iv: Opt<JsValue<'js>>|
         -> rquickjs::Result<Object<'js>> {
            let (algorithm, mode, padding) =
                crate::crypto::parse_transformation(&transformation).map_err(js_err_message)?;
            let iv = iv.0.as_ref().map(js_bytes).unwrap_or_default();
            let symmetric =
                crate::crypto::Symmetric::new(algorithm, mode, padding, js_bytes(&key), iv)
                    .map_err(js_err_message)?;
            Ok(crate::crypto::crypto_object(&ctx, Rc::new(symmetric)))
        },
    )
}

fn get_cookie<'js>(ctx: &Ctx<'js>, host: &Rc<Host>) -> rquickjs::Result<Function<'js>> {
    let host = host.clone();
    Function::new(
        ctx.clone(),
        move |args: rquickjs::function::Rest<JsValue<'js>>| -> String {
            let tag = args
                .0
                .first()
                .and_then(|value| value.as_string())
                .and_then(|value| value.to_string().ok())
                .unwrap_or_default();
            let cookie = host
                .cookies
                .as_ref()
                .map(|store| store.get_cookie(&tag))
                .unwrap_or_default();
            let Some(key) = args
                .0
                .get(1)
                .and_then(|value| value.as_string())
                .and_then(|value| value.to_string().ok())
            else {
                return cookie;
            };
            crate::host::cookie_map(&cookie)
                .remove(&key)
                .unwrap_or_default()
        },
    )
}

fn symmetric_decrypt_str<'js>(ctx: &Ctx<'js>) -> rquickjs::Result<Function<'js>> {
    Function::new(
        ctx.clone(),
        |str: String,
         key: JsValue<'js>,
         transformation: String,
         iv: String|
         -> rquickjs::Result<String> {
            let (algorithm, mode, padding) =
                crate::crypto::parse_transformation(&transformation).map_err(js_err_message)?;
            let symmetric = crate::crypto::Symmetric::new(
                algorithm,
                mode,
                padding,
                js_bytes(&key),
                iv.into_bytes(),
            )
            .map_err(js_err_message)?;
            let data = crate::crypto::decode_any(&str).map_err(js_err_message)?;
            let plain = symmetric.decrypt(&data).map_err(js_err_message)?;
            Ok(String::from_utf8_lossy(&plain).to_string())
        },
    )
}

fn create_asymmetric_crypto<'js>(ctx: &Ctx<'js>) -> rquickjs::Result<Function<'js>> {
    Function::new(
        ctx.clone(),
        |ctx: Ctx<'js>, transformation: String| -> rquickjs::Result<Object<'js>> {
            let padding =
                crate::asymmetric::RsaPadding::parse(&transformation).map_err(js_err_message)?;
            Ok(crate::asymmetric::asymmetric_object(
                &ctx,
                Rc::new(std::cell::RefCell::new(
                    crate::asymmetric::RsaKeys::default(),
                )),
                padding,
            ))
        },
    )
}

fn create_sign<'js>(ctx: &Ctx<'js>) -> rquickjs::Result<Function<'js>> {
    Function::new(ctx.clone(), |ctx: Ctx<'js>, algorithm: String| {
        crate::asymmetric::create_sign(&ctx, &algorithm)
    })
}

fn digest_hex<'js>(ctx: &Ctx<'js>) -> rquickjs::Result<Function<'js>> {
    Function::new(
        ctx.clone(),
        |data: String, algorithm: String| -> rquickjs::Result<String> {
            let bytes =
                crate::crypto::digest_bytes(&algorithm, data.as_bytes()).map_err(js_err_message)?;
            Ok(hex::encode(bytes))
        },
    )
}

fn hmac<'js>(ctx: &Ctx<'js>, hex_out: bool) -> rquickjs::Result<Function<'js>> {
    Function::new(
        ctx.clone(),
        move |data: String, algorithm: String, key: String| -> rquickjs::Result<String> {
            let bytes = crate::crypto::hmac_bytes(&algorithm, key.as_bytes(), data.as_bytes())
                .map_err(js_err_message)?;
            if hex_out {
                return Ok(hex::encode(bytes));
            }
            use base64::Engine as _;
            Ok(base64::engine::general_purpose::STANDARD.encode(bytes))
        },
    )
}

fn md5_encode16<'js>(ctx: &Ctx<'js>) -> rquickjs::Result<Function<'js>> {
    Function::new(ctx.clone(), |text: String| -> String { md5_hex16(&text) })
}

fn md5_hex16(text: &str) -> String {
    let full = hex::encode(crate::crypto::digest_bytes("md5", text.as_bytes()).unwrap_or_default());
    full.get(8..24).unwrap_or_default().to_string()
}

fn cache_file<'js>(ctx: &Ctx<'js>, host: &Rc<Host>) -> rquickjs::Result<Function<'js>> {
    let host = host.clone();
    Function::new(
        ctx.clone(),
        move |args: rquickjs::function::Rest<JsValue<'js>>| -> rquickjs::Result<String> {
            let url = args.0.first().map(js_text).unwrap_or_default();
            let save_time = args
                .0
                .get(1)
                .map(|value| js_to_f64(value).max(0.0) as u64)
                .unwrap_or(0);
            cache_file_text(&host, &url, save_time).map_err(js_err)
        },
    )
}

fn import_script<'js>(ctx: &Ctx<'js>, host: &Rc<Host>) -> rquickjs::Result<Function<'js>> {
    let host = host.clone();
    Function::new(
        ctx.clone(),
        move |path: String| -> rquickjs::Result<String> {
            if !path.starts_with("http") {
                return Err(js_err(Error::Js(format!(
                    "importScript 只支持 http(s) 链接（本地文件不做）：{path}"
                ))));
            }
            let text = cache_file_text(&host, &path, 0).map_err(js_err)?;
            if text.trim().is_empty() {
                return Err(js_err(Error::Js(format!("{path} 内容获取失败或者为空"))));
            }
            Ok(text)
        },
    )
}

fn cache_file_text(host: &Rc<Host>, url: &str, save_time: u64) -> Result<String> {
    let key = legado_core::CacheNamespace::CacheFile.key(&md5_hex16(url));
    if let Some(store) = host.cache.as_ref()
        && let Ok(Some(cached)) = host.runtime.block_on(store.get(&key))
        && !cached.is_empty()
    {
        return Ok(String::from_utf8_lossy(&cached).into_owned());
    }
    let text = send_with(host, url, None, None, &HashMap::new())?.text;
    if let Some(store) = host.cache.as_ref() {
        let ttl = (save_time > 0).then(|| std::time::Duration::from_secs(save_time));
        let _ = host
            .runtime
            .block_on(store.put(&key, text.as_bytes().to_vec().into(), ttl));
    }
    Ok(text)
}

fn t2s<'js>(ctx: &Ctx<'js>) -> rquickjs::Result<Function<'js>> {
    Function::new(ctx.clone(), |text: JsValue<'js>| -> String {
        zhconv::zhconv(&js_text(&text), zhconv::Variant::ZhHans)
    })
}

fn s2t<'js>(ctx: &Ctx<'js>) -> rquickjs::Result<Function<'js>> {
    Function::new(ctx.clone(), |text: JsValue<'js>| -> String {
        zhconv::zhconv(&js_text(&text), zhconv::Variant::ZhHant)
    })
}

fn html_format<'js>(ctx: &Ctx<'js>) -> rquickjs::Result<Function<'js>> {
    Function::new(ctx.clone(), |html: String| -> String {
        crate::html_format::format_keep_img(&html, None)
    })
}

fn hex_encode<'js>(ctx: &Ctx<'js>) -> rquickjs::Result<Function<'js>> {
    Function::new(ctx.clone(), |text: String| -> String {
        hex::encode(text.as_bytes())
    })
}

fn hex_decode<'js>(ctx: &Ctx<'js>) -> rquickjs::Result<Function<'js>> {
    Function::new(ctx.clone(), |text: String| -> String {
        hex::decode(text.trim())
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
            .unwrap_or_default()
    })
}

fn hex_decode_bytes<'js>(ctx: &Ctx<'js>) -> rquickjs::Result<Function<'js>> {
    Function::new(
        ctx.clone(),
        |ctx: Ctx<'js>, text: String| -> rquickjs::Result<JsValue<'js>> {
            Ok(bytes_to_js(
                &ctx,
                &hex::decode(text.trim()).unwrap_or_default(),
            ))
        },
    )
}

fn md5_encode<'js>(ctx: &Ctx<'js>) -> rquickjs::Result<Function<'js>> {
    use md5::{Digest as _, Md5};
    Function::new(ctx.clone(), |text: String| -> String {
        let mut hasher = Md5::new();
        hasher.update(text.as_bytes());
        hex::encode(hasher.finalize())
    })
}

fn encode_uri<'js>(ctx: &Ctx<'js>) -> rquickjs::Result<Function<'js>> {
    Function::new(ctx.clone(), |text: String| -> String {
        let mut out = String::with_capacity(text.len());
        for byte in text.as_bytes() {
            match byte {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'.' | b'-' | b'*' | b'_' => {
                    out.push(char::from(*byte));
                }
                b' ' => out.push('+'),
                _ => out.push_str(&format!("%{byte:02X}")),
            }
        }
        out
    })
}

fn str_to_bytes<'js>(ctx: &Ctx<'js>) -> rquickjs::Result<Function<'js>> {
    Function::new(ctx.clone(), |ctx: Ctx<'js>, text: String| -> JsValue<'js> {
        bytes_to_js(&ctx, text.as_bytes())
    })
}

fn bytes_to_str<'js>(ctx: &Ctx<'js>) -> rquickjs::Result<Function<'js>> {
    Function::new(ctx.clone(), |value: JsValue<'js>| -> String {
        String::from_utf8_lossy(&js_bytes(&value)).into_owned()
    })
}

pub(crate) fn js_to_f64(value: &JsValue<'_>) -> f64 {
    if let Some(number) = value.as_number() {
        return number;
    }
    if let Some(int) = value.as_int() {
        return f64::from(int);
    }
    if let Some(text) = value.as_string().and_then(|text| text.to_string().ok()) {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return 0.0;
        }
        return trimmed.parse::<f64>().unwrap_or(f64::NAN);
    }
    if let Some(flag) = value.as_bool() {
        return if flag { 1.0 } else { 0.0 };
    }
    if value.is_null() {
        return 0.0;
    }
    f64::NAN
}

fn js_truthy(value: &JsValue<'_>) -> bool {
    if let Some(flag) = value.as_bool() {
        return flag;
    }
    if value.is_null() || value.is_undefined() {
        return false;
    }
    if let Some(number) = value.as_number() {
        return number != 0.0 && !number.is_nan();
    }
    if let Some(text) = value.as_string() {
        return text
            .to_string()
            .map(|text| !text.is_empty())
            .unwrap_or(false);
    }
    true
}

fn time_format<'js>(ctx: &Ctx<'js>) -> rquickjs::Result<Function<'js>> {
    Function::new(
        ctx.clone(),
        |millis: JsValue<'_>| -> rquickjs::Result<String> {
            use chrono::{Local, TimeZone as _};
            let millis = js_to_f64(&millis);
            if !millis.is_finite() {
                return Err(js_err_message(
                    "java.timeFormat: Cannot convert NaN to java.lang.Long".into(),
                ));
            }
            Ok(match Local.timestamp_millis_opt(millis as i64) {
                chrono::LocalResult::Single(time) => time.format("%Y/%m/%d %H:%M").to_string(),
                _ => String::new(),
            })
        },
    )
}

fn time_format_utc<'js>(ctx: &Ctx<'js>) -> rquickjs::Result<Function<'js>> {
    Function::new(
        ctx.clone(),
        |millis: JsValue<'_>, format: String, offset_millis: i32| -> rquickjs::Result<String> {
            let millis = js_to_f64(&millis);
            if !millis.is_finite() {
                return Err(js_err_message(
                    "java.timeFormatUTC: Cannot convert NaN to java.lang.Long".into(),
                ));
            }
            crate::date_format::format(millis as i64, offset_millis, &format)
                .map_err(js_err_message)
        },
    )
}

fn to_num_chapter<'js>(ctx: &Ctx<'js>) -> rquickjs::Result<Function<'js>> {
    Function::new(
        ctx.clone(),
        |ctx: Ctx<'js>, value: Opt<JsValue<'js>>| -> JsValue<'js> {
            let text = match value.0.as_ref() {
                Some(value) if !value.is_null() && !value.is_undefined() => js_text(value),
                _ => return JsValue::new_null(ctx),
            };
            match rquickjs::String::from_str(ctx.clone(), &to_num_chapter_text(&text)) {
                Ok(text) => text.into_value(),
                Err(_) => JsValue::new_null(ctx),
            }
        },
    )
}

fn to_num_chapter_text(text: &str) -> String {
    static TITLE_NUM: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r"(第)([^\n\r\u{0085}\u{2028}\u{2029}]+?)(章)").expect("章节号正则")
    });
    match TITLE_NUM.captures(text) {
        Some(caps) => format!("{}{}{}", &caps[1], string_to_int(&caps[2]), &caps[3]),
        None => text.to_string(),
    }
}

fn string_to_int(text: &str) -> i32 {
    let number: String = full_to_half(text)
        .chars()
        .filter(|ch| !is_java_space(*ch))
        .collect();
    match parse_decimal_int(&number) {
        Some(value) => value,
        None => chinese_num_to_int(&number),
    }
}

fn is_java_space(ch: char) -> bool {
    matches!(ch, ' ' | '\t' | '\n' | '\u{000B}' | '\u{000C}' | '\r')
}

fn full_to_half(text: &str) -> String {
    text.chars()
        .map(|ch| match ch as u32 {
            12_288 => ' ',
            code @ 65_281..=65_374 => char::from_u32(code - 65_248).unwrap_or(ch),
            _ => ch,
        })
        .collect()
}

fn parse_decimal_int(text: &str) -> Option<i32> {
    let (negative, digits) = match text.as_bytes().first() {
        Some(b'-') => (true, &text[1..]),
        Some(b'+') => (false, &text[1..]),
        _ => (false, text),
    };
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let mut value: i64 = 0;
    for byte in digits.bytes() {
        value = value.checked_mul(10)?.checked_add(i64::from(byte - b'0'))?;
        if value > i64::from(i32::MAX) + 1 {
            return None;
        }
    }
    i32::try_from(if negative { -value } else { value }).ok()
}

fn chinese_num_to_int(text: &str) -> i32 {
    let chars: Vec<char> = text.chars().collect();
    let mut result: i32 = 0;
    let mut tmp: i32 = 0;
    let mut billion: i32 = 0;
    for (index, ch) in chars.iter().enumerate() {
        let Some(tmp_num) = chinese_num(*ch) else {
            return -1;
        };
        if tmp_num == 100_000_000 {
            result = result.wrapping_add(tmp).wrapping_mul(tmp_num);
            billion = billion.wrapping_mul(100_000_000).wrapping_add(result);
            result = 0;
            tmp = 0;
        } else if tmp_num == 10_000 {
            result = result.wrapping_add(tmp).wrapping_mul(tmp_num);
            tmp = 0;
        } else if tmp_num >= 10 {
            if tmp == 0 {
                tmp = 1;
            }
            result = result.wrapping_add(tmp_num.wrapping_mul(tmp));
            tmp = 0;
        } else if index >= 2 && index == chars.len() - 1 {
            let Some(previous) = chinese_num(chars[index - 1]) else {
                return -1;
            };
            tmp = if previous > 10 {
                tmp_num.wrapping_mul(previous) / 10
            } else {
                tmp.wrapping_mul(10).wrapping_add(tmp_num)
            };
        } else {
            tmp = tmp.wrapping_mul(10).wrapping_add(tmp_num);
        }
    }
    result.wrapping_add(tmp).wrapping_add(billion)
}

fn chinese_num(ch: char) -> Option<i32> {
    Some(match ch {
        '零' | '〇' => 0,
        '一' | '壹' => 1,
        '二' | '贰' | '两' => 2,
        '三' | '叁' => 3,
        '四' | '肆' => 4,
        '五' | '伍' => 5,
        '六' | '陆' => 6,
        '七' | '柒' => 7,
        '八' | '捌' => 8,
        '九' | '玖' => 9,
        '十' | '拾' => 10,
        '百' | '佰' => 100,
        '千' | '仟' => 1000,
        '万' => 10_000,
        '亿' => 100_000_000,
        _ => return None,
    })
}

fn random_uuid<'js>(ctx: &Ctx<'js>) -> rquickjs::Result<Function<'js>> {
    Function::new(ctx.clone(), || -> String {
        uuid::Uuid::new_v4().to_string()
    })
}

fn android_id<'js>(ctx: &Ctx<'js>, host: &Rc<Host>) -> rquickjs::Result<Function<'js>> {
    let id = host.android_id.clone();
    Function::new(ctx.clone(), move || -> String { id.clone() })
}

fn log<'js>(ctx: &Ctx<'js>) -> rquickjs::Result<Function<'js>> {
    Function::new(
        ctx.clone(),
        |ctx: Ctx<'js>, values: rquickjs::function::Rest<JsValue<'js>>| -> JsValue<'js> {
            let text = values.0.iter().map(js_text).collect::<Vec<_>>().join(" ");
            tracing::info!("[js] {text}");
            values
                .0
                .into_iter()
                .next()
                .unwrap_or_else(|| JsValue::new_undefined(ctx.clone()))
        },
    )
}

fn toast<'js>(ctx: &Ctx<'js>, name: &'static str) -> rquickjs::Result<Function<'js>> {
    Function::new(ctx.clone(), move |value: JsValue<'js>| {
        let text = js_text(&value);
        if name == "longToast" {
            tracing::warn!("[js 提示] {text}");
        } else {
            tracing::info!("[js 提示] {text}");
        }
    })
}

pub(crate) fn bytes_to_js<'js>(ctx: &Ctx<'js>, bytes: &[u8]) -> JsValue<'js> {
    match Array::new(ctx.clone()) {
        Ok(array) => {
            for (index, byte) in bytes.iter().enumerate() {
                let _ = array.set(index, *byte);
            }
            array.into_value()
        }
        Err(_) => JsValue::new_null(ctx.clone()),
    }
}

pub(crate) fn js_bytes(value: &JsValue<'_>) -> Vec<u8> {
    if let Some(array) = value.as_array() {
        return (0..array.len())
            .filter_map(|index| array.get::<u8>(index).ok())
            .collect();
    }
    js_text(value).into_bytes()
}

pub(crate) fn js_text(value: &JsValue<'_>) -> String {
    if let Some(text) = value.as_string()
        && let Ok(text) = text.to_string()
    {
        return text;
    }
    if value.is_null() || value.is_undefined() {
        return String::new();
    }
    if let Some(number) = value.as_number() {
        return if number.fract() == 0.0 {
            format!("{}", number as i64)
        } else {
            number.to_string()
        };
    }
    if let Some(value) = value.as_bool() {
        return value.to_string();
    }
    if is_java_list(value) {
        return java_list_to_string(&java_list_items(value).unwrap_or_default());
    }
    if let Some(array) = value.as_array() {
        return (0..array.len())
            .filter_map(|index| array.get::<JsValue<'_>>(index).ok())
            .map(|item| js_text(&item))
            .collect::<Vec<_>>()
            .join("\n");
    }
    if let Some(object) = value.as_object()
        && let Ok(function) = object.get::<_, Function<'_>>("outerHtml")
        && let Ok(text) = function.call::<_, JsValue<'_>>(())
        && let Some(text) = text.as_string().and_then(|text| text.to_string().ok())
        && !text.is_empty()
    {
        return text;
    }
    String::new()
}

fn string_map(value: &JsValue<'_>) -> HashMap<String, String> {
    let mut map = HashMap::new();
    if let Some(text) = value.as_string().and_then(|text| text.to_string().ok()) {
        if let Ok(serde_json::Value::Object(fields)) = serde_json::from_str(&text) {
            for (key, value) in fields {
                map.insert(key, json_text(&value));
            }
        }
        return map;
    }
    if let Some(object) = value.as_object() {
        for entry in object.props::<String, JsValue<'_>>().flatten() {
            map.insert(entry.0, js_text(&entry.1));
        }
    }
    map
}
