use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use std::collections::HashMap;

use legado_core::{
    Book, BookChapter, CacheStore, CookieStore, HttpFetcher, JsVariables, RuleRunner, Source,
};
use rquickjs::function::Opt;
use rquickjs::{Ctx, Function, Object, Value as JsValue};

pub(crate) struct Binding {
    pub result: String,
    pub result_items: Option<Vec<String>>,
    pub src: String,
    pub content: String,
    pub base_url: String,
    pub page: Option<i32>,
    pub key: Option<String>,
    pub source: Option<Box<dyn Source>>,
    pub book: Option<Book>,
    pub chapter: Option<BookChapter>,
    pub variables: JsVariables,
    pub response: Option<legado_core::HttpResponse>,
}

impl Binding {
    pub fn from_request(
        request: &legado_core::JsRequest<'_>,
        result_items: Option<&[String]>,
    ) -> Self {
        let result_items = result_items.map(<[String]>::to_vec);
        let result = match &result_items {
            Some(items) => legado_core::java_list_to_string(items),
            None => request.result.unwrap_or_default().to_string(),
        };
        let src = request.src.unwrap_or_default().to_string();
        Self {
            src: src.clone(),
            content: src,
            result,
            result_items,
            base_url: request.base_url.to_string(),
            page: request.page,
            key: request.key.map(str::to_string),
            source: request.source.map(Source::to_owned_source),
            book: request.book.cloned(),
            chapter: request.chapter.cloned(),
            variables: request
                .variables
                .as_ref()
                .map(|variables| (*variables).clone())
                .unwrap_or_default(),
            response: request.response.cloned(),
        }
    }
}

fn to_json<T: serde::Serialize>(value: &T) -> Option<serde_json::Value> {
    serde_json::to_value(value).ok()
}

pub(crate) struct Host {
    pub http: Option<Arc<dyn HttpFetcher>>,
    pub cache: Option<Arc<dyn CacheStore>>,
    pub cookies: Option<Arc<dyn CookieStore>>,
    pub rules: Option<Arc<dyn RuleRunner>>,
    pub android_id: String,
    pub runtime: tokio::runtime::Runtime,
    current: RefCell<Option<Binding>>,
    fonts: RefCell<Vec<Rc<crate::ttf::Font>>>,
}

impl Host {
    pub fn new(deps: crate::worker::Deps, android_id: String) -> std::io::Result<Rc<Self>> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        Ok(Rc::new(Self {
            http: deps.http,
            cache: deps.cache,
            cookies: deps.cookies,
            rules: deps.rules,
            android_id,
            runtime,
            current: RefCell::new(None),
            fonts: RefCell::new(Vec::new()),
        }))
    }

    pub fn register_font(&self, font: Rc<crate::ttf::Font>) -> usize {
        let mut fonts = self.fonts.borrow_mut();
        fonts.push(font);
        fonts.len() - 1
    }

    pub fn font(&self, id: usize) -> Option<Rc<crate::ttf::Font>> {
        self.fonts.borrow().get(id).cloned()
    }

    pub fn set_binding(&self, binding: Option<Binding>) {
        *self.current.borrow_mut() = binding;
    }

    pub fn take_binding(&self) -> Option<Binding> {
        self.current.borrow_mut().take()
    }

    pub fn with_binding<T>(&self, f: impl FnOnce(Option<&Binding>) -> T) -> T {
        let current = self.current.borrow();
        f(current.as_ref())
    }

    pub fn with_binding_mut<T>(&self, f: impl FnOnce(Option<&mut Binding>) -> T) -> T {
        let mut current = self.current.borrow_mut();
        f(current.as_mut())
    }

    pub fn take_variables(&self) -> JsVariables {
        self.with_binding_mut(|binding| {
            binding
                .map(|binding| std::mem::take(&mut binding.variables))
                .unwrap_or_default()
        })
    }
}

pub(crate) fn json_to_js<'js>(
    ctx: &Ctx<'js>,
    value: &serde_json::Value,
) -> rquickjs::Result<JsValue<'js>> {
    Ok(match value {
        serde_json::Value::Null => JsValue::new_null(ctx.clone()),
        serde_json::Value::Bool(value) => JsValue::new_bool(ctx.clone(), *value),
        serde_json::Value::Number(number) => {
            if let Some(value) = number.as_i64() {
                JsValue::new_int(ctx.clone(), value as i32)
            } else if let Some(value) = number.as_f64() {
                JsValue::new_float(ctx.clone(), value)
            } else {
                JsValue::new_null(ctx.clone())
            }
        }
        serde_json::Value::String(text) => {
            rquickjs::String::from_str(ctx.clone(), text)?.into_value()
        }
        serde_json::Value::Array(items) => {
            let array = rquickjs::Array::new(ctx.clone())?;
            for (index, item) in items.iter().enumerate() {
                array.set(index, json_to_js(ctx, item)?)?;
            }
            array.into_value()
        }
        serde_json::Value::Object(map) => {
            let object = Object::new(ctx.clone())?;
            for (key, item) in map {
                object.set(key.as_str(), json_to_js(ctx, item)?)?;
            }
            object.into_value()
        }
    })
}

pub(crate) fn set_fields<'js>(
    object: &Object<'js>,
    value: &serde_json::Value,
) -> rquickjs::Result<()> {
    let ctx = object.ctx().clone();
    if let serde_json::Value::Object(map) = value {
        for (key, item) in map {
            object.set(key.as_str(), json_to_js(&ctx, item)?)?;
        }
    }
    Ok(())
}

pub(crate) fn map_object<'js>(
    ctx: &Ctx<'js>,
    map: &HashMap<String, String>,
) -> rquickjs::Result<Object<'js>> {
    let object = Object::new(ctx.clone())?;
    for (key, value) in map {
        object.set(key.as_str(), value.as_str())?;
    }
    let get = rquickjs::Function::new(ctx.clone(), {
        let map = map.clone();
        move |key: String| -> Option<String> { map.get(&key).cloned() }
    })?;
    object.set("get", get)?;
    Ok(object)
}

pub(crate) fn install_static(ctx: &Ctx<'_>, host: &Rc<Host>) -> rquickjs::Result<()> {
    let globals = ctx.globals();
    globals.set("java", crate::java::install(ctx, host)?)?;
    globals.set("cookie", cookie_object(ctx, host)?)?;
    globals.set("cache", cache_object(ctx, host)?)?;
    crate::packages::install(ctx, host)?;
    Ok(())
}

pub(crate) fn bind_eval(ctx: &Ctx<'_>, host: &Rc<Host>) -> rquickjs::Result<()> {
    let globals = ctx.globals();
    host.with_binding(|binding| -> rquickjs::Result<()> {
        let Some(binding) = binding else {
            return Ok(());
        };
        match &binding.response {
            Some(response) => {
                globals.set("result", crate::java::response_object(ctx, response)?)?
            }
            None => match &binding.result_items {
                Some(items) => {
                    globals.set("result", crate::java::string_list(ctx, items.clone())?)?
                }
                None => globals.set("result", binding.result.as_str())?,
            },
        }
        globals.set("baseUrl", binding.base_url.as_str())?;
        globals.set("src", binding.src.as_str())?;
        match binding.page {
            Some(page) => globals.set("page", page)?,
            None => globals.set("page", JsValue::new_null(ctx.clone()))?,
        }
        match &binding.key {
            Some(key) => globals.set("key", key.as_str())?,
            None => globals.set("key", JsValue::new_null(ctx.clone()))?,
        }

        let source = Object::new(ctx.clone())?;
        if let Some(model) = &binding.source {
            set_fields(&source, &model.to_json())?;
        }
        source_methods(
            ctx,
            host,
            &source,
            binding.source.as_ref().map(|model| model.key()),
        )?;
        globals.set("source", source)?;

        match &binding.book {
            Some(model) => {
                let book = Object::new(ctx.clone())?;
                set_fields(&book, &to_json(model).unwrap_or(serde_json::Value::Null))?;
                book_methods(ctx, host, &book)?;
                globals.set("book", book)?;
            }
            None => globals.set("book", JsValue::new_null(ctx.clone()))?,
        }

        match &binding.chapter {
            Some(model) => {
                let chapter = Object::new(ctx.clone())?;
                set_fields(&chapter, &to_json(model).unwrap_or(serde_json::Value::Null))?;
                chapter_methods(ctx, host, &chapter)?;
                globals.set("chapter", chapter)?;
            }
            None => globals.set("chapter", JsValue::new_null(ctx.clone()))?,
        }
        Ok(())
    })
}

fn source_methods<'js>(
    ctx: &Ctx<'js>,
    host: &Rc<Host>,
    object: &Object<'js>,
    key: Option<&str>,
) -> rquickjs::Result<()> {
    let source_key = key.unwrap_or_default().to_string();
    let source_key = Rc::new(source_key);

    object.set("getKey", {
        let key = source_key.clone();
        Function::new(ctx.clone(), move || -> String { (*key).clone() })?
    })?;
    object.set("getSource", JsValue::new_null(ctx.clone()))?;
    object.set("getLoginHeader", {
        let host = host.clone();
        let key = source_key.clone();
        Function::new(ctx.clone(), move || -> String {
            cache_get(&host, &format!("loginHeader_{}", *key)).unwrap_or_default()
        })?
    })?;
    object.set("getLoginHeaderMap", {
        let host = host.clone();
        let key = source_key.clone();
        Function::new(
            ctx.clone(),
            move |ctx: Ctx<'js>| -> rquickjs::Result<Object<'js>> {
                let text = cache_get(&host, &format!("loginHeader_{}", *key)).unwrap_or_default();
                let map: HashMap<String, String> = serde_json::from_str(&text).unwrap_or_default();
                map_object(&ctx, &map)
            },
        )?
    })?;
    object.set("putLoginHeader", {
        let host = host.clone();
        let key = source_key.clone();
        Function::new(ctx.clone(), move |header: JsValue<'js>| {
            let text = crate::java::js_text(&header);
            if let Ok(map) = serde_json::from_str::<HashMap<String, String>>(&text) {
                let cookie = map
                    .get("Cookie")
                    .or_else(|| map.get("cookie"))
                    .cloned()
                    .unwrap_or_default();
                if !cookie.is_empty()
                    && let Some(store) = host.cookies.as_ref()
                {
                    store.replace_cookie(&key, &cookie);
                }
            }
            cache_put(&host, &format!("loginHeader_{}", *key), &text);
        })
    })?;
    object.set("getLoginInfo", {
        let host = host.clone();
        let key = source_key.clone();
        Function::new(ctx.clone(), move || -> String {
            cache_get(&host, &format!("userInfo_{}", *key)).unwrap_or_default()
        })
    })?;
    object.set("getLoginInfoMap", {
        let host = host.clone();
        let key = source_key.clone();
        Function::new(
            ctx.clone(),
            move |ctx: Ctx<'js>| -> rquickjs::Result<Object<'js>> {
                let text = cache_get(&host, &format!("userInfo_{}", *key)).unwrap_or_default();
                let map: HashMap<String, String> = serde_json::from_str(&text).unwrap_or_default();
                map_object(&ctx, &map)
            },
        )?
    })?;
    object.set("getVariable", {
        let host = host.clone();
        let key = source_key.clone();
        Function::new(ctx.clone(), move |name: Opt<String>| -> String {
            let text = cache_get(&host, &format!("sourceVariable_{}", *key)).unwrap_or_default();
            match name.0 {
                None => text,
                Some(name) => serde_json::from_str::<HashMap<String, String>>(&text)
                    .ok()
                    .and_then(|map| map.get(&name).cloned())
                    .unwrap_or_default(),
            }
        })
    })?;
    object.set("setVariable", {
        let host = host.clone();
        let key = source_key.clone();
        Function::new(ctx.clone(), move |value: JsValue<'js>| {
            cache_put(
                &host,
                &format!("sourceVariable_{}", *key),
                &crate::java::js_text(&value),
            );
        })
    })?;
    Ok(())
}

fn chapter_methods<'js>(
    ctx: &Ctx<'js>,
    host: &Rc<Host>,
    object: &Object<'js>,
) -> rquickjs::Result<()> {
    object.set("putVariable", {
        let host = host.clone();
        Function::new(
            ctx.clone(),
            move |key: String, value: Opt<JsValue<'js>>| -> bool {
                let value = match value.0.as_ref() {
                    Some(value) if !value.is_null() && !value.is_undefined() => {
                        Some(crate::java::js_text(value))
                    }
                    _ => None,
                };
                if let Some(value) = &value
                    && value.encode_utf16().count() >= 10_000
                {
                    return true;
                }
                host.with_binding_mut(|binding| {
                    if let Some(binding) = binding {
                        binding.variables.put_chapter(&key, value.as_deref());
                    }
                });
                true
            },
        )?
    })?;
    Ok(())
}

fn book_methods<'js>(
    ctx: &Ctx<'js>,
    host: &Rc<Host>,
    object: &Object<'js>,
) -> rquickjs::Result<()> {
    object.set("getName", {
        let object = object.clone();
        Function::new(ctx.clone(), move || -> String {
            object.get::<_, String>("name").unwrap_or_default()
        })?
    })?;
    object.set("getVariableMap", {
        let host = host.clone();
        Function::new(
            ctx.clone(),
            move |ctx: Ctx<'js>| -> rquickjs::Result<Object<'js>> {
                let map = host.with_binding(|binding| {
                    binding.map(|b| b.variables.clone()).unwrap_or_default()
                });
                let mut flat = HashMap::new();
                if let Some(book) = map.book() {
                    flat.extend(book.clone());
                }
                if let Some(chapter) = map.chapter() {
                    flat.extend(chapter.clone());
                }
                map_object(&ctx, &flat)
            },
        )?
    })?;
    Ok(())
}

fn cookie_object<'js>(ctx: &Ctx<'js>, host: &Rc<Host>) -> rquickjs::Result<Object<'js>> {
    let object = Object::new(ctx.clone())?;
    object.set("getCookie", {
        let host = host.clone();
        Function::new(ctx.clone(), move |url: String| -> String {
            host.cookies
                .as_ref()
                .map(|store| store.get_cookie(&url))
                .unwrap_or_default()
        })?
    })?;
    object.set("setCookie", {
        let host = host.clone();
        Function::new(ctx.clone(), move |url: String, cookie: String| {
            if let Some(store) = host.cookies.as_ref() {
                store.set_cookie(&url, &cookie);
            }
        })?
    })?;
    object.set("removeCookie", {
        let host = host.clone();
        Function::new(ctx.clone(), move |url: String| {
            if let Some(store) = host.cookies.as_ref() {
                store.remove_cookie(&url);
            }
        })?
    })?;
    object.set("getKey", {
        let host = host.clone();
        Function::new(ctx.clone(), move |url: String, key: String| -> String {
            let cookie = host
                .cookies
                .as_ref()
                .map(|store| store.get_cookie(&url))
                .unwrap_or_default();
            cookie_map(&cookie).remove(&key).unwrap_or_default()
        })?
    })?;
    Ok(object)
}

pub(crate) fn cookie_map(cookie: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    if cookie.trim().is_empty() {
        return map;
    }
    for pair in cookie.split(';') {
        let mut parts = pair.splitn(2, '=');
        let key = parts.next().unwrap_or_default().trim();
        let Some(value) = parts.next() else {
            continue;
        };
        let value = value.trim();
        if !value.is_empty() || value == "null" {
            map.insert(key.to_string(), value.to_string());
        }
    }
    map
}

fn cache_object<'js>(ctx: &Ctx<'js>, host: &Rc<Host>) -> rquickjs::Result<Object<'js>> {
    let object = Object::new(ctx.clone())?;
    object.set("put", {
        let host = host.clone();
        Function::new(ctx.clone(), move |key: String, value: JsValue<'js>| {
            cache_put(&host, &key, &crate::java::js_text(&value));
        })?
    })?;
    object.set("get", {
        let host = host.clone();
        Function::new(ctx.clone(), move |key: String| -> String {
            cache_get(&host, &key).unwrap_or_default()
        })?
    })?;
    object.set("delete", {
        let host = host.clone();
        Function::new(ctx.clone(), move |key: String| {
            cache_delete(&host, &key);
        })?
    })?;
    for name in ["putMemory", "putFile"] {
        object.set(name, {
            let host = host.clone();
            Function::new(ctx.clone(), move |key: String, value: JsValue<'js>| {
                cache_put(&host, &key, &crate::java::js_text(&value));
            })?
        })?;
    }
    for name in ["getFromMemory", "getFile"] {
        object.set(name, {
            let host = host.clone();
            Function::new(ctx.clone(), move |key: String| -> String {
                cache_get(&host, &key).unwrap_or_default()
            })?
        })?;
    }
    for name in ["deleteMemory", "deleteFile"] {
        object.set(name, {
            let host = host.clone();
            Function::new(ctx.clone(), move |key: String| {
                cache_delete(&host, &key);
            })?
        })?;
    }
    Ok(object)
}

fn cache_key(key: &str) -> String {
    legado_core::CacheNamespace::JsCache.key(key)
}

fn cache_get(host: &Rc<Host>, key: &str) -> Option<String> {
    let store = host.cache.as_ref()?;
    let key = cache_key(key);
    let bytes = host.runtime.block_on(store.get(&key)).ok()??;
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

fn cache_put(host: &Rc<Host>, key: &str, value: &str) {
    let Some(store) = host.cache.as_ref() else {
        return;
    };
    let key = cache_key(key);
    let _ = host
        .runtime
        .block_on(store.put(&key, value.as_bytes().to_vec().into(), None));
}

fn cache_delete(host: &Rc<Host>, key: &str) {
    let Some(store) = host.cache.as_ref() else {
        return;
    };
    let key = cache_key(key);
    let _ = host.runtime.block_on(store.delete(&key));
}
