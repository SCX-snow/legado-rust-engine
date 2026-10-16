use std::collections::HashMap;
use std::sync::{Arc, OnceLock, RwLock};
use std::time::Instant;

use legado_core::{
    Book, BookChapter, Error, JsEvaluator, JsRequest, JsValue, JsVariables, LocalBoxFuture, Result,
    RuleRequest, RuleRunner, Source, Variables, java_list_to_string,
};

use crate::compile::{
    Body, CompiledRule, ReplaceSpec, Rule, Span, Step, compile as compile_rule, rule_error,
};
use crate::content::{Content, Value};
use crate::selector::Selector;
use crate::selector::css::RuleOutput;
use crate::split::Piece;
use crate::trace::{TraceEvent, TraceSink};

pub struct RuleEngine {
    cache: RwLock<HashMap<String, Arc<CompiledRule>>>,
    js: OnceLock<Arc<dyn JsEvaluator>>,
}

impl Default for RuleEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl RuleEngine {
    pub fn new() -> Self {
        Self {
            cache: RwLock::new(HashMap::new()),
            js: OnceLock::new(),
        }
    }

    pub fn set_js(&self, js: Arc<dyn JsEvaluator>) {
        let _ = self.js.set(js);
    }

    fn js(&self) -> Option<&dyn JsEvaluator> {
        self.js.get().map(AsRef::as_ref)
    }

    pub fn compile(&self, rule: &str) -> Result<Arc<CompiledRule>> {
        if let Some(hit) = self.cache.read().expect("规则缓存锁被毒化").get(rule) {
            return Ok(hit.clone());
        }
        let compiled = Arc::new(compile_rule(self, rule)?);
        self.cache
            .write()
            .expect("规则缓存锁被毒化")
            .insert(rule.to_string(), compiled.clone());
        Ok(compiled)
    }

    pub fn cached_rules(&self) -> usize {
        self.cache.read().expect("规则缓存锁被毒化").len()
    }

    pub async fn eval(&self, rule: &str, ctx: &mut EvalContext<'_>) -> Result<Value> {
        let compiled = self.compile(rule)?;
        self.eval_compiled(&compiled, ctx, RuleOutput::Elements)
            .await
    }

    pub async fn eval_string(&self, rule: &str, ctx: &mut EvalContext<'_>) -> Result<String> {
        Ok(unescape_html(&self.eval_string_raw(rule, ctx).await?))
    }

    pub async fn eval_string_raw(&self, rule: &str, ctx: &mut EvalContext<'_>) -> Result<String> {
        let compiled = self.compile(rule)?;
        Ok(self
            .eval_compiled(&compiled, ctx, RuleOutput::Text)
            .await?
            .into_text())
    }

    pub async fn eval_list(&self, rule: &str, ctx: &mut EvalContext<'_>) -> Result<Vec<String>> {
        let compiled = self.compile(rule)?;
        let (value, kind) = self
            .eval_compiled_as(&compiled, ctx, RuleOutput::Text, true)
            .await?;
        Ok(finish_list(value, kind))
    }

    pub async fn eval_compiled(
        &self,
        compiled: &CompiledRule,
        ctx: &mut EvalContext<'_>,
        output: RuleOutput,
    ) -> Result<Value> {
        Ok(self.eval_compiled_as(compiled, ctx, output, false).await?.0)
    }

    async fn eval_compiled_as(
        &self,
        compiled: &CompiledRule,
        ctx: &mut EvalContext<'_>,
        output: RuleOutput,
        list_mode: bool,
    ) -> Result<(Value, ResultKind)> {
        let mut state = EvalState {
            variables: Some(js_variables(ctx)),
            list_mode,
            ..EvalState::default()
        };
        let value = {
            let borrowed: &EvalContext<'_> = ctx;
            self.eval_steps(compiled, &borrowed.content, borrowed, &mut state, output)
                .await?
        };
        state.flush(ctx);
        Ok((
            value,
            ResultKind {
                list: state.result_is_list,
                js_other: state.result_is_js_other,
            },
        ))
    }

    fn eval_steps<'a>(
        &'a self,
        compiled: &'a CompiledRule,
        input: &'a Content,
        ctx: &'a EvalContext<'_>,
        state: &'a mut EvalState,
        output: RuleOutput,
    ) -> LocalBoxFuture<'a, Result<Value>> {
        state.items_stack.push(None);
        Box::pin(async move {
            let result = self
                .eval_steps_inner(compiled, input, ctx, state, output)
                .await;
            state.items_stack.pop();
            result
        })
    }

    fn eval_steps_inner<'a>(
        &'a self,
        compiled: &'a CompiledRule,
        input: &'a Content,
        ctx: &'a EvalContext<'_>,
        state: &'a mut EvalState,
        output: RuleOutput,
    ) -> LocalBoxFuture<'a, Result<Value>> {
        Box::pin(async move {
            let mut owned: Option<Content> = None;
            let mut value = Value::None;
            let mut previous_is_list = false;
            let mut result_is_js_other = false;

            for (index, step) in compiled.steps.iter().enumerate() {
                let started = Instant::now();
                let current = owned.as_ref().unwrap_or(input);
                let slot = (state.list_mode && previous_is_list).then(|| current.to_items());
                if let Some(target) = state.items_stack.last_mut() {
                    *target = slot;
                }
                let result = self
                    .eval_step(compiled, step, current, ctx, state, output)
                    .await?;
                result_is_js_other = state.last_js_other;
                previous_is_list = step_result_is_list(step, &result, previous_is_list);

                if let Some(trace) = ctx.trace {
                    let input_text = current.to_source();
                    let output_text = result.to_text();
                    trace.event(TraceEvent {
                        index,
                        rule: step.span.slice(&compiled.text),
                        input: &input_text,
                        output: &output_text,
                        elapsed: started.elapsed(),
                    });
                }

                if matches!(result, Value::None) && index + 1 < compiled.steps.len() {
                    return Ok(Value::None);
                }

                value = result;
                if index + 1 < compiled.steps.len() {
                    owned = Some(std::mem::replace(&mut value, Value::None).into_content());
                }
            }
            state.result_is_list = previous_is_list;
            state.result_is_js_other = result_is_js_other;
            Ok(value)
        })
    }

    fn eval_step<'a>(
        &'a self,
        compiled: &'a CompiledRule,
        step: &'a Step,
        input: &'a Content,
        ctx: &'a EvalContext<'_>,
        state: &'a mut EvalState,
        output: RuleOutput,
    ) -> LocalBoxFuture<'a, Result<Value>> {
        Box::pin(async move {
            for (key, put_rule) in &step.puts {
                let value = self
                    .eval_steps(put_rule, input, ctx, state, RuleOutput::Text)
                    .await?;
                state.puts.push((key.clone(), value.into_text()));
            }

            let mut js_other = false;
            let mut value = match &step.body {
                Body::Rule(rule) => {
                    self.eval_rule(compiled, rule, input, ctx, state, output)
                        .await?
                }
                Body::Script(pieces) => {
                    let code = self.eval_template(pieces, input, ctx, state).await?;
                    if code.trim().is_empty() && (state.list_mode || step.replace.is_some()) {
                        keep_input(input)
                    } else {
                        let (value, other) =
                            self.eval_js_value_ex(&code, input, ctx, state).await?;
                        js_other = other;
                        value
                    }
                }
                Body::Identity => keep_input(input),
            };
            state.last_js_other = js_other;

            if string_rule(&step.body) && value.to_text().is_empty() {
                if state.list_mode || (expanded_rule_is_blank(&step.body) && step.replace.is_some())
                {
                    value = keep_input(input);
                } else if blank_rule(&step.body) {
                    value = Value::None;
                }
            }

            if let Some(replace) = &step.replace
                && !matches!(value, Value::None)
            {
                value = match value {
                    Value::List(items) if state.list_mode => {
                        let mut replaced = Vec::with_capacity(items.len());
                        for item in items {
                            replaced.push(
                                apply_replace(self, replace, &item, input, ctx, state).await?,
                            );
                        }
                        Value::List(replaced)
                    }
                    Value::List(items)
                        if items.is_empty()
                            && !state.list_mode
                            && step_takes_empty_as_text(step, input) =>
                    {
                        Value::Text(apply_replace(self, replace, "", input, ctx, state).await?)
                    }
                    Value::List(items) if items.is_empty() => Value::List(items),
                    other => {
                        let text = other.to_text();
                        Value::Text(apply_replace(self, replace, &text, input, ctx, state).await?)
                    }
                };
            }
            Ok(value)
        })
    }

    fn eval_rule<'a>(
        &'a self,
        compiled: &'a CompiledRule,
        rule: &'a Rule,
        input: &'a Content,
        ctx: &'a EvalContext<'_>,
        state: &'a mut EvalState,
        output: RuleOutput,
    ) -> LocalBoxFuture<'a, Result<Value>> {
        Box::pin(async move {
            match rule {
                Rule::Leaf { selector, span } => {
                    let value = selector.eval(input, output).map_err(|message| {
                        Error::Rule(rule_error(&compiled.text, *span, message))
                    })?;
                    Ok(if state.list_mode && matches!(value, Value::None) {
                        Value::List(Vec::new())
                    } else {
                        value
                    })
                }
                Rule::Template { pieces, .. } => Ok(Value::Text(
                    self.eval_template(pieces, input, ctx, state)
                        .await?
                        .trim()
                        .to_string(),
                )),
                Rule::Literal {
                    pieces, selector, ..
                } => match output {
                    RuleOutput::Text => Ok(Value::Text(
                        self.eval_template(pieces, input, ctx, state)
                            .await?
                            .trim()
                            .to_string(),
                    )),
                    RuleOutput::Elements => {
                        self.eval_rule(compiled, selector, input, ctx, state, output)
                            .await
                    }
                },
                Rule::Interpolate { pieces, span } => {
                    let text = self
                        .eval_template(pieces, input, ctx, state)
                        .await?
                        .trim()
                        .to_string();
                    let nested = self.compile(&text).map_err(|error| {
                        Error::Rule(rule_error(&compiled.text, *span, error.to_string()))
                    })?;
                    state.interpolate_depth += 1;
                    let result = if state.interpolate_depth > MAX_INTERPOLATE_DEPTH {
                        Err(Error::Rule(rule_error(
                            &compiled.text,
                            *span,
                            format!("模板展开超过 {MAX_INTERPOLATE_DEPTH} 层，规则可能在自我复读"),
                        )))
                    } else {
                        self.eval_steps(&nested, input, ctx, state, output).await
                    };
                    state.interpolate_depth -= 1;
                    result
                }
                Rule::Fallback(parts) => {
                    let mut saw_empty_text = false;
                    for part in parts {
                        let value = self
                            .eval_rule(compiled, part, input, ctx, state, output)
                            .await?;
                        if !value.is_empty() {
                            return Ok(value);
                        }
                        if matches!(&value, Value::Text(text) if text.is_empty()) {
                            saw_empty_text = true;
                        }
                    }
                    Ok(if state.list_mode {
                        Value::List(Vec::new())
                    } else if saw_empty_text || empty_gives_string(parts) {
                        Value::Text(String::new())
                    } else {
                        Value::None
                    })
                }
                Rule::Concat(parts) => {
                    let mut values = Vec::with_capacity(parts.len());
                    for part in parts {
                        values.push(
                            self.eval_rule(compiled, part, input, ctx, state, output)
                                .await?,
                        );
                    }
                    Ok(concat(values))
                }
                Rule::Interleave(parts) => {
                    let mut values = Vec::with_capacity(parts.len());
                    for part in parts {
                        values.push(
                            self.eval_rule(compiled, part, input, ctx, state, output)
                                .await?,
                        );
                    }
                    Ok(interleave(values))
                }
            }
        })
    }

    async fn eval_template(
        &self,
        pieces: &[Piece],
        input: &Content,
        ctx: &EvalContext<'_>,
        state: &mut EvalState,
    ) -> Result<String> {
        let mut out = String::new();
        for piece in pieces {
            match piece {
                Piece::Literal(text) => out.push_str(text),
                Piece::Expr(script) => {
                    if is_inner_rule(script) {
                        out.push_str(&self.eval_inner_rule(script, ctx, state).await?)
                    } else {
                        out.push_str(&self.eval_js(script, input, ctx, state).await?)
                    }
                }
                Piece::Get(key) => out.push_str(&read_variable(ctx, state, key)),
                Piece::Ref(index) => match input.as_items().and_then(|items| items.get(*index)) {
                    Some(value) => out.push_str(value),
                    None => {
                        out.push('$');
                        out.push_str(&index.to_string());
                    }
                },
            }
        }
        Ok(out)
    }

    async fn eval_inner_rule(
        &self,
        rule: &str,
        ctx: &EvalContext<'_>,
        state: &mut EvalState,
    ) -> Result<String> {
        let compiled = self.compile(rule)?;
        state.interpolate_depth += 1;
        let result = if state.interpolate_depth > MAX_INTERPOLATE_DEPTH {
            Err(Error::Rule(rule_error(
                rule,
                Span::new(0, rule.len()),
                format!("模板展开超过 {MAX_INTERPOLATE_DEPTH} 层，规则可能在自我复读"),
            )))
        } else {
            self.eval_steps(&compiled, &ctx.content, ctx, state, RuleOutput::Text)
                .await
        };
        state.interpolate_depth -= 1;
        Ok(result?.to_text())
    }

    async fn eval_js(
        &self,
        script: &str,
        input: &Content,
        ctx: &EvalContext<'_>,
        state: &mut EvalState,
    ) -> Result<String> {
        Ok(match self.eval_js_value(script, input, ctx, state).await? {
            Value::List(items) | Value::Elements(items) => java_list_to_string(&items),
            value => value.into_text(),
        })
    }

    async fn eval_js_value(
        &self,
        script: &str,
        input: &Content,
        ctx: &EvalContext<'_>,
        state: &mut EvalState,
    ) -> Result<Value> {
        Ok(self.eval_js_value_ex(script, input, ctx, state).await?.0)
    }

    async fn eval_js_value_ex(
        &self,
        script: &str,
        input: &Content,
        ctx: &EvalContext<'_>,
        state: &mut EvalState,
    ) -> Result<(Value, bool)> {
        let Some(js) = ctx.js else {
            return Err(Error::Js(
                "没有接 JsEvaluator，`@js` / `{{}}` 求不了".into(),
            ));
        };
        let result = input.to_source();
        let src = ctx.content.to_source();
        let list_mode = state.list_mode;
        let EvalState {
            puts,
            variables,
            items_stack,
            ..
        } = state;
        if let Some(variables) = variables.as_mut() {
            for (key, value) in puts.iter() {
                variables.put(key, Some(value));
            }
        }
        let items = items_stack.last().and_then(|slot| slot.as_deref());
        let value = js
            .eval_value(
                JsRequest {
                    script,
                    source: ctx.source,
                    base_url: &ctx.base_url,
                    page: ctx.page,
                    key: ctx.key.as_deref(),
                    result: Some(&result),
                    src: Some(&src),
                    response: None,
                    book: ctx.book.as_deref(),
                    chapter: ctx.chapter.as_deref(),
                    variables: variables.as_mut(),
                },
                items,
            )
            .await?;
        Ok(match value {
            JsValue::Text(text) => (Value::Text(text), false),
            JsValue::List(items) => (
                if list_mode {
                    Value::List(items)
                } else {
                    Value::Text(java_list_to_string(&items))
                },
                false,
            ),
            JsValue::Null => (Value::None, false),
            JsValue::Other(text) => (Value::Text(text), true),
        })
    }

    async fn run_from_js(
        &self,
        request: RuleRequest<'_>,
        output: RuleOutput,
        list_mode: bool,
    ) -> Result<(Value, ResultKind)> {
        let incoming_chapter = request
            .variables
            .as_ref()
            .and_then(|variables| variables.chapter().cloned());
        let incoming_book = request
            .variables
            .as_ref()
            .and_then(|variables| variables.book().cloned());

        let mut book = request.book.cloned();
        let mut chapter = request.chapter.cloned();
        if let Some(book) = book.as_mut() {
            book.variables = incoming_book.clone().unwrap_or_default();
        }
        if let Some(chapter) = chapter.as_mut() {
            chapter.variables = incoming_chapter.clone().unwrap_or_default();
        }

        let compiled = self.compile(request.rule)?;
        let (value, kind) = {
            let mut ctx = EvalContext::new(Content::Text(request.content.to_string()));
            ctx.base_url = request.base_url.to_string();
            ctx.source = request.source;
            ctx.js = self.js();
            ctx.page = request.page;
            ctx.key = request.key.map(str::to_string);
            ctx.book = book.as_mut();
            ctx.chapter = chapter.as_mut();
            self.eval_compiled_as(&compiled, &mut ctx, output, list_mode)
                .await?
        };

        if let Some(target) = request.variables {
            *target = JsVariables::new(
                chapter
                    .as_ref()
                    .map(|chapter| chapter.variables.clone())
                    .or(incoming_chapter),
                book.as_ref()
                    .map(|book| book.variables.clone())
                    .or(incoming_book),
                request
                    .source
                    .map(|source| legado_core::model::source::source_variables(source.key())),
            );
        }
        Ok((value, kind))
    }
}

impl RuleRunner for RuleEngine {
    fn get_string<'a>(&'a self, request: RuleRequest<'a>) -> LocalBoxFuture<'a, Result<String>> {
        Box::pin(async move {
            let unescape = request.unescape;
            let text = self
                .run_from_js(request, RuleOutput::Text, false)
                .await?
                .0
                .into_text();
            Ok(if unescape { unescape_html(&text) } else { text })
        })
    }

    fn get_elements<'a>(
        &'a self,
        request: RuleRequest<'a>,
    ) -> LocalBoxFuture<'a, Result<Vec<String>>> {
        Box::pin(async move {
            let value = self
                .run_from_js(request, RuleOutput::Elements, false)
                .await?
                .0;
            Ok(match value {
                Value::Elements(items) => items,
                other => other.into_list(),
            })
        })
    }

    fn get_string_list<'a>(
        &'a self,
        request: RuleRequest<'a>,
    ) -> LocalBoxFuture<'a, Result<Vec<String>>> {
        Box::pin(async move {
            let (value, kind) = self.run_from_js(request, RuleOutput::Text, true).await?;
            Ok(finish_list(value, kind))
        })
    }
}

pub fn unescape_html(text: &str) -> String {
    if !text.contains('&') {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find('&') {
        out.push_str(&rest[..start]);
        let tail = &rest[start..];
        match tail.find(';').filter(|end| *end <= 10) {
            Some(end) => match decode_entity(&tail[1..end]) {
                Some(decoded) => {
                    out.push(decoded);
                    rest = &tail[end + 1..];
                }
                None => {
                    out.push('&');
                    rest = &tail[1..];
                }
            },
            None => {
                out.push('&');
                rest = &tail[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

fn decode_entity(entity: &str) -> Option<char> {
    if let Some(code) = entity.strip_prefix('#') {
        let value = match code.strip_prefix(['x', 'X']) {
            Some(hex) => u32::from_str_radix(hex, 16).ok()?,
            None => code.parse().ok()?,
        };
        return char::from_u32(value);
    }
    crate::entities::decode(entity)
}

fn js_variables(ctx: &EvalContext<'_>) -> JsVariables {
    JsVariables::new(
        ctx.chapter
            .as_ref()
            .map(|chapter| chapter.variables.clone()),
        ctx.book.as_ref().map(|book| book.variables.clone()),
        ctx.source
            .as_ref()
            .map(|source| legado_core::model::source::source_variables(source.key())),
    )
}

const MAX_INTERPOLATE_DEPTH: usize = 8;

fn step_result_is_list(step: &Step, value: &Value, previous: bool) -> bool {
    match &step.body {
        Body::Rule(Rule::Template { .. } | Rule::Interpolate { .. } | Rule::Literal { .. }) => {
            false
        }
        Body::Script(_) => matches!(value, Value::List(_) | Value::Elements(_)),
        Body::Identity => previous,
        _ => true,
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct ResultKind {
    list: bool,
    js_other: bool,
}

fn finish_list(value: Value, kind: ResultKind) -> Vec<String> {
    match value {
        Value::None => Vec::new(),
        _ if kind.js_other => Vec::new(),
        value if kind.list => match value {
            Value::Text(text) => vec![text],
            other => other.into_list(),
        },
        value => value.to_text().split('\n').map(str::to_string).collect(),
    }
}

fn empty_gives_string(parts: &[Rule]) -> bool {
    matches!(
        parts.first(),
        Some(Rule::Leaf {
            selector: Selector::Json(_) | Selector::XPath { .. },
            ..
        })
    )
}

fn step_takes_empty_as_text(step: &Step, input: &Content) -> bool {
    match &step.body {
        Body::Rule(Rule::Leaf { selector, .. }) => match selector {
            Selector::Json(_) => true,
            Selector::Auto { .. } => input.looks_like_json(),
            _ => false,
        },
        _ => false,
    }
}

fn string_rule(body: &Body) -> bool {
    matches!(
        body,
        Body::Rule(Rule::Template { .. } | Rule::Interpolate { .. } | Rule::Literal { .. })
    )
}

fn expanded_rule_is_blank(body: &Body) -> bool {
    matches!(
        body,
        Body::Rule(Rule::Template { .. } | Rule::Literal { .. })
    )
}

fn blank_rule(body: &Body) -> bool {
    matches!(body, Body::Rule(Rule::Template { pieces, .. }) if pieces.is_empty())
}

fn keep_input(input: &Content) -> Value {
    match input {
        Content::List(items) => Value::List(items.clone()),
        Content::Json(value) => Value::Json(value.clone()),
        other => Value::Text(other.to_source()),
    }
}

fn is_inner_rule(text: &str) -> bool {
    let text = text.trim_start();
    text.starts_with('@')
        || text.starts_with("$.")
        || text.starts_with("$[")
        || text.starts_with("//")
}

#[derive(Debug, Default)]
struct EvalState {
    puts: Vec<(String, String)>,
    interpolate_depth: usize,
    variables: Option<JsVariables>,
    list_mode: bool,
    result_is_list: bool,
    result_is_js_other: bool,
    last_js_other: bool,
    items_stack: Vec<Option<Vec<String>>>,
}

impl EvalState {
    fn flush(&mut self, ctx: &mut EvalContext<'_>) {
        if let Some(variables) = self.variables.as_mut() {
            for (key, value) in &self.puts {
                variables.put(key, Some(value));
            }
        }
        self.puts.clear();

        let Some(variables) = self.variables.take() else {
            return;
        };
        if let Some(source) = ctx.source
            && let Some(values) = variables.source()
        {
            for (key, value) in values {
                source.put(key, value);
            }
        }
        if let Some(chapter) = ctx.chapter.as_mut()
            && let Some(values) = variables.chapter()
        {
            chapter.variables = values.clone();
        }
        if let Some(book) = ctx.book.as_mut()
            && let Some(values) = variables.book()
        {
            book.variables = values.clone();
        }
    }
}

fn read_variable(ctx: &EvalContext<'_>, state: &EvalState, key: &str) -> String {
    if let Some((_, value)) = state.puts.iter().rev().find(|(name, _)| name == key) {
        return value.clone();
    }
    match key {
        "bookName" => {
            if let Some(book) = ctx.book.as_ref() {
                return book.name.clone();
            }
        }
        "title" => {
            if let Some(chapter) = ctx.chapter.as_ref() {
                return chapter.title.clone();
            }
        }
        _ => {}
    }
    if let Some(value) = ctx
        .chapter
        .as_ref()
        .and_then(|chapter| chapter.get_variable(key))
        .filter(|value| !value.is_empty())
    {
        return value.to_string();
    }
    if let Some(value) = ctx
        .book
        .as_ref()
        .and_then(|book| book.get_variable(key))
        .filter(|value| !value.is_empty())
    {
        return value.to_string();
    }
    ctx.source
        .as_ref()
        .and_then(|source| source.get(key))
        .filter(|value| !value.is_empty())
        .unwrap_or_default()
}

fn concat(values: Vec<Value>) -> Value {
    if values.iter().all(|value| matches!(value, Value::None)) {
        return Value::None;
    }
    if values
        .iter()
        .all(|value| matches!(value, Value::Elements(_)))
    {
        return Value::Elements(
            values
                .into_iter()
                .flat_map(Value::into_list)
                .collect::<Vec<_>>(),
        );
    }
    Value::List(
        values
            .into_iter()
            .flat_map(Value::into_list)
            .collect::<Vec<_>>(),
    )
}

fn interleave(values: Vec<Value>) -> Value {
    let lists: Vec<Vec<String>> = values.into_iter().map(Value::into_list).collect();
    let mut out = Vec::new();
    if let Some(first) = lists.first() {
        for index in 0..first.len() {
            for list in &lists {
                if let Some(item) = list.get(index) {
                    out.push(item.clone());
                }
            }
        }
    }
    if out.is_empty() {
        Value::None
    } else {
        Value::List(out)
    }
}

async fn apply_replace(
    engine: &RuleEngine,
    spec: &ReplaceSpec,
    text: &str,
    input: &Content,
    ctx: &EvalContext<'_>,
    state: &mut EvalState,
) -> Result<String> {
    let pattern = match &spec.pieces {
        Some(pieces) => engine.eval_template(pieces, input, ctx, state).await?,
        None => spec.pattern.clone(),
    };

    let owned;
    let regex: &legado_regex::Regex = match &spec.regex {
        Some(Ok(regex)) => regex,
        Some(Err(error)) => return Ok(replace_fallback(spec, &pattern, text, error)),
        None => match legado_regex::Regex::new(&pattern) {
            Ok(regex) => {
                owned = regex;
                &owned
            }
            Err(error) => {
                let message = error.to_string();
                return Ok(replace_fallback(spec, &pattern, text, &message));
            }
        },
    };

    let runtime = |error: legado_regex::Error| Error::Other(error.to_string());
    if spec.first {
        return match regex.find(text) {
            Ok(Some((start, end))) => regex
                .replace(&text[start..end], &spec.replacement)
                .map_err(runtime),
            Ok(None) => Ok(String::new()),
            Err(error) => Err(runtime(error)),
        };
    }
    regex.replace_all(text, &spec.replacement).map_err(runtime)
}

fn replace_fallback(spec: &ReplaceSpec, pattern: &str, text: &str, _error: &str) -> String {
    if spec.first {
        spec.replacement.clone()
    } else {
        text.replace(pattern, &spec.replacement)
    }
}

pub struct EvalContext<'a> {
    pub content: Content,
    pub base_url: String,
    pub source: Option<&'a dyn Source>,
    pub book: Option<&'a mut Book>,
    pub chapter: Option<&'a mut BookChapter>,
    pub js: Option<&'a dyn JsEvaluator>,
    pub page: Option<i32>,
    pub key: Option<String>,
    pub trace: Option<&'a dyn TraceSink>,
}

impl<'a> EvalContext<'a> {
    pub fn new(content: Content) -> Self {
        Self {
            content,
            base_url: String::new(),
            source: None,
            book: None,
            chapter: None,
            js: None,
            page: None,
            key: None,
            trace: None,
        }
    }

    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into();
        self
    }

    pub fn with_source(mut self, source: &'a dyn Source) -> Self {
        self.source = Some(source);
        self
    }

    pub fn with_book(mut self, book: &'a mut Book) -> Self {
        self.book = Some(book);
        self
    }

    pub fn with_chapter(mut self, chapter: &'a mut BookChapter) -> Self {
        self.chapter = Some(chapter);
        self
    }

    pub fn with_js(mut self, js: &'a dyn JsEvaluator) -> Self {
        self.js = Some(js);
        self
    }

    pub fn with_page(mut self, page: i32) -> Self {
        self.page = Some(page);
        self
    }

    pub fn with_key(mut self, key: impl Into<String>) -> Self {
        self.key = Some(key.into());
        self
    }

    pub fn with_trace(mut self, trace: &'a dyn TraceSink) -> Self {
        self.trace = Some(trace);
        self
    }

    pub fn put_variable(&mut self, key: &str, value: Option<&str>) {
        if let Some(chapter) = self.chapter.as_mut() {
            chapter.put_variable(key, value);
        } else if let Some(book) = self.book.as_mut() {
            book.put_variable(key, value);
        }
    }
}
