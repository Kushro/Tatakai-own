//! One QuickJS runtime per extension, each on its own OS thread.
//!
//! Limits (globals.v1.md section 1): 256 MiB heap, 15 s wall-clock per host call (interrupt handler + event-loop
//! deadline). A call that times out or runs out of memory discards the runtime; the next call rebuilds it.
//! Network `fetch` is started on the shared tokio runtime and its completion is delivered back to this thread's
//! event loop, so several fetches of one extension run in parallel. Browser runtime operations
//! (`configureBrowserRuntime`, docs/redesign/09 section 7) use the same pattern.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rquickjs::{ArrayBuffer, Context, Ctx, Exception, Function, Runtime};
use scraper::{ElementRef, Html, Selector};
use serde::Deserialize;
use serde_json::json;
use tk_core::net::{SafeHttp, SafeRequest, read_capped, sanitize_headers};
use tokio::sync::oneshot;

use crate::browser::{BrowserError, BrowserRuntime, browser_url_allowed};
use crate::error::ExtError;
use crate::hashing::{Algo, hmac};

/// Whole-call budget. Aggregating bundles (Toko) run dozens of providers at once, each bounded by
/// `providerOptions.timeoutMs` plus retries, so a source listing needs well over the 15 s a single route took.
pub const CALL_TIMEOUT: Duration = Duration::from_secs(40);
/// Toko's stream route (cheerio over many provider pages) runs out at 64 MiB.
pub const MEMORY_LIMIT: usize = 256 * 1024 * 1024;
const STACK_LIMIT: usize = 1024 * 1024;
pub const FETCH_CAP: usize = 25 * 1024 * 1024;
const LOG_LINES: usize = 500;
const LOG_LINE_BYTES: usize = 4096;
/// A call that opens a browser may run this long (from its start): a Cloudflare solve alone takes 15-45 s.
pub const BROWSER_CALL_TIMEOUT: Duration = Duration::from_secs(90);
/// Legacy limits of the browser runtime (`seanime-browser-host.cjs`).
const BROWSER_LIMIT: usize = 2;
const BROWSER_EXPR_BYTES: usize = 32 * 1024;
const BROWSER_RESULT_BYTES: usize = 2_000_000;

const PRELUDE: &str = include_str!("js/prelude.js");
const HOST_JS: &str = include_str!("js/host.js");

/// Per-extension console buffer shown by the Extensions screen.
#[derive(Clone, Default)]
pub struct LogBuf(Arc<Mutex<VecDeque<String>>>);

impl LogBuf {
    pub fn push(&self, level: &str, msg: &str) {
        let mut line = format!("[{level}] {msg}");
        if line.len() > LOG_LINE_BYTES {
            let mut cut = LOG_LINE_BYTES;
            while !line.is_char_boundary(cut) {
                cut -= 1;
            }
            line.truncate(cut);
            line.push_str("...");
        }
        if let Ok(mut q) = self.0.lock() {
            if q.len() >= LOG_LINES {
                q.pop_front();
            }
            q.push_back(line);
        }
    }

    pub fn lines(&self) -> Vec<String> {
        self.0.lock().map(|q| q.iter().cloned().collect()).unwrap_or_default()
    }
}

pub(crate) struct WorkerConfig {
    pub id: String,
    pub manifest_json: String,
    pub bundle: Arc<String>,
    pub permissions: Vec<String>,
    pub http: SafeHttp,
    pub logs: LogBuf,
    pub call_timeout: Duration,
    /// Present only when the bundle may use a browser (permitted `browserDomains` and an injected runtime).
    pub browser: Option<BrowserCfg>,
    /// User preferences as a JSON object of strings (wave B, `ext.preferences`), exposed to the bundle as
    /// `globalThis.preferences` (and `instance.setPreferences(...)` / `instance.preferences`) before it loads.
    pub preferences: String,
}

pub(crate) struct BrowserCfg {
    pub runtime: Arc<dyn BrowserRuntime>,
    /// `browser_domains(manifest)`; never empty.
    pub domains: Vec<String>,
}

/// Browsers this worker opened, with their per-operation timeout. Shared by the natives and the cleanup so a
/// browser whose id never reached JS (stale completion, discarded runtime) is still closed.
type OwnedBrowsers = Arc<Mutex<HashMap<String, Duration>>>;

pub(crate) enum CallKind {
    Route,
    Method,
}

enum Cmd {
    Call { kind: CallKind, name: String, payload: String, reply: oneshot::Sender<Result<String, ExtError>> },
}

pub(crate) struct Worker {
    tx: mpsc::Sender<Cmd>,
}

impl Worker {
    /// Spawn the worker thread. Must be called from inside a tokio runtime (its handle drives `fetch`).
    pub fn spawn(cfg: WorkerConfig) -> Result<Self, ExtError> {
        let handle = tokio::runtime::Handle::try_current().map_err(|_| ExtError::Runtime("no tokio runtime".into()))?;
        let (tx, rx) = mpsc::channel::<Cmd>();
        std::thread::Builder::new()
            .name(format!("ext-{}", cfg.id))
            .stack_size(8 * 1024 * 1024)
            .spawn(move || worker_main(cfg, handle, rx))
            .map_err(|e| ExtError::Runtime(e.to_string()))?;
        Ok(Self { tx })
    }

    pub async fn call(&self, kind: CallKind, name: String, payload: String) -> Result<String, ExtError> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(Cmd::Call { kind, name, payload, reply })
            .map_err(|_| ExtError::Runtime("extension worker stopped".into()))?;
        rx.await.map_err(|_| ExtError::Runtime("extension worker stopped".into()))?
    }
}

enum Event {
    Fetch { id: u64, result: Result<FetchOut, String> },
    /// `created`: the id a `new` operation opened (closed when the completion turns out to be stale).
    Browser { seq: u64, result: Result<serde_json::Value, String>, created: Option<String> },
}

struct FetchOut {
    status: u16,
    status_text: String,
    url: String,
    redirected: bool,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

struct Js {
    ctx: Context,
    /// Keeps the runtime alive for the lifetime of the context (declared after `ctx` so it drops last).
    _rt: Runtime,
}

fn worker_main(cfg: WorkerConfig, handle: tokio::runtime::Handle, rx: mpsc::Receiver<Cmd>) {
    let (ev_tx, ev_rx) = mpsc::channel::<Event>();
    let start = Instant::now();
    let deadline = Arc::new(AtomicU64::new(u64::MAX));
    let owned: OwnedBrowsers = Arc::default();
    let mut js: Option<Js> = None;

    while let Ok(Cmd::Call { kind, name, payload, reply }) = rx.recv() {
        // stale completions from a previous call; a browser opened for it that JS never saw is closed
        while let Ok(ev) = ev_rx.try_recv() {
            if let Event::Browser { created: Some(id), .. } = ev {
                close_owned(&cfg, &handle, &owned, Some(id));
            }
        }
        deadline.store(start.elapsed().as_millis() as u64 + cfg.call_timeout.as_millis() as u64, Ordering::SeqCst);
        let result = (|| {
            if js.is_none() {
                js = Some(build(&cfg, &handle, &ev_tx, &deadline, &owned, start)?);
            }
            let state = js.as_ref().ok_or_else(|| ExtError::Runtime("no runtime".into()))?;
            run_call(state, &ev_rx, &deadline, start, kind, &name, &payload)
        })();
        if matches!(result, Err(ExtError::Timeout | ExtError::OutOfMemory | ExtError::Runtime(_))) {
            js = None; // discard; rebuilt on the next call
            close_owned(&cfg, &handle, &owned, None); // its browser handles died with it
        }
        deadline.store(u64::MAX, Ordering::SeqCst);
        let _ = reply.send(result);
    }
    close_owned(&cfg, &handle, &owned, None); // extension stopped (disabled, removed, replaced, host dropped)
}

/// Close one owned browser (`Some(id)`) or all of them, without waiting.
fn close_owned(cfg: &WorkerConfig, handle: &tokio::runtime::Handle, owned: &OwnedBrowsers, only: Option<String>) {
    let Some(b) = &cfg.browser else { return };
    let ids: Vec<String> = {
        let mut map = owned.lock().unwrap_or_else(|p| p.into_inner());
        match only {
            Some(id) => map.remove(&id).map(|_| id).into_iter().collect(),
            None => map.drain().map(|(id, _)| id).collect(),
        }
    };
    for id in ids {
        let rt = b.runtime.clone();
        handle.spawn(async move {
            let _ = rt.close(&id).await;
        });
    }
}

fn describe(ctx: &Ctx<'_>, err: rquickjs::Error) -> ExtError {
    let msg = match err {
        rquickjs::Error::Exception => {
            let v = ctx.catch();
            match v.as_exception() {
                Some(ex) => ex.message().unwrap_or_else(|| "exception".into()),
                None => v.as_string().and_then(|s| s.to_string().ok()).unwrap_or_else(|| "exception".into()),
            }
        }
        rquickjs::Error::Allocation => "out of memory".to_owned(),
        other => other.to_string(),
    };
    classify(msg)
}

fn classify(msg: String) -> ExtError {
    let lower = msg.to_ascii_lowercase();
    if lower.contains("out of memory") {
        ExtError::OutOfMemory
    } else if lower.contains("interrupted") {
        ExtError::Timeout
    } else {
        ExtError::Js(msg)
    }
}

fn build(
    cfg: &WorkerConfig,
    handle: &tokio::runtime::Handle,
    ev_tx: &mpsc::Sender<Event>,
    deadline: &Arc<AtomicU64>,
    owned: &OwnedBrowsers,
    start: Instant,
) -> Result<Js, ExtError> {
    let rt = Runtime::new().map_err(|e| ExtError::Runtime(e.to_string()))?;
    rt.set_memory_limit(MEMORY_LIMIT);
    rt.set_max_stack_size(STACK_LIMIT);
    let dl = deadline.clone();
    rt.set_interrupt_handler(Some(Box::new(move || start.elapsed().as_millis() as u64 > dl.load(Ordering::Relaxed))));
    let ctx = Context::full(&rt).map_err(|e| ExtError::Runtime(e.to_string()))?;
    ctx.with(|ctx| -> Result<(), ExtError> {
        install_natives(&ctx, cfg, handle, ev_tx, start).map_err(|e| describe(&ctx, e))?;
        install_browser_natives(&ctx, cfg, handle, ev_tx, deadline, owned).map_err(|e| describe(&ctx, e))?;
        ctx.eval::<(), _>(PRELUDE).map_err(|e| describe(&ctx, e))?;
        ctx.eval::<(), _>(HOST_JS).map_err(|e| describe(&ctx, e))?;
        let set_prefs: Function = ctx.globals().get("__tk_set_prefs").map_err(|e| describe(&ctx, e))?;
        set_prefs.call::<_, ()>((cfg.preferences.clone(),)).map_err(|e| describe(&ctx, e))?;
        let load: Function = ctx.globals().get("__tk_load").map_err(|e| describe(&ctx, e))?;
        load.call::<_, ()>((cfg.manifest_json.clone(), cfg.bundle.as_str().to_owned())).map_err(|e| describe(&ctx, e))?;
        Ok(())
    })?;
    Ok(Js { ctx, _rt: rt })
}

/// Run pending promise jobs. NOTE: `Runtime::execute_pending_job` is avoided on purpose: rquickjs 0.9 builds its
/// `JobException(Context)` without taking a context reference, so dropping it corrupts the QuickJS GC.
/// `Ctx::execute_pending_job` + `catch()` has the same effect without that hazard.
fn pump(js: &Js) -> Result<(), ExtError> {
    loop {
        let step = js.ctx.with(|ctx| -> Result<bool, ExtError> {
            if !ctx.execute_pending_job() {
                return Ok(false);
            }
            let ex = ctx.catch();
            if matches!(ex.type_of(), rquickjs::Type::Uninitialized | rquickjs::Type::Undefined | rquickjs::Type::Null) {
                return Ok(true);
            }
            let msg = match ex.as_exception() {
                Some(e) => e.message().unwrap_or_else(|| "exception".into()),
                None => ex.as_string().and_then(|s| s.to_string().ok()).unwrap_or_else(|| "exception".into()),
            };
            Err(classify(msg))
        })?;
        if !step {
            return Ok(());
        }
    }
}

#[derive(Deserialize)]
struct Status {
    done: bool,
    #[serde(default)]
    envelope: Option<String>,
    #[serde(default)]
    timer: f64,
    #[serde(default)]
    fetches: u32,
    #[serde(default)]
    browsers: u32,
}

#[derive(Deserialize)]
struct Envelope {
    ok: bool,
    #[serde(default)]
    value: serde_json::Value,
    #[serde(default)]
    status: u16,
    #[serde(default)]
    message: String,
}

fn run_call(
    js: &Js,
    ev_rx: &mpsc::Receiver<Event>,
    deadline: &AtomicU64,
    start: Instant,
    kind: CallKind,
    name: &str,
    payload: &str,
) -> Result<String, ExtError> {
    let kind_str = match kind {
        CallKind::Route => "route",
        CallKind::Method => "method",
    };
    js.ctx.with(|ctx| -> Result<(), ExtError> {
        let f: Function = ctx.globals().get("__tk_dispatch").map_err(|e| describe(&ctx, e))?;
        f.call::<_, ()>((kind_str, name.to_owned(), payload.to_owned())).map_err(|e| describe(&ctx, e))
    })?;

    loop {
        pump(js)?;
        let status_json: String = js.ctx.with(|ctx| -> Result<String, ExtError> {
            let f: Function = ctx.globals().get("__tk_status").map_err(|e| describe(&ctx, e))?;
            f.call::<_, String>(()).map_err(|e| describe(&ctx, e))
        })?;
        let status: Status = serde_json::from_str(&status_json).map_err(|e| ExtError::Runtime(e.to_string()))?;
        if status.done {
            let env: Envelope = serde_json::from_str(status.envelope.as_deref().unwrap_or("{}")).map_err(|e| ExtError::Runtime(e.to_string()))?;
            return if env.ok {
                Ok(env.value.to_string())
            } else {
                Err(match env.status {
                    400 => ExtError::BadRequest(env.message),
                    404 => ExtError::NotFound(env.message),
                    501 => ExtError::NotImplemented(env.message),
                    _ => classify(env.message),
                })
            };
        }

        let now_ms = start.elapsed().as_millis() as u64;
        let dl = deadline.load(Ordering::SeqCst);
        if now_ms >= dl {
            return Err(ExtError::Timeout);
        }
        if status.timer == 0.0 {
            js.ctx.with(|ctx| -> Result<(), ExtError> {
                let f: Function = ctx.globals().get("__tk_run_timers").map_err(|e| describe(&ctx, e))?;
                f.call::<_, ()>(()).map_err(|e| describe(&ctx, e))
            })?;
            continue;
        }
        if status.timer < 0.0 && status.fetches == 0 && status.browsers == 0 {
            return Err(ExtError::Js("extension call never settled (no pending timers or requests)".into()));
        }
        let mut wait = Duration::from_millis(dl - now_ms);
        if status.timer > 0.0 {
            wait = wait.min(Duration::from_millis(status.timer.ceil() as u64));
        }
        match ev_rx.recv_timeout(wait) {
            Ok(Event::Fetch { id, result }) => deliver_fetch(js, id, result)?,
            Ok(Event::Browser { seq, result, .. }) => deliver_browser(js, seq, result)?,
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return Err(ExtError::Runtime("event channel closed".into())),
        }
    }
}

fn deliver_fetch(js: &Js, id: u64, result: Result<FetchOut, String>) -> Result<(), ExtError> {
    js.ctx.with(|ctx| -> Result<(), ExtError> {
        let f: Function = ctx.globals().get("__tk_fetch_done").map_err(|e| describe(&ctx, e))?;
        match result {
            Ok(out) => {
                let meta = json!({
                    "status": out.status, "statusText": out.status_text, "url": out.url, "redirected": out.redirected,
                    "headers": out.headers,
                })
                .to_string();
                let ab = ArrayBuffer::new(ctx.clone(), out.body).map_err(|e| describe(&ctx, e))?;
                f.call::<_, ()>((id as f64, meta, ab)).map_err(|e| describe(&ctx, e))
            }
            Err(msg) => f
                .call::<_, ()>((id as f64, json!({ "error": msg }).to_string(), Option::<ArrayBuffer>::None))
                .map_err(|e| describe(&ctx, e)),
        }
    })
}

fn deliver_browser(js: &Js, seq: u64, result: Result<serde_json::Value, String>) -> Result<(), ExtError> {
    let msg = match result {
        Ok(value) => json!({ "ok": true, "value": value }),
        Err(error) => json!({ "ok": false, "error": error }),
    };
    js.ctx.with(|ctx| -> Result<(), ExtError> {
        let f: Function = ctx.globals().get("__tk_browser_done").map_err(|e| describe(&ctx, e))?;
        f.call::<_, ()>((seq as f64, msg.to_string())).map_err(|e| describe(&ctx, e))
    })
}

// ---- natives --------------------------------------------------------------------------------------------------------

thread_local! {
    static DOCS: RefCell<Vec<Html>> = const { RefCell::new(Vec::new()) };
}

#[derive(Deserialize)]
struct FetchMeta {
    url: String,
    method: String,
    #[serde(default)]
    headers: BTreeMap<String, String>,
}

fn type_err(ctx: &Ctx<'_>, msg: &str) -> rquickjs::Error {
    Exception::throw_type(ctx, msg)
}

fn index_of_all(html: &Html, els: &[ElementRef<'_>]) -> Vec<i32> {
    let index: std::collections::HashMap<_, _> = html.tree.nodes().enumerate().map(|(i, n)| (n.id(), i)).collect();
    els.iter().filter_map(|e| index.get(&e.id()).map(|i| *i as i32)).collect()
}

fn with_el<R>(ctx: &Ctx<'_>, doc: i32, node: i32, f: impl FnOnce(&Html, ElementRef<'_>) -> R) -> rquickjs::Result<R> {
    DOCS.with(|d| {
        let docs = d.borrow();
        let html = docs.get(doc as usize).ok_or_else(|| type_err(ctx, "invalid document handle"))?;
        let n = html.tree.nodes().nth(node.max(0) as usize).ok_or_else(|| type_err(ctx, "invalid node handle"))?;
        let el = ElementRef::wrap(n).ok_or_else(|| type_err(ctx, "node is not an element"))?;
        Ok(f(html, el))
    })
}

fn parse_selector(ctx: &Ctx<'_>, s: &str) -> rquickjs::Result<Selector> {
    Selector::parse(s).map_err(|_| Exception::throw_syntax(ctx, &format!("'{s}' is not a valid selector")))
}

fn install_natives<'js>(
    ctx: &Ctx<'js>,
    cfg: &WorkerConfig,
    handle: &tokio::runtime::Handle,
    ev_tx: &mpsc::Sender<Event>,
    start: Instant,
) -> rquickjs::Result<()> {
    let g = ctx.globals();
    g.set("__host_platform", "win32")?;

    let logs = cfg.logs.clone();
    g.set("__host_log", Function::new(ctx.clone(), move |level: String, msg: String| {
        logs.push(&level, &msg);
    })?)?;
    g.set("__host_now", Function::new(ctx.clone(), move || start.elapsed().as_secs_f64() * 1000.0)?)?;

    g.set("__host_random", Function::new(ctx.clone(), |ctx: Ctx<'js>, n: usize| -> rquickjs::Result<ArrayBuffer<'js>> {
        let mut bytes = Vec::with_capacity(n + 16);
        while bytes.len() < n {
            bytes.extend_from_slice(uuid::Uuid::new_v4().as_bytes());
        }
        bytes.truncate(n);
        ArrayBuffer::new(ctx, bytes)
    })?)?;
    g.set("__host_hash", Function::new(ctx.clone(), |ctx: Ctx<'js>, algo: String, data: ArrayBuffer<'js>| -> rquickjs::Result<ArrayBuffer<'js>> {
        let a = Algo::parse(&algo).ok_or_else(|| Exception::throw_message(&ctx, "Digest method not supported"))?;
        ArrayBuffer::new(ctx.clone(), a.digest(data.as_bytes().unwrap_or(&[])))
    })?)?;
    g.set("__host_hmac", Function::new(ctx.clone(), |ctx: Ctx<'js>, algo: String, key: ArrayBuffer<'js>, data: ArrayBuffer<'js>| -> rquickjs::Result<ArrayBuffer<'js>> {
        let a = Algo::parse(&algo).ok_or_else(|| Exception::throw_message(&ctx, "Digest method not supported"))?;
        ArrayBuffer::new(ctx.clone(), hmac(a, key.as_bytes().unwrap_or(&[]), data.as_bytes().unwrap_or(&[])))
    })?)?;
    g.set("__host_utf8_encode", Function::new(ctx.clone(), |ctx: Ctx<'js>, s: String| ArrayBuffer::new(ctx, s.into_bytes()))?)?;
    g.set("__host_utf8_decode", Function::new(ctx.clone(), |b: ArrayBuffer<'js>| -> String {
        String::from_utf8_lossy(b.as_bytes().unwrap_or(&[])).into_owned()
    })?)?;
    g.set("__host_b64_encode", Function::new(ctx.clone(), |b: ArrayBuffer<'js>| -> String {
        use base64::Engine;
        base64::engine::general_purpose::STANDARD.encode(b.as_bytes().unwrap_or(&[]))
    })?)?;
    g.set("__host_b64_decode", Function::new(ctx.clone(), |ctx: Ctx<'js>, s: String| -> rquickjs::Result<ArrayBuffer<'js>> {
        use base64::Engine;
        let t = s.trim_end_matches('=');
        let bytes = base64::engine::general_purpose::STANDARD_NO_PAD
            .decode(t)
            .map_err(|_| Exception::throw_message(&ctx, "invalid base64"))?;
        ArrayBuffer::new(ctx, bytes)
    })?)?;
    g.set("__host_hex_encode", Function::new(ctx.clone(), |b: ArrayBuffer<'js>| -> String { hex::encode(b.as_bytes().unwrap_or(&[])) })?)?;
    g.set("__host_hex_decode", Function::new(ctx.clone(), |ctx: Ctx<'js>, s: String| -> rquickjs::Result<ArrayBuffer<'js>> {
        // Node semantics: decode up to the first invalid pair
        let valid: String = s.chars().take_while(char::is_ascii_hexdigit).collect();
        let valid = &valid[..valid.len() - valid.len() % 2];
        ArrayBuffer::new(ctx, hex::decode(valid).unwrap_or_default())
    })?)?;

    // fetch: permission gate + SSRF-safe client; completion arrives through the event loop
    let perms = cfg.permissions.clone();
    let http = cfg.http.clone();
    let tx = ev_tx.clone();
    let rt_handle = handle.clone();
    g.set("__host_fetch_start", Function::new(ctx.clone(), move |ctx: Ctx<'js>, id: f64, meta: String, body: Option<ArrayBuffer<'js>>| -> rquickjs::Result<()> {
        let m: FetchMeta = serde_json::from_str(&meta).map_err(|_| type_err(&ctx, "invalid fetch request"))?;
        let url = url::Url::parse(&m.url).map_err(|_| type_err(&ctx, "Invalid URL"))?;
        let host = url.host_str().unwrap_or_default().to_owned();
        if !tk_core::net::host_matches_permissions(&host, &perms) {
            return Err(Exception::throw_message(&ctx, "Network permission denied"));
        }
        let method = reqwest::Method::from_bytes(m.method.as_bytes()).map_err(|_| type_err(&ctx, "invalid HTTP method"))?;
        let body_bytes = body.and_then(|b| b.as_bytes().map(<[u8]>::to_vec));
        let http = http.clone();
        let tx = tx.clone();
        let id = id as u64;
        rt_handle.spawn(async move {
            let result = do_fetch(&http, url, method, m.headers, body_bytes).await;
            let _ = tx.send(Event::Fetch { id, result });
        });
        Ok(())
    })?)?;

    // HTML documents (cheerio-like adapter lives in prelude.js)
    g.set("__host_html_parse", Function::new(ctx.clone(), |html: String| -> i32 {
        DOCS.with(|d| {
            let mut docs = d.borrow_mut();
            docs.push(Html::parse_document(&html));
            (docs.len() - 1) as i32
        })
    })?)?;
    g.set("__host_html_root", Function::new(ctx.clone(), |ctx: Ctx<'js>, doc: i32| -> rquickjs::Result<i32> {
        DOCS.with(|d| {
            let docs = d.borrow();
            let html = docs.get(doc as usize).ok_or_else(|| type_err(&ctx, "invalid document handle"))?;
            Ok(index_of_all(html, &[html.root_element()]).first().copied().unwrap_or(0))
        })
    })?)?;
    g.set("__host_html_select", Function::new(ctx.clone(), |ctx: Ctx<'js>, doc: i32, node: i32, sel: String| -> rquickjs::Result<Vec<i32>> {
        let selector = parse_selector(&ctx, &sel)?;
        with_el(&ctx, doc, node, |html, el| {
            let found: Vec<ElementRef<'_>> = el.select(&selector).collect();
            index_of_all(html, &found)
        })
    })?)?;
    g.set("__host_html_children", Function::new(ctx.clone(), |ctx: Ctx<'js>, doc: i32, node: i32, sel: String| -> rquickjs::Result<Vec<i32>> {
        let selector = if sel.is_empty() { None } else { Some(parse_selector(&ctx, &sel)?) };
        with_el(&ctx, doc, node, |html, el| {
            let kids: Vec<ElementRef<'_>> =
                el.children().filter_map(ElementRef::wrap).filter(|c| selector.as_ref().is_none_or(|s| s.matches(c))).collect();
            index_of_all(html, &kids)
        })
    })?)?;
    g.set("__host_html_parent", Function::new(ctx.clone(), |ctx: Ctx<'js>, doc: i32, node: i32| -> rquickjs::Result<i32> {
        with_el(&ctx, doc, node, |html, el| {
            el.parent().and_then(ElementRef::wrap).map_or(-1, |p| index_of_all(html, &[p]).first().copied().unwrap_or(-1))
        })
    })?)?;
    g.set("__host_html_sibling", Function::new(ctx.clone(), |ctx: Ctx<'js>, doc: i32, node: i32, dir: i32| -> rquickjs::Result<i32> {
        with_el(&ctx, doc, node, |html, el| {
            let sib = if dir >= 0 { el.next_siblings().find_map(ElementRef::wrap) } else { el.prev_siblings().find_map(ElementRef::wrap) };
            sib.map_or(-1, |p| index_of_all(html, &[p]).first().copied().unwrap_or(-1))
        })
    })?)?;
    g.set("__host_html_attr", Function::new(ctx.clone(), |ctx: Ctx<'js>, doc: i32, node: i32, name: String| -> rquickjs::Result<Option<String>> {
        with_el(&ctx, doc, node, |_, el| el.value().attr(&name).map(str::to_owned))
    })?)?;
    g.set("__host_html_text", Function::new(ctx.clone(), |ctx: Ctx<'js>, doc: i32, node: i32| -> rquickjs::Result<String> {
        with_el(&ctx, doc, node, |_, el| el.text().collect::<String>())
    })?)?;
    g.set("__host_html_inner", Function::new(ctx.clone(), |ctx: Ctx<'js>, doc: i32, node: i32| -> rquickjs::Result<String> {
        with_el(&ctx, doc, node, |_, el| el.inner_html())
    })?)?;
    Ok(())
}

/// Browser runtime natives (`__host_browser_domains`, `__host_browser_start`); the JS objects are built by
/// `__tk_browser_runtime` in prelude.js. Without a [`BrowserCfg`] only an empty domain list is exposed, so the
/// bundle's `configureBrowserRuntime` is never called.
fn install_browser_natives<'js>(
    ctx: &Ctx<'js>,
    cfg: &WorkerConfig,
    handle: &tokio::runtime::Handle,
    ev_tx: &mpsc::Sender<Event>,
    deadline: &Arc<AtomicU64>,
    owned: &OwnedBrowsers,
) -> rquickjs::Result<()> {
    let g = ctx.globals();
    let Some(b) = &cfg.browser else {
        g.set("__host_browser_domains", "[]")?;
        return Ok(());
    };
    g.set("__host_browser_domains", serde_json::to_string(&b.domains).unwrap_or_else(|_| "[]".into()))?;
    let runtime = b.runtime.clone();
    let domains = Arc::new(b.domains.clone());
    let ext_id = cfg.id.clone();
    let owned = owned.clone();
    let tx = ev_tx.clone();
    let rt_handle = handle.clone();
    let dl = deadline.clone();
    let call_ms = cfg.call_timeout.as_millis() as u64;
    // op: new (arg = options JSON) | navigate (url) | evaluate (expression) | cookies (JSON url array) | close
    g.set("__host_browser_start", Function::new(ctx.clone(), move |seq: f64, op: String, id: String, arg: String| {
        if op == "new" {
            // A call that opens a browser may run up to BROWSER_CALL_TIMEOUT from its start (Cloudflare solve).
            let now = dl.load(Ordering::SeqCst);
            if now != u64::MAX {
                let extended = now.saturating_sub(call_ms) + BROWSER_CALL_TIMEOUT.as_millis() as u64;
                if extended > now {
                    dl.store(extended, Ordering::SeqCst);
                }
            }
        }
        let runtime = runtime.clone();
        let domains = domains.clone();
        let ext_id = ext_id.clone();
        let owned = owned.clone();
        let tx = tx.clone();
        let seq = seq as u64;
        rt_handle.spawn(async move {
            let (result, created) = browser_op(&*runtime, &ext_id, &domains, &owned, &op, &id, &arg).await;
            let _ = tx.send(Event::Browser { seq, result: result.map_err(|e| e.to_string()), created });
        });
    })?)?;
    Ok(())
}

fn lock_owned(owned: &OwnedBrowsers) -> std::sync::MutexGuard<'_, HashMap<String, Duration>> {
    owned.lock().unwrap_or_else(|p| p.into_inner())
}

/// Legacy: `Math.min(30, Math.max(1, Number(options.timeout) || 20))` seconds.
fn browser_op_timeout(opts: &serde_json::Value) -> Duration {
    let t = match opts.get("timeout") {
        Some(serde_json::Value::Number(n)) => n.as_f64(),
        Some(serde_json::Value::String(s)) => s.trim().parse::<f64>().ok(),
        _ => None,
    };
    let secs = t.filter(|t| t.is_finite() && *t != 0.0).unwrap_or(20.0).clamp(1.0, 30.0);
    Duration::from_secs_f64(secs)
}

/// Safety net over the runtime's own per-operation timeout.
async fn bounded<T>(timeout: Duration, fut: impl std::future::Future<Output = Result<T, BrowserError>>) -> Result<T, BrowserError> {
    tokio::time::timeout(timeout + Duration::from_secs(2), fut).await.unwrap_or(Err(BrowserError::Timeout))
}

fn capped(v: serde_json::Value) -> Result<serde_json::Value, BrowserError> {
    if serde_json::to_string(&v).map_or(0, |s| s.len()) > BROWSER_RESULT_BYTES { Err(BrowserError::TooLarge) } else { Ok(v) }
}

/// One browser operation with the legacy checks enforced here too (the runtime repeats the URL checks): a browser
/// id is only usable by the worker that opened it, at most [`BROWSER_LIMIT`] are open, every URL passes
/// [`browser_url_allowed`], expressions and results are size-capped. Returns the created id for `new`.
async fn browser_op(
    runtime: &dyn BrowserRuntime,
    ext_id: &str,
    domains: &[String],
    owned: &OwnedBrowsers,
    op: &str,
    id: &str,
    arg: &str,
) -> (Result<serde_json::Value, BrowserError>, Option<String>) {
    if op == "new" {
        if domains.is_empty() {
            return (Err(BrowserError::NoDomains), None);
        }
        if lock_owned(owned).len() >= BROWSER_LIMIT {
            return (Err(BrowserError::Limit), None);
        }
        let opts: serde_json::Value = serde_json::from_str(arg).unwrap_or(serde_json::Value::Null);
        let opts = if opts.is_object() { opts } else { json!({}) };
        let timeout = browser_op_timeout(&opts);
        return match runtime.new_browser(ext_id, domains, opts).await {
            Ok(id) => {
                lock_owned(owned).insert(id.clone(), timeout);
                (Ok(serde_json::Value::String(id.clone())), Some(id))
            }
            Err(e) => (Err(e), None),
        };
    }
    let Some(timeout) = lock_owned(owned).get(id).copied() else {
        return (Err(BrowserError::Closed), None);
    };
    let id: String = id.to_owned();
    let result = match op {
        "close" => {
            lock_owned(owned).remove(&id);
            runtime.close(&id).await.map(|()| serde_json::Value::Null)
        }
        "navigate" => {
            if browser_url_allowed(arg, domains) {
                bounded(timeout, runtime.navigate(&id, arg)).await.map(|()| serde_json::Value::Null)
            } else {
                Err(BrowserError::UrlNotAllowed(arg.to_owned()))
            }
        }
        "evaluate" => {
            if arg.len() > BROWSER_EXPR_BYTES {
                Err(BrowserError::Failed("Invalid ChromeDP expression".into()))
            } else {
                bounded(timeout, runtime.evaluate(&id, arg)).await.and_then(capped)
            }
        }
        "cookies" => {
            let urls: Vec<String> = serde_json::from_str(arg).unwrap_or_default();
            if urls.is_empty() || urls.len() > 10 {
                Err(BrowserError::Failed("ChromeDP cookie URLs are required".into()))
            } else if let Some(bad) = urls.iter().find(|u| !browser_url_allowed(u, domains)) {
                Err(BrowserError::UrlNotAllowed(bad.clone()))
            } else {
                bounded(timeout, runtime.cookies(&id, &urls)).await.map(|c| json!({ "cookies": c })).and_then(capped)
            }
        }
        other => Err(BrowserError::Failed(format!("Unsupported ChromeDP browser method: {other}"))),
    };
    if matches!(result, Err(BrowserError::Closed)) {
        lock_owned(owned).remove(&id); // auto-closed (150 s) or gone
    }
    (result, None)
}

async fn do_fetch(
    http: &SafeHttp,
    url: url::Url,
    method: reqwest::Method,
    headers: BTreeMap<String, String>,
    body: Option<Vec<u8>>,
) -> Result<FetchOut, String> {
    let headers = sanitize_headers(http.policy(), &headers).map_err(|e| e.to_string())?;
    let original = url.clone();
    let req = SafeRequest { method, url, headers, body: body.map(Into::into) };
    let resp = http.send(req).await.map_err(|e| e.to_string())?;
    let status = resp.response.status();
    let final_url = resp.final_url.to_string();
    let headers: Vec<(String, String)> = resp
        .response
        .headers()
        .iter()
        .filter_map(|(k, v)| v.to_str().ok().map(|v| (k.as_str().to_owned(), v.to_owned())))
        .collect();
    let body = read_capped(resp.response, FETCH_CAP).await.map_err(|e| e.to_string())?;
    Ok(FetchOut {
        status: status.as_u16(),
        status_text: status.canonical_reason().unwrap_or("").to_owned(),
        redirected: final_url != original.as_str(),
        url: final_url,
        headers,
        body: body.to_vec(),
    })
}
