//! Settings > Subtitles: style (size, font, colour, outline, background, position, bold) with a live preview, the
//! app fonts folder for ASS tracks, secondary subtitle language, and translation (provider, endpoint, model,
//! region, key in `Secrets`, target language, live / pre-translation toggles, "Test", "Clear cache"), all through
//! `SettingsAction::SetSubs`. Classic: `VideoSettingsPanel.tsx` (style) and `TranslationPanel.tsx`.
//!
//! Split out of `screens/settings.rs` by the wave-A contract (docs/redesign/15-parity-wave-a-contract.md
//! section 2). Owner: worker **d**. The page shell (`mod.rs`: index, scroll spy, `SettingsPanel`, `SettingRow`) is
//! frozen.
//!
//! The provider key never goes to `ui-settings.json`: "Save key" calls `Backend::set_translation_key`, empties the
//! field and the row shows "Key saved" / "Remove" from `Backend::translation_key_present`
//! (`data-translation-key="saved|none"`). "Test" exposes `data-translate-test="idle|running|ok|error"`.

use dioxus::prelude::*;

use super::{SettingRow, lang_seg};
use crate::app::{Services, Ui};
use crate::backend::{CueVm, TranslateRequestVm, TranslationTestVm};
use crate::components::{Button, ButtonSize, ButtonVariant, ErrorState, LoadingIndicator, Segmented, Switch, TextField, use_snackbar};
use crate::i18n::{t, tf};
use crate::model::subs::{CLEAR_CACHE_KEY, SECONDARY_LANGS, TRANSLATE_LANGS};
use crate::model::{SubtitlePrefs, SubtitleStyleVm, TranslationPrefs, TranslationProviderVm, UiError};
use crate::screens::player::subs::{StyleEditor, app_font_families, app_fonts_dir, commit_subs, use_font_families};

/// "Test" state (`data-translate-test`).
#[derive(Debug, Clone, PartialEq)]
enum TestState {
    Idle,
    Running,
    Ok(TranslationTestVm),
    Failed(UiError),
}

impl TestState {
    fn id(&self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Running => "running",
            Self::Ok(_) => "ok",
            Self::Failed(_) => "error",
        }
    }
}

/// Provider choice -> the id used by `Segmented` and back.
fn provider_from_id(id: &str) -> TranslationProviderVm {
    TranslationProviderVm::ALL.into_iter().find(|p| p.id() == id).unwrap_or_default()
}

/// Secondary language choice: `"off"` = none.
fn secondary_from_id(id: &str) -> Option<String> {
    Some(id.to_owned()).filter(|l| l != "off" && SECONDARY_LANGS.contains(&l.as_str()))
}

/// Field of the translation settings a text box edits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TextPref {
    Endpoint,
    Model,
    Region,
}

/// `prefs` with one text field replaced (pure; the reducer sanitizes).
fn with_text(prefs: &TranslationPrefs, field: TextPref, value: &str) -> TranslationPrefs {
    let mut next = prefs.clone();
    match field {
        TextPref::Endpoint => next.endpoint = value.to_owned(),
        TextPref::Model => next.model = value.to_owned(),
        TextPref::Region => next.region = value.to_owned(),
    }
    next
}

/// Hint under the key field: OpenAI-compatible local servers need none.
fn key_hint(p: TranslationProviderVm) -> &'static str {
    match p {
        TranslationProviderVm::OpenAi => "settings.subs.key_hint_optional",
        TranslationProviderVm::DeepL => "settings.subs.key_hint_deepl",
        _ => "settings.subs.key_hint",
    }
}

/// Placeholder of the endpoint field.
fn endpoint_placeholder(p: TranslationProviderVm) -> &'static str {
    match p {
        TranslationProviderVm::DeepLx => "http://127.0.0.1:1188/translate",
        TranslationProviderVm::OpenAi => "https://api.openai.com/v1",
        _ => "",
    }
}

#[component]
pub fn SubtitlesSection() -> Element {
    let services = use_context::<Services>();
    let ui = use_context::<Ui>();
    let snack = use_snackbar();
    let backend = services.backend.clone();
    let s = (ui.settings)();
    let subs = s.subs.clone();
    let tr = subs.translation.clone();
    let provider = tr.provider;

    // --- fonts ---
    let data_dir = use_hook(|| backend.data_dir_display());
    let fonts_dir = use_hook(|| app_fonts_dir(&data_dir));
    let (mut fonts_res, mut rescan) = use_font_families(fonts_dir.clone());
    let mut app_fonts = use_resource({
        let dir = fonts_dir.clone();
        move || {
            let dir = dir.clone();
            async move {
                let _ = std::fs::create_dir_all(&dir);
                app_font_families(dir).await
            }
        }
    });
    let fonts_list = fonts_res.read().clone().unwrap_or_default();
    let fonts_loading = fonts_res.read().is_none();
    let app_fonts_count = app_fonts.read().as_ref().map(Vec::len);
    let fonts_dir_text = fonts_dir.to_string_lossy().into_owned();

    // --- key in Secrets ---
    let mut key_present = use_resource({
        let backend = backend.clone();
        move || {
            let backend = backend.clone();
            // Re-run when the provider changes.
            let p = ui.settings.read().subs.translation.provider;
            async move { if p.needs_key() { backend.translation_key_present(p).await } else { Ok(false) } }
        }
    });
    let mut key_draft = use_signal(String::new);
    let mut key_busy = use_signal(|| false);
    let mut key_err = use_signal(|| None::<UiError>);

    // --- test / cache ---
    let mut test = use_signal(|| TestState::Idle);
    let mut clearing = use_signal(|| false);

    let commit = {
        let backend = backend.clone();
        move |next: SubtitlePrefs| commit_subs(ui, &backend, next)
    };
    let commit_tr = {
        let commit = commit.clone();
        move |next: TranslationPrefs| {
            let mut all = ui.settings.peek().subs.clone();
            all.translation = next;
            commit(all);
            // Settings changed: an earlier test result no longer describes them. (A local copy keeps the
            // closure `Fn`: it is cloned into several handlers.)
            let mut test = test;
            test.set(TestState::Idle);
        }
    };

    let on_style = {
        let commit = commit.clone();
        move |style: SubtitleStyleVm| {
            let mut all = ui.settings.peek().subs.clone();
            all.style = style;
            commit(all);
        }
    };
    let on_secondary = {
        let commit = commit.clone();
        move |id: String| {
            let mut all = ui.settings.peek().subs.clone();
            all.secondary_lang = secondary_from_id(&id);
            commit(all);
        }
    };
    let on_provider = {
        let commit_tr = commit_tr.clone();
        move |id: String| {
            let mut next = ui.settings.peek().subs.translation.clone();
            next.provider = provider_from_id(&id);
            let (mut key_draft, mut key_err) = (key_draft, key_err);
            key_draft.set(String::new());
            key_err.set(None);
            commit_tr(next);
        }
    };
    let text_input = {
        let commit_tr = commit_tr.clone();
        move |field: TextPref| {
            let commit_tr = commit_tr.clone();
            move |v: String| {
                let next = with_text(&ui.settings.peek().subs.translation, field, &v);
                commit_tr(next);
            }
        }
    };
    let on_target = {
        let commit_tr = commit_tr.clone();
        move |e: FormEvent| {
            let mut next = ui.settings.peek().subs.translation.clone();
            next.target_lang = e.value();
            commit_tr(next);
        }
    };
    let on_live = {
        let commit_tr = commit_tr.clone();
        move |v: bool| {
            let mut next = ui.settings.peek().subs.translation.clone();
            next.live = v;
            commit_tr(next);
        }
    };
    let on_pre = {
        let commit_tr = commit_tr.clone();
        move |v: bool| {
            let mut next = ui.settings.peek().subs.translation.clone();
            next.pretranslate = v;
            commit_tr(next);
        }
    };

    let save_key = {
        let backend = backend.clone();
        move |remove: bool| {
            let (mut key_busy, mut key_err, mut key_draft, mut key_present, mut test) = (key_busy, key_err, key_draft, key_present, test);
            let p = ui.settings.peek().subs.translation.provider;
            let draft = key_draft.peek().trim().to_owned();
            if !remove && draft.is_empty() {
                return;
            }
            let backend = backend.clone();
            key_busy.set(true);
            key_err.set(None);
            spawn(async move {
                let res = backend.set_translation_key(p, if remove { None } else { Some(draft.as_str()) }).await;
                key_busy.set(false);
                match res {
                    Ok(()) => {
                        key_draft.set(String::new());
                        key_present.restart();
                        test.set(TestState::Idle);
                        snack.show(t(if remove { "settings.subs.key_removed" } else { "settings.subs.key_saved_toast" }));
                    }
                    Err(e) => key_err.set(Some(e)),
                }
            });
        }
    };

    let run_test = {
        let backend = backend.clone();
        move |_: ()| {
            let mut test = test;
            if *test.peek() == TestState::Running {
                return;
            }
            let backend = backend.clone();
            test.set(TestState::Running);
            spawn(async move {
                let res = backend.translation_test().await;
                test.set(match res {
                    Ok(vm) => TestState::Ok(vm),
                    Err(e) => TestState::Failed(e),
                });
            });
        }
    };

    let clear_cache = {
        let backend = backend.clone();
        move |_: MouseEvent| {
            if *clearing.peek() {
                return;
            }
            let backend = backend.clone();
            clearing.set(true);
            spawn(async move {
                let req = TranslateRequestVm { cues: Vec::<CueVm>::new(), source_lang: None, target_lang: String::new(), cache_key: CLEAR_CACHE_KEY.to_owned() };
                let res = backend.translate_cues(&req).await;
                clearing.set(false);
                match res {
                    Ok(list) => {
                        let n = list.first().map_or(0, |c| c.index);
                        snack.show(tf("settings.subs.cache_cleared", &[("n", &n.to_string())]));
                    }
                    Err(e) => snack.show(tf("settings.subs.cache_failed", &[("detail", &e.detail)])),
                }
            });
        }
    };

    // --- view values ---
    let provider_opts: Vec<(String, String)> =
        TranslationProviderVm::ALL.iter().map(|p| (p.id().to_owned(), t(&format!("translate.provider.{}", p.id())))).collect();
    let mut secondary_opts: Vec<(String, String)> = vec![("off".to_owned(), t("subs.off"))];
    secondary_opts.extend(lang_seg(&SECONDARY_LANGS));
    let secondary_sel = subs.secondary_lang.clone().unwrap_or_else(|| "off".to_owned());
    let key_state = match &*key_present.read() {
        Some(Ok(true)) => "saved",
        _ => "none",
    };
    let key_loading = key_present.read().is_none();
    let key_read_err = key_present.read().as_ref().and_then(|r| r.as_ref().err()).cloned();
    let test_now = test();
    let draft = key_draft();
    let target = tr.target_lang.clone();
    let off = provider == TranslationProviderVm::Off;
    let save_key_btn = save_key.clone();
    let remove_key_btn = save_key.clone();
    let save_key_enter = save_key.clone();
    let key_label = if key_state == "saved" { t("settings.subs.key_replace") } else { t("settings.subs.key") };

    rsx! {
        div { class: "tk-subs-settings", "data-provider": provider.id(),
            p { class: "tk-panel__hint", {t("settings.subs.hint")} }

            // ----- Style -----
            h3 { class: "tk-subs-settings__title", {t("subs.style")} }
            StyleEditor { style: subs.style.clone(), fonts: fonts_list, fonts_loading, on_change: on_style }
            SettingRow { label: t("settings.subs.fonts_dir"), hint: t("settings.subs.fonts_dir_hint"), stack: true,
                div { class: "tk-subs-fonts-dir", "data-app-fonts": app_fonts_count.map(|n| n.to_string()).unwrap_or_default(),
                    code { class: "tk-subs-path", "{fonts_dir_text}" }
                    span { class: "tk-panel__hint",
                        match app_fonts_count {
                            None => rsx! { LoadingIndicator { small: true } },
                            Some(0) => rsx! { {t("settings.subs.fonts_none")} },
                            Some(n) => rsx! { {tf("settings.subs.fonts_found", &[("n", &n.to_string())])} },
                        }
                    }
                    Button {
                        label: t("settings.subs.fonts_rescan"),
                        icon: "refresh".to_string(),
                        variant: ButtonVariant::Text,
                        size: ButtonSize::Sm,
                        disabled: fonts_loading,
                        onclick: move |_| {
                            rescan.set(true);
                            fonts_res.restart();
                            app_fonts.restart();
                        },
                    }
                }
            }

            // ----- Secondary subtitle -----
            SettingRow { label: t("subs.secondary"), hint: t("subs.secondary.hint"), stack: true,
                div { "data-secondary-lang": "{secondary_sel}",
                    Segmented { options: secondary_opts, selected: secondary_sel.clone(), label: t("subs.secondary"), small: true, on_select: on_secondary }
                }
            }

            // ----- Translation -----
            h3 { class: "tk-subs-settings__title", {t("subs.translate")} }
            SettingRow { label: t("settings.subs.provider"), hint: t("settings.subs.provider_hint"), stack: true,
                Segmented { options: provider_opts, selected: provider.id().to_string(), label: t("settings.subs.provider"), small: true, on_select: on_provider }
            }
            if !off {
                if provider.needs_endpoint() {
                    SettingRow { label: t("settings.subs.endpoint"), hint: t("settings.subs.endpoint_hint"), stack: true,
                        TextField {
                            label: t("settings.subs.endpoint"),
                            value: tr.endpoint.clone(),
                            placeholder: endpoint_placeholder(provider).to_string(),
                            mono: true,
                            oninput: text_input(TextPref::Endpoint),
                        }
                    }
                }
                if provider == TranslationProviderVm::OpenAi {
                    SettingRow { label: t("settings.subs.model"), hint: t("settings.subs.model_hint"), stack: true,
                        TextField { label: t("settings.subs.model"), value: tr.model.clone(), placeholder: "gpt-4o-mini".to_string(), mono: true, oninput: text_input(TextPref::Model) }
                    }
                }
                if provider == TranslationProviderVm::Azure {
                    SettingRow { label: t("settings.subs.region"), hint: t("settings.subs.region_hint"), stack: true,
                        TextField { label: t("settings.subs.region"), value: tr.region.clone(), placeholder: "westeurope".to_string(), mono: true, oninput: text_input(TextPref::Region) }
                    }
                }
                if provider.needs_key() {
                    SettingRow { label: t("settings.subs.key"), hint: t(key_hint(provider)), stack: true,
                        div { class: "tk-subs-key", "data-translation-key": key_state,
                            if key_loading {
                                LoadingIndicator { small: true }
                            } else if key_state == "saved" {
                                div { class: "tk-subs-key__saved", role: "status",
                                    span { {t("settings.subs.key_saved")} }
                                    Button {
                                        label: t("common.remove"),
                                        icon: "trash".to_string(),
                                        variant: ButtonVariant::Text,
                                        size: ButtonSize::Sm,
                                        disabled: key_busy(),
                                        onclick: move |_| remove_key_btn(true),
                                    }
                                }
                            }
                            div { class: "tk-subs-key__edit",
                                TextField {
                                    label: key_label.clone(),
                                    value: draft.clone(),
                                    kind: "password".to_string(),
                                    mono: true,
                                    disabled: key_busy(),
                                    oninput: move |v: String| key_draft.set(v),
                                    onsubmit: move |_| save_key_enter(false),
                                }
                                Button {
                                    label: t("settings.subs.key_save"),
                                    icon: "check".to_string(),
                                    variant: ButtonVariant::Tonal,
                                    size: ButtonSize::Sm,
                                    disabled: key_busy() || draft.trim().is_empty(),
                                    onclick: move |_| save_key_btn(false),
                                }
                            }
                            if let Some(e) = key_err() {
                                ErrorState { error: e, compact: true, retry: false, title: t("settings.subs.key_failed") }
                            } else if let Some(e) = key_read_err {
                                ErrorState { error: e, compact: true, title: t("settings.subs.key_failed"), on_retry: move |_| key_present.restart() }
                            }
                        }
                    }
                }
                SettingRow { label: t("settings.subs.target"), hint: t("settings.subs.target_hint"),
                    select { class: "tk-subs-select", "aria-label": t("settings.subs.target"), value: "{target}", onchange: on_target,
                        for code in TRANSLATE_LANGS {
                            option { key: "{code}", value: "{code}", selected: target == code, {t(&format!("subs.lang.{code}"))} }
                        }
                    }
                }
                SettingRow { label: t("translate.live"), hint: t("settings.subs.live_hint"),
                    Switch { checked: tr.live, label: t("translate.live"), on_toggle: on_live }
                }
                SettingRow { label: t("translate.pretranslate"), hint: t("settings.subs.pretranslate_hint"),
                    Switch { checked: tr.pretranslate, label: t("translate.pretranslate"), on_toggle: on_pre }
                }
                SettingRow { label: t("settings.subs.test"), hint: t("settings.subs.test_hint"), stack: true,
                    div { class: "tk-subs-test", "data-translate-test": test_now.id(),
                        Button {
                            label: t("settings.subs.test_run"),
                            icon: "language".to_string(),
                            variant: ButtonVariant::Tonal,
                            size: ButtonSize::Sm,
                            disabled: test_now == TestState::Running,
                            onclick: {
                                let run = run_test.clone();
                                move |_| run(())
                            },
                        }
                        match &test_now {
                            TestState::Idle => rsx! {},
                            TestState::Running => rsx! { LoadingIndicator { small: true } },
                            TestState::Ok(vm) => rsx! {
                                div { class: "tk-subs-test__result", role: "status",
                                    span { class: "tk-subs-test__in", "{vm.sample_in}" }
                                    span { class: "tk-subs-test__out", "{vm.sample_out}" }
                                    span { class: "tk-panel__hint", {tf("settings.subs.test_latency", &[("ms", &vm.latency_ms.to_string())])} }
                                }
                            },
                            TestState::Failed(e) => rsx! {
                                ErrorState { error: e.clone(), compact: true, title: t("settings.subs.test_failed"), on_retry: run_test.clone() }
                            },
                        }
                    }
                }
            } else {
                p { class: "tk-panel__hint", {t("subs.translate.off_hint")} }
            }
            SettingRow { label: t("settings.subs.cache"), hint: t("settings.subs.cache_hint"),
                Button {
                    label: t("settings.subs.cache_clear"),
                    icon: "trash".to_string(),
                    variant: ButtonVariant::Outlined,
                    size: ButtonSize::Sm,
                    disabled: clearing(),
                    onclick: clear_cache,
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_and_secondary_ids_round_trip() {
        for p in TranslationProviderVm::ALL {
            assert_eq!(provider_from_id(p.id()), p);
        }
        assert_eq!(provider_from_id("nope"), TranslationProviderVm::Off);
        assert_eq!(secondary_from_id("off"), None);
        assert_eq!(secondary_from_id("en").as_deref(), Some("en"));
        assert_eq!(secondary_from_id("xx"), None);
    }

    #[test]
    fn text_fields_edit_only_their_own_value() {
        let base = TranslationPrefs { endpoint: "a".into(), model: "b".into(), region: "c".into(), ..TranslationPrefs::default() };
        let e = with_text(&base, TextPref::Endpoint, "http://x");
        assert_eq!((e.endpoint.as_str(), e.model.as_str(), e.region.as_str()), ("http://x", "b", "c"));
        assert_eq!(with_text(&base, TextPref::Model, "m").model, "m");
        assert_eq!(with_text(&base, TextPref::Region, "r").region, "r");
        assert_eq!(key_hint(TranslationProviderVm::OpenAi), "settings.subs.key_hint_optional");
        assert!(endpoint_placeholder(TranslationProviderVm::Google).is_empty());
    }

    #[test]
    fn test_state_ids() {
        assert_eq!(TestState::Idle.id(), "idle");
        assert_eq!(TestState::Running.id(), "running");
        assert_eq!(TestState::Failed(UiError::not_wired("x")).id(), "error");
        let ok = TranslationTestVm { sample_in: "a".into(), sample_out: "b".into(), latency_ms: 1 };
        assert_eq!(TestState::Ok(ok).id(), "ok");
    }
}
