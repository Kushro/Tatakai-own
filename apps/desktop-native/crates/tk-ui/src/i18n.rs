//! i18n: `en` and `es` only (06 #4). Japanese falls back to English. No literal text in components:
//! everything goes through [`t`] / [`tf`].
//!
//! Catalogs are split by namespace (docs/redesign/15-parity-wave-a-contract.md section 2): one JSON file per
//! owner under `i18n/{en,es}/<namespace>.json`, merged here at compile time. A key lives in exactly one
//! namespace (same file name in both languages); a worker adds keys only to the files of its namespace.

use std::collections::HashMap;

use dioxus::prelude::{GlobalSignal, ReadableExt, Signal};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU8, Ordering};

/// Namespace files, in the order they are merged. Keep both lists in sync (same names, same order).
pub const NAMESPACES: [&str; 9] =
    ["common", "search", "details", "player", "settings_player", "subs", "system", "library", "torrent"];

const EN_FILES: [&str; 9] = [
    include_str!("../i18n/en/common.json"),
    include_str!("../i18n/en/search.json"),
    include_str!("../i18n/en/details.json"),
    include_str!("../i18n/en/player.json"),
    include_str!("../i18n/en/settings_player.json"),
    include_str!("../i18n/en/subs.json"),
    include_str!("../i18n/en/system.json"),
    include_str!("../i18n/en/library.json"),
    include_str!("../i18n/en/torrent.json"),
];
const ES_FILES: [&str; 9] = [
    include_str!("../i18n/es/common.json"),
    include_str!("../i18n/es/search.json"),
    include_str!("../i18n/es/details.json"),
    include_str!("../i18n/es/player.json"),
    include_str!("../i18n/es/settings_player.json"),
    include_str!("../i18n/es/subs.json"),
    include_str!("../i18n/es/system.json"),
    include_str!("../i18n/es/library.json"),
    include_str!("../i18n/es/torrent.json"),
];

/// The namespace files of a language (`NAMESPACES` order).
pub fn files(lang: Lang) -> &'static [&'static str; 9] {
    match lang {
        Lang::En => &EN_FILES,
        Lang::Es => &ES_FILES,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Lang {
    #[default]
    En,
    Es,
}

impl Lang {
    pub fn code(self) -> &'static str {
        match self {
            Self::En => "en",
            Self::Es => "es",
        }
    }
}

/// User choice in Settings: follow the system or force a language.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LangChoice {
    #[default]
    System,
    En,
    Es,
}

impl LangChoice {
    pub const ALL: [LangChoice; 3] = [Self::System, Self::En, Self::Es];

    pub fn id(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::En => "en",
            Self::Es => "es",
        }
    }
}

/// BCP-47 / locale tag (`es-MX`, `es_ES.UTF-8`, `ja-JP`) -> supported language (anything else: English).
pub fn lang_from_tag(tag: &str) -> Lang {
    let primary = tag.trim().split(['-', '_', '.']).next().unwrap_or("").to_ascii_lowercase();
    if primary == "es" { Lang::Es } else { Lang::En }
}

/// Apply the override on top of the detected system tag.
pub fn resolve(choice: LangChoice, system_tag: &str) -> Lang {
    match choice {
        LangChoice::System => lang_from_tag(system_tag),
        LangChoice::En => Lang::En,
        LangChoice::Es => Lang::Es,
    }
}

type Catalog = HashMap<String, String>;

fn parse(json: &str) -> Catalog {
    serde_json::from_str(json).unwrap_or_default()
}

/// Merge every namespace file of a language. A key defined twice keeps the first file's value (the test
/// `no_key_is_defined_in_two_namespaces` forbids that situation).
fn merge(files: &[&str]) -> Catalog {
    let mut all = Catalog::new();
    for f in files {
        for (k, v) in parse(f) {
            all.entry(k).or_insert(v);
        }
    }
    all
}

fn catalog(lang: Lang) -> &'static Catalog {
    static EN: OnceLock<Catalog> = OnceLock::new();
    static ES: OnceLock<Catalog> = OnceLock::new();
    match lang {
        Lang::En => EN.get_or_init(|| merge(&EN_FILES)),
        Lang::Es => ES.get_or_init(|| merge(&ES_FILES)),
    }
}

static CURRENT: AtomicU8 = AtomicU8::new(0);

/// Bumped on every language change so components that call [`t`] while rendering re-render.
static LANG_REV: GlobalSignal<u64> = Signal::global(|| 0);

fn in_runtime() -> bool {
    dioxus::dioxus_core::Runtime::try_current().is_some()
}

/// Switch the UI language. Call from an effect or event handler, never while rendering.
pub fn set_lang(lang: Lang) {
    let changed = CURRENT.swap(lang as u8, Ordering::Relaxed) != lang as u8;
    if changed && in_runtime() {
        *LANG_REV.write() += 1;
    }
}

pub fn lang() -> Lang {
    if CURRENT.load(Ordering::Relaxed) == Lang::Es as u8 { Lang::Es } else { Lang::En }
}

/// Look up `key` in the current language; falls back to English, then to the key itself.
pub fn t(key: &str) -> String {
    if in_runtime() {
        // Subscribe the calling component to language changes.
        let _ = LANG_REV.read();
    }
    lookup(lang(), key)
}

/// [`t`] with `{name}` placeholders replaced.
pub fn tf(key: &str, args: &[(&str, &str)]) -> String {
    let mut s = t(key);
    for (name, value) in args {
        s = s.replace(&format!("{{{name}}}"), value);
    }
    s
}

/// A count: `{key}.one` when `n` is 1, `key` otherwise, with `{n}` replaced.
pub fn tn(key: &str, n: usize) -> String {
    let n_text = n.to_string();
    let args = [("n", n_text.as_str())];
    if n == 1 { tf(&format!("{key}.one"), &args) } else { tf(key, &args) }
}

pub fn lookup(lang: Lang, key: &str) -> String {
    catalog(lang)
        .get(key)
        .or_else(|| catalog(Lang::En).get(key))
        .cloned()
        .unwrap_or_else(|| key.to_owned())
}

/// All keys of a catalog (tests, tooling).
pub fn keys(lang: Lang) -> Vec<&'static str> {
    let mut k: Vec<&str> = catalog(lang).keys().map(String::as_str).collect();
    k.sort_unstable();
    k
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;
    use std::path::Path;

    #[test]
    fn catalogs_parse_and_are_not_empty() {
        for (lang, list) in [(Lang::En, &EN_FILES), (Lang::Es, &ES_FILES)] {
            for (ns, json) in NAMESPACES.iter().zip(list.iter()) {
                assert!(serde_json::from_str::<Catalog>(json).is_ok(), "{}/{ns}.json is invalid", lang.code());
            }
        }
        assert!(keys(Lang::En).len() > 50);
    }

    /// A key belongs to exactly one namespace (its owner's file), in both languages.
    #[test]
    fn no_key_is_defined_in_two_namespaces() {
        for (lang, list) in [(Lang::En, &EN_FILES), (Lang::Es, &ES_FILES)] {
            let mut seen: std::collections::BTreeMap<String, &str> = std::collections::BTreeMap::new();
            for (ns, json) in NAMESPACES.iter().zip(list.iter()) {
                for k in parse(json).into_keys() {
                    if let Some(prev) = seen.insert(k.clone(), ns) {
                        panic!("{}: key {k} is in both {prev}.json and {ns}.json", lang.code());
                    }
                }
            }
        }
    }

    /// Each namespace file has the same key set in `en` and `es` (a key never moves between namespaces).
    #[test]
    fn namespaces_match_between_languages() {
        for (ns, (en, es)) in NAMESPACES.iter().zip(EN_FILES.iter().zip(ES_FILES.iter())) {
            let en: BTreeSet<_> = parse(en).into_keys().collect();
            let es: BTreeSet<_> = parse(es).into_keys().collect();
            assert_eq!(en, es, "{ns}.json differs between en and es");
        }
    }

    #[test]
    fn en_and_es_have_identical_keys() {
        let en: BTreeSet<_> = keys(Lang::En).into_iter().collect();
        let es: BTreeSet<_> = keys(Lang::Es).into_iter().collect();
        let only_en: Vec<_> = en.difference(&es).collect();
        let only_es: Vec<_> = es.difference(&en).collect();
        assert!(only_en.is_empty() && only_es.is_empty(), "only in en: {only_en:?}; only in es: {only_es:?}");
    }

    #[test]
    fn placeholders_match_between_languages() {
        fn placeholders(s: &str) -> BTreeSet<String> {
            s.split('{').skip(1).filter_map(|p| p.split_once('}')).map(|(n, _)| n.to_owned()).collect()
        }
        for k in keys(Lang::En) {
            assert_eq!(placeholders(&lookup(Lang::En, k)), placeholders(&lookup(Lang::Es, k)), "placeholders differ for {k}");
        }
    }

    #[test]
    fn no_value_is_empty() {
        for lang in [Lang::En, Lang::Es] {
            for k in keys(lang) {
                assert!(!lookup(lang, k).trim().is_empty(), "{lang:?}:{k} is empty");
            }
        }
    }

    fn walk(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
        for e in std::fs::read_dir(dir).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, out);
            } else if p.extension().is_some_and(|x| x == "rs") {
                out.push(p);
            }
        }
    }

    /// Every `t("...")` / `tf("...")` / `key: "..."` literal used in the sources exists in the catalog.
    #[test]
    fn every_key_used_in_sources_exists() {
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut files = Vec::new();
        walk(&src, &mut files);
        let known: BTreeSet<_> = keys(Lang::En).into_iter().collect();
        let mut missing = Vec::new();
        for f in files {
            if f.file_name().is_some_and(|n| n == "i18n.rs") {
                continue;
            }
            let text = std::fs::read_to_string(&f).unwrap();
            for marker in ["t(\"", "tf(\"", "tn(\"", "tr!(\""] {
                let mut rest = text.as_str();
                while let Some(i) = rest.find(marker) {
                    // Skip identifiers that merely end in `t`/`tf` (e.g. `select(\"`).
                    let before = rest[..i].chars().last();
                    let after = &rest[i + marker.len()..];
                    if let Some(end) = after.find('"')
                        && !before.is_some_and(|c| c.is_alphanumeric() || c == '_')
                        && !known.contains(&after[..end])
                    {
                        missing.push(format!("{}: {}", f.file_name().unwrap().to_string_lossy(), &after[..end]));
                    }
                    if marker == "tn(\""
                        && let Some(end) = after.find('"')
                        && !known.contains(format!("{}.one", &after[..end]).as_str())
                    {
                        missing.push(format!("{}: {}.one", f.file_name().unwrap().to_string_lossy(), &after[..end]));
                    }
                    rest = after;
                }
            }
        }
        assert!(missing.is_empty(), "keys used but not defined: {missing:#?}");
    }

    #[test]
    fn counts_pick_the_singular_form() {
        let es = |key: &str, n: usize| {
            let k = if n == 1 { format!("{key}.one") } else { key.to_owned() };
            lookup(Lang::Es, &k).replace("{n}", &n.to_string())
        };
        assert_eq!(es("torrent.files.selected_n", 1), "1 seleccionado");
        assert_eq!(es("torrent.files.selected_n", 3), "3 seleccionados");
        assert_eq!(tn("torrent.files.count", 1), lookup(lang(), "torrent.files.count.one").replace("{n}", "1"));
        assert_eq!(tn("torrent.files.count", 28), lookup(lang(), "torrent.files.count").replace("{n}", "28"));
    }

    #[test]
    fn tag_detection_and_override() {
        assert_eq!(lang_from_tag("es-MX"), Lang::Es);
        assert_eq!(lang_from_tag("es_ES.UTF-8"), Lang::Es);
        assert_eq!(lang_from_tag("ja-JP"), Lang::En);
        assert_eq!(lang_from_tag(""), Lang::En);
        assert_eq!(resolve(LangChoice::System, "es-AR"), Lang::Es);
        assert_eq!(resolve(LangChoice::En, "es-AR"), Lang::En);
        assert_eq!(resolve(LangChoice::Es, "en-US"), Lang::Es);
    }

    #[test]
    fn missing_keys_fall_back_to_the_key() {
        assert_eq!(lookup(Lang::Es, "no.such.key"), "no.such.key");
    }
}
