//! i18n: `en` and `es` only (06 #4). Japanese falls back to English. No literal text in components:
//! everything goes through [`t`] / [`tf`].

use std::collections::HashMap;

use dioxus::prelude::{GlobalSignal, ReadableExt, Signal};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU8, Ordering};

const EN_JSON: &str = include_str!("../i18n/en.json");
const ES_JSON: &str = include_str!("../i18n/es.json");

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

fn catalog(lang: Lang) -> &'static Catalog {
    static EN: OnceLock<Catalog> = OnceLock::new();
    static ES: OnceLock<Catalog> = OnceLock::new();
    match lang {
        Lang::En => EN.get_or_init(|| parse(EN_JSON)),
        Lang::Es => ES.get_or_init(|| parse(ES_JSON)),
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
        assert!(serde_json::from_str::<Catalog>(EN_JSON).is_ok(), "en.json is invalid");
        assert!(serde_json::from_str::<Catalog>(ES_JSON).is_ok(), "es.json is invalid");
        assert!(keys(Lang::En).len() > 50);
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
            for marker in ["t(\"", "tf(\"", "tr!(\""] {
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
                    rest = after;
                }
            }
        }
        assert!(missing.is_empty(), "keys used but not defined: {missing:#?}");
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
