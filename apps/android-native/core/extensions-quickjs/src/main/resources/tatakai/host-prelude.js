// SPDX-License-Identifier: AGPL-3.0-or-later
// Tatakai native extension host - JS prelude (contracts/extension-host/globals.v1.md).
// Evaluated once per QuickJS runtime BEFORE the extension bundle. It only uses the host bridge functions named __host_*:
//   __host_log(level, message)                 __host_sleep(ms) -> Promise
//   __host_fetch(requestJson) -> Promise<json> __host_hash(algo, b64) -> b64        __host_hmac(algo, keyB64, dataB64) -> b64
//   __host_random(n) -> b64                    __host_html(op, handle, arg) -> json  __host_args() -> json string
// There is no fs, no sockets, no eval of remote code and no access to the app database or the Supabase session.
(function (g) {
  'use strict';

  var hostLog = g.__host_log, hostSleep = g.__host_sleep, hostFetch = g.__host_fetch;
  var hostHash = g.__host_hash, hostHmac = g.__host_hmac, hostRandom = g.__host_random, hostHtml = g.__host_html;
  var START = Date.now();

  // ---------------------------------------------------------------- base64 / utf8 ----------------------------------------------------
  var B64 = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/';
  var B64REV = {};
  for (var bi = 0; bi < 64; bi++) B64REV[B64.charAt(bi)] = bi;
  B64REV['-'] = 62; B64REV['_'] = 63;

  function bytesToB64(u8) {
    var out = '', i = 0, n;
    for (; i + 2 < u8.length; i += 3) {
      n = (u8[i] << 16) | (u8[i + 1] << 8) | u8[i + 2];
      out += B64.charAt((n >> 18) & 63) + B64.charAt((n >> 12) & 63) + B64.charAt((n >> 6) & 63) + B64.charAt(n & 63);
    }
    var r = u8.length - i;
    if (r === 1) {
      n = u8[i] << 16;
      out += B64.charAt((n >> 18) & 63) + B64.charAt((n >> 12) & 63) + '==';
    } else if (r === 2) {
      n = (u8[i] << 16) | (u8[i + 1] << 8);
      out += B64.charAt((n >> 18) & 63) + B64.charAt((n >> 12) & 63) + B64.charAt((n >> 6) & 63) + '=';
    }
    return out;
  }

  function b64ToBytes(s) {
    s = String(s).replace(/[\s=]+/g, '');
    var out = [], buf = 0, bits = 0;
    for (var i = 0; i < s.length; i++) {
      var v = B64REV[s.charAt(i)];
      if (v === undefined) continue;
      buf = (buf << 6) | v; bits += 6;
      if (bits >= 8) { bits -= 8; out.push((buf >> bits) & 255); buf &= (1 << bits) - 1; }
    }
    return new Uint8Array(out);
  }

  function utf8Encode(str) {
    str = String(str);
    var out = [];
    for (var i = 0; i < str.length; i++) {
      var c = str.charCodeAt(i);
      if (c >= 0xd800 && c <= 0xdbff && i + 1 < str.length) {
        var d = str.charCodeAt(i + 1);
        if (d >= 0xdc00 && d <= 0xdfff) { c = 0x10000 + ((c - 0xd800) << 10) + (d - 0xdc00); i++; }
      }
      if (c >= 0xd800 && c <= 0xdfff) c = 0xfffd;
      if (c < 0x80) out.push(c);
      else if (c < 0x800) out.push(0xc0 | (c >> 6), 0x80 | (c & 63));
      else if (c < 0x10000) out.push(0xe0 | (c >> 12), 0x80 | ((c >> 6) & 63), 0x80 | (c & 63));
      else out.push(0xf0 | (c >> 18), 0x80 | ((c >> 12) & 63), 0x80 | ((c >> 6) & 63), 0x80 | (c & 63));
    }
    return new Uint8Array(out);
  }

  function utf8Decode(u8) {
    var out = '', i = 0, n = u8.length;
    while (i < n) {
      var c = u8[i++], cp;
      if (c < 0x80) cp = c;
      else if (c >= 0xc2 && c < 0xe0 && i < n && (u8[i] & 0xc0) === 0x80) cp = ((c & 31) << 6) | (u8[i++] & 63);
      else if (c >= 0xe0 && c < 0xf0 && i + 1 < n && (u8[i] & 0xc0) === 0x80 && (u8[i + 1] & 0xc0) === 0x80) {
        cp = ((c & 15) << 12) | ((u8[i] & 63) << 6) | (u8[i + 1] & 63); i += 2;
        if (cp < 0x800 || (cp >= 0xd800 && cp <= 0xdfff)) cp = 0xfffd;
      } else if (c >= 0xf0 && c < 0xf5 && i + 2 < n && (u8[i] & 0xc0) === 0x80 && (u8[i + 1] & 0xc0) === 0x80 && (u8[i + 2] & 0xc0) === 0x80) {
        cp = ((c & 7) << 18) | ((u8[i] & 63) << 12) | ((u8[i + 1] & 63) << 6) | (u8[i + 2] & 63); i += 3;
        if (cp < 0x10000 || cp > 0x10ffff) cp = 0xfffd;
      } else cp = 0xfffd;
      if (cp >= 0x10000) { cp -= 0x10000; out += String.fromCharCode(0xd800 + (cp >> 10), 0xdc00 + (cp & 1023)); }
      else out += String.fromCharCode(cp);
    }
    return out;
  }

  function toBytes(data, enc) {
    if (typeof data === 'string') return Buffer.from(data, enc || 'utf8');
    if (data instanceof Uint8Array) return data;
    if (data instanceof ArrayBuffer) return new Uint8Array(data);
    if (ArrayBuffer.isView(data)) return new Uint8Array(data.buffer, data.byteOffset, data.byteLength);
    if (Array.isArray(data)) return new Uint8Array(data);
    throw new TypeError('The "data" argument must be of type string or an instance of Buffer, TypedArray, or DataView');
  }

  g.atob = function (s) {
    var bytes = b64ToBytes(s), out = '';
    for (var i = 0; i < bytes.length; i++) out += String.fromCharCode(bytes[i]);
    return out;
  };
  g.btoa = function (s) {
    s = String(s);
    var u8 = new Uint8Array(s.length);
    for (var i = 0; i < s.length; i++) {
      var c = s.charCodeAt(i);
      if (c > 255) throw new Error('InvalidCharacterError: btoa received a character outside the Latin1 range');
      u8[i] = c;
    }
    return bytesToB64(u8);
  };

  // ---------------------------------------------------------------- Buffer ------------------------------------------------------------
  function hexOf(u8) {
    var h = '';
    for (var i = 0; i < u8.length; i++) h += (u8[i] < 16 ? '0' : '') + u8[i].toString(16);
    return h;
  }
  function encodeString(str, enc) {
    switch (String(enc || 'utf8').toLowerCase()) {
      case 'utf8': case 'utf-8': return utf8Encode(str);
      case 'hex': {
        var n = str.length >> 1, out = new Uint8Array(n);
        for (var i = 0; i < n; i++) {
          var b = parseInt(str.substr(i * 2, 2), 16);
          if (isNaN(b)) { out = out.subarray(0, i); break; }
          out[i] = b;
        }
        return out;
      }
      case 'base64': case 'base64url': return b64ToBytes(str);
      case 'latin1': case 'binary': case 'ascii': {
        var l = new Uint8Array(str.length);
        for (var j = 0; j < str.length; j++) l[j] = str.charCodeAt(j) & 255;
        return l;
      }
      case 'utf16le': case 'ucs2': case 'ucs-2': case 'utf-16le': {
        var u = new Uint8Array(str.length * 2);
        for (var k = 0; k < str.length; k++) { var cc = str.charCodeAt(k); u[k * 2] = cc & 255; u[k * 2 + 1] = cc >> 8; }
        return u;
      }
      default: throw new TypeError('Unknown encoding: ' + enc);
    }
  }
  function decodeBytes(u8, enc) {
    switch (String(enc || 'utf8').toLowerCase()) {
      case 'utf8': case 'utf-8': return utf8Decode(u8);
      case 'hex': return hexOf(u8);
      case 'base64': return bytesToB64(u8);
      case 'base64url': return bytesToB64(u8).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
      case 'latin1': case 'binary': { var s = ''; for (var i = 0; i < u8.length; i++) s += String.fromCharCode(u8[i]); return s; }
      case 'ascii': { var a = ''; for (var j = 0; j < u8.length; j++) a += String.fromCharCode(u8[j] & 127); return a; }
      case 'utf16le': case 'ucs2': case 'ucs-2': case 'utf-16le': {
        var w = ''; for (var k = 0; k + 1 < u8.length; k += 2) w += String.fromCharCode(u8[k] | (u8[k + 1] << 8)); return w;
      }
      default: throw new TypeError('Unknown encoding: ' + enc);
    }
  }

  class Buffer extends Uint8Array {
    static from(value, encOrOffset, length) {
      var bytes;
      if (typeof value === 'string') bytes = encodeString(value, encOrOffset);
      else if (value instanceof ArrayBuffer) bytes = new Uint8Array(value, encOrOffset || 0, length === undefined ? undefined : length);
      else if (ArrayBuffer.isView(value) || Array.isArray(value)) bytes = Uint8Array.from(ArrayBuffer.isView(value) ? new Uint8Array(value.buffer, value.byteOffset, value.byteLength) : value);
      else if (value && value.type === 'Buffer' && Array.isArray(value.data)) bytes = Uint8Array.from(value.data);
      else throw new TypeError('The first argument must be of type string or an instance of Buffer, ArrayBuffer, or Array or an Array-like Object.');
      var copy = new Buffer(bytes.length);
      copy.set(bytes);
      return copy;
    }
    static alloc(size, fill) {
      var b = new Buffer(size);
      if (fill !== undefined && fill !== 0) b.fill(typeof fill === 'string' ? fill.charCodeAt(0) : fill);
      return b;
    }
    static allocUnsafe(size) { return new Buffer(size); }
    static isBuffer(v) { return v instanceof Buffer; }
    static byteLength(v, enc) { return typeof v === 'string' ? encodeString(v, enc).length : v.byteLength; }
    static concat(list, total) {
      var len = 0, i;
      for (i = 0; i < list.length; i++) len += list[i].length;
      if (total !== undefined) len = total;
      var out = new Buffer(len), pos = 0;
      for (i = 0; i < list.length && pos < len; i++) { var part = list[i].subarray(0, len - pos); out.set(part, pos); pos += part.length; }
      return out;
    }
    toString(enc, start, end) {
      var view = (start !== undefined || end !== undefined) ? this.subarray(start || 0, end === undefined ? this.length : end) : this;
      return decodeBytes(view, enc);
    }
    toJSON() { return { type: 'Buffer', data: Array.prototype.slice.call(this) }; }
    equals(other) {
      if (this.length !== other.length) return false;
      for (var i = 0; i < this.length; i++) if (this[i] !== other[i]) return false;
      return true;
    }
    slice(start, end) { return this.subarray(start, end); }
    write(str, offset, length, enc) {
      offset = offset || 0;
      var bytes = encodeString(str, typeof length === 'string' ? length : enc);
      var n = Math.min(bytes.length, this.length - offset, typeof length === 'number' ? length : Infinity);
      this.set(bytes.subarray(0, n), offset);
      return n;
    }
    readUInt8(o) { return this[o || 0]; }
    readUInt16BE(o) { o = o || 0; return (this[o] << 8) | this[o + 1]; }
    readUInt32BE(o) { o = o || 0; return ((this[o] * 0x1000000) + ((this[o + 1] << 16) | (this[o + 2] << 8) | this[o + 3])); }
  }
  g.Buffer = Buffer;

  // ---------------------------------------------------------------- console / process / misc ------------------------------------------
  function fmtArg(a) {
    if (typeof a === 'string') return a;
    if (a instanceof Error) return a.stack || (a.name + ': ' + a.message);
    try { return typeof a === 'object' && a !== null ? JSON.stringify(a) : String(a); } catch (e) { return String(a); }
  }
  function logger(level) {
    return function () {
      var parts = [];
      for (var i = 0; i < arguments.length; i++) parts.push(fmtArg(arguments[i]));
      var msg = parts.join(' ');
      if (msg.length > 4096) msg = msg.slice(0, 4096) + '...[truncated]';
      hostLog(level, msg);
    };
  }
  g.console = { log: logger('log'), info: logger('info'), warn: logger('warn'), error: logger('error'), debug: logger('debug'), trace: logger('debug') };

  var noop = function () {};
  var hr = function (prev) {
    var t = Date.now() - START, s = Math.floor(t / 1000), ns = (t % 1000) * 1e6;
    return prev ? [s - prev[0], ns - prev[1]] : [s, ns];
  };
  hr.bigint = function () { return BigInt(Date.now() - START) * 1000000n; };
  g.process = {
    platform: 'android', version: 'v20.11.0', versions: { node: '20.11.0' }, env: { NODE_ENV: 'production' }, argv: [], pid: 1,
    nextTick: function (fn) { var a = Array.prototype.slice.call(arguments, 1); Promise.resolve().then(function () { fn.apply(null, a); }); },
    hrtime: hr, cwd: function () { return '/'; }, on: noop, once: noop, off: noop, emit: noop, exit: noop, kill: noop,
    emitWarning: noop, memoryUsage: function () { return { rss: 0, heapTotal: 0, heapUsed: 0, external: 0 }; }, uptime: function () { return (Date.now() - START) / 1000; },
  };
  g.global = g;
  g.navigator = { userAgent: 'Mozilla/5.0 (Linux; Android 14) AppleWebKit/537.36 (KHTML, like Gecko) Tatakai/1.0' };
  g.performance = { now: function () { return Date.now() - START; }, timeOrigin: START };
  g.queueMicrotask = function (fn) { Promise.resolve().then(fn); };

  g.crypto = {
    randomUUID: function () {
      var b = b64ToBytes(hostRandom(16));
      b[6] = (b[6] & 15) | 64; b[8] = (b[8] & 63) | 128;
      var h = hexOf(b);
      return h.slice(0, 8) + '-' + h.slice(8, 12) + '-' + h.slice(12, 16) + '-' + h.slice(16, 20) + '-' + h.slice(20);
    },
    getRandomValues: function (arr) {
      var raw = b64ToBytes(hostRandom(arr.byteLength));
      new Uint8Array(arr.buffer, arr.byteOffset, arr.byteLength).set(raw);
      return arr;
    },
  };

  g.structuredClone = function clone(v, seen) {
    seen = seen || new Map();
    if (v === null || typeof v !== 'object') {
      if (typeof v === 'function') throw new Error('DataCloneError: function could not be cloned');
      return v;
    }
    if (seen.has(v)) return seen.get(v);
    var out;
    if (v instanceof Date) return new Date(v.getTime());
    if (v instanceof RegExp) return new RegExp(v.source, v.flags);
    if (v instanceof ArrayBuffer) return v.slice(0);
    if (ArrayBuffer.isView(v)) return new v.constructor(v);
    if (v instanceof Map) { out = new Map(); seen.set(v, out); v.forEach(function (val, key) { out.set(clone(key, seen), clone(val, seen)); }); return out; }
    if (v instanceof Set) { out = new Set(); seen.set(v, out); v.forEach(function (val) { out.add(clone(val, seen)); }); return out; }
    out = Array.isArray(v) ? [] : {};
    seen.set(v, out);
    Object.keys(v).forEach(function (k) { out[k] = clone(v[k], seen); });
    return out;
  };

  // ---------------------------------------------------------------- timers ----------------------------------------------------------
  var timers = Object.create(null), timerSeq = 1;
  function schedule(fn, ms, args, repeat) {
    var id = timerSeq++;
    ms = Math.max(Number(ms) || 0, repeat ? 4 : 0);
    timers[id] = true;
    (function arm() {
      hostSleep(ms | 0).then(function () {
        if (!timers[id]) return;
        if (!repeat) delete timers[id];
        try { fn.apply(null, args); } catch (e) { hostLog('error', 'Uncaught error in timer: ' + (e && e.message)); }
        if (repeat && timers[id]) arm();
      });
    })();
    return id;
  }
  g.setTimeout = function (fn, ms) { return schedule(fn, ms, Array.prototype.slice.call(arguments, 2), false); };
  g.setInterval = function (fn, ms) { return schedule(fn, ms, Array.prototype.slice.call(arguments, 2), true); };
  g.setImmediate = function (fn) {
    var args = Array.prototype.slice.call(arguments, 1), id = timerSeq++;
    timers[id] = true;
    Promise.resolve().then(function () {
      if (!timers[id]) return;
      delete timers[id];
      try { fn.apply(null, args); } catch (e) { hostLog('error', 'Uncaught error in immediate: ' + (e && e.message)); }
    });
    return id;
  };
  g.clearTimeout = g.clearInterval = g.clearImmediate = function (id) { delete timers[id]; };
  g.__tk_reset_timers = function () { timers = Object.create(null); };

  // ---------------------------------------------------------------- TextEncoder / TextDecoder -----------------------------------------
  g.TextEncoder = class TextEncoder {
    get encoding() { return 'utf-8'; }
    encode(s) { return utf8Encode(s === undefined ? '' : s); }
  };
  g.TextDecoder = class TextDecoder {
    constructor(label) {
      var l = String(label || 'utf-8').toLowerCase();
      if (l === 'utf8') l = 'utf-8';
      if (l !== 'utf-8' && l !== 'utf-16le') throw new RangeError('The "' + label + '" encoding is not supported');
      this.encoding = l;
    }
    decode(input) {
      if (input === undefined) return '';
      var u8 = toBytes(input);
      if (this.encoding === 'utf-16le') return decodeBytes(u8, 'utf16le');
      if (u8.length >= 3 && u8[0] === 0xef && u8[1] === 0xbb && u8[2] === 0xbf) u8 = u8.subarray(3);
      return utf8Decode(u8);
    }
  };

  // ---------------------------------------------------------------- URL / URLSearchParams ---------------------------------------------
  function formEncode(s) {
    return encodeURIComponent(s).replace(/%20/g, '+').replace(/[!'()~]/g, function (c) { return '%' + c.charCodeAt(0).toString(16).toUpperCase(); });
  }
  function formDecode(s) {
    s = s.replace(/\+/g, ' ');
    try { return decodeURIComponent(s); } catch (e) { return s; }
  }
  function looseEncode(s) {
    try { return encodeURI(s).replace(/%25([0-9A-Fa-f]{2})/g, '%$1'); } catch (e) { return s; }
  }

  class URLSearchParams {
    constructor(init) {
      this._list = [];
      this._url = null;
      if (init instanceof URLSearchParams) this._list = init._list.map(function (p) { return [p[0], p[1]]; });
      else if (typeof init === 'string') this._parse(init);
      else if (init && typeof init[Symbol.iterator] === 'function') { for (var pair of init) this._list.push([String(pair[0]), String(pair[1])]); }
      else if (init && typeof init === 'object') { Object.keys(init).forEach(function (k) { this._list.push([k, String(init[k])]); }, this); }
    }
    _parse(s) {
      this._list = [];
      if (s.charAt(0) === '?') s = s.slice(1);
      if (!s) return;
      s.split('&').forEach(function (part) {
        if (!part) return;
        var i = part.indexOf('=');
        this._list.push(i < 0 ? [formDecode(part), ''] : [formDecode(part.slice(0, i)), formDecode(part.slice(i + 1))]);
      }, this);
    }
    _sync() { if (this._url) this._url._setSearchFromParams(this.toString()); }
    get size() { return this._list.length; }
    append(k, v) { this._list.push([String(k), String(v)]); this._sync(); }
    delete(k) { this._list = this._list.filter(function (p) { return p[0] !== k; }); this._sync(); }
    get(k) { for (var i = 0; i < this._list.length; i++) if (this._list[i][0] === k) return this._list[i][1]; return null; }
    getAll(k) { return this._list.filter(function (p) { return p[0] === k; }).map(function (p) { return p[1]; }); }
    has(k) { return this._list.some(function (p) { return p[0] === k; }); }
    set(k, v) {
      k = String(k); v = String(v);
      var found = false, out = [];
      this._list.forEach(function (p) { if (p[0] === k) { if (!found) { found = true; out.push([k, v]); } } else out.push(p); });
      if (!found) out.push([k, v]);
      this._list = out; this._sync();
    }
    sort() { this._list.sort(function (a, b) { return a[0] < b[0] ? -1 : a[0] > b[0] ? 1 : 0; }); this._sync(); }
    forEach(cb, thisArg) { this._list.forEach(function (p) { cb.call(thisArg, p[1], p[0], this); }, this); }
    keys() { return this._list.map(function (p) { return p[0]; })[Symbol.iterator](); }
    values() { return this._list.map(function (p) { return p[1]; })[Symbol.iterator](); }
    entries() { return this._list.map(function (p) { return [p[0], p[1]]; })[Symbol.iterator](); }
    [Symbol.iterator]() { return this.entries(); }
    toString() { return this._list.map(function (p) { return formEncode(p[0]) + '=' + formEncode(p[1]); }).join('&'); }
  }
  g.URLSearchParams = URLSearchParams;

  var DEFAULT_PORTS = { 'http:': '80', 'https:': '443', 'ws:': '80', 'wss:': '443', 'ftp:': '21' };
  var SPECIAL = { 'http:': 1, 'https:': 1, 'ws:': 1, 'wss:': 1, 'ftp:': 1, 'file:': 1 };

  function removeDots(path) {
    var out = [];
    path.split('/').forEach(function (seg, i, all) {
      if (seg === '..') { if (out.length > 1) out.pop(); if (i === all.length - 1) out.push(''); }
      else if (seg === '.') { if (i === all.length - 1) out.push(''); }
      else out.push(seg);
    });
    var res = out.join('/');
    return res.charAt(0) === '/' ? res : '/' + res;
  }

  class URL {
    constructor(input, base) {
      input = String(input).trim();
      var m = /^([a-zA-Z][a-zA-Z0-9+.-]*):/.exec(input);
      if (!m) {
        if (base === undefined) throw new TypeError('Invalid URL: ' + input);
        var b = base instanceof URL ? base : new URL(base);
        this._init(b, input);
      } else {
        this._parseAbsolute(input);
      }
    }
    _parseAbsolute(input) {
      var m = /^([a-zA-Z][a-zA-Z0-9+.-]*:)(?:\/\/([^\/?#]*))?([^?#]*)(\?[^#]*)?(#.*)?$/.exec(input);
      if (!m) throw new TypeError('Invalid URL: ' + input);
      this.protocol = m[1].toLowerCase();
      this.username = ''; this.password = ''; this.hostname = ''; this.port = '';
      if (m[2] !== undefined) {
        var auth = m[2], at = auth.lastIndexOf('@');
        if (at >= 0) {
          var cred = auth.slice(0, at), ci = cred.indexOf(':');
          this.username = ci < 0 ? cred : cred.slice(0, ci);
          this.password = ci < 0 ? '' : cred.slice(ci + 1);
          auth = auth.slice(at + 1);
        }
        var pm = /^(\[[^\]]*\]|[^:]*)(?::(\d*))?$/.exec(auth);
        if (!pm) throw new TypeError('Invalid URL: ' + input);
        this.hostname = pm[1].toLowerCase();
        this.port = pm[2] || '';
        if (SPECIAL[this.protocol] && !this.hostname && this.protocol !== 'file:') throw new TypeError('Invalid URL: ' + input);
        if (DEFAULT_PORTS[this.protocol] === this.port) this.port = '';
      }
      var path = m[3] || '';
      if (SPECIAL[this.protocol] || m[2] !== undefined) path = removeDots(path || '/');
      this.pathname = looseEncode(path).replace(/\?/g, '%3F').replace(/#/g, '%23');
      this._search = m[4] && m[4] !== '?' ? looseEncode(m[4]) : '';
      this.hash = m[5] && m[5] !== '#' ? looseEncode(m[5]) : '';
      this._finish();
    }
    _init(b, rel) {
      this.protocol = b.protocol; this.username = b.username; this.password = b.password;
      this.hostname = b.hostname; this.port = b.port;
      var hashIdx = rel.indexOf('#'), hash = '';
      if (hashIdx >= 0) { hash = rel.slice(hashIdx); rel = rel.slice(0, hashIdx); }
      var qIdx = rel.indexOf('?'), query = null;
      if (qIdx >= 0) { query = rel.slice(qIdx); rel = rel.slice(0, qIdx); }
      if (rel.slice(0, 2) === '//') { this._parseAbsolute(b.protocol + rel + (query || '') + hash); return; }
      if (rel === '') {
        this.pathname = b.pathname;
        this._search = query !== null ? looseEncode(query === '?' ? '' : query) : b._search;
      } else {
        var path = rel.charAt(0) === '/' ? rel : b.pathname.replace(/[^\/]*$/, '') + rel;
        this.pathname = looseEncode(removeDots(path));
        this._search = query !== null && query !== '?' ? looseEncode(query) : '';
      }
      this.hash = hash && hash !== '#' ? looseEncode(hash) : '';
      this._finish();
    }
    _finish() {
      var self = this;
      this.searchParams = new URLSearchParams(this._search);
      this.searchParams._url = this;
      this._setSearchFromParams = function (s) { self._search = s ? '?' + s : ''; };
    }
    get search() { return this._search; }
    set search(v) { v = String(v); this._search = v && v !== '?' ? (v.charAt(0) === '?' ? v : '?' + v) : ''; this.searchParams._list = []; this.searchParams._parse(this._search); }
    get host() { return this.hostname + (this.port ? ':' + this.port : ''); }
    get origin() { return SPECIAL[this.protocol] && this.protocol !== 'file:' ? this.protocol + '//' + this.host : 'null'; }
    get href() {
      var auth = this.username ? this.username + (this.password ? ':' + this.password : '') + '@' : '';
      var hasAuthority = this.hostname !== '' || SPECIAL[this.protocol];
      return this.protocol + (hasAuthority ? '//' : '') + auth + this.host + this.pathname + this._search + this.hash;
    }
    toString() { return this.href; }
    toJSON() { return this.href; }
  }
  g.URL = URL;

  // ---------------------------------------------------------------- Headers / Response / fetch ---------------------------------------
  class Headers {
    constructor(init) {
      this._m = Object.create(null);
      if (init instanceof Headers) init.forEach(function (v, k) { this.append(k, v); }, this);
      else if (Array.isArray(init)) init.forEach(function (p) { this.append(p[0], p[1]); }, this);
      else if (init && typeof init === 'object') Object.keys(init).forEach(function (k) { this.append(k, init[k]); }, this);
    }
    append(k, v) { k = String(k).toLowerCase(); this._m[k] = this._m[k] === undefined ? String(v) : this._m[k] + ', ' + String(v); }
    set(k, v) { this._m[String(k).toLowerCase()] = String(v); }
    get(k) { var v = this._m[String(k).toLowerCase()]; return v === undefined ? null : v; }
    has(k) { return this._m[String(k).toLowerCase()] !== undefined; }
    delete(k) { delete this._m[String(k).toLowerCase()]; }
    forEach(cb, t) { Object.keys(this._m).forEach(function (k) { cb.call(t, this._m[k], k, this); }, this); }
    keys() { return Object.keys(this._m)[Symbol.iterator](); }
    values() { var m = this._m; return Object.keys(m).map(function (k) { return m[k]; })[Symbol.iterator](); }
    entries() { var m = this._m; return Object.keys(m).map(function (k) { return [k, m[k]]; })[Symbol.iterator](); }
    [Symbol.iterator]() { return this.entries(); }
  }
  g.Headers = Headers;

  class Response {
    constructor(bodyBytes, init) {
      init = init || {};
      this._bytes = bodyBytes || new Uint8Array(0);
      this.status = init.status === undefined ? 200 : init.status;
      this.statusText = init.statusText || '';
      this.ok = this.status >= 200 && this.status < 300;
      this.headers = new Headers(init.headers);
      this.url = init.url || '';
      this.redirected = false;
      this.bodyUsed = false;
      this.type = 'basic';
    }
    arrayBuffer() { this.bodyUsed = true; var b = this._bytes; return Promise.resolve(b.buffer.slice(b.byteOffset, b.byteOffset + b.byteLength)); }
    text() { this.bodyUsed = true; return Promise.resolve(utf8Decode(this._bytes)); }
    json() { return this.text().then(function (t) { return JSON.parse(t); }); }
    clone() { return new Response(this._bytes, { status: this.status, statusText: this.statusText, headers: this.headers, url: this.url }); }
  }
  g.Response = Response;

  function bodyToB64(body) {
    if (body === undefined || body === null) return null;
    if (typeof body === 'string') return bytesToB64(utf8Encode(body));
    if (body instanceof URLSearchParams) return bytesToB64(utf8Encode(body.toString()));
    if (body instanceof ArrayBuffer || ArrayBuffer.isView(body)) return bytesToB64(toBytes(body));
    throw new TypeError('Unsupported request body type');
  }

  g.fetch = function (input, init) {
    return new Promise(function (resolve, reject) {
      try {
        init = init || {};
        var url = typeof input === 'string' ? input : (input && input.url ? input.url : String(input));
        var headers = {};
        new Headers(init.headers || (input && input.headers)).forEach(function (v, k) { headers[k] = v; });
        var method = String(init.method || (input && input.method) || 'GET').toUpperCase();
        var signal = init.signal;
        if (signal && signal.aborted) { reject(abortError()); return; }
        var req = { url: url, method: method, headers: headers, body: bodyToB64(init.body !== undefined ? init.body : input && input.body) };
        hostFetch(JSON.stringify(req)).then(function (text) {
          var res = JSON.parse(text);
          if (res.error) { reject(new TypeError(res.error)); return; }
          resolve(new Response(b64ToBytes(res.body), { status: res.status, statusText: res.statusText, headers: res.headers, url: res.url }));
        }, function (e) { reject(new TypeError(String(e && e.message ? e.message : e))); });
      } catch (e) { reject(e); }
    });
  };
  g.__tatakai_fetch__ = g.fetch;

  function abortError() { var e = new Error('The operation was aborted'); e.name = 'AbortError'; return e; }
  g.AbortSignal = class AbortSignal {
    constructor() { this.aborted = false; this.reason = undefined; this._l = []; }
    addEventListener(t, fn) { if (t === 'abort') this._l.push(fn); }
    removeEventListener(t, fn) { this._l = this._l.filter(function (f) { return f !== fn; }); }
    throwIfAborted() { if (this.aborted) throw abortError(); }
    static timeout(ms) { var c = new g.AbortController(); g.setTimeout(function () { c.abort(); }, ms); return c.signal; }
  };
  g.AbortController = class AbortController {
    constructor() { this.signal = new g.AbortSignal(); }
    abort(reason) {
      if (this.signal.aborted) return;
      this.signal.aborted = true; this.signal.reason = reason || abortError();
      this.signal._l.forEach(function (f) { try { f({ type: 'abort' }); } catch (e) { /* ignore */ } });
    }
  };

  // ---------------------------------------------------------------- __tatakai_parse_html__ (cheerio-like adapter over jsoup) ---------
  function H(op, handle, arg) { return JSON.parse(hostHtml(op, handle, arg === undefined ? '' : String(arg))); }
  class Cheerio {
    constructor(handle) { this._h = handle; this.length = H('len', handle); }
    eq(i) { return new Cheerio(H('eq', this._h, i)); }
    first() { return new Cheerio(H('eq', this._h, 0)); }
    last() { return new Cheerio(H('eq', this._h, -1)); }
    parent() { return new Cheerio(H('parent', this._h)); }
    children(sel) { return new Cheerio(H('children', this._h, sel || '')); }
    next() { return new Cheerio(H('next', this._h)); }
    prev() { return new Cheerio(H('prev', this._h)); }
    find(sel) { return new Cheerio(H('find', this._h, sel)); }
    attr(name) { var r = H('attr', this._h, name); return r === null ? undefined : r; }
    text() { return H('text', this._h); }
    html() { return H('html', this._h); }
    each(cb) {
      for (var i = 0; i < this.length; i++) cb(i, new Cheerio(H('eq', this._h, i)));
      return this;
    }
  }
  g.__tatakai_parse_html__ = function (html) {
    if (typeof html !== 'string' || html.length === 0) {
      var err = new Error('InvalidInputError: html must be a non-empty string');
      err.name = 'InvalidInputError';
      throw err;
    }
    return new Cheerio(H('parse', 0, html));
  };
  g.__tatakai_anitomy__ = function () { return {}; };

  // ---------------------------------------------------------------- require() -------------------------------------------------------
  function hashImpl(algo) {
    algo = String(algo).toLowerCase().replace('-', '');
    if (['md5', 'sha1', 'sha256', 'sha512'].indexOf(algo) < 0) throw new Error('Digest method not supported: ' + algo);
    return algo;
  }
  function digestOut(b64, enc) {
    var bytes = b64ToBytes(b64);
    if (!enc || enc === 'buffer') return Buffer.from(bytes);
    return decodeBytes(bytes, enc);
  }
  function Accumulator() { this.parts = []; }
  Accumulator.prototype.add = function (data, enc) { this.parts.push(toBytes(data, enc)); };
  Accumulator.prototype.all = function () { return bytesToB64(Buffer.concat(this.parts)); };

  var cryptoModule = {
    createHash: function (algo) {
      var a = hashImpl(algo), acc = new Accumulator();
      var h = { update: function (d, enc) { acc.add(d, enc); return h; }, digest: function (enc) { return digestOut(hostHash(a, acc.all()), enc); } };
      return h;
    },
    createHmac: function (algo, key) {
      var a = hashImpl(algo), acc = new Accumulator(), k = bytesToB64(toBytes(key));
      var h = { update: function (d, enc) { acc.add(d, enc); return h; }, digest: function (enc) { return digestOut(hostHmac(a, k, acc.all()), enc); } };
      return h;
    },
    randomUUID: function () { return g.crypto.randomUUID(); },
    randomBytes: function (n) { return Buffer.from(b64ToBytes(hostRandom(n))); },
    getRandomValues: function (a) { return g.crypto.getRandomValues(a); },
  };

  class EventEmitter {
    constructor() { this._events = Object.create(null); }
    on(n, fn) { (this._events[n] = this._events[n] || []).push(fn); return this; }
    addListener(n, fn) { return this.on(n, fn); }
    once(n, fn) { var self = this; function w() { self.off(n, w); return fn.apply(this, arguments); } w.listener = fn; return this.on(n, w); }
    off(n, fn) { var l = this._events[n]; if (l) this._events[n] = l.filter(function (f) { return f !== fn && f.listener !== fn; }); return this; }
    removeListener(n, fn) { return this.off(n, fn); }
    removeAllListeners(n) { if (n === undefined) this._events = Object.create(null); else delete this._events[n]; return this; }
    emit(n) {
      var l = this._events[n];
      if (!l || !l.length) { if (n === 'error') throw arguments[1]; return false; }
      var args = Array.prototype.slice.call(arguments, 1);
      l.slice().forEach(function (f) { f.apply(this, args); }, this);
      return true;
    }
    listenerCount(n) { return (this._events[n] || []).length; }
  }
  EventEmitter.EventEmitter = EventEmitter;

  function urlParse(s) {
    try {
      var u = new URL(s);
      return { protocol: u.protocol, auth: u.username ? u.username + (u.password ? ':' + u.password : '') : null, host: u.host, port: u.port || null,
        hostname: u.hostname, hash: u.hash || null, search: u.search || null, query: u.search ? u.search.slice(1) : null, pathname: u.pathname,
        path: u.pathname + u.search, href: u.href };
    } catch (e) {
      var m = /^([^?#]*)(\?[^#]*)?(#.*)?$/.exec(String(s));
      return { protocol: null, auth: null, host: null, port: null, hostname: null, hash: m[3] || null, search: m[2] || null,
        query: m[2] ? m[2].slice(1) : null, pathname: m[1] || null, path: (m[1] || '') + (m[2] || ''), href: String(s) };
    }
  }

  function normalizePath(p) {
    var abs = p.charAt(0) === '/', out = [];
    p.split('/').forEach(function (seg) {
      if (!seg || seg === '.') return;
      if (seg === '..') { if (out.length && out[out.length - 1] !== '..') out.pop(); else if (!abs) out.push('..'); }
      else out.push(seg);
    });
    var r = (abs ? '/' : '') + out.join('/');
    if (p.length > 1 && p.charAt(p.length - 1) === '/' && r !== '/') r += '/';
    return r || (abs ? '/' : '.');
  }
  var pathModule = {
    sep: '/', delimiter: ':',
    normalize: normalizePath,
    isAbsolute: function (p) { return String(p).charAt(0) === '/'; },
    join: function () { var parts = Array.prototype.filter.call(arguments, function (a) { return a !== ''; }); return parts.length ? normalizePath(parts.join('/')) : '.'; },
    resolve: function () {
      var r = '';
      for (var i = arguments.length - 1; i >= 0 && r.charAt(0) !== '/'; i--) if (arguments[i]) r = arguments[i] + (r ? '/' + r : '');
      if (r.charAt(0) !== '/') r = '/' + r;
      var n = normalizePath(r);
      return n.length > 1 && n.charAt(n.length - 1) === '/' ? n.slice(0, -1) : n;
    },
    basename: function (p, ext) { p = String(p).replace(/\/+$/, ''); var b = p.slice(p.lastIndexOf('/') + 1); return ext && b.slice(-ext.length) === ext && b !== ext ? b.slice(0, -ext.length) : b; },
    dirname: function (p) { p = String(p).replace(/\/+$/, ''); var i = p.lastIndexOf('/'); return i < 0 ? '.' : i === 0 ? '/' : p.slice(0, i); },
    extname: function (p) { var b = pathModule.basename(p), i = b.lastIndexOf('.'); return i <= 0 ? '' : b.slice(i); },
  };

  var querystringModule = {
    escape: function (s) { return encodeURIComponent(s); },
    unescape: function (s) { try { return decodeURIComponent(s); } catch (e) { return s; } },
    parse: function (str, sep, eq) {
      sep = sep || '&'; eq = eq || '=';
      var out = Object.create(null);
      String(str || '').split(sep).forEach(function (part) {
        if (!part) return;
        var i = part.indexOf(eq);
        var k = formDecode(i < 0 ? part : part.slice(0, i)), v = i < 0 ? '' : formDecode(part.slice(i + eq.length));
        if (k in out) { if (Array.isArray(out[k])) out[k].push(v); else out[k] = [out[k], v]; } else out[k] = v;
      });
      return out;
    },
    stringify: function (obj, sep, eq) {
      sep = sep || '&'; eq = eq || '=';
      return Object.keys(obj || {}).map(function (k) {
        var v = obj[k];
        return (Array.isArray(v) ? v : [v]).map(function (x) { return encodeURIComponent(k) + eq + encodeURIComponent(x === undefined || x === null ? '' : x); }).join(sep);
      }).join(sep);
    },
  };

  function utilFormat(f) {
    var args = Array.prototype.slice.call(arguments, 1), i = 0;
    if (typeof f !== 'string') return [f].concat(args).map(function (a) { return typeof a === 'string' ? a : utilInspect(a); }).join(' ');
    var s = f.replace(/%[sdifjoO%]/g, function (m) {
      if (m === '%%') return '%';
      if (i >= args.length) return m;
      var a = args[i++];
      switch (m) {
        case '%s': return typeof a === 'string' ? a : utilInspect(a);
        case '%d': return String(Number(a));
        case '%i': return String(parseInt(a, 10));
        case '%f': return String(parseFloat(a));
        case '%j': try { return JSON.stringify(a); } catch (e) { return '[Circular]'; }
        default: return utilInspect(a);
      }
    });
    for (; i < args.length; i++) s += ' ' + (typeof args[i] === 'string' ? args[i] : utilInspect(args[i]));
    return s;
  }
  function utilInspect(v) {
    if (typeof v === 'string') return "'" + v + "'";
    if (v instanceof Error) return v.stack || String(v);
    try { return typeof v === 'object' && v !== null ? JSON.stringify(v) : String(v); } catch (e) { return '[Object]'; }
  }
  function deepEqual(a, b) {
    if (a === b) return true;
    if (typeof a !== 'object' || typeof b !== 'object' || a === null || b === null) return a !== a && b !== b;
    if (Object.getPrototypeOf(a) !== Object.getPrototypeOf(b)) return false;
    var ka = Object.keys(a), kb = Object.keys(b);
    if (ka.length !== kb.length) return false;
    return ka.every(function (k) { return Object.prototype.hasOwnProperty.call(b, k) && deepEqual(a[k], b[k]); });
  }
  var utilModule = {
    format: utilFormat, inspect: utilInspect,
    promisify: function (fn) {
      return function () {
        var args = Array.prototype.slice.call(arguments), self = this;
        return new Promise(function (res, rej) { args.push(function (err, val) { if (err) rej(err); else res(val); }); fn.apply(self, args); });
      };
    },
    types: {
      isPromise: function (v) { return v instanceof Promise; }, isDate: function (v) { return v instanceof Date; },
      isRegExp: function (v) { return v instanceof RegExp; }, isMap: function (v) { return v instanceof Map; }, isSet: function (v) { return v instanceof Set; },
      isTypedArray: function (v) { return ArrayBuffer.isView(v) && !(v instanceof DataView); }, isUint8Array: function (v) { return v instanceof Uint8Array; },
    },
    TextEncoder: g.TextEncoder, TextDecoder: g.TextDecoder,
    inherits: function (ctor, sup) { Object.setPrototypeOf(ctor.prototype, sup.prototype); Object.setPrototypeOf(ctor, sup); ctor.super_ = sup; },
    isDeepStrictEqual: deepEqual,
  };

  var STUBS = ['os', 'http', 'https', 'stream', 'zlib', 'string_decoder', 'assert', 'timers', 'readline', 'perf_hooks', 'async_hooks', 'module', 'tty'];
  var DATA_KEYS = { EOL: undefined, constants: undefined, isTTY: false };
  function makeStub(name) {
    function fail(member) { throw new Error(name + '.' + member + ' is not available in the native extension runtime'); }
    var target = function () { fail('(call)'); };
    return new Proxy(target, {
      get: function (t, key) {
        if (typeof key === 'symbol' || key === 'then' || key === '__esModule' || key === 'prototype' || key === 'name' || key === 'length') return t[key];
        if (key in DATA_KEYS) return DATA_KEYS[key];
        var f = function () { fail(key); };
        return f;
      },
      apply: function () { fail('(call)'); },
      construct: function () { fail('(constructor)'); },
    });
  }

  var MODULES = {
    crypto: function () { return cryptoModule; },
    buffer: function () { return { Buffer: Buffer }; },
    events: function () { return EventEmitter; },
    url: function () {
      return { URL: URL, URLSearchParams: URLSearchParams, parse: urlParse,
        format: function (o) { return typeof o === 'string' ? o : o instanceof URL ? o.href : (o.protocol ? o.protocol + '//' : '') + (o.host || o.hostname || '') + (o.pathname || '') + (o.search || ''); },
        fileURLToPath: function () { throw new Error('url.fileURLToPath is not available in the native extension runtime'); } };
    },
    path: function () { return pathModule; },
    querystring: function () { return querystringModule; },
    util: function () { return utilModule; },
  };
  var UNSUPPORTED_MSG = function (n) { return "Module '" + n + "' is not supported by the native extension runtime"; };
  var cache = Object.create(null);
  g.__tk_require = function (name) {
    name = String(name);
    if (name.indexOf('node:') === 0) name = name.slice(5);
    if (cache[name]) return cache[name];
    var mod;
    if (Object.prototype.hasOwnProperty.call(MODULES, name)) mod = MODULES[name]();
    else if (STUBS.indexOf(name) >= 0) mod = makeStub(name);
    else throw new Error(UNSUPPORTED_MSG(name));
    cache[name] = mod;
    return mod;
  };
  g.__tk_require.resolve = function (name) { throw new Error(UNSUPPORTED_MSG(name)); };

  // ---------------------------------------------------------------- bundle instantiation and calls -----------------------------------
  g.__tk_instantiate = function () {
    var module = { exports: {} };
    g.__tk_bundle_fn.call(module.exports, module, module.exports, g.__tk_require, g.__tatakai_fetch__, g.__tatakai_anitomy__,
      g.__tatakai_parse_html__, g.Buffer, g.process, g, g.setImmediate, g.clearImmediate, g.fetch);
    var ex = module.exports && module.exports.default ? module.exports.default : module.exports;
    var inst = typeof ex === 'function' ? new ex() : ex;
    if (!inst || typeof inst !== 'object') throw new Error('Extension bundle produced no usable export');
    g.__tk_instance = inst;
    return true;
  };
  g.__tk_has = function (name) { return !!g.__tk_instance && typeof g.__tk_instance[name] === 'function'; };
  g.__tk_call = async function (method, argsJson, collect) {
    var inst = g.__tk_instance, fn = inst && inst[method];
    if (typeof fn !== 'function') throw new Error('__TK_NOT_IMPLEMENTED__' + method);
    var args = JSON.parse(argsJson), chunks = [];
    if (collect) args.push(function (c) { try { chunks.push(JSON.parse(JSON.stringify(c === undefined ? null : c))); } catch (e) { /* ignore */ } });
    var r = await fn.apply(inst, args);
    return JSON.stringify({ result: r === undefined ? null : r, chunks: chunks });
  };
})(globalThis);
