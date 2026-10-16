use std::cell::RefCell;
use std::rc::{Rc, Weak};

use rquickjs::function::{Opt, Rest};
use rquickjs::{Array, Ctx, Function, Object, Value as JsValue};

use crate::host::Host;
use crate::java::{js_err, js_text, response_object, send, send_body};
use crate::rules;

pub(crate) struct DocState {
    html: String,
    views: Vec<Weak<RefCell<Vec<String>>>>,
}

pub(crate) type Doc = Rc<RefCell<DocState>>;

const WRAPPER_OPEN: &str = "<div data-legado-el>";
const WRAPPED_TEXT: &str = "[data-legado-el] > *@text";
const WRAPPED_INNER: &str = "[data-legado-el] > *@__innerHtml";

fn wrap(html: &str) -> String {
    format!("{WRAPPER_OPEN}{html}</div>")
}

fn has_class(host: &Rc<Host>, html: &str, name: &str) -> bool {
    let rule = "[data-legado-el] > *@class";
    let wanted = name.to_lowercase();
    rules::run_string(host, rule, Some(wrap(html)), true)
        .unwrap_or_default()
        .split_whitespace()
        .any(|token| token.to_lowercase() == wanted)
}

pub(crate) fn class<'js>(ctx: &Ctx<'js>, host: &Rc<Host>) -> rquickjs::Result<Object<'js>> {
    let object = Object::new(ctx.clone())?;
    object.prop(
        "parse",
        Function::new(ctx.clone(), {
            let host = host.clone();
            move |ctx: Ctx<'js>, html: JsValue<'js>| -> Object<'js> {
                document(&ctx, &host, js_text(&html))
            }
        })?,
    )?;
    object.prop(
        "connect",
        Function::new(ctx.clone(), {
            let host = host.clone();
            move |ctx: Ctx<'js>, url: String| -> Object<'js> { connection(&ctx, &host, url) }
        })?,
    )?;
    Ok(object)
}

fn connection<'js>(ctx: &Ctx<'js>, host: &Rc<Host>, url: String) -> Object<'js> {
    let object = Object::new(ctx.clone()).expect("建 connection 对象");
    for name in [
        "timeout",
        "header",
        "userAgent",
        "ignoreContentType",
        "ignoreHttpErrors",
        "followRedirects",
        "maxBodySize",
        "data",
        "referrer",
        "cookie",
        "validateTLSCertificates",
        "parser",
        "method",
    ] {
        let ret = object.clone();
        object
            .set(
                name,
                Function::new(ctx.clone(), move |_args: Rest<JsValue<'_>>| ret.clone())
                    .expect("建链式方法"),
            )
            .expect("挂链式方法");
    }
    {
        let host = host.clone();
        let url = url.clone();
        object
            .prop(
                "get",
                Function::new(
                    ctx.clone(),
                    move |ctx: Ctx<'js>| -> rquickjs::Result<JsValue<'js>> {
                        let response = send(&host, &url, None).map_err(js_err)?;
                        Ok(rquickjs::String::from_str(ctx.clone(), &response.text)?.into_value())
                    },
                )
                .expect("建 get"),
            )
            .expect("挂 get");
    }
    {
        let host = host.clone();
        let url = url.clone();
        object
            .prop(
                "post",
                Function::new(
                    ctx.clone(),
                    move |ctx: Ctx<'js>, body: Opt<String>| -> rquickjs::Result<JsValue<'js>> {
                        let response =
                            send_body(&host, &url, body.0, &Default::default()).map_err(js_err)?;
                        Ok(rquickjs::String::from_str(ctx.clone(), &response.text)?.into_value())
                    },
                )
                .expect("建 post"),
            )
            .expect("挂 post");
    }
    {
        let host = host.clone();
        let url = url.clone();
        object
            .prop(
                "execute",
                Function::new(
                    ctx.clone(),
                    move |ctx: Ctx<'js>| -> rquickjs::Result<JsValue<'js>> {
                        let response = send(&host, &url, None).map_err(js_err)?;
                        response_object(&ctx, &response)
                    },
                )
                .expect("建 execute"),
            )
            .expect("挂 execute");
    }
    object
}

pub(crate) fn shared_document(html: String) -> Doc {
    Rc::new(RefCell::new(DocState {
        html,
        views: Vec::new(),
    }))
}

pub(crate) fn document<'js>(ctx: &Ctx<'js>, host: &Rc<Host>, html: String) -> Object<'js> {
    let doc = shared_document(html);
    let object = Object::new(ctx.clone()).expect("建文档对象");

    object
        .prop(
            "select",
            Function::new(ctx.clone(), {
                let host = host.clone();
                let doc = doc.clone();
                move |ctx: Ctx<'js>, selector: String| -> Array<'js> {
                    let html = doc.borrow().html.clone();
                    let items =
                        rules::run_elements(&host, &selector, Some(html)).unwrap_or_default();
                    elements(&ctx, &host, doc.clone(), items)
                }
            })
            .expect("建 select"),
        )
        .expect("挂 select");
    object
        .prop(
            "selectFirst",
            Function::new(ctx.clone(), {
                let host = host.clone();
                let doc = doc.clone();
                move |ctx: Ctx<'js>, selector: String| -> JsValue<'js> {
                    let html = doc.borrow().html.clone();
                    let items =
                        rules::run_elements(&host, &selector, Some(html)).unwrap_or_default();
                    first(&ctx, &host, doc.clone(), items)
                }
            })
            .expect("建 selectFirst"),
        )
        .expect("挂 selectFirst");
    object
        .prop(
            "text",
            Function::new(ctx.clone(), {
                let host = host.clone();
                let doc = doc.clone();
                move || -> String {
                    let html = doc.borrow().html.clone();
                    rules::run_string(&host, "@text", Some(html), true).unwrap_or_default()
                }
            })
            .expect("建 text"),
        )
        .expect("挂 text");
    object
        .prop(
            "html",
            Function::new(ctx.clone(), {
                let host = host.clone();
                let doc = doc.clone();
                move || -> String {
                    let html = doc.borrow().html.clone();
                    rules::run_string(&host, "@__innerHtml", Some(html), true).unwrap_or_default()
                }
            })
            .expect("建 html"),
        )
        .expect("挂 html");
    let outer = Function::new(ctx.clone(), {
        let doc = doc.clone();
        move || -> String { doc.borrow().html.clone() }
    })
    .expect("建 outerHtml");
    object
        .prop("outerHtml", outer.clone())
        .expect("挂 outerHtml");
    object.prop("toString", outer).expect("挂 toString");
    object
}

fn first<'js>(ctx: &Ctx<'js>, host: &Rc<Host>, doc: Doc, items: Vec<String>) -> JsValue<'js> {
    match items.into_iter().next() {
        Some(html) => element(ctx, host, doc, &html).into_value(),
        None => JsValue::new_null(ctx.clone()),
    }
}

pub(crate) fn elements<'js>(
    ctx: &Ctx<'js>,
    host: &Rc<Host>,
    doc: Doc,
    items: Vec<String>,
) -> Array<'js> {
    let items = Rc::new(RefCell::new(items));
    {
        let mut state = doc.borrow_mut();
        state.views.retain(|view| view.strong_count() > 0);
        state.views.push(Rc::downgrade(&items));
    }

    let array = Array::new(ctx.clone()).expect("建元素数组");
    for (index, html) in items.borrow().iter().enumerate() {
        let _ = array.set(index, element(ctx, host, doc.clone(), html));
    }
    let props = array.as_object();

    props
        .prop(
            "size",
            Function::new(ctx.clone(), {
                let items = items.clone();
                move || -> usize { items.borrow().len() }
            })
            .expect("建 size"),
        )
        .expect("挂 size");
    props
        .prop(
            "isEmpty",
            Function::new(ctx.clone(), {
                let items = items.clone();
                move || -> bool { items.borrow().is_empty() }
            })
            .expect("建 isEmpty"),
        )
        .expect("挂 isEmpty");
    props
        .prop(
            "first",
            Function::new(ctx.clone(), {
                let host = host.clone();
                let doc = doc.clone();
                let items = items.clone();
                move |ctx: Ctx<'js>| -> JsValue<'js> {
                    first(&ctx, &host, doc.clone(), items.borrow().clone())
                }
            })
            .expect("建 first"),
        )
        .expect("挂 first");
    props
        .prop(
            "last",
            Function::new(ctx.clone(), {
                let host = host.clone();
                let doc = doc.clone();
                let items = items.clone();
                move |ctx: Ctx<'js>| -> JsValue<'js> {
                    let last = items.borrow().last().cloned().into_iter().collect();
                    first(&ctx, &host, doc.clone(), last)
                }
            })
            .expect("建 last"),
        )
        .expect("挂 last");
    props
        .prop(
            "get",
            Function::new(ctx.clone(), {
                let host = host.clone();
                let doc = doc.clone();
                let items = items.clone();
                move |ctx: Ctx<'js>, index: usize| -> JsValue<'js> {
                    let one = items.borrow().get(index).cloned().into_iter().collect();
                    first(&ctx, &host, doc.clone(), one)
                }
            })
            .expect("建 get"),
        )
        .expect("挂 get");
    props
        .prop(
            "toArray",
            Function::new(ctx.clone(), {
                let array = array.clone();
                move || -> Array<'js> { array.clone() }
            })
            .expect("建 toArray"),
        )
        .expect("挂 toArray");
    props
        .prop(
            "text",
            Function::new(ctx.clone(), {
                let host = host.clone();
                let items = items.clone();
                move || -> String {
                    items
                        .borrow()
                        .iter()
                        .map(|html| {
                            rules::run_string(&host, WRAPPED_TEXT, Some(wrap(html)), true)
                                .unwrap_or_default()
                        })
                        .filter(|text| !text.is_empty())
                        .collect::<Vec<_>>()
                        .join(" ")
                }
            })
            .expect("建 text"),
        )
        .expect("挂 text");
    props
        .prop(
            "html",
            Function::new(ctx.clone(), {
                let host = host.clone();
                let items = items.clone();
                move || -> String {
                    items
                        .borrow()
                        .iter()
                        .map(|html| {
                            rules::run_string(&host, WRAPPED_INNER, Some(wrap(html)), true)
                                .unwrap_or_default()
                        })
                        .collect::<Vec<_>>()
                        .join("")
                }
            })
            .expect("建 html"),
        )
        .expect("挂 html");
    props
        .prop(
            "attr",
            Function::new(ctx.clone(), {
                let host = host.clone();
                let items = items.clone();
                move |name: String| -> String {
                    let rule = format!("[data-legado-el] > *@{name}");
                    items
                        .borrow()
                        .iter()
                        .map(|html| {
                            rules::run_string(&host, &rule, Some(wrap(html)), true)
                                .unwrap_or_default()
                        })
                        .find(|value| !value.is_empty())
                        .unwrap_or_default()
                }
            })
            .expect("建 attr"),
        )
        .expect("挂 attr");
    props
        .prop(
            "hasClass",
            Function::new(ctx.clone(), {
                let host = host.clone();
                let items = items.clone();
                move |name: String| -> bool {
                    items
                        .borrow()
                        .iter()
                        .any(|html| has_class(&host, html, &name))
                }
            })
            .expect("建 hasClass"),
        )
        .expect("挂 hasClass");
    props
        .prop(
            "eachText",
            Function::new(ctx.clone(), {
                let host = host.clone();
                let items = items.clone();
                move |ctx: Ctx<'js>| -> Array<'js> {
                    let out = Array::new(ctx.clone()).expect("建数组");
                    for (index, html) in items.borrow().iter().enumerate() {
                        let text = rules::run_string(&host, WRAPPED_TEXT, Some(wrap(html)), true)
                            .unwrap_or_default();
                        let _ = out.set(index, text);
                    }
                    out
                }
            })
            .expect("建 eachText"),
        )
        .expect("挂 eachText");
    props
        .prop(
            "select",
            Function::new(ctx.clone(), {
                let host = host.clone();
                let doc = doc.clone();
                let items = items.clone();
                move |ctx: Ctx<'js>, selector: String| -> Array<'js> {
                    elements(
                        &ctx,
                        &host,
                        doc.clone(),
                        select_within(&host, &items, &selector),
                    )
                }
            })
            .expect("建 select"),
        )
        .expect("挂 select");
    props
        .prop(
            "selectFirst",
            Function::new(ctx.clone(), {
                let host = host.clone();
                let doc = doc.clone();
                let items = items.clone();
                move |ctx: Ctx<'js>, selector: String| -> JsValue<'js> {
                    let found = select_within(&host, &items, &selector);
                    first(&ctx, &host, doc.clone(), found)
                }
            })
            .expect("建 selectFirst"),
        )
        .expect("挂 selectFirst");
    props
        .prop(
            "remove",
            Function::new(ctx.clone(), {
                let doc = doc.clone();
                let items = items.clone();
                let array = array.clone();
                move || -> Array<'js> {
                    let targets = items.borrow().clone();
                    remove_items(&doc, &targets);
                    array.clone()
                }
            })
            .expect("建 remove"),
        )
        .expect("挂 remove");
    let outer = Function::new(ctx.clone(), {
        let items = items.clone();
        move || -> String { items.borrow().join("") }
    })
    .expect("建 outerHtml");
    props
        .prop("outerHtml", outer.clone())
        .expect("挂 outerHtml");
    props.prop("toString", outer).expect("挂 toString");
    array
}

pub(crate) fn element<'js>(ctx: &Ctx<'js>, host: &Rc<Host>, doc: Doc, html: &str) -> Object<'js> {
    let object = Object::new(ctx.clone()).expect("建元素对象");
    let html = Rc::new(html.to_string());

    object
        .prop(
            "text",
            Function::new(ctx.clone(), {
                let host = host.clone();
                let html = html.clone();
                move || -> String {
                    rules::run_string(&host, WRAPPED_TEXT, Some(wrap(&html)), true)
                        .unwrap_or_default()
                }
            })
            .expect("建 text"),
        )
        .expect("挂 text");
    object
        .prop(
            "html",
            Function::new(ctx.clone(), {
                let host = host.clone();
                let html = html.clone();
                move || -> String {
                    rules::run_string(&host, WRAPPED_INNER, Some(wrap(&html)), true)
                        .unwrap_or_default()
                }
            })
            .expect("建 html"),
        )
        .expect("挂 html");
    let outer = Function::new(ctx.clone(), {
        let html = html.clone();
        move || -> String { (*html).clone() }
    })
    .expect("建 outerHtml");
    object
        .prop("outerHtml", outer.clone())
        .expect("挂 outerHtml");
    object.prop("toString", outer).expect("挂 toString");
    object
        .prop(
            "attr",
            Function::new(ctx.clone(), {
                let host = host.clone();
                let html = html.clone();
                move |name: String| -> String {
                    let rule = format!("[data-legado-el] > *@{name}");
                    rules::run_string(&host, &rule, Some(wrap(&html)), true).unwrap_or_default()
                }
            })
            .expect("建 attr"),
        )
        .expect("挂 attr");
    object
        .prop(
            "hasClass",
            Function::new(ctx.clone(), {
                let host = host.clone();
                let html = html.clone();
                move |name: String| -> bool { has_class(&host, &html, &name) }
            })
            .expect("建 hasClass"),
        )
        .expect("挂 hasClass");
    object
        .prop(
            "select",
            Function::new(ctx.clone(), {
                let host = host.clone();
                let html = html.clone();
                let doc = doc.clone();
                move |ctx: Ctx<'js>, selector: String| -> Array<'js> {
                    let found = rules::run_elements(&host, &selector, Some((*html).clone()))
                        .unwrap_or_default();
                    elements(&ctx, &host, doc.clone(), found)
                }
            })
            .expect("建 select"),
        )
        .expect("挂 select");
    object
        .prop(
            "selectFirst",
            Function::new(ctx.clone(), {
                let host = host.clone();
                let html = html.clone();
                let doc = doc.clone();
                move |ctx: Ctx<'js>, selector: String| -> JsValue<'js> {
                    let found = rules::run_elements(&host, &selector, Some((*html).clone()))
                        .unwrap_or_default();
                    first(&ctx, &host, doc.clone(), found)
                }
            })
            .expect("建 selectFirst"),
        )
        .expect("挂 selectFirst");
    object
        .prop(
            "remove",
            Function::new(ctx.clone(), {
                let doc = doc.clone();
                let html = html.clone();
                move || remove_items(&doc, std::slice::from_ref(&html))
            })
            .expect("建 remove"),
        )
        .expect("挂 remove");
    object
}

fn select_within(host: &Rc<Host>, items: &Rc<RefCell<Vec<String>>>, selector: &str) -> Vec<String> {
    let mut found = Vec::new();
    for html in items.borrow().iter() {
        found.extend(rules::run_elements(host, selector, Some(html.clone())).unwrap_or_default());
    }
    found
}

fn remove_items(doc: &Doc, targets: &[String]) {
    let mut state = doc.borrow_mut();
    for html in targets {
        while let Some(at) = state.html.find(html.as_str()) {
            state.html.replace_range(at..at + html.len(), "");
        }
    }
    state.views.retain(|weak| {
        let Some(view) = weak.upgrade() else {
            return false;
        };
        let mut items = view.borrow_mut();
        for item in items.iter_mut() {
            for html in targets {
                if item.contains(html.as_str()) {
                    *item = item.replace(html.as_str(), "");
                }
            }
        }
        true
    });
}
