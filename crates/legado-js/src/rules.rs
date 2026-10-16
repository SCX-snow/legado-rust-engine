use std::rc::Rc;

use legado_core::{Book, BookChapter, Error, JsVariables, Result, RuleRequest, Source};
use rquickjs::Value as JsValue;

use crate::host::Host;
use crate::java::js_text;

pub(crate) struct RuleSnapshot {
    pub(crate) content: String,
    pub(crate) base_url: String,
    pub(crate) source: Option<Box<dyn Source>>,
    pub(crate) book: Option<Book>,
    pub(crate) chapter: Option<BookChapter>,
    pub(crate) page: Option<i32>,
    pub(crate) key: Option<String>,
    pub(crate) variables: JsVariables,
}

impl RuleSnapshot {
    pub(crate) fn take(host: &Rc<Host>, content: Option<String>) -> Option<Self> {
        host.with_binding(|binding| {
            let binding = binding?;
            Some(Self {
                content: content.unwrap_or_else(|| binding.content.clone()),
                base_url: binding.base_url.clone(),
                source: binding.source.as_deref().map(Source::to_owned_source),
                book: binding.book.clone(),
                chapter: binding.chapter.clone(),
                page: binding.page,
                key: binding.key.clone(),
                variables: binding.variables.clone(),
            })
        })
    }
}

macro_rules! rule_request {
    ($rule:expr, $snapshot:expr, $variables:expr, $unescape:expr) => {
        RuleRequest {
            rule: $rule,
            content: &$snapshot.content,
            base_url: &$snapshot.base_url,
            source: $snapshot.source.as_deref(),
            book: $snapshot.book.as_ref(),
            chapter: $snapshot.chapter.as_ref(),
            page: $snapshot.page,
            key: $snapshot.key.as_deref(),
            variables: Some($variables),
            unescape: $unescape,
        }
    };
}

pub(crate) fn run_string(
    host: &Rc<Host>,
    rule: &str,
    content: Option<String>,
    unescape: bool,
) -> Result<String> {
    let Some(runner) = host.rules.clone() else {
        return Err(Error::Js("没有接 RuleRunner，java.getString 用不了".into()));
    };
    let Some(snapshot) = RuleSnapshot::take(host, content) else {
        return Err(Error::Js("没有求值上下文，java.getString 用不了".into()));
    };
    let mut variables = snapshot.variables;
    let request = rule_request!(rule, snapshot, &mut variables, unescape);
    let result = host.runtime.block_on(runner.get_string(request));
    write_back(host, variables);
    result
}

pub(crate) fn run_string_list(
    host: &Rc<Host>,
    rule: &str,
    content: Option<String>,
) -> Result<Vec<String>> {
    let Some(runner) = host.rules.clone() else {
        return Err(Error::Js(
            "没有接 RuleRunner，java.getStringList 用不了".into(),
        ));
    };
    let Some(snapshot) = RuleSnapshot::take(host, content) else {
        return Err(Error::Js(
            "没有求值上下文，java.getStringList 用不了".into(),
        ));
    };
    let mut variables = snapshot.variables;
    let request = rule_request!(rule, snapshot, &mut variables, false);
    let result = host.runtime.block_on(runner.get_string_list(request));
    write_back(host, variables);
    result
}

pub(crate) fn run_elements(
    host: &Rc<Host>,
    rule: &str,
    content: Option<String>,
) -> Result<Vec<String>> {
    let Some(runner) = host.rules.clone() else {
        return Err(Error::Js(
            "没有接 RuleRunner，java.getElement 用不了".into(),
        ));
    };
    let Some(snapshot) = RuleSnapshot::take(host, content) else {
        return Err(Error::Js("没有求值上下文，java.getElement 用不了".into()));
    };
    let mut variables = snapshot.variables;
    let request = rule_request!(rule, snapshot, &mut variables, true);
    let result = host.runtime.block_on(runner.get_elements(request));
    write_back(host, variables);
    result
}

fn write_back(host: &Rc<Host>, variables: JsVariables) {
    host.with_binding_mut(|binding| {
        if let Some(binding) = binding {
            binding.variables = variables;
        }
    });
}

pub(crate) fn content_arg(value: Option<&JsValue<'_>>) -> (Option<String>, bool) {
    match value {
        Some(value) if value.is_bool() => (None, value.as_bool().unwrap_or(true)),
        Some(value) if !value.is_null() && !value.is_undefined() => (Some(js_text(value)), true),
        _ => (None, true),
    }
}
