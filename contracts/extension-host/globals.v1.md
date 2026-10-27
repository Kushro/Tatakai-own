# Extension host - globals, `require` and compatibility levels (v1)

Contract for the native `.kai` hosts: Kotlin (`apps/android-native/core/extensions`, `core/extensions-quickjs`, QuickJS via
`quickjs-kt`) and Rust (`apps/desktop-native/crates/tk-ext`, `rquickjs`). It is derived from what the legacy loaders inject:
`src/core/extensions/mobile/mobileExtensionHost.ts` (`evaluateBundle`), `requireShim.ts`, `parseHtml.ts`, and the routes in
`desktop/runtime/extension-api-host/host-server.cjs`. Routes and shapes: `routes.v1.json`. Fixtures: `../fixtures/extensions/`.

## 1. Evaluation model

* One QuickJS runtime per installed extension (never shared). Memory limit **64 MiB**, interrupt handler + wall-clock
  timeout **15 s per host call** (a route call, not the whole session). Exceeding either fails the call with `ExtError::Timeout` /
  `ExtError::OutOfMemory`; the runtime is discarded and re-created on the next call.
* The bundle is evaluated as a CommonJS function body with the parameters, in this order (same as the legacy `new Function`):
  `module, exports, require, __tatakai_fetch__, __tatakai_anitomy__, __tatakai_parse_html__, Buffer, process, global, setImmediate, clearImmediate, fetch`.
* Instance = `module.exports.default || module.exports`; if it is a function it is called with `new`. The result must be an object,
  otherwise installation fails ("produced no usable export").
* No ambient authority: no `fs`, no sockets, no `eval` of remote code, no timers beyond `setTimeout/setInterval/setImmediate`
  driven by the host event loop, no access to the app database or Supabase session.
* Bundle size <= 10 MiB (`10485760` bytes), checked at install time on the uncompressed `bundle.js`.

## 2. Globals the host must provide

| Name | Behaviour |
|---|---|
| `fetch(url, init?)` | Host implemented (OkHttp / reqwest). Returns a Fetch-like `Response` (`ok, status, statusText, headers.get(), text(), json(), arrayBuffer()`). **Gated** by the manifest: host must match `network:domain:<host>` (exact or subdomain; `*` = any) else rejects with `Error('Network permission denied')`. Applies the SSRF policy of the platform (no loopback / private / link-local targets, DNS re-validation after redirects, max 5 redirects, 30 s timeout, response cap 25 MiB). Cookies: none persisted across calls. |
| `__tatakai_fetch__` | Same function object as `fetch` (some bundles capture `globalThis.__tatakai_fetch__` at load, others use the bare identifier: define BOTH the parameter and the global before evaluating the bundle). |
| `__tatakai_parse_html__(html)` -> cheerio-like | `parseHtml`. Throws `InvalidInputError` for empty / non-string input. Chainable adapter, one-for-one with `parseHtml.ts`: `length, eq(i), last(), parent(), children(sel?), next(), prev(), find(sel), first(), attr(name), text(), html(), each((i, el) => void)`. Kotlin: jsoup. Rust: `scraper`. |
| `__tatakai_anitomy__(filename)` | Optional. Returns `{}` when the host has no anitomy; must exist as a function. |
| `Buffer` | Minimal: `Buffer.from(string|array|ArrayBuffer, encoding?)`, `alloc`, `concat`, `isBuffer`, `byteLength`, instance `toString(encoding)` for `utf8 | hex | base64 | latin1`, `slice/subarray`, `length`. |
| `process` | Inert stub, same shape as `createMobileNodeGlobals`: `platform:'android'|'win32'`, `versions.node:'20.11.0'`, `env:{NODE_ENV:'production'}`, `nextTick` (microtask), `hrtime` (+`.bigint`), `cwd()`, `argv`, `on/once/off/emit` no-ops, `exit/kill` no-ops. |
| `global` / `globalThis` | The runtime global object. |
| `setTimeout / clearTimeout / setInterval / clearInterval / setImmediate / clearImmediate` | Host event loop. |
| `console` | `log/info/warn/error/debug` forwarded to the per-extension log buffer (Extensions screen shows it). Never to logcat/stdout with payloads over 4 KiB. |
| `URL`, `URLSearchParams` | Standard semantics (WHATWG). |
| `TextEncoder`, `TextDecoder` | `utf-8` (and `utf-16le` decode). |
| `atob`, `btoa` | Standard. |
| `crypto` (global) | `randomUUID()`, `getRandomValues(typedArray)`. |
| `queueMicrotask`, `structuredClone` | Standard. |
| `performance.now()` | Monotonic ms. |

Not provided (accessing them is `undefined`): `window`, `document`, `XMLHttpRequest`, `WebSocket`, `localStorage`, `navigator`
(except `navigator.userAgent` string), `Worker`, `import()` of remote URLs.

## 3. `require(name)` whitelist

`name` has an optional `node:` prefix stripped. Resolution order: supported module -> load-safe stub -> throws
`Error("Cannot find module 'x'")`.

| Tier | Modules | Behaviour |
|---|---|---|
| **supported** | `crypto` (subset: `createHash('md5'|'sha1'|'sha256'|'sha512').update(data).digest('hex'|'base64')`, `createHmac` same algos, `randomUUID`, `randomBytes(n)` -> Buffer), `buffer` (`{ Buffer }`), `events` (EventEmitter: `on/once/off/emit/removeListener/removeAllListeners/listenerCount`), `url` (`URL`, `URLSearchParams`, `parse`, `format`, `fileURLToPath` throws), `path` (posix `join/resolve/basename/dirname/extname/normalize/sep`), `querystring` (`parse/stringify`), `util` (`format`, `inspect` (JSON-ish), `promisify`, `types`, `TextEncoder`, `TextDecoder`, `inherits`, `isDeepStrictEqual`) | Real behaviour. |
| **stub (load-safe)** | `os`, `http`, `https`, `stream`, `zlib`, `string_decoder`, `assert`, `timers`, `readline`, `perf_hooks`, `async_hooks`, `module`, `tty` | `require` succeeds so the bundle registers; **every function/method throws** `Error('<module>.<fn> is not available in the native extension runtime')` only when called. Properties read as `undefined`/`false`. |
| **unsupported** | `fs`, `child_process`, `net`, `tls`, `dgram`, `cluster`, `worker_threads`, `vm`, `dns`, `http2`, `inspector`, `v8`, `repl`, any non-builtin bare specifier (e.g. `undici`, `cheerio`), any relative specifier (`./x`, `../x`) | `require` throws `Error("Module '<name>' is not supported by the native extension runtime")`. |

## 4. Compatibility level (computed at install time, stored with the extension)

Static analysis of `bundle.js` (no execution). Collect specifiers with:

```
/\brequire\s*\(\s*(['"`])([^'"`\r\n]+)\1\s*\)/g          # string-literal calls; strip a leading "node:"
```

| Level | Rule |
|---|---|
| `full` | Every collected specifier is **supported**, and there is no non-literal `require(...)`. |
| `partial` | At least one specifier is a **stub**, or a non-literal `require(<expr>)` / `require.resolve` / dynamic `import()` exists, and nothing is unsupported. Installable and enabled by default, UI shows a warning chip. |
| `unsupported` | At least one **unsupported** specifier. Installation SUCCEEDS (the user can see why) but the extension is forced `enabled=false`; UI explains: `Uses Node module 'fs' which the native runtime does not provide`. |

Additional install-time validation (error, not a compat level): `manifest.json` missing/invalid JSON, missing required fields
(`id, name, version, type, description, permissions`), `type` not in `torrent|onlinestream|custom`, `id` not matching
`^[a-z0-9][a-z0-9._-]{0,63}$`, `bundle.js` missing or > 10 MiB, a ZIP entry whose path escapes the archive (`..`, absolute path, drive letter), more than
64 entries, or uncompressed size over 32 MiB. Error messages name the failing field.

Renderer extensions (`manifest.contributes.rendererEntry`, ESM `activate(ctx)` with React) are **ignored** by the native hosts:
the entry file is not extracted and never executed. `contributes.themes` are ignored in M0 (importable as tokens in M3).

## 5. Fixtures that both hosts must pass

`contracts/fixtures/extensions/` (source directories; `node contracts/fixtures/extensions/build.mjs` zips them to `dist/*.kai`):

| Fixture | Expected |
|---|---|
| `synthetic-full` | `compat = full`; `health`, `providers` -> 2 providers (`alpha` hls, `beta` mp4), `stream?anilistId=1&episode=3` -> the 3 normalized sources listed in `expected.stream.json` (order: providerPriority asc), `custom/home` returns 1 section; `fetch` to a non-permitted domain rejects with `Network permission denied`. |
| `synthetic-fs` | `compat = unsupported` (requires `fs`); installs but `enabled=false`; any `call` returns `ExtError::Unsupported`. |
