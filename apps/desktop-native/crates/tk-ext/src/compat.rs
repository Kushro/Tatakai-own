//! Install-time compatibility analysis. Rust port of `contracts/extension-host/compat.mjs` (regex-free scanner so
//! no extra crate is needed); must return the same level for the same bundle text.

use tk_core::Compat;

pub const SUPPORTED_MODULES: [&str; 7] = ["crypto", "buffer", "events", "url", "path", "querystring", "util"];
pub const STUB_MODULES: [&str; 13] =
    ["os", "http", "https", "stream", "zlib", "string_decoder", "assert", "timers", "readline", "perf_hooks", "async_hooks", "module", "tty"];
pub const UNSUPPORTED_BUILTINS: [&str; 13] =
    ["fs", "child_process", "net", "tls", "dgram", "cluster", "worker_threads", "vm", "dns", "http2", "inspector", "v8", "repl"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompatReport {
    pub level: Compat,
    pub specifiers: Vec<String>,
    pub unsupported: Vec<String>,
    pub stubs: Vec<String>,
    pub dynamic: bool,
    pub reason: Option<String>,
}

fn is_word(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

fn skip_ws(s: &[u8], mut i: usize) -> usize {
    while i < s.len() && (s[i].is_ascii_whitespace() || s[i] == 0x0b || s[i] == 0x0c) {
        i += 1;
    }
    i
}

/// Positions of `\brequire` keywords.
fn require_sites(s: &[u8]) -> impl Iterator<Item = usize> + '_ {
    let kw = b"require";
    (0..s.len().saturating_sub(kw.len() - 1)).filter(move |&i| s[i..].starts_with(kw) && (i == 0 || !is_word(s[i - 1])))
}

pub fn analyze_bundle(source: &str) -> CompatReport {
    let s = source.as_bytes();
    let mut specifiers: Vec<String> = Vec::new();
    let mut any_count = 0usize;
    let mut literal_count = 0usize;
    let mut resolve = false;
    let mut next_free = 0usize; // regex matches do not overlap
    for site in require_sites(s) {
        if site < next_free {
            continue;
        }
        let after = site + "require".len();
        // `require\s*\.\s*resolve\b`
        let j = skip_ws(s, after);
        if j < s.len() && s[j] == b'.' {
            let k = skip_ws(s, j + 1);
            if s[k..].starts_with(b"resolve") && s.get(k + 7).is_none_or(|c| !is_word(*c)) {
                resolve = true;
            }
        }
        // `require\s*(`
        if j >= s.len() || s[j] != b'(' {
            continue;
        }
        any_count += 1;
        next_free = j + 1;
        // literal form
        let p = skip_ws(s, j + 1);
        let Some(&q) = s.get(p) else { continue };
        if !matches!(q, b'\'' | b'"' | b'`') {
            continue;
        }
        let mut e = p + 1;
        while e < s.len() && !matches!(s[e], b'\'' | b'"' | b'`' | b'\r' | b'\n') {
            e += 1;
        }
        if e == p + 1 || e >= s.len() || s[e] != q {
            continue;
        }
        let close = skip_ws(s, e + 1);
        if s.get(close) == Some(&b')') {
            let spec = &source[p + 1..e];
            specifiers.push(spec.strip_prefix("node:").unwrap_or(spec).to_owned());
            literal_count += 1;
            next_free = close + 1;
        }
    }
    let dynamic_import = (0..s.len().saturating_sub(5)).any(|i| {
        s[i..].starts_with(b"import")
            && (i == 0 || !(is_word(s[i - 1]) || s[i - 1] == b'.' || s[i - 1] == b'$'))
            && s.get(skip_ws(s, i + 6)) == Some(&b'(')
    });
    let dynamic = any_count > literal_count || resolve || dynamic_import;

    let mut unique: Vec<String> = Vec::new();
    for sp in specifiers {
        if !unique.contains(&sp) {
            unique.push(sp);
        }
    }
    let unsupported: Vec<String> =
        unique.iter().filter(|x| !SUPPORTED_MODULES.contains(&x.as_str()) && !STUB_MODULES.contains(&x.as_str())).cloned().collect();
    let stubs: Vec<String> = unique.iter().filter(|x| STUB_MODULES.contains(&x.as_str())).cloned().collect();

    if let Some(first) = unsupported.first() {
        let kind = if UNSUPPORTED_BUILTINS.contains(&first.as_str()) { "Node module" } else { "module" };
        return CompatReport {
            level: Compat::Unsupported,
            reason: Some(format!("Uses {kind} '{first}' which the native runtime does not provide")),
            specifiers: unique,
            unsupported,
            stubs,
            dynamic,
        };
    }
    if !stubs.is_empty() || dynamic {
        let reason = match stubs.first() {
            Some(st) => format!("Uses stubbed Node module '{st}'"),
            None => "Uses dynamic require/import".to_owned(),
        };
        return CompatReport { level: Compat::Partial, specifiers: unique, unsupported, stubs, dynamic, reason: Some(reason) };
    }
    CompatReport { level: Compat::Full, specifiers: unique, unsupported, stubs, dynamic, reason: None }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn level(src: &str) -> Compat {
        analyze_bundle(src).level
    }

    #[test]
    fn mirrors_the_reference_cases() {
        assert_eq!(level("const c = require('crypto');"), Compat::Full);
        assert_eq!(level("require('node:crypto'); require(`path`)"), Compat::Full);
        assert_eq!(level("require('http')"), Compat::Partial);
        assert_eq!(level("require( \"zlib\" )"), Compat::Partial);
        assert_eq!(level("const m = 'x'; require(m)"), Compat::Partial);
        assert_eq!(level("require.resolve('x')"), Compat::Partial);
        assert_eq!(level("const m = import('x')"), Compat::Partial);
        assert_eq!(level("foo.import('x')"), Compat::Full);
        assert_eq!(level("require('fs')"), Compat::Unsupported);
        assert_eq!(level("require('node:fs')"), Compat::Unsupported);
        assert_eq!(level("require('./local')"), Compat::Unsupported);
        assert_eq!(level("require('cheerio'); require('crypto')"), Compat::Unsupported);
        assert_eq!(level("no requires at all, mrequire('fs') is not one"), Compat::Full);
        assert_eq!(level("const x = 1;"), Compat::Full);
    }

    #[test]
    fn reasons_match_the_fixture_text() {
        let r = analyze_bundle("const fs = require('node:fs');");
        assert_eq!(r.reason.as_deref(), Some("Uses Node module 'fs' which the native runtime does not provide"));
        assert_eq!(analyze_bundle("require('undici')").reason.as_deref(), Some("Uses module 'undici' which the native runtime does not provide"));
        assert_eq!(analyze_bundle("require('os')").reason.as_deref(), Some("Uses stubbed Node module 'os'"));
        assert_eq!(analyze_bundle("require(x)").reason.as_deref(), Some("Uses dynamic require/import"));
    }
}
