//! UI settings persisted as `ui-settings.json` (atomic write). Corrupt or missing files fall back to defaults.
//! Includes `Settings::sources` (source selection prefs, wave 1); files from before it load with the defaults.

use std::path::Path;

use tk_ui::model::Settings;

pub fn load(path: &Path) -> Settings {
    match std::fs::read_to_string(path) {
        Ok(text) => match serde_json::from_str::<Settings>(&text) {
            Ok(s) => s.sanitized(),
            Err(e) => {
                tracing::warn!(path = %path.display(), error = %e, "settings file is invalid; using defaults");
                Settings::default()
            }
        },
        Err(_) => Settings::default(),
    }
}

pub fn save(path: &Path, settings: &Settings) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_string_pretty(settings).map_err(std::io::Error::other)? + "\n")?;
    std::fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tk_ui::model::ThemeMode;

    #[test]
    fn round_trip_and_defaults() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("ui-settings.json");
        assert_eq!(load(&path), Settings::default());
        let s = Settings { mode: ThemeMode::Amoled, palette: "ocean".into(), ..Settings::default() };
        save(&path, &s).unwrap();
        assert_eq!(load(&path), s);
        assert!(!tmp.path().join("ui-settings.json.tmp").exists());
    }

    #[test]
    fn corrupt_file_yields_defaults_and_bad_values_are_repaired() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("ui-settings.json");
        std::fs::write(&path, "{nope").unwrap();
        assert_eq!(load(&path), Settings::default());
        std::fs::write(&path, r#"{"palette":"does-not-exist","volume":999}"#).unwrap();
        let s = load(&path);
        assert_eq!((s.palette.as_str(), s.volume), ("tatakai-violet", 100));
    }

    #[test]
    fn source_prefs_persist_and_old_files_get_defaults() {
        use tk_ui::backend::SourceKindVm;
        use tk_ui::model::{QualityVm, SourceModeVm, SourcePrefsVm};
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("ui-settings.json");
        let sources = SourcePrefsVm {
            mode: SourceModeVm::Manual,
            prefer: Some(SourceKindVm::Torrent),
            min_quality: QualityVm::P1080,
            sub_lang: "en".into(),
            dub_lang: "es".into(),
        };
        let s = Settings { reduce_motion: true, sources, ..Settings::default() };
        save(&path, &s).unwrap();
        assert_eq!(load(&path), s);
        // a file written before wave 1 has no `sources` / `reduce_motion`
        std::fs::write(&path, r#"{"volume":40}"#).unwrap();
        let old = load(&path);
        assert_eq!((old.sources, old.reduce_motion, old.volume), (SourcePrefsVm::default(), false, 40));
        // unknown languages are repaired by `sanitized()`
        std::fs::write(&path, r#"{"sources":{"mode":"manual","sub_lang":"xx","dub_lang":"zz"}}"#).unwrap();
        let fixed = load(&path).sources;
        assert_eq!((fixed.mode, fixed.sub_lang.as_str(), fixed.dub_lang.as_str()), (SourceModeVm::Manual, "es", "ja"));
    }
}
