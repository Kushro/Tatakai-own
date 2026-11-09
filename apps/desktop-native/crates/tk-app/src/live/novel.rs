//! Novels + narration (wave B, worker **m**; docs/redesign/16-parity-wave-b-contract.md section 4; checklist
//! `novel.*`). Contract stubs: m replaces every body. tk-core side: `tk_core::novel` (chapter text -> paragraphs with
//! `scraper`, provider merge), `tk_core::metadata::anilist::manga` (format `NOVEL`; shared with l, read-only),
//! `LocalStore` tables of `tk-core/src/store/reading.rs` (shared with l through the `read_*` calls) and
//! `tk-core/src/store/tts.rs` (`tts_cache`). Narration engines in `crate::tts` (`b.tts`): system voice through an
//! external process (`spd-say` / PowerShell `System.Speech` / `say`), OpenAI-style `/v1/audio/speech`, ElevenLabs,
//! and the optional local KittenTTS daemon (Python, `<data>/tts/kitten`). API keys in `b.secrets` under
//! `tk_core::secrets::keys::tts(provider.id())`.

use tk_ui::backend::{
    ChapterProviderVm, ChapterRefVm, KittenStatusVm, NovelContentVm, NovelDetailsVm, ReadHomeVm, SearchPageVm,
    SearchQueryVm, TtsRequestVm, TtsSpeakVm, TtsVoiceVm, UiResult,
};
use tk_ui::model::{TtsProviderVm, UiError};

use crate::live::LiveBackend;

pub async fn home(_b: &LiveBackend) -> UiResult<ReadHomeVm> {
    Err(UiError::not_wired("novel_home"))
}
pub async fn search(_b: &LiveBackend, _q: &SearchQueryVm) -> UiResult<SearchPageVm> {
    Err(UiError::not_wired("novel_search"))
}
pub async fn details(_b: &LiveBackend, _id: &str) -> UiResult<NovelDetailsVm> {
    Err(UiError::not_wired("novel_details"))
}
pub async fn chapters(_b: &LiveBackend, _id: &str, _translator: Option<&str>, _refresh: bool) -> UiResult<Vec<ChapterProviderVm>> {
    Err(UiError::not_wired("novel_chapters"))
}
pub async fn content(_b: &LiveBackend, _chapter: &ChapterRefVm) -> UiResult<NovelContentVm> {
    Err(UiError::not_wired("novel_content"))
}
pub async fn check_sources(_b: &LiveBackend, _id: &str) -> UiResult<Vec<(String, u32)>> {
    Err(UiError::not_wired("novel_check_sources"))
}
pub async fn download_chapters(_b: &LiveBackend, _id: &str, _chapters: &[ChapterRefVm]) -> UiResult<u32> {
    Err(UiError::not_wired("novel_download_chapters"))
}
pub async fn tts_voices(_b: &LiveBackend, _provider: TtsProviderVm) -> UiResult<Vec<TtsVoiceVm>> {
    Err(UiError::not_wired("tts_voices"))
}
pub async fn tts_speak(_b: &LiveBackend, _req: &TtsRequestVm) -> UiResult<TtsSpeakVm> {
    Err(UiError::not_wired("tts_speak"))
}
pub async fn tts_stop(_b: &LiveBackend) -> UiResult<()> {
    Err(UiError::not_wired("tts_stop"))
}
pub async fn set_tts_key(_b: &LiveBackend, _provider: TtsProviderVm, _key: Option<&str>) -> UiResult<()> {
    Err(UiError::not_wired("set_tts_key"))
}
pub async fn tts_key_present(_b: &LiveBackend, _provider: TtsProviderVm) -> UiResult<bool> {
    Err(UiError::not_wired("tts_key_present"))
}
pub async fn kitten_status(_b: &LiveBackend) -> UiResult<KittenStatusVm> {
    Err(UiError::not_wired("kitten_status"))
}
pub async fn kitten_install(_b: &LiveBackend) -> UiResult<KittenStatusVm> {
    Err(UiError::not_wired("kitten_install"))
}
pub async fn kitten_remove(_b: &LiveBackend) -> UiResult<()> {
    Err(UiError::not_wired("kitten_remove"))
}
