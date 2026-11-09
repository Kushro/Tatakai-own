//! `Backend` subtitle / translation / capture calls (wave A, worker **d**; docs/redesign/15 section 4). Bodies: `fetch_subtitle` = `reqwest` GET with the stream headers (size cap 5 MiB, text decoded as UTF-8 with BOM / Latin-1 fallback); `translate_cues` = `tk_core::translate::translate` in batches + `b.store.translation_*` cache keyed by `tk_core::translate::cache_key`; `translation_test` translates one fixed sentence with the saved `Settings::subs.translation`; `screenshot_path` = `<Pictures>/Tatakai/<safe title> - E<ep> - <yyyyMMdd-HHmmss>.png` (`directories::UserDirs::picture_dir`, else `<data>/screenshots`). Contract stubs.

use tk_ui::backend::UiResult;
use tk_ui::model::UiError;
use std::path::PathBuf;

use tk_ui::backend::{CueVm, TranslateRequestVm, TranslationTestVm};
use crate::live::LiveBackend;

/// `Backend::fetch_subtitle`.
pub async fn fetch_subtitle(_b: &LiveBackend, _url: &str, _headers: &[(String, String)]) -> UiResult<String> {
    Err(UiError::not_wired("fetch_subtitle"))
}

/// `Backend::translate_cues`.
pub async fn translate_cues(_b: &LiveBackend, _req: &TranslateRequestVm) -> UiResult<Vec<CueVm>> {
    Err(UiError::not_wired("translate_cues"))
}

/// `Backend::translation_test`.
pub async fn translation_test(_b: &LiveBackend) -> UiResult<TranslationTestVm> {
    Err(UiError::not_wired("translation_test"))
}

/// `Backend::screenshot_path`.
pub async fn screenshot_path(_b: &LiveBackend, _title: &str, _episode: u32) -> UiResult<PathBuf> {
    Err(UiError::not_wired("screenshot_path"))
}
