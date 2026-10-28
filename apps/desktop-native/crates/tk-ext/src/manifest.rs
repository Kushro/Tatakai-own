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
}
