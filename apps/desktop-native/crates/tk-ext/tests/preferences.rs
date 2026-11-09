//! Wave B (worker j, `ext.preferences`, `ext.install-url`): user preferences reach the bundle as
//! `globalThis.preferences` / `instance.setPreferences`, survive a reinstall, and packages install from memory.

use std::collections::BTreeMap;
use std::io::Write;

use serde_json::{Value, json};
use tk_ext::ExtensionHost;

fn kai(id: &str, version: &str, bundle: &str) -> Vec<u8> {
    let manifest = json!({
        "id": id, "name": id, "version": version, "type": "onlinestream", "description": "t", "permissions": [],
        "preferences": [{ "key": "quality", "type": "select", "options": ["720", "1080"], "default": "720" }],
    });
    let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, data) in [("manifest.json", manifest.to_string()), ("bundle.js", bundle.to_owned())] {
        w.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
        w.write_all(data.as_bytes()).unwrap();
    }
    w.finish().unwrap().into_inner()
}

const BUNDLE: &str = r#"
module.exports = class {
  setPreferences(p) { this.fromSetter = p; }
  read() { return { global: globalThis.preferences, setter: this.fromSetter, viaGet: getPreference('quality', 'none'), missing: getPreference('x', 'dflt') }; }
};
"#;

#[tokio::test]
async fn preferences_reach_the_runtime_and_survive_an_update() {
    let dir = tempfile::tempdir().unwrap();
    let host = ExtensionHost::open(dir.path().join("ext")).unwrap();
    let info = host.install_kai_bytes(&kai("prefs.demo", "1.0.0", BUNDLE)).unwrap();
    assert_eq!(info.version, "1.0.0");
    assert_eq!(host.manifest("prefs.demo").unwrap().preference_fields().len(), 1);

    let empty = host.invoke("prefs.demo", "read", json!([])).await.unwrap();
    assert_eq!(empty["global"], json!({}));
    assert_eq!(empty["viaGet"], "none");

    let mut prefs = BTreeMap::new();
    prefs.insert("quality".to_owned(), "1080".to_owned());
    host.set_preferences("prefs.demo", prefs.clone()).unwrap();
    let got: Value = host.invoke("prefs.demo", "read", json!([])).await.unwrap();
    assert_eq!(got["global"]["quality"], "1080");
    assert_eq!(got["setter"]["quality"], "1080");
    assert_eq!(got["viaGet"], "1080");
    assert_eq!(got["missing"], "dflt");
    assert!(host.logs("prefs.demo").iter().any(|l| l.contains("preferences: 1 value(s) loaded (quality)")));

    // An update (same id, newer version) keeps the preferences.
    host.install_kai_bytes(&kai("prefs.demo", "1.1.0", BUNDLE)).unwrap();
    assert_eq!(host.preferences("prefs.demo"), prefs);
    let again = host.invoke("prefs.demo", "read", json!([])).await.unwrap();
    assert_eq!(again["global"]["quality"], "1080");
    assert_eq!(host.get("prefs.demo").unwrap().version, "1.1.0");

    assert!(host.install_kai_bytes(b"not a zip").is_err());
    assert!(host.set_preferences("nope", BTreeMap::new()).is_err());
}
