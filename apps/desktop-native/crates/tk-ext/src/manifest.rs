//! `.kai` manifest (contracts/extension-host/routes.v1.json `package.manifest`).

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::InstallError;

pub const VALID_TYPES: [&str; 3] = ["torrent", "onlinestream", "custom"];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiServer {
    #[serde(default)]
    pub namespace: Option<String>,
    #[serde(default)]
    pub contract: Option<String>,
    #[serde(default)]
    pub routes: Option<Vec<String>>,
    #[serde(default)]
    pub health_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomSource {
    pub id: String,
    pub kind: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub id: String,
    pub name: String,
    pub version: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub description: String,
    pub permissions: Vec<String>,
    #[serde(default)]
    pub api_server: Option<ApiServer>,
    #[serde(default)]
    pub capabilities: Vec<String>,
    #[serde(default)]
    pub custom_sources: Vec<CustomSource>,
    #[serde(default)]
    pub priority: Option<f64>,
    /// Sites the bundle may open in the host browser runtime (Cloudflare bypass); only those also covered by a
    /// `network:domain:*` permission are honoured (`browser::browser_domains`).
    #[serde(default)]
    pub browser_domains: Vec<String>,
    /// User preferences the bundle reads at run time (wave B, `ext.preferences`): a list of
    /// `{ key, label|title, hint|summary, type: text|secret|password|toggle|boolean|switch|select|list, options |
    /// entries + entryValues, default }`. `settings` is accepted as an alias.
    #[serde(default)]
    pub preferences: Vec<Value>,
    #[serde(default)]
    pub settings: Vec<Value>,
    /// Optional update feed (`ext.updates.check`): a JSON `{ version, url, notes? }` (or a `.kai` URL) the host
    /// compares with `version`.
    #[serde(default)]
    pub update_url: Option<String>,
}

/// Control of one declared preference.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrefKind {
    Text,
    Secret,
    Toggle,
    Select,
}

/// One declared preference (normalized from `Manifest::preferences` / `settings`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrefField {
    pub key: String,
    pub label: String,
    pub hint: Option<String>,
    pub kind: PrefKind,
    /// `(value, label)` of a `Select`.
    pub options: Vec<(String, String)>,
    /// Default as text (`"true"` / `"false"` for toggles); empty when none.
    pub default: String,
}

/// Preference keys: `[A-Za-z0-9_.-]{1,64}`.
pub fn valid_pref_key(k: &str) -> bool {
    !k.is_empty() && k.len() <= 64 && k.bytes().all(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'.' | b'-'))
}

fn value_text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        _ => String::new(),
    }
}

fn pref_field(v: &Value) -> Option<PrefField> {
    let s = |k: &str| v.get(k).and_then(Value::as_str).map(str::trim).filter(|x| !x.is_empty()).map(str::to_owned);
    let key = s("key").or_else(|| s("id")).or_else(|| s("name")).filter(|k| valid_pref_key(k))?;
    let label = s("label").or_else(|| s("title")).unwrap_or_else(|| key.clone());
    let hint = s("hint").or_else(|| s("summary")).or_else(|| s("description"));
    let ty = s("type").unwrap_or_default().to_ascii_lowercase();
    let mut options: Vec<(String, String)> = Vec::new();
    if let Some(list) = v.get("options").and_then(Value::as_array) {
        for o in list {
            match o {
                Value::Object(_) => {
                    let value = o.get("value").map(value_text).unwrap_or_default();
                    let label = o.get("label").map(value_text).filter(|l| !l.is_empty()).unwrap_or_else(|| value.clone());
                    if !value.is_empty() {
                        options.push((value, label));
                    }
                }
                other => {
                    let t = value_text(other);
                    if !t.is_empty() {
                        options.push((t.clone(), t));
                    }
                }
            }
        }
    } else if let Some(values) = v.get("entryValues").and_then(Value::as_array) {
        let labels = v.get("entries").and_then(Value::as_array);
        for (i, val) in values.iter().enumerate() {
            let value = value_text(val);
            let label = labels.and_then(|l| l.get(i)).map(value_text).filter(|l| !l.is_empty()).unwrap_or_else(|| value.clone());
            if !value.is_empty() {
                options.push((value, label));
            }
        }
    }
    let secret_name = {
        let probe = format!("{key} {label}").to_ascii_lowercase();
        ["password", "token", "secret", "apikey", "api_key", "api key", "api-key"].iter().any(|w| probe.contains(w))
    };
    let kind = match ty.as_str() {
        "toggle" | "boolean" | "bool" | "switch" | "checkbox" => PrefKind::Toggle,
        "select" | "list" | "enum" | "dropdown" => PrefKind::Select,
        "secret" | "password" => PrefKind::Secret,
        _ if !options.is_empty() => PrefKind::Select,
        _ if secret_name || v.get("secret").and_then(Value::as_bool) == Some(true) => PrefKind::Secret,
        _ => PrefKind::Text,
    };
    if kind == PrefKind::Select && options.is_empty() {
        return None;
    }
    let default = v.get("default").or_else(|| v.get("defaultValue")).map(value_text).unwrap_or_default();
    let default = match kind {
        PrefKind::Toggle => (default == "true").to_string(),
        _ => default,
    };
    Some(PrefField { key, label, hint, kind, options, default })
}

impl Manifest {
    pub fn parse(json: &str) -> Result<Self, InstallError> {
        let v: Value = serde_json::from_str(json).map_err(|e| InstallError::invalid(format!("manifest.json: invalid JSON ({e})")))?;
        let obj = v.as_object().ok_or_else(|| InstallError::invalid("manifest.json: must be a JSON object"))?;
        for field in ["id", "name", "version", "type", "description"] {
            match obj.get(field) {
                Some(Value::String(s)) if !s.trim().is_empty() || field == "description" => {}
                _ => return Err(InstallError::invalid(format!("manifest.{field}: required string is missing or empty"))),
            }
        }
        match obj.get("permissions") {
            Some(Value::Array(a)) if a.iter().all(Value::is_string) => {}
            _ => return Err(InstallError::invalid("manifest.permissions: required array of strings is missing")),
        }
        let m: Manifest = serde_json::from_value(v).map_err(|e| InstallError::invalid(format!("manifest.json: {e}")))?;
        m.validate()?;
        Ok(m)
    }

    fn validate(&self) -> Result<(), InstallError> {
        if !valid_id(&self.id) {
            return Err(InstallError::invalid("manifest.id: must match ^[a-z0-9][a-z0-9._-]{0,63}$"));
        }
        if !VALID_TYPES.contains(&self.kind.as_str()) {
            return Err(InstallError::invalid("manifest.type: must be one of torrent | onlinestream | custom"));
        }
        Ok(())
    }

    /// `apiServer.namespace`, falling back to the id.
    pub fn namespace(&self) -> &str {
        self.api_server.as_ref().and_then(|a| a.namespace.as_deref()).unwrap_or(&self.id)
    }

    /// `apiServer.routes`, default `['sources','stream','torrent']`.
    pub fn gated_routes(&self) -> Vec<String> {
        self.api_server
            .as_ref()
            .and_then(|a| a.routes.clone())
            .unwrap_or_else(|| ["sources", "stream", "torrent"].iter().map(|s| (*s).to_owned()).collect())
    }

    pub fn priority(&self) -> f64 {
        self.priority.unwrap_or(100.0)
    }

    /// Declared preferences (`preferences`, else `settings`), invalid entries skipped, first key wins.
    pub fn preference_fields(&self) -> Vec<PrefField> {
        let list = if self.preferences.is_empty() { &self.settings } else { &self.preferences };
        let mut out: Vec<PrefField> = Vec::new();
        for f in list.iter().filter_map(pref_field) {
            if out.len() < 100 && !out.iter().any(|x| x.key == f.key) {
                out.push(f);
            }
        }
        out
    }
}

/// `^[a-z0-9][a-z0-9._-]{0,63}$`
pub fn valid_id(id: &str) -> bool {
    let b = id.as_bytes();
    !b.is_empty()
        && b.len() <= 64
        && (b[0].is_ascii_lowercase() || b[0].is_ascii_digit())
        && b.iter().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, b'.' | b'_' | b'-'))
}

#[cfg(test)]
mod tests {
    use super::*;

    const OK: &str = r#"{"id":"my.ext","name":"N","version":"1.0.0","type":"onlinestream","description":"d","permissions":["network:domain:a.test"]}"#;

    fn err(json: &str) -> String {
        Manifest::parse(json).unwrap_err().to_string()
    }

    #[test]
    fn valid_manifest_defaults() {
        let m = Manifest::parse(OK).unwrap();
        assert_eq!(m.namespace(), "my.ext");
        assert_eq!(m.gated_routes(), ["sources", "stream", "torrent"]);
        assert_eq!(m.priority(), 100.0);
    }

    #[test]
    fn invalid_manifests_name_the_field() {
        assert!(err("{ not json").contains("invalid JSON"));
        assert!(err(r#"{"id":"a"}"#).contains("manifest.name"));
        assert!(err(&OK.replace(r#""permissions":["network:domain:a.test"]"#, r#""x":1"#)).contains("manifest.permissions"));
        assert!(err(&OK.replace("onlinestream", "plugin")).contains("manifest.type"));
        for bad in ["Bad", "-x", "has space", "a/b", ""] {
            assert!(!valid_id(bad), "{bad}");
        }
        assert!(err(&OK.replace("my.ext", "UPPER")).contains("manifest.id"));
        assert!(valid_id(&"a".repeat(64)) && !valid_id(&"a".repeat(65)));
    }

    #[test]
    fn preference_fields_are_normalized() {
        let json = OK.replace(
            r#""permissions""#,
            r#""preferences":[
                {"key":"quality","label":"Quality","type":"select","options":[{"value":"1080","label":"1080p"},"720"],"default":"1080"},
                {"key":"apiKey","title":"API key"},
                {"key":"dub","type":"boolean","default":true},
                {"key":"lang","entries":["Spanish","English"],"entryValues":["es","en"]},
                {"key":"bad key"},
                {"key":"quality","type":"text"},
                {"key":"empty","type":"select"}
            ],"permissions""#,
        );
        let m = Manifest::parse(&json).unwrap();
        let f = m.preference_fields();
        assert_eq!(f.len(), 4);
        assert_eq!((f[0].kind, f[0].options.len(), f[0].default.as_str()), (PrefKind::Select, 2, "1080"));
        assert_eq!(f[0].options[1], ("720".to_owned(), "720".to_owned()));
        assert_eq!(f[1].kind, PrefKind::Secret);
        assert_eq!((f[2].kind, f[2].default.as_str()), (PrefKind::Toggle, "true"));
        assert_eq!(f[3].options[0], ("es".to_owned(), "Spanish".to_owned()));
        assert!(Manifest::parse(OK).unwrap().preference_fields().is_empty());
    }
}
