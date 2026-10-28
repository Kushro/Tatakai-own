//! One QuickJS runtime per extension, each on its own OS thread.
//!
//! Limits (globals.v1.md section 1): 64 MiB heap, 15 s wall-clock per host call (interrupt handler + event-loop
//! deadline). A call that times out or runs out of memory discards the runtime; the next call rebuilds it.
//! Network `fetch` is started on the shared tokio runtime and its completion is delivered back to this thread's
//! event loop, so several fetches of one extension run in parallel.

use std::cell::RefCell;
use std::collections::{BTreeMap, VecDeque};
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

use crate::error::ExtError;
use crate::hashing::{Algo, hmac};

pub const CALL_TIMEOUT: Duration = Duration::from_secs(15);
pub const MEMORY_LIMIT: usize = 64 * 1024 * 1024;
const STACK_LIMIT: usize = 1024 * 1024;
pub const FETCH_CAP: usize = 25 * 1024 * 1024;
const LOG_LINES: usize = 500;
const LOG_LINE_BYTES: usize = 4096;

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
}

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
    let mut js: Option<Js> = None;

    while let Ok(Cmd::Call { kind, name, payload, reply }) = rx.recv() {
        while ev_rx.try_recv().is_ok() {} // stale completions from a previous call
        deadline.store(start.elapsed().as_millis() as u64 + cfg.call_timeout.as_millis() as u64, Ordering::SeqCst);
        let result = (|| {
            if js.is_none() {
                js = Some(build(&cfg, &handle, &ev_tx, &deadline, start)?);
            }
            let state = js.as_ref().ok_or_else(|| ExtError::Runtime("no runtime".into()))?;
            run_call(state, &ev_rx, &deadline, start, kind, &name, &payload)
        })();
        if matches!(result, Err(ExtError::Timeout | ExtError::OutOfMemory | ExtError::Runtime(_))) {
            js = None; // discard; rebuilt on the next call
        }
        deadline.store(u64::MAX, Ordering::SeqCst);
        let _ = reply.send(result);
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

fn build(cfg: &WorkerConfig, handle: &tokio::runtime::Handle, ev_tx: &mpsc::Sender<Event>, deadline: &Arc<AtomicU64>, start: Instant) -> Result<Js, ExtError> {
    let rt = Runtime::new().map_err(|e| ExtError::Runtime(e.to_string()))?;
    rt.set_memory_limit(MEMORY_LIMIT);
    rt.set_max_stack_size(STACK_LIMIT);
    let dl = deadline.clone();
    rt.set_interrupt_handler(Some(Box::new(move || start.elapsed().as_millis() as u64 > dl.load(Ordering::Relaxed))));
    let ctx = Context::full(&rt).map_err(|e| ExtError::Runtime(e.to_string()))?;
    ctx.with(|ctx| -> Result<(), ExtError> {
        install_natives(&ctx, cfg, handle, ev_tx, start).map_err(|e| describe(&ctx, e))?;
        ctx.eval::<(), _>(PRELUDE).map_err(|e| describe(&ctx, e))?;
        ctx.eval::<(), _>(HOST_JS).map_err(|e| describe(&ctx, e))?;
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
        if status.timer < 0.0 && status.fetches == 0 {
            return Err(ExtError::Js("extension call never settled (no pending timers or requests)".into()));
        }
        let mut wait = Duration::from_millis(dl - now_ms);
        if status.timer > 0.0 {
            wait = wait.min(Duration::from_millis(status.timer.ceil() as u64));
        }
        match ev_rx.recv_timeout(wait) {
            Ok(Event::Fetch { id, result }) => deliver_fetch(js, id, result)?,
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
