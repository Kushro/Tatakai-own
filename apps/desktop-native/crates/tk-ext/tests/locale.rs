//! Number parsing / printing in extensions must not follow the process locale (its own test binary: it changes the
//! process-wide locale). GTK sets `LC_NUMERIC` from the user's locale at start; with a decimal-comma locale QuickJS
//! read the literal `0.4` as `0` (C `strtod`), which broke every fractional score in extensions inside the app.

use std::io::Write;

use serde_json::json;
use tk_ext::ExtensionHost;

const BUNDLE: &str = r#"
module.exports = class {
  run() {
    return {
      mul: 100 * 0.4 + 100 * 0.5 + 1 * 0.1,
      parse: parseFloat("1.5"),
      num: Number("2.25"),
      str: String(1.5),
      fixed: (1.25).toFixed(1),
      json: JSON.parse('{"a":0.25}').a,
      exp: 2.5e-3 * 1000,
    };
  }
};
"#;

/// A decimal-comma locale installed on this machine, if any.
#[cfg(unix)]
fn comma_locale() -> Option<&'static std::ffi::CStr> {
    for name in [c"es_AR.UTF-8", c"es_ES.UTF-8", c"de_DE.UTF-8", c"fr_FR.UTF-8", c"pt_BR.UTF-8"] {
        // SAFETY: single-threaded at this point of the test; setlocale returns null when the locale is missing.
        if !unsafe { libc::setlocale(libc::LC_ALL, name.as_ptr()) }.is_null() {
            return Some(name);
        }
    }
    None
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn fractional_numbers_survive_a_decimal_comma_process_locale() {
    let Some(loc) = comma_locale() else {
        eprintln!("no decimal-comma locale installed: skipped");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let manifest = json!({ "id": "loc", "name": "loc", "version": "1.0.0", "type": "onlinestream", "description": "t", "permissions": [] });
    let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, data) in [("manifest.json", manifest.to_string()), ("bundle.js", BUNDLE.to_owned())] {
        w.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
        w.write_all(data.as_bytes()).unwrap();
    }
    let file = dir.path().join("loc.kai");
    std::fs::write(&file, w.finish().unwrap().into_inner()).unwrap();
    let host = ExtensionHost::open(dir.path().join("ext")).unwrap();
    host.install_kai(&file).unwrap();
    let r = host.invoke("loc", "run", json!([])).await.unwrap();
    assert_eq!(r["mul"], json!(90.1), "locale {loc:?}: {r}");
    assert_eq!(r["parse"], json!(1.5));
    assert_eq!(r["num"], json!(2.25));
    assert_eq!(r["str"], "1.5");
    assert_eq!(r["fixed"], "1.3");
    assert_eq!(r["json"], json!(0.25));
    assert_eq!(r["exp"], json!(2.5));
}
