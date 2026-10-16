use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant};

use legado_core::{CacheStore, CookieStore, Error, HttpFetcher, JsVariables, Result, RuleRunner};
use rquickjs::{Context, Ctx, Runtime};

use crate::host::{Binding, Host};

thread_local! {
    static JS_THREAD: RefCell<Option<JsThread>> = const { RefCell::new(None) };
}

struct JsThread {
    host: Rc<Host>,
    ctx: Option<Ctx<'static>>,
}

pub(crate) struct Job {
    pub binding: Binding,
    pub script: String,
    pub lib_key: u64,
    pub lib: Option<String>,
    pub reply: tokio::sync::oneshot::Sender<Result<Output>>,
}

pub(crate) struct Output {
    pub value: String,
    pub items: Option<Vec<String>>,
    pub kind: JsKind,
    pub variables: JsVariables,
    pub response: Option<legado_core::HttpResponse>,
}

#[derive(Clone)]
pub(crate) struct WorkerConfig {
    pub memory_limit: usize,
    pub stack_size: usize,
    pub timeout: Duration,
    pub android_id: String,
}

impl Default for WorkerConfig {
    fn default() -> Self {
        Self {
            memory_limit: 256 * 1024 * 1024,
            stack_size: 1024 * 1024,
            timeout: Duration::from_secs(30),
            android_id: String::new(),
        }
    }
}

const MAX_CACHED_CONTEXTS: usize = 3;

pub(crate) struct Deps {
    pub http: Option<Arc<dyn HttpFetcher>>,
    pub cache: Option<Arc<dyn CacheStore>>,
    pub cookies: Option<Arc<dyn CookieStore>>,
    pub rules: Option<Arc<dyn RuleRunner>>,
}

pub(crate) struct Worker {
    tx: Sender<Job>,
    stop_at: Arc<AtomicU64>,
    started: Instant,
    handle: Option<thread::JoinHandle<()>>,
}

impl Worker {
    pub(crate) fn spawn(config: WorkerConfig, deps: Deps) -> Result<Self> {
        let (tx, rx) = std::sync::mpsc::channel::<Job>();
        let stop_at = Arc::new(AtomicU64::new(u64::MAX));
        let started = Instant::now();

        let worker_stop = stop_at.clone();
        let worker_started = started;
        let handle = thread::Builder::new()
            .name("legado-js".into())
            .spawn(move || {
                if let Err(error) = run(config, deps, rx, worker_stop, worker_started) {
                    tracing::error!("JS 工作线程退出: {error}");
                }
            })
            .map_err(|error| Error::Js(format!("起 JS 线程失败: {error}")))?;
        Ok(Self {
            tx,
            stop_at,
            started,
            handle: Some(handle),
        })
    }

    pub(crate) async fn eval(
        &self,
        binding: Binding,
        script: String,
        lib_key: u64,
        lib: Option<String>,
        timeout: Duration,
    ) -> Result<Output> {
        let (reply, receiver) = tokio::sync::oneshot::channel();
        self.arm(timeout);
        self.tx
            .send(Job {
                binding,
                script,
                lib_key,
                lib,
                reply,
            })
            .map_err(|_| Error::Js("JS 线程已经退出".into()))?;
        match tokio::time::timeout(timeout + Duration::from_millis(500), receiver).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err(Error::Js("JS 线程没有回话".into())),
            Err(_) => {
                self.abort();
                Err(Error::Js(format!(
                    "JS 求值超过 {}ms，已中断",
                    timeout.as_millis()
                )))
            }
        }
    }

    fn arm(&self, timeout: Duration) {
        let at = self.started.elapsed().as_millis() as u64 + timeout.as_millis() as u64;
        self.stop_at.store(at, Ordering::Relaxed);
    }

    fn abort(&self) {
        self.stop_at.store(0, Ordering::Relaxed);
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.abort();
        let (tx, _) = std::sync::mpsc::channel();
        drop(std::mem::replace(&mut self.tx, tx));
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

fn run(
    config: WorkerConfig,
    deps: Deps,
    jobs: Receiver<Job>,
    stop_at: Arc<AtomicU64>,
    started: Instant,
) -> Result<()> {
    let runtime = Runtime::new().map_err(|error| Error::Js(format!("建 QuickJS 失败: {error}")))?;
    runtime.set_memory_limit(config.memory_limit);
    runtime.set_max_stack_size(config.stack_size);
    runtime.set_interrupt_handler(Some(Box::new(move || {
        let limit = stop_at.load(Ordering::Relaxed);
        limit != u64::MAX && started.elapsed().as_millis() as u64 >= limit
    })));

    let host = Host::new(deps, config.android_id.clone())?;
    JS_THREAD.with(|slot| {
        *slot.borrow_mut() = Some(JsThread {
            host: host.clone(),
            ctx: None,
        });
    });
    let mut contexts: HashMap<u64, Context> = HashMap::new();
    let mut context_order: VecDeque<u64> = VecDeque::new();

    while let Ok(job) = jobs.recv() {
        let context = match contexts.get(&job.lib_key) {
            Some(context) => context.clone(),
            None => match build_context(&runtime, &host, job.lib.as_deref()) {
                Ok(context) => {
                    contexts.insert(job.lib_key, context.clone());
                    context_order.push_back(job.lib_key);
                    while context_order.len() > MAX_CACHED_CONTEXTS {
                        if let Some(oldest) = context_order.pop_front() {
                            contexts.remove(&oldest);
                        }
                        runtime.run_gc();
                    }
                    context
                }
                Err(error) => {
                    let _ = job.reply.send(Err(error));
                    continue;
                }
            },
        };

        host.set_binding(Some(job.binding));
        let result = eval_script(&context, &host, &job.script);
        let variables = host.take_variables();
        host.set_binding(None);
        let _ = job.reply.send(result.map(|evaluated| Output {
            value: evaluated.value,
            items: evaluated.items,
            kind: evaluated.kind,
            variables,
            response: evaluated.response,
        }));
    }

    contexts.clear();
    JS_THREAD.with(|slot| *slot.borrow_mut() = None);
    drop(host);
    runtime.run_gc();
    std::mem::forget(runtime);
    Ok(())
}

fn build_context(runtime: &Runtime, host: &Rc<Host>, lib: Option<&str>) -> Result<Context> {
    let context =
        Context::full(runtime).map_err(|error| Error::Js(format!("建 JS 上下文失败: {error}")))?;
    context.with(|ctx| {
        crate::host::install_static(&ctx, host)
            .map_err(|error| Error::Js(format!("装宿主对象失败: {error}")))?;
        if let Some(lib) = lib {
            ctx.eval_with_options::<rquickjs::Value, _>(lib, eval_options())
                .map_err(|error| Error::Js(format!("jsLib 执行失败: {error}")))?;
        }
        Ok::<(), Error>(())
    })?;
    Ok(context)
}

fn eval_script(context: &Context, host: &Rc<Host>, script: &str) -> Result<Evaluated> {
    context.with(|ctx| eval_in_ctx(&ctx, host, script))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum JsKind {
    Text,
    Null,
    Other,
}

pub(crate) struct Evaluated {
    pub value: String,
    pub items: Option<Vec<String>>,
    pub kind: JsKind,
    pub response: Option<legado_core::HttpResponse>,
}

fn eval_in_ctx(ctx: &Ctx<'_>, host: &Rc<Host>, script: &str) -> Result<Evaluated> {
    crate::host::bind_eval(ctx, host)
        .map_err(|error| Error::Js(format!("绑定求值变量失败: {error}")))?;
    let _guard = CtxGuard::set(ctx);
    eval_source(ctx, script)
}

fn eval_options() -> rquickjs::context::EvalOptions {
    let mut options = rquickjs::context::EvalOptions::default();
    options.strict = false;
    options
}

const BOUND_NAMES: [&str; 11] = [
    "java", "cookie", "cache", "source", "book", "result", "baseUrl", "chapter", "src", "page",
    "key",
];

fn eval_source(ctx: &Ctx<'_>, script: &str) -> Result<Evaluated> {
    let params = BOUND_NAMES.join(", ");
    let args = BOUND_NAMES
        .iter()
        .map(|name| format!("globalThis.{name}"))
        .collect::<Vec<_>>()
        .join(", ");
    let wrapped = format!(
        "(function({params}){{ return eval({}); }})({args})",
        serde_json::to_string(script).unwrap_or_else(|_| "\"\"".to_string())
    );
    match eval_with(ctx, &wrapped) {
        Ok(value) => Ok(evaluated(&value)),
        Err(caught) => {
            let message = caught.to_string();
            drop(caught);
            if message.contains("return not in a function") {
                let wrapped = format!("(function(){{{script}\n}})()");
                return match eval_with(ctx, &wrapped) {
                    Ok(value) => Ok(evaluated(&value)),
                    Err(caught) => Err(Error::Js(caught.to_string())),
                };
            }
            Err(Error::Js(message))
        }
    }
}

fn eval_with<'js>(
    ctx: &Ctx<'js>,
    script: &str,
) -> std::result::Result<rquickjs::Value<'js>, rquickjs::CaughtError<'js>> {
    ctx.eval_with_options::<rquickjs::Value, _>(script, eval_options())
        .catch(ctx)
}

fn js_kind(value: &rquickjs::Value<'_>) -> JsKind {
    if value.as_string().is_some() {
        JsKind::Text
    } else if value.is_null() || value.is_undefined() {
        JsKind::Null
    } else {
        JsKind::Other
    }
}

fn evaluated(value: &rquickjs::Value<'_>) -> Evaluated {
    Evaluated {
        value: crate::java::js_text(value),
        items: crate::java::java_list_items(value),
        kind: js_kind(value),
        response: crate::java::response_from_js(value),
    }
}

pub(crate) fn on_js_thread() -> bool {
    JS_THREAD.with(|slot| {
        slot.borrow()
            .as_ref()
            .is_some_and(|state| state.ctx.is_some())
    })
}

pub(crate) fn eval_inline(binding: Binding, script: String) -> Result<Output> {
    let Some((host, ctx)) = current() else {
        return Err(Error::Js("不在 JS 线程上，内联求值用不了".into()));
    };
    let saved = host.take_binding();
    host.set_binding(Some(binding));
    let result = eval_in_ctx(&ctx, &host, &script);
    let variables = host.take_variables();
    host.set_binding(saved);
    crate::host::bind_eval(&ctx, &host)
        .map_err(|error| Error::Js(format!("还原求值变量失败: {error}")))?;
    result.map(|evaluated| Output {
        value: evaluated.value,
        items: evaluated.items,
        kind: evaluated.kind,
        variables,
        response: evaluated.response,
    })
}

fn current() -> Option<(Rc<Host>, Ctx<'static>)> {
    JS_THREAD.with(|slot| {
        let state = slot.borrow();
        let state = state.as_ref()?;
        Some((state.host.clone(), state.ctx.clone()?))
    })
}

struct CtxGuard {
    previous: Option<Ctx<'static>>,
}

impl CtxGuard {
    fn set(ctx: &Ctx<'_>) -> Self {
        let owned: Ctx<'static> = unsafe { std::mem::transmute(ctx.clone()) };
        let previous = JS_THREAD.with(|slot| {
            slot.borrow_mut()
                .as_mut()
                .and_then(|state| state.ctx.replace(owned))
        });
        Self { previous }
    }
}

impl Drop for CtxGuard {
    fn drop(&mut self) {
        let previous = self.previous.take();
        JS_THREAD.with(|slot| {
            if let Some(state) = slot.borrow_mut().as_mut() {
                state.ctx = previous;
            }
        });
    }
}

use rquickjs::CatchResultExt as _;
