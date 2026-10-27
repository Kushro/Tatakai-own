// Tatakai native extension host - JS shims (globals.v1.md). Evaluated once per runtime, before the bundle.
// Host natives (__host_*) are installed from Rust (runtime.rs). No ambient authority is exposed from here.
(function (G) {
  'use strict';
  const hide = (o, k, v) => Object.defineProperty(o, k, { value: v, writable: true, configurable: true, enumerable: false });
  G.global = G;

  // ---- byte helpers -------------------------------------------------------------------------------------------------
  const isView = (x) => ArrayBuffer.isView(x);
  const toU8 = (x) => {
    if (x instanceof ArrayBuffer) return new Uint8Array(x);
    if (isView(x)) return new Uint8Array(x.buffer, x.byteOffset, x.byteLength);
    if (Array.isArray(x)) return Uint8Array.from(x);
    throw new TypeError('expected ArrayBuffer, TypedArray or array of bytes');
  };
  const toAB = (u8) =>
    u8.byteOffset === 0 && u8.byteLength === u8.buffer.byteLength ? u8.buffer : u8.buffer.slice(u8.byteOffset, u8.byteOffset + u8.byteLength);
  const fromCharCodes = (u8) => {
    let out = '';
    for (let i = 0; i < u8.length; i += 8192) out += String.fromCharCode.apply(null, u8.subarray(i, i + 8192));
    return out;
  };

  // ---- console ------------------------------------------------------------------------------------------------------
  const show = (a) => {
    if (typeof a === 'string') return a;
    if (a instanceof Error) return a.stack || String(a);
    try {
      return typeof a === 'object' && a !== null ? JSON.stringify(a) : String(a);
    } catch (e) {
      return String(a);
    }
  };
  G.console = {};
  for (const lvl of ['log', 'info', 'warn', 'error', 'debug', 'trace']) {
    G.console[lvl] = (...args) => __host_log(lvl, args.map(show).join(' '));
  }
  G.console.table = G.console.dir = G.console.log;
  G.console.assert = (c, ...a) => { if (!c) G.console.error('Assertion failed', ...a); };
  G.console.time = G.console.timeEnd = G.console.group = G.console.groupEnd = () => {};

  // ---- timers (driven by the host loop: __tk_next_timer / __tk_run_timers) ------------------------------------------
  let timerSeq = 0;
  const timers = new Map();
  class Timeout {
    constructor(id) { this._id = id; }
    ref() { return this; }
    unref() { return this; }
    hasRef() { return true; }
    refresh() { return this; }
    [Symbol.toPrimitive]() { return this._id; }
  }
  const addTimer = (fn, ms, args, interval) => {
    if (typeof fn !== 'function') throw new TypeError('The "callback" argument must be of type function');
    const id = ++timerSeq;
    const d = Math.max(interval ? 1 : 0, Number(ms) || 0);
    timers.set(id, { at: __host_now() + d, fn, args, interval: interval ? d : null });
    return new Timeout(id);
  };
  const clearTimer = (t) => { timers.delete(t instanceof Timeout ? t._id : Number(t)); };
  G.setTimeout = (fn, ms, ...args) => addTimer(fn, ms, args, false);
  G.setInterval = (fn, ms, ...args) => addTimer(fn, ms, args, true);
  G.setImmediate = (fn, ...args) => addTimer(fn, 0, args, false);
  G.clearTimeout = G.clearInterval = G.clearImmediate = clearTimer;
  G.__tk_next_timer = () => {
    let best = -1;
    const now = __host_now();
    for (const t of timers.values()) {
      const d = Math.max(0, t.at - now);
      if (best < 0 || d < best) best = d;
    }
    return best;
  };
  G.__tk_run_timers = () => {
    const now = __host_now();
    const due = [...timers.entries()].filter((e) => e[1].at <= now).sort((a, b) => a[1].at - b[1].at || a[0] - b[0]);
    for (const [id, t] of due) {
      if (!timers.has(id)) continue;
      if (t.interval !== null) t.at = now + t.interval; else timers.delete(id);
      try { t.fn(...t.args); } catch (e) { G.console.error('timer callback threw: ' + (e && e.message)); }
    }
  };
  G.queueMicrotask = (fn) => { Promise.resolve().then(fn); };
  G.performance = { now: () => __host_now(), timeOrigin: Date.now(), mark() {}, measure() {} };

  G.structuredClone = (v) => {
    const seen = new WeakMap();
    const c = (x) => {
      if (x === null || typeof x !== 'object') {
        if (typeof x === 'function' || typeof x === 'symbol') throw new Error('DataCloneError: value could not be cloned');
        return x;
      }
      if (seen.has(x)) return seen.get(x);
      let out;
      if (x instanceof Date) out = new Date(x.getTime());
      else if (x instanceof RegExp) out = new RegExp(x.source, x.flags);
      else if (x instanceof Map) { out = new Map(); seen.set(x, out); x.forEach((val, k) => out.set(c(k), c(val))); }
      else if (x instanceof Set) { out = new Set(); seen.set(x, out); x.forEach((val) => out.add(c(val))); }
      else if (x instanceof ArrayBuffer) out = x.slice(0);
      else if (isView(x)) out = new x.constructor(x);
      else if (Array.isArray(x)) { out = []; seen.set(x, out); x.forEach((val, i) => { out[i] = c(val); }); }
      else if (x instanceof Error) { out = new Error(x.message); out.name = x.name; }
      else { out = {}; seen.set(x, out); for (const k of Object.keys(x)) out[k] = c(x[k]); }
      seen.set(x, out);
      return out;
    };
    return c(v);
  };

  // ---- atob / btoa / TextEncoder / TextDecoder ----------------------------------------------------------------------
  G.btoa = (s) => {
    s = String(s);
    const b = new Uint8Array(s.length);
    for (let i = 0; i < s.length; i++) {
      const c = s.charCodeAt(i);
      if (c > 255) { const e = new Error('The string to be encoded contains characters outside of the Latin1 range.'); e.name = 'InvalidCharacterError'; throw e; }
      b[i] = c;
    }
    return __host_b64_encode(b.buffer);
  };
  G.atob = (s) => {
    let ab;
    try { ab = __host_b64_decode(String(s).replace(/[\t\n\f\r ]+/g, '')); } catch (e) {
      const err = new Error('The string to be decoded is not correctly encoded.'); err.name = 'InvalidCharacterError'; throw err;
    }
    return fromCharCodes(new Uint8Array(ab));
  };
  G.TextEncoder = class TextEncoder {
    get encoding() { return 'utf-8'; }
    encode(s = '') { return new Uint8Array(__host_utf8_encode(String(s))); }
    encodeInto(s, dest) { const b = this.encode(s); const n = Math.min(b.length, dest.length); dest.set(b.subarray(0, n)); return { read: s.length, written: n }; }
  };
  G.TextDecoder = class TextDecoder {
    constructor(label = 'utf-8') {
      const l = String(label).toLowerCase().trim();
      const map = { 'utf-8': 'utf-8', utf8: 'utf-8', 'utf-16le': 'utf-16le', 'utf-16': 'utf-16le', ucs2: 'utf-16le', latin1: 'windows-1252', 'iso-8859-1': 'windows-1252', ascii: 'windows-1252', 'windows-1252': 'windows-1252' };
      if (!map[l]) throw new RangeError('The "' + label + '" encoding is not supported');
      this.encoding = map[l];
    }
    decode(input) {
      if (input === undefined || input === null) return '';
      const u8 = toU8(input);
      if (this.encoding === 'utf-16le') {
        let out = '';
        for (let i = 0; i + 1 < u8.length; i += 2) out += String.fromCharCode(u8[i] | (u8[i + 1] << 8));
        return out;
      }
      if (this.encoding === 'windows-1252') return fromCharCodes(u8);
      return __host_utf8_decode(toAB(u8));
    }
  };

  // ---- URLSearchParams / URL ----------------------------------------------------------------------------------------
  const qsEnc = (s) => encodeURIComponent(s).replace(/%20/g, '+').replace(/[!'()~]/g, (c) => '%' + c.charCodeAt(0).toString(16).toUpperCase());
  const qsDec = (s) => { try { return decodeURIComponent(s.replace(/\+/g, ' ')); } catch (e) { return s; } };
  class URLSearchParams {
    constructor(init) {
      this._l = [];
      this._url = null;
      if (init === undefined || init === null) return;
      if (typeof init === 'string') {
        if (init[0] === '?') init = init.slice(1);
        for (const part of init.split('&')) {
          if (!part) continue;
          const i = part.indexOf('=');
          this._l.push(i < 0 ? [qsDec(part), ''] : [qsDec(part.slice(0, i)), qsDec(part.slice(i + 1))]);
        }
      } else if (typeof init[Symbol.iterator] === 'function') {
        for (const [k, v] of init) this._l.push([String(k), String(v)]);
      } else {
        for (const k of Object.keys(init)) this._l.push([k, String(init[k])]);
      }
    }
    _sync() { if (this._url) this._url._setSearch(this.toString()); }
    get size() { return this._l.length; }
    append(k, v) { this._l.push([String(k), String(v)]); this._sync(); }
    delete(k) { this._l = this._l.filter((e) => e[0] !== String(k)); this._sync(); }
    get(k) { const e = this._l.find((x) => x[0] === String(k)); return e ? e[1] : null; }
    getAll(k) { return this._l.filter((x) => x[0] === String(k)).map((x) => x[1]); }
    has(k) { return this._l.some((x) => x[0] === String(k)); }
    set(k, v) {
      k = String(k); v = String(v);
      const i = this._l.findIndex((x) => x[0] === k);
      if (i < 0) this._l.push([k, v]); else { this._l[i][1] = v; this._l = this._l.filter((x, j) => j <= i || x[0] !== k); }
      this._sync();
    }
    sort() { this._l.sort((a, b) => (a[0] < b[0] ? -1 : a[0] > b[0] ? 1 : 0)); this._sync(); }
    forEach(cb, thisArg) { for (const [k, v] of this._l) cb.call(thisArg, v, k, this); }
    *entries() { for (const e of this._l) yield [e[0], e[1]]; }
    *keys() { for (const e of this._l) yield e[0]; }
    *values() { for (const e of this._l) yield e[1]; }
    [Symbol.iterator]() { return this.entries(); }
    toString() { return this._l.map((e) => qsEnc(e[0]) + '=' + qsEnc(e[1])).join('&'); }
  }
  G.URLSearchParams = URLSearchParams;

  const SPECIAL = { 'http:': '80', 'https:': '443', 'ftp:': '21', 'ws:': '80', 'wss:': '443', 'file:': '' };
  const encPath = (s) => s.replace(/[^\x21-\x7e]|["<>`{}^|\\]/g, (c) => (c === '\\' ? '/' : encodeURIComponent(c)));
  const normPath = (p) => {
    const out = [];
    const segs = p.split('/');
    for (let i = 0; i < segs.length; i++) {
      const s = segs[i];
      if (s === '..') { if (out.length > 1) out.pop(); if (i === segs.length - 1) out.push(''); }
      else if (s === '.') { if (i === segs.length - 1) out.push(''); }
      else out.push(s);
    }
    return out.join('/') || '/';
  };
  const invalidUrl = (input) => { const e = new TypeError('Invalid URL: ' + input); e.code = 'ERR_INVALID_URL'; e.input = input; return e; };
  class URL {
    constructor(input, base) {
      input = String(input).trim();
      let m = /^([a-zA-Z][a-zA-Z0-9+.-]*:)(.*)$/.exec(input);
      if (!m) {
        if (base === undefined) throw invalidUrl(input);
        const b = base instanceof URL ? base : new URL(base);
        return Object.assign(this, URL._resolve(b, input));
      }
      const protocol = m[1].toLowerCase();
      let rest = m[2];
      // special scheme without "//" relative to same-scheme base (e.g. "http:foo") is rare; treat as absolute
      let hash = '', search = '';
      const hi = rest.indexOf('#'); if (hi >= 0) { hash = rest.slice(hi); rest = rest.slice(0, hi); }
      const qi = rest.indexOf('?'); if (qi >= 0) { search = rest.slice(qi); rest = rest.slice(0, qi); }
      let username = '', password = '', hostname = '', port = '', pathname;
      if (protocol in SPECIAL || rest.startsWith('//')) {
        rest = rest.replace(/^[\\/]*/, '//').slice(2);
        const si = rest.search(/[\\/]/);
        let auth = si < 0 ? rest : rest.slice(0, si);
        pathname = si < 0 ? '/' : rest.slice(si);
        const at = auth.lastIndexOf('@');
        if (at >= 0) {
          const cred = auth.slice(0, at); auth = auth.slice(at + 1);
          const ci = cred.indexOf(':');
          username = ci < 0 ? cred : cred.slice(0, ci); password = ci < 0 ? '' : cred.slice(ci + 1);
        }
        const pm = /^(\[[^\]]*\]|[^:]*)(?::(\d*))?$/.exec(auth);
        if (!pm) throw invalidUrl(input);
        hostname = pm[1].toLowerCase(); port = pm[2] || '';
        if (protocol !== 'file:' && !hostname) throw invalidUrl(input);
        if (port && port === SPECIAL[protocol]) port = '';
        if (port && Number(port) > 65535) throw invalidUrl(input);
        pathname = normPath(encPath(pathname.replace(/\\/g, '/')));
      } else {
        pathname = rest; // opaque path (mailto:, data:)
      }
      this._init(protocol, username, password, hostname, port, pathname, search, hash);
    }
    static _resolve(b, input) {
      let hash = '', search = null;
      let rest = input;
      const hi = rest.indexOf('#'); if (hi >= 0) { hash = rest.slice(hi); rest = rest.slice(0, hi); }
      const qi = rest.indexOf('?'); if (qi >= 0) { search = rest.slice(qi); rest = rest.slice(0, qi); }
      if (rest.startsWith('//')) return new URL(b.protocol + input);
      let pathname;
      if (rest === '') { pathname = b.pathname; if (search === null) search = b.search; }
      else if (rest[0] === '/') pathname = normPath(encPath(rest));
      else pathname = normPath(b.pathname.replace(/[^/]*$/, '') + encPath(rest));
      const u = Object.create(URL.prototype);
      u._init(b.protocol, b.username, b.password, b.hostname, b.port, pathname, search || '', hash);
      return u;
    }
    _init(protocol, username, password, hostname, port, pathname, search, hash) {
      this.protocol = protocol; this.username = username; this.password = password; this.hostname = hostname; this.port = port;
      this.pathname = pathname; this._search = search === '?' ? '' : search; this.hash = hash === '#' ? '' : hash;
      this._sp = null;
    }
    _setSearch(s) { this._search = s ? '?' + s : ''; }
    get search() { return this._search; }
    set search(v) { v = String(v); this._search = v && v !== '?' ? (v[0] === '?' ? v : '?' + v) : ''; this._sp = null; }
    get searchParams() {
      if (!this._sp) { this._sp = new URLSearchParams(this._search); this._sp._url = this; }
      return this._sp;
    }
    get host() { return this.hostname + (this.port ? ':' + this.port : ''); }
    set host(v) { const m = /^([^:]*)(?::(\d*))?$/.exec(String(v)); if (m) { this.hostname = m[1].toLowerCase(); this.port = m[2] || ''; } }
    get origin() { return this.protocol in SPECIAL && this.protocol !== 'file:' ? this.protocol + '//' + this.host : 'null'; }
    get href() {
      const auth = this.username ? this.username + (this.password ? ':' + this.password : '') + '@' : '';
      const slashes = this.protocol in SPECIAL || this.hostname ? '//' : '';
      return this.protocol + slashes + auth + this.host + this.pathname + this._search + this.hash;
    }
    set href(v) { Object.assign(this, new URL(v)); }
    toString() { return this.href; }
    toJSON() { return this.href; }
    static canParse(u, b) { try { new URL(u, b); return true; } catch (e) { return false; } }
  }
  G.URL = URL;

  // ---- Buffer -------------------------------------------------------------------------------------------------------
  const normEnc = (e) => {
    e = String(e || 'utf8').toLowerCase();
    if (e === 'utf-8') return 'utf8';
    if (e === 'binary') return 'latin1';
    if (e === 'ucs2' || e === 'ucs-2' || e === 'utf-16le') return 'utf16le';
    return e;
  };
  const encodeStr = (s, enc) => {
    switch (normEnc(enc)) {
      case 'utf8': return new Uint8Array(__host_utf8_encode(s));
      case 'hex': return new Uint8Array(__host_hex_decode(s));
      case 'base64': case 'base64url': return new Uint8Array(__host_b64_decode(s.replace(/-/g, '+').replace(/_/g, '/').replace(/[\s=]+/g, '')));
      case 'latin1': case 'ascii': { const b = new Uint8Array(s.length); for (let i = 0; i < s.length; i++) b[i] = s.charCodeAt(i) & 255; return b; }
      case 'utf16le': { const b = new Uint8Array(s.length * 2); for (let i = 0; i < s.length; i++) { const c = s.charCodeAt(i); b[2 * i] = c & 255; b[2 * i + 1] = c >> 8; } return b; }
      default: { const e = new TypeError('Unknown encoding: ' + enc); e.code = 'ERR_UNKNOWN_ENCODING'; throw e; }
    }
  };
  class Buffer extends Uint8Array {
    static from(v, encOrOffset, length) {
      if (typeof v === 'string') { const b = encodeStr(v, encOrOffset); const out = new Buffer(b.length); out.set(b); return out; }
      if (v instanceof ArrayBuffer) return new Buffer(v, encOrOffset || 0, length === undefined ? v.byteLength - (encOrOffset || 0) : length);
      if (isView(v)) { const out = new Buffer(v.byteLength); out.set(toU8(v)); return out; }
      if (Array.isArray(v)) { const out = new Buffer(v.length); for (let i = 0; i < v.length; i++) out[i] = v[i] & 255; return out; }
      if (v && v.type === 'Buffer' && Array.isArray(v.data)) return Buffer.from(v.data);
      throw new TypeError('The first argument must be of type string or an instance of Buffer, ArrayBuffer, or Array or an Array-like Object.');
    }
    static alloc(n, fill, enc) { const b = new Buffer(n); if (fill !== undefined && fill !== 0) b.fill(typeof fill === 'string' ? encodeStr(fill, enc)[0] || 0 : fill); return b; }
    static allocUnsafe(n) { return new Buffer(n); }
    static isBuffer(b) { return b instanceof Buffer; }
    static isEncoding(e) { try { encodeStr('', e); return true; } catch (x) { return false; } }
    static byteLength(v, enc) { return typeof v === 'string' ? encodeStr(v, enc).length : v.byteLength; }
    static concat(list, total) {
      const n = total === undefined ? list.reduce((a, b) => a + b.length, 0) : total;
      const out = new Buffer(n);
      let off = 0;
      for (const b of list) { if (off >= n) break; const u = toU8(b); const take = Math.min(u.length, n - off); out.set(u.subarray(0, take), off); off += take; }
      return out;
    }
    static compare(a, b) { const n = Math.min(a.length, b.length); for (let i = 0; i < n; i++) if (a[i] !== b[i]) return a[i] < b[i] ? -1 : 1; return a.length === b.length ? 0 : a.length < b.length ? -1 : 1; }
    toString(enc, start, end) {
      const u = this.subarray(start || 0, end === undefined ? this.length : end);
      const bytes = toU8(u);
      switch (normEnc(enc)) {
        case 'utf8': return __host_utf8_decode(toAB(bytes));
        case 'hex': return __host_hex_encode(toAB(bytes));
        case 'base64': return __host_b64_encode(toAB(bytes));
        case 'base64url': return __host_b64_encode(toAB(bytes)).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
        case 'latin1': return fromCharCodes(bytes);
        case 'ascii': return fromCharCodes(bytes.map((x) => x & 127));
        case 'utf16le': { let o = ''; for (let i = 0; i + 1 < bytes.length; i += 2) o += String.fromCharCode(bytes[i] | (bytes[i + 1] << 8)); return o; }
        default: { const e = new TypeError('Unknown encoding: ' + enc); e.code = 'ERR_UNKNOWN_ENCODING'; throw e; }
      }
    }
    slice(s, e) { return this.subarray(s, e); }
    equals(o) { return Buffer.compare(this, o) === 0; }
    toJSON() { return { type: 'Buffer', data: Array.from(this) }; }
    write(str, off = 0, len, enc) { const b = encodeStr(str, enc); const n = Math.min(b.length, this.length - off, len === undefined ? Infinity : len); this.set(b.subarray(0, n), off); return n; }
    readUInt8(o = 0) { return this[o]; }
    readUInt16BE(o = 0) { return (this[o] << 8) | this[o + 1]; }
    readUInt16LE(o = 0) { return this[o] | (this[o + 1] << 8); }
    readUInt32BE(o = 0) { return ((this[o] * 16777216) + ((this[o + 1] << 16) | (this[o + 2] << 8) | this[o + 3])) >>> 0; }
    readUInt32LE(o = 0) { return ((this[o + 3] * 16777216) + ((this[o + 2] << 16) | (this[o + 1] << 8) | this[o])) >>> 0; }
    writeUInt8(v, o = 0) { this[o] = v; return o + 1; }
    writeUInt16BE(v, o = 0) { this[o] = v >> 8; this[o + 1] = v; return o + 2; }
    writeUInt32BE(v, o = 0) { this[o] = v >>> 24; this[o + 1] = v >>> 16; this[o + 2] = v >>> 8; this[o + 3] = v; return o + 4; }
  }
  G.Buffer = Buffer;

  // ---- crypto -------------------------------------------------------------------------------------------------------
  const ALGOS = ['md5', 'sha1', 'sha256', 'sha512'];
  const dataBytes = (d, enc) => (typeof d === 'string' ? encodeStr(d, enc) : toU8(d));
  const outDigest = (ab, enc) => {
    const b = Buffer.from(new Uint8Array(ab));
    return enc === undefined || enc === 'buffer' ? b : b.toString(enc);
  };
  const checkAlgo = (a) => {
    const n = String(a).toLowerCase().replace('-', '');
    if (!ALGOS.includes(n)) throw new Error('Digest method not supported');
    return n;
  };
  const cat = (chunks) => { let n = 0; for (const c of chunks) n += c.length; const o = new Uint8Array(n); let off = 0; for (const c of chunks) { o.set(c, off); off += c.length; } return o; };
  const nodeCrypto = {
    createHash(algo) {
      const a = checkAlgo(algo); const chunks = []; let done = false;
      const h = {
        update(d, enc) { if (done) throw new Error('Digest already called'); chunks.push(dataBytes(d, enc)); return h; },
        digest(enc) { if (done) throw new Error('Digest already called'); done = true; return outDigest(__host_hash(a, toAB(cat(chunks))), enc); },
        copy() { const c = nodeCrypto.createHash(a); chunks.forEach((x) => c.update(x)); return c; },
      };
      return h;
    },
    createHmac(algo, key) {
      const a = checkAlgo(algo); const k = toAB(dataBytes(key)); const chunks = []; let done = false;
      const h = {
        update(d, enc) { if (done) throw new Error('Digest already called'); chunks.push(dataBytes(d, enc)); return h; },
        digest(enc) { if (done) throw new Error('Digest already called'); done = true; return outDigest(__host_hmac(a, k, toAB(cat(chunks))), enc); },
      };
      return h;
    },
    randomBytes(n) { return Buffer.from(new Uint8Array(__host_random(n))); },
    randomUUID() { return G.crypto.randomUUID(); },
    randomInt(min, max) { if (max === undefined) { max = min; min = 0; } return min + (new DataView(__host_random(4)).getUint32(0) % (max - min)); },
    getRandomValues(a) { return G.crypto.getRandomValues(a); },
    getHashes() { return ALGOS.slice(); },
    timingSafeEqual(a, b) { if (a.length !== b.length) throw new RangeError('Input buffers must have the same byte length'); let r = 0; for (let i = 0; i < a.length; i++) r |= a[i] ^ b[i]; return r === 0; },
  };
  G.crypto = {
    randomUUID() {
      const b = new Uint8Array(__host_random(16));
      b[6] = (b[6] & 0x0f) | 0x40; b[8] = (b[8] & 0x3f) | 0x80;
      const h = Array.from(b, (x) => x.toString(16).padStart(2, '0')).join('');
      return h.slice(0, 8) + '-' + h.slice(8, 12) + '-' + h.slice(12, 16) + '-' + h.slice(16, 20) + '-' + h.slice(20);
    },
    getRandomValues(arr) {
      const u = toU8(arr);
      u.set(new Uint8Array(__host_random(u.length)));
      return arr;
    },
  };

  // ---- process ------------------------------------------------------------------------------------------------------
  const t0 = Date.now();
  const noop = () => undefined;
  const hrtime = (prev) => {
    const ms = __host_now();
    let s = Math.floor(ms / 1000), ns = Math.floor((ms % 1000) * 1e6);
    if (prev) { s -= prev[0]; ns -= prev[1]; if (ns < 0) { s--; ns += 1e9; } }
    return [s, ns];
  };
  hrtime.bigint = () => BigInt(Math.floor(__host_now() * 1e6));
  const proc = {
    platform: __host_platform, arch: 'x64', pid: 1, ppid: 0, version: 'v20.11.0', title: 'node',
    versions: { node: '20.11.0', v8: '11.3.244.8-node.17' },
    env: { NODE_ENV: 'production' }, argv: ['node', 'extension'], execArgv: [], exitCode: undefined,
    nextTick(fn, ...args) { Promise.resolve().then(() => fn(...args)); },
    hrtime, cwd: () => '/', uptime: () => (Date.now() - t0) / 1000,
    memoryUsage: () => ({ rss: 0, heapTotal: 0, heapUsed: 0, external: 0, arrayBuffers: 0 }),
    emitWarning: noop, exit: noop, kill: noop, abort: noop, chdir: noop, umask: () => 0,
    stdout: { write: (s) => { G.console.log(String(s)); return true; }, isTTY: false },
    stderr: { write: (s) => { G.console.error(String(s)); return true; }, isTTY: false },
  };
  for (const k of ['on', 'once', 'off', 'addListener', 'removeListener', 'removeAllListeners', 'emit', 'prependListener', 'setMaxListeners']) proc[k] = function () { return proc; };
  G.process = proc;

  // ---- node modules -------------------------------------------------------------------------------------------------
  class EventEmitter {
    constructor() { hide(this, '_events', new Map()); }
    _ev() { if (!this._events) hide(this, '_events', new Map()); return this._events; }
    on(n, f) { const e = this._ev(); if (!e.has(n)) e.set(n, []); e.get(n).push(f); return this; }
    addListener(n, f) { return this.on(n, f); }
    prependListener(n, f) { const e = this._ev(); if (!e.has(n)) e.set(n, []); e.get(n).unshift(f); return this; }
    once(n, f) { const w = (...a) => { this.off(n, w); return f.apply(this, a); }; w.listener = f; return this.on(n, w); }
    off(n, f) { const l = this._ev().get(n); if (l) { const i = l.findIndex((x) => x === f || x.listener === f); if (i >= 0) l.splice(i, 1); } return this; }
    removeListener(n, f) { return this.off(n, f); }
    removeAllListeners(n) { if (n === undefined) this._ev().clear(); else this._ev().delete(n); return this; }
    emit(n, ...a) {
      const l = this._ev().get(n);
      if (!l || !l.length) { if (n === 'error') throw (a[0] instanceof Error ? a[0] : new Error('Unhandled error.')); return false; }
      for (const f of l.slice()) f.apply(this, a);
      return true;
    }
    listenerCount(n) { const l = this._ev().get(n); return l ? l.length : 0; }
    listeners(n) { return (this._ev().get(n) || []).map((x) => x.listener || x); }
    eventNames() { return [...this._ev().keys()]; }
    setMaxListeners() { return this; }
    getMaxListeners() { return 10; }
  }
  EventEmitter.EventEmitter = EventEmitter;
  EventEmitter.defaultMaxListeners = 10;

  const pathMod = (() => {
    const norm = (p) => {
      if (p === '') return '.';
      const abs = p[0] === '/'; const trail = p[p.length - 1] === '/';
      const out = [];
      for (const s of p.split('/')) { if (!s || s === '.') continue; if (s === '..') { if (out.length && out[out.length - 1] !== '..') out.pop(); else if (!abs) out.push('..'); } else out.push(s); }
      let r = out.join('/');
      if (!r && !abs) r = '.';
      if (r && trail) r += '/';
      return (abs ? '/' : '') + r;
    };
    const basename = (p, ext) => { p = p.replace(/\/+$/, ''); let b = p.slice(p.lastIndexOf('/') + 1); if (ext && b.endsWith(ext) && b !== ext) b = b.slice(0, -ext.length); return b; };
    const dirname = (p) => { p = p.replace(/\/+$/, ''); const i = p.lastIndexOf('/'); return i < 0 ? '.' : i === 0 ? '/' : p.slice(0, i); };
    const extname = (p) => { const b = basename(p); const i = b.lastIndexOf('.'); return i <= 0 ? '' : b.slice(i); };
    const join = (...a) => norm(a.filter((x) => x !== '').join('/') || '.');
    const resolve = (...a) => { let r = ''; for (let i = a.length - 1; i >= 0 && r[0] !== '/'; i--) if (a[i]) r = a[i] + (r ? '/' + r : ''); if (r[0] !== '/') r = '/' + r; const n = norm(r); return n.length > 1 ? n.replace(/\/$/, '') : n; };
    const isAbsolute = (p) => p[0] === '/';
    const relative = (f, t) => { const a = resolve(f).split('/').filter(Boolean), b = resolve(t).split('/').filter(Boolean); let i = 0; while (i < a.length && a[i] === b[i]) i++; return [...a.slice(i).map(() => '..'), ...b.slice(i)].join('/'); };
    const parse = (p) => { const base = basename(p), ext = extname(p); return { root: p[0] === '/' ? '/' : '', dir: dirname(p), base, ext, name: ext ? base.slice(0, -ext.length) : base }; };
    const format = (o) => (o.dir ? o.dir + '/' : o.root || '') + (o.base || (o.name || '') + (o.ext || ''));
    const m = { sep: '/', delimiter: ':', normalize: norm, basename, dirname, extname, join, resolve, isAbsolute, relative, parse, format };
    m.posix = m; m.win32 = m;
    return m;
  })();

  const qsMod = {
    parse(s, sep = '&', eq = '=') {
      const out = {};
      for (const part of String(s).split(sep)) {
        if (!part) continue;
        const i = part.indexOf(eq);
        const k = qsDec(i < 0 ? part : part.slice(0, i)); const v = i < 0 ? '' : qsDec(part.slice(i + eq.length));
        if (k in out) out[k] = [].concat(out[k], v); else out[k] = v;
      }
      return out;
    },
    stringify(o, sep = '&', eq = '=') {
      return Object.keys(o || {}).flatMap((k) => [].concat(o[k]).map((v) => encodeURIComponent(k) + eq + encodeURIComponent(v == null ? '' : v))).join(sep);
    },
    escape: encodeURIComponent, unescape: qsDec,
  };

  const urlMod = {
    URL, URLSearchParams,
    parse(s, parseQuery) {
      let u; try { u = new URL(s); } catch (e) { u = null; }
      if (!u) { const q = s.indexOf('?'); const search = q < 0 ? '' : s.slice(q); const pathname = q < 0 ? s : s.slice(0, q); return { href: s, protocol: null, host: null, hostname: null, port: null, pathname, search: search || null, query: parseQuery ? qsMod.parse(search.slice(1)) : search.slice(1) || null, hash: null, path: s }; }
      return { href: u.href, protocol: u.protocol, auth: u.username ? u.username + (u.password ? ':' + u.password : '') : null, host: u.host, hostname: u.hostname, port: u.port || null, pathname: u.pathname, search: u.search || null, query: parseQuery ? qsMod.parse(u.search.slice(1)) : u.search.slice(1) || null, hash: u.hash || null, path: u.pathname + u.search };
    },
    format(o) {
      if (o instanceof URL) return o.href;
      if (typeof o === 'string') return o;
      const proto = o.protocol ? (o.protocol.endsWith(':') ? o.protocol : o.protocol + ':') : '';
      const host = o.host || ((o.hostname || '') + (o.port ? ':' + o.port : ''));
      const search = o.search || (o.query && typeof o.query === 'object' ? '?' + qsMod.stringify(o.query) : '');
      return proto + (host ? '//' : '') + (o.auth ? o.auth + '@' : '') + host + (o.pathname || '') + search + (o.hash || '');
    },
    resolve(from, to) { return new URL(to, new URL(from, 'resolve://')).href.replace(/^resolve:\/\//, ''); },
    fileURLToPath() { throw new Error('url.fileURLToPath is not available in the native extension runtime'); },
    pathToFileURL() { throw new Error('url.pathToFileURL is not available in the native extension runtime'); },
  };

  const utilMod = (() => {
    const inspect = (v, o) => {
      const seen = new WeakSet();
      const depth = o && typeof o.depth === 'number' ? o.depth : 2;
      const f = (x, d) => {
        if (typeof x === 'string') return d === 0 ? x : "'" + x + "'";
        if (typeof x === 'function') return '[Function' + (x.name ? ': ' + x.name : '') + ']';
        if (typeof x === 'bigint') return x + 'n';
        if (typeof x === 'symbol') return x.toString();
        if (x === null || typeof x !== 'object') return String(x);
        if (x instanceof Error) return x.stack || String(x);
        if (seen.has(x)) return '[Circular]';
        if (d > depth) return Array.isArray(x) ? '[Array]' : '[Object]';
        seen.add(x);
        if (Array.isArray(x)) return '[ ' + x.map((e) => f(e, d + 1)).join(', ') + ' ]';
        if (x instanceof Map) return 'Map(' + x.size + ') { ' + [...x].map(([k, v]) => f(k, d + 1) + ' => ' + f(v, d + 1)).join(', ') + ' }';
        if (x instanceof Set) return 'Set(' + x.size + ') { ' + [...x].map((e) => f(e, d + 1)).join(', ') + ' }';
        if (x instanceof Date) return x.toISOString();
        return '{ ' + Object.keys(x).map((k) => k + ': ' + f(x[k], d + 1)).join(', ') + ' }';
      };
      return f(v, 0);
    };
    const format = (fmt, ...args) => {
      if (typeof fmt !== 'string') return [fmt, ...args].map((a) => (typeof a === 'string' ? a : inspect(a))).join(' ');
      let i = 0;
      let out = fmt.replace(/%[sdifjoO%]/g, (m) => {
        if (m === '%%') return '%';
        if (i >= args.length) return m;
        const a = args[i++];
        switch (m) {
          case '%s': return typeof a === 'string' ? a : inspect(a);
          case '%d': return String(Number(a));
          case '%i': return String(parseInt(a, 10));
          case '%f': return String(parseFloat(a));
          case '%j': try { return JSON.stringify(a); } catch (e) { return '[Circular]'; }
          default: return inspect(a);
        }
      });
      for (; i < args.length; i++) out += ' ' + (typeof args[i] === 'string' ? args[i] : inspect(args[i]));
      return out;
    };
    const isDeepStrictEqual = (a, b) => {
      if (Object.is(a, b)) return true;
      if (typeof a !== 'object' || typeof b !== 'object' || a === null || b === null) return false;
      if (Object.getPrototypeOf(a) !== Object.getPrototypeOf(b)) return false;
      if (a instanceof Date) return a.getTime() === b.getTime();
      const ka = Object.keys(a), kb = Object.keys(b);
      return ka.length === kb.length && ka.every((k) => Object.prototype.hasOwnProperty.call(b, k) && isDeepStrictEqual(a[k], b[k]));
    };
    const tag = (x) => Object.prototype.toString.call(x);
    return {
      format, inspect, isDeepStrictEqual, TextEncoder: G.TextEncoder, TextDecoder: G.TextDecoder,
      promisify: (fn) => (...args) => new Promise((res, rej) => fn(...args, (err, val) => (err ? rej(err) : res(val)))),
      callbackify: (fn) => (...args) => { const cb = args.pop(); fn(...args).then((v) => cb(null, v), (e) => cb(e)); },
      inherits(ctor, sup) { Object.defineProperty(ctor, 'super_', { value: sup, writable: true, configurable: true }); Object.setPrototypeOf(ctor.prototype, sup.prototype); },
      deprecate: (fn) => fn,
      isArray: Array.isArray, isString: (x) => typeof x === 'string', isFunction: (x) => typeof x === 'function', isNumber: (x) => typeof x === 'number',
      types: {
        isPromise: (x) => x instanceof Promise, isDate: (x) => x instanceof Date, isRegExp: (x) => x instanceof RegExp, isMap: (x) => x instanceof Map,
        isSet: (x) => x instanceof Set, isTypedArray: (x) => isView(x) && !(x instanceof DataView), isUint8Array: (x) => x instanceof Uint8Array,
        isArrayBuffer: (x) => x instanceof ArrayBuffer, isAsyncFunction: (x) => tag(x) === '[object AsyncFunction]', isNativeError: (x) => x instanceof Error,
      },
    };
  })();

  const stubModule = (name) => {
    const fns = new Map();
    const make = (prop) => {
      const f = function () { throw new Error(name + '.' + String(prop) + ' is not available in the native extension runtime'); };
      return f;
    };
    return new Proxy({}, {
      get(_t, prop) {
        if (typeof prop === 'symbol' || prop === 'then' || prop === 'default' || prop === '__esModule' || prop === 'constants') return undefined;
        if (name === 'os' && prop === 'EOL') return '\n';
        if (!fns.has(prop)) fns.set(prop, make(prop));
        return fns.get(prop);
      },
      has: () => true,
    });
  };

  const SUPPORTED = { crypto: nodeCrypto, buffer: { Buffer }, events: EventEmitter, url: urlMod, path: pathMod, querystring: qsMod, util: utilMod };
  const STUBS = ['os', 'http', 'https', 'stream', 'zlib', 'string_decoder', 'assert', 'timers', 'readline', 'perf_hooks', 'async_hooks', 'module', 'tty'];
  const stubCache = {};
  G.__tk_require = function require(name) {
    name = String(name).replace(/^node:/, '');
    if (Object.prototype.hasOwnProperty.call(SUPPORTED, name)) return SUPPORTED[name];
    if (STUBS.indexOf(name) >= 0) return stubCache[name] || (stubCache[name] = stubModule(name));
    throw new Error("Module '" + name + "' is not supported by the native extension runtime");
  };
  G.__tk_require.resolve = (n) => { throw new Error("Cannot find module '" + n + "'"); };

  // ---- fetch / Headers / Response (network gate + SSRF policy live in Rust) ----------------------------------------
  class Headers {
    constructor(init) {
      this._m = new Map();
      if (!init) return;
      if (init instanceof Headers) init.forEach((v, k) => this.append(k, v));
      else if (Array.isArray(init) || (typeof init[Symbol.iterator] === 'function')) for (const [k, v] of init) this.append(k, v);
      else for (const k of Object.keys(init)) this.append(k, init[k]);
    }
    append(k, v) { k = String(k).toLowerCase(); const l = this._m.get(k) || []; l.push(String(v)); this._m.set(k, l); }
    set(k, v) { this._m.set(String(k).toLowerCase(), [String(v)]); }
    get(k) { const l = this._m.get(String(k).toLowerCase()); return l ? l.join(', ') : null; }
    has(k) { return this._m.has(String(k).toLowerCase()); }
    delete(k) { this._m.delete(String(k).toLowerCase()); }
    forEach(cb, t) { for (const [k, l] of this._m) cb.call(t, l.join(', '), k, this); }
    *entries() { for (const [k, l] of this._m) yield [k, l.join(', ')]; }
    *keys() { for (const k of this._m.keys()) yield k; }
    *values() { for (const l of this._m.values()) yield l.join(', '); }
    [Symbol.iterator]() { return this.entries(); }
  }
  G.Headers = Headers;
  class Response {
    constructor(body, init) {
      init = init || {};
      this._bytes = body === undefined || body === null ? new Uint8Array(0) : typeof body === 'string' ? new TextEncoder().encode(body) : toU8(body);
      this.status = init.status === undefined ? 200 : init.status;
      this.statusText = init.statusText || '';
      this.headers = new Headers(init.headers);
      this.url = init.url || '';
      this.redirected = !!init.redirected;
      this.type = 'basic';
      this.bodyUsed = false;
    }
    get ok() { return this.status >= 200 && this.status < 300; }
    _take() { if (this.bodyUsed) return Promise.reject(new TypeError('Body has already been consumed.')); this.bodyUsed = true; return Promise.resolve(this._bytes); }
    text() { return this._take().then((b) => new TextDecoder().decode(b)); }
    json() { return this.text().then((t) => JSON.parse(t)); }
    arrayBuffer() { return this._take().then((b) => toAB(b).slice(0)); }
    clone() { return new Response(this._bytes.slice(0), { status: this.status, statusText: this.statusText, headers: this.headers, url: this.url }); }
  }
  G.Response = Response;
  const pending = new Map();
  let fetchSeq = 0;
  G.fetch = function fetch(input, init) {
    return new Promise((resolve, reject) => {
      try {
        init = init || {};
        const req = input && typeof input === 'object' && !(input instanceof URL) ? input : null;
        const url = req ? req.url : String(input);
        const headers = {};
        new Headers(init.headers || (req && req.headers)).forEach((v, k) => { headers[k] = v; });
        let body = init.body !== undefined ? init.body : req ? req.body : undefined;
        let bytes;
        if (body !== undefined && body !== null) {
          if (typeof body === 'string') bytes = new TextEncoder().encode(body);
          else if (body instanceof URLSearchParams) { bytes = new TextEncoder().encode(body.toString()); if (!headers['content-type']) headers['content-type'] = 'application/x-www-form-urlencoded;charset=UTF-8'; }
          else bytes = toU8(body);
        }
        const id = ++fetchSeq;
        const meta = { url: String(url), method: String(init.method || (req && req.method) || 'GET').toUpperCase(), headers };
        pending.set(id, { resolve, reject });
        try { __host_fetch_start(id, JSON.stringify(meta), bytes ? toAB(bytes) : undefined); } catch (e) { pending.delete(id); throw e; }
      } catch (e) { reject(e); }
    });
  };
  G.__tatakai_fetch__ = G.fetch;
  G.__tk_fetch_done = (id, metaJson, ab) => {
    const p = pending.get(id);
    if (!p) return;
    pending.delete(id);
    const meta = JSON.parse(metaJson);
    if (meta.error) { p.reject(new TypeError(meta.error)); return; }
    const r = new Response(ab ? new Uint8Array(ab) : null, { status: meta.status, statusText: meta.statusText, headers: meta.headers, url: meta.url, redirected: meta.redirected });
    p.resolve(r);
  };
  G.__tk_pending_fetches = () => pending.size;
  G.__tatakai_anitomy__ = () => ({});

  // ---- cheerio-like HTML adapter over host-side `scraper` documents --------------------------------------------------
  G.__tatakai_parse_html__ = function parseHtml(html) {
    if (typeof html !== 'string' || html.length === 0) {
      const err = new Error('InvalidInputError: html must be a non-empty string');
      err.name = 'InvalidInputError';
      throw err;
    }
    const doc = __host_html_parse(html);
    const uniq = (a) => [...new Set(a)];
    const wrap = (ids) => ({
      length: ids.length,
      eq: (i) => wrap(i < 0 ? (ids.length + i >= 0 ? [ids[ids.length + i]] : []) : i < ids.length ? [ids[i]] : []),
      last: () => wrap(ids.length ? [ids[ids.length - 1]] : []),
      parent: () => wrap(uniq(ids.map((n) => __host_html_parent(doc, n)).filter((n) => n >= 0))),
      children: (s) => wrap(ids.flatMap((n) => __host_html_children(doc, n, s || ''))),
      next: () => wrap(ids.map((n) => __host_html_sibling(doc, n, 1)).filter((n) => n >= 0)),
      prev: () => wrap(ids.map((n) => __host_html_sibling(doc, n, -1)).filter((n) => n >= 0)),
      find: (s) => wrap(uniq(ids.flatMap((n) => __host_html_select(doc, n, String(s))))),
      first: () => wrap(ids.length ? [ids[0]] : []),
      attr: (name) => (ids.length ? __host_html_attr(doc, ids[0], String(name)) : undefined),
      text: () => ids.map((n) => __host_html_text(doc, n)).join(''),
      html: () => (ids.length ? __host_html_inner(doc, ids[0]) : null),
      each(cb) { ids.forEach((n, i) => cb(i, wrap([n]))); return wrap(ids); },
    });
    return wrap([__host_html_root(doc)]);
  };

  // `require` and the other CommonJS wrapper parameters are passed explicitly by __tk_load (host.js).
  Error.captureStackTrace = Error.captureStackTrace || function (o) { if (o && typeof o === 'object' && !('stack' in o)) o.stack = String(new Error().stack); };
  G.navigator = { userAgent: 'Tatakai/1.0 (native extension runtime)' };
})(globalThis);
