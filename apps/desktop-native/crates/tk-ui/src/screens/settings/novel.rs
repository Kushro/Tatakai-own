//! Settings > Novel reader (wave B, worker **m**; checklist `novel.tts.settings`, `novel.reader.typography`,
//! `novel.reader.custom-font`, `novel.reader.flow`, `novel.tts.*`, `novel.tts.kitten`; classic
//! `panels/NovelReaderPanel.tsx`, `TtsIntegrationSettings.tsx`, `desktop/services/kitten-tts.cjs`):
//!
//! - typography and flow (the same controls as the reader's sheet, `screens/novel/controls.rs`);
//! - narrator: provider (`data-provider`), voice list from `tts_voices`, speed / pitch, auto-advance, highlight,
//!   endpoint / model, the API key stored with `set_tts_key` and shown only as saved / remove
//!   (`tts_key_present`, `data-tts-key="saved|none"`), "Test voice" (`data-tts-state`);
//! - KittenTTS (local): model choice (`nano-int8` / `mini`), install with progress (`kitten_install`, progress from
//!   `model::novel::kitten_progress`), remove, and what is missing (`data-kitten="missing|installing|ready|error"`,
//!   `data-kitten-runtime`, `data-kitten-phonemizer`), with how to install `espeak-ng`.
//!
//! Typography changes commit at once (no snackbar per slider step); the narrator and KittenTTS confirm through
//! `commit_saved` (`SettingsAction::SetNovel`).

use std::time::Duration;

use dioxus::prelude::*;

use super::SettingRow;
use crate::app::{Services, Ui};
use crate::backend::KittenStatusVm;
use crate::components::{
    Button, ButtonSize, ButtonVariant, ErrorState, LoadingIndicator, Segmented, Switch, TextField,
    use_snackbar,
};
use crate::i18n::{t, tf};
use crate::model::novel::{
    KITTEN_MODELS, KITTEN_RUNTIME_MB, TTS_RATE_MAX, TTS_RATE_MIN, kitten_model_info, kitten_progress,
};
use crate::model::{SettingsAction, TtsPrefs, TtsProviderVm, UiError};
use crate::screens::novel::assets::{STOP_JS, play_clip_js, register, use_novel_assets};
use crate::screens::novel::controls::{FlowControls, TypographyControls, commit_novel};

/// `data-kitten` of a status (`installing` while the install call runs).
pub fn kitten_state(status: Option<&KittenStatusVm>, installing: bool) -> &'static str {
    if installing {
        return "installing";
    }
    match status {
        None => "missing",
        Some(s) if s.available => "ready",
        Some(s) if s.error.is_some() && s.model_present && s.runtime.is_some() => "error",
        Some(_) => "missing",
    }
}

/// The prefs after picking another provider: voice and model belong to the old one.
pub fn with_provider(prefs: &TtsPrefs, provider: TtsProviderVm) -> TtsPrefs {
    let mut next = prefs.clone();
    if next.provider != provider {
        next.provider = provider;
        next.voice.clear();
        next.model = None;
    }
    next
}

/// Install hint for `espeak-ng` on this OS (`novel.kitten.espeak.<os>`).
fn espeak_hint_key() -> &'static str {
    if cfg!(windows) {
        "novel.kitten.espeak.windows"
    } else if cfg!(target_os = "macos") {
        "novel.kitten.espeak.macos"
    } else {
        "novel.kitten.espeak.linux"
    }
}

#[component]
pub fn NovelSection(commit_saved: Callback<SettingsAction>) -> Element {
    let services = use_context::<Services>();
    let ui = use_context::<Ui>();
    use_novel_assets();
    let backend = services.backend.clone();
    let novel = ui.settings.read().novel.clone();
    let tts = novel.tts.clone();
    let provider = tts.provider;
    let data_dir = use_hook({
        let backend = backend.clone();
        move || backend.data_dir_display()
    });

    let on_typo = {
        let backend = backend.clone();
        move |next| commit_novel(ui, &backend, next)
    };
    let on_flow = on_typo.clone();
    let commit_tts = move |next: TtsPrefs| {
        let mut all = ui.settings.peek().novel.clone();
        all.tts = next;
        commit_saved.call(SettingsAction::SetNovel(all));
    };
    // Text fields and sliders save without a snackbar per keystroke.
    let commit_tts_quiet = {
        let backend = backend.clone();
        move |next: TtsPrefs| {
            let mut all = ui.settings.peek().novel.clone();
            all.tts = next;
            commit_novel(ui, &backend, all);
        }
    };

    rsx! {
        div { class: "tk-novel-settings", "data-provider": "{provider.id()}",
            p { class: "tk-panel__hint", {t("settings.novel.hint")} }
            h3 { class: "tk-novel-ctl__title", {t("novel.settings.typography")} }
            TypographyControls { prefs: novel.clone(), data_dir: data_dir.clone(), on_change: on_typo }
            h3 { class: "tk-novel-ctl__title", {t("novel.settings.flow")} }
            FlowControls { prefs: novel.clone(), on_change: on_flow }
            h3 { class: "tk-novel-ctl__title", {t("novel.settings.narrator")} }
            NarratorSettings { tts: tts.clone(), on_change: commit_tts_quiet, on_saved: commit_tts }
            h3 { class: "tk-novel-ctl__title", {t("novel.kitten.title")} }
            KittenSettings { tts: tts.clone(), on_change: commit_tts }
        }
    }
}

#[component]
fn NarratorSettings(
    tts: TtsPrefs,
    on_change: EventHandler<TtsPrefs>,
    on_saved: EventHandler<TtsPrefs>,
) -> Element {
    let services = use_context::<Services>();
    let ui = use_context::<Ui>();
    let snack = use_snackbar();
    let provider = tts.provider;
    let voices = use_resource({
        let backend = services.backend.clone();
        move || {
            let backend = backend.clone();
            let s = ui.settings.read();
            let p = s.novel.tts.provider;
            // Also re-list when the compatible endpoint changes.
            let _endpoint = s.novel.tts.endpoint.clone();
            drop(s);
            async move { backend.tts_voices(p).await }
        }
    });
    let mut key_present = use_resource({
        let backend = services.backend.clone();
        move || {
            let backend = backend.clone();
            let p = ui.settings.read().novel.tts.provider;
            async move {
                if p.needs_key() {
                    backend.tts_key_present(p).await
                } else {
                    Ok(false)
                }
            }
        }
    });
    let mut key_draft = use_signal(String::new);
    let mut key_busy = use_signal(|| false);
    let mut key_err = use_signal(|| None::<UiError>);
    let mut test_state = use_signal(|| "idle");
    let mut test_err = use_signal(|| None::<UiError>);

    let provider_opts: Vec<(String, String)> = TtsProviderVm::ALL
        .iter()
        .map(|p| (p.id().to_owned(), t(&format!("novel.tts.provider.{}", p.id()))))
        .collect();
    let voice_list = voices
        .read()
        .as_ref()
        .and_then(|r| r.as_ref().ok())
        .cloned()
        .unwrap_or_default();
    let voices_loading = voices.read().is_none();
    let voices_err = voices.read().as_ref().and_then(|r| r.as_ref().err()).cloned();
    let key_state = match &*key_present.read() {
        Some(Ok(true)) => "saved",
        _ => "none",
    };
    let key_loading = key_present.read().is_none();
    let draft = key_draft();

    let save_key = {
        let backend = services.backend.clone();
        move |remove: bool| {
            let (mut key_busy, mut key_err, mut key_draft, mut key_present) =
                (key_busy, key_err, key_draft, key_present);
            let p = ui.settings.peek().novel.tts.provider;
            let value = key_draft.peek().trim().to_owned();
            if !remove && value.is_empty() {
                return;
            }
            let backend = backend.clone();
            key_busy.set(true);
            key_err.set(None);
            spawn(async move {
                let res = backend
                    .set_tts_key(p, if remove { None } else { Some(value.as_str()) })
                    .await;
                key_busy.set(false);
                match res {
                    Ok(()) => {
                        key_draft.set(String::new());
                        key_present.restart();
                        snack.show(t(if remove {
                            "novel.tts.key_removed"
                        } else {
                            "novel.tts.key_saved_toast"
                        }));
                    }
                    Err(e) => key_err.set(Some(e)),
                }
            });
        }
    };
    let save_btn = save_key.clone();
    let save_enter = save_key.clone();
    let remove_btn = save_key.clone();

    let run_test = {
        let backend = services.backend.clone();
        move |_| {
            if *test_state.peek() == "playing" {
                let _ = document::eval(STOP_JS);
                let b = backend.clone();
                spawn(async move {
                    let _ = b.tts_stop().await;
                });
                test_state.set("idle");
                return;
            }
            let backend = backend.clone();
            let prefs = ui.settings.peek().novel.tts.clone();
            test_state.set("playing");
            test_err.set(None);
            spawn(async move {
                let req = crate::backend::TtsRequestVm {
                    text: t("novel.tts.sample"),
                    provider: prefs.provider,
                    voice: None,
                    rate: prefs.rate,
                };
                match backend.tts_speak(&req).await {
                    Ok(vm) => {
                        if let Some(clip) = vm.clip {
                            let url = register("clip", &clip.path).unwrap_or(clip.url.clone());
                            let mut ev = document::eval(&play_clip_js(&url));
                            if let Ok(out) = ev.recv::<String>().await
                                && out == "error"
                            {
                                test_err.set(Some(UiError::new(
                                    crate::model::ErrorKind::Other,
                                    t("novel.tts.play_failed"),
                                )));
                            }
                        }
                    }
                    Err(e) => test_err.set(Some(e)),
                }
                test_state.set("idle");
            });
        }
    };

    let tts_p = tts.clone();
    let tts_v = tts.clone();
    let tts_rate = tts.clone();
    let tts_pitch = tts.clone();
    let tts_auto = tts.clone();
    let tts_hl = tts.clone();
    let tts_ep = tts.clone();
    let tts_model = tts.clone();
    let endpoint = tts.endpoint.clone().unwrap_or_default();
    let model = tts.model.clone().unwrap_or_default();
    let model_placeholder = match provider {
        TtsProviderVm::ElevenLabs => "eleven_multilingual_v2",
        TtsProviderVm::OpenAiCompatible => "kokoro",
        _ => "tts-1",
    };
    let key_label = if key_state == "saved" {
        t("novel.tts.key_replace")
    } else {
        t("novel.tts.key")
    };

    rsx! {
        div { class: "tk-novel-narrator", "data-provider": "{provider.id()}",
            SettingRow { label: t("novel.tts.provider"), hint: t("novel.tts.provider_hint"), stack: true,
                Segmented {
                    options: provider_opts,
                    selected: provider.id().to_string(),
                    label: t("novel.tts.provider"),
                    small: true,
                    on_select: move |id: String| {
                        if let Some(p) = TtsProviderVm::from_id(&id) {
                            key_draft.set(String::new());
                            key_err.set(None);
                            on_saved.call(with_provider(&tts_p, p));
                        }
                    },
                }
            }
            if provider.needs_endpoint() {
                SettingRow { label: t("novel.tts.endpoint"), hint: t("novel.tts.endpoint_hint"), stack: true,
                    TextField {
                        label: t("novel.tts.endpoint"),
                        value: endpoint.clone(),
                        placeholder: "http://127.0.0.1:8880/v1".to_string(),
                        mono: true,
                        oninput: move |v: String| {
                            let mut next = tts_ep.clone();
                            next.endpoint = Some(v).filter(|s| !s.trim().is_empty());
                            on_change.call(next);
                        },
                    }
                }
            }
            if matches!(provider, TtsProviderVm::OpenAi | TtsProviderVm::OpenAiCompatible | TtsProviderVm::ElevenLabs) {
                SettingRow { label: t("novel.tts.model"), hint: t("novel.tts.model_hint"), stack: true,
                    TextField {
                        label: t("novel.tts.model"),
                        value: model.clone(),
                        placeholder: model_placeholder.to_string(),
                        mono: true,
                        oninput: move |v: String| {
                            let mut next = tts_model.clone();
                            next.model = Some(v).filter(|s| !s.trim().is_empty());
                            on_change.call(next);
                        },
                    }
                }
            }
            if provider.needs_key() {
                SettingRow { label: t("novel.tts.key"), hint: t(if provider == TtsProviderVm::OpenAiCompatible { "novel.tts.key_hint_optional" } else { "novel.tts.key_hint" }), stack: true,
                    div { class: "tk-novel-key", "data-tts-key": "{key_state}",
                        if key_loading {
                            LoadingIndicator { small: true }
                        } else if key_state == "saved" {
                            div { class: "tk-novel-key__saved", role: "status",
                                span { {t("novel.tts.key_saved")} }
                                Button {
                                    label: t("common.remove"),
                                    icon: "trash".to_string(),
                                    variant: ButtonVariant::Text,
                                    size: ButtonSize::Sm,
                                    disabled: key_busy(),
                                    onclick: move |_| remove_btn(true),
                                }
                            }
                        }
                        div { class: "tk-novel-key__edit",
                            TextField {
                                label: key_label.clone(),
                                value: draft.clone(),
                                kind: "password".to_string(),
                                mono: true,
                                disabled: key_busy(),
                                oninput: move |v: String| key_draft.set(v),
                                onsubmit: move |_| save_enter(false),
                            }
                            Button {
                                label: t("common.save"),
                                icon: "check".to_string(),
                                variant: ButtonVariant::Tonal,
                                size: ButtonSize::Sm,
                                disabled: key_busy() || draft.trim().is_empty(),
                                onclick: move |_| save_btn(false),
                            }
                        }
                        if let Some(e) = key_err() {
                            ErrorState { error: e, compact: true, retry: false, title: t("novel.tts.key_failed") }
                        }
                    }
                }
            }
            SettingRow { label: t("novel.tts.voice"), hint: t("novel.tts.voice_hint"),
                div { class: "tk-novel-voice", "data-voices": "{voice_list.len()}",
                    if voices_loading {
                        LoadingIndicator { small: true }
                    } else {
                        select {
                            "aria-label": t("novel.tts.voice"),
                            value: "{tts.voice}",
                            onchange: move |e| {
                                let mut next = tts_v.clone();
                                next.voice = e.value();
                                on_saved.call(next);
                            },
                            option { value: "", selected: tts.voice.is_empty(), {t("novel.tts.voice_default")} }
                            for v in voice_list.clone() {
                                option {
                                    key: "{v.id}",
                                    value: "{v.id}",
                                    selected: v.id == tts.voice,
                                    {match (&v.lang, &v.gender) {
                                        (Some(l), Some(g)) => format!("{} · {l} · {g}", v.name),
                                        (Some(l), None) => format!("{} · {l}", v.name),
                                        _ => v.name.clone(),
                                    }}
                                }
                            }
                        }
                    }
                    if let Some(e) = voices_err {
                        ErrorState { error: e, compact: true, title: t("novel.tts.voices_failed"), on_retry: move |_| {
                            let mut v = voices;
                            v.restart();
                        } }
                    }
                }
            }
            SettingRow { label: t("novel.tts.speed"), hint: t("novel.tts.speed_hint"),
                input {
                    r#type: "range",
                    min: "{TTS_RATE_MIN}",
                    max: "{TTS_RATE_MAX}",
                    step: "5",
                    value: "{tts.rate}",
                    "aria-label": t("novel.tts.speed"),
                    "data-tts-rate": "{tts.rate}",
                    onchange: move |e| {
                        if let Ok(n) = e.value().parse::<u8>() {
                            let mut next = tts_rate.clone();
                            next.rate = n;
                            on_change.call(next);
                        }
                    },
                }
                span { class: "tk-novel-ctl__value", "{tts.rate} %" }
            }
            if provider == TtsProviderVm::System {
                SettingRow { label: t("novel.tts.pitch"), hint: t("novel.tts.pitch_hint"),
                    input {
                        r#type: "range",
                        min: "{TTS_RATE_MIN}",
                        max: "{TTS_RATE_MAX}",
                        step: "5",
                        value: "{tts.pitch}",
                        "aria-label": t("novel.tts.pitch"),
                        onchange: move |e| {
                            if let Ok(n) = e.value().parse::<u8>() {
                                let mut next = tts_pitch.clone();
                                next.pitch = n;
                                on_change.call(next);
                            }
                        },
                    }
                    span { class: "tk-novel-ctl__value", "{tts.pitch} %" }
                }
            }
            SettingRow { label: t("novel.tts.auto_advance"), hint: t("novel.tts.auto_advance_hint"),
                Switch { checked: tts.auto_advance, label: t("novel.tts.auto_advance"), on_toggle: move |v: bool| {
                    let mut next = tts_auto.clone();
                    next.auto_advance = v;
                    on_saved.call(next);
                } }
            }
            SettingRow { label: t("novel.tts.highlight"), hint: t("novel.tts.highlight_hint"),
                Switch { checked: tts.highlight, label: t("novel.tts.highlight"), on_toggle: move |v: bool| {
                    let mut next = tts_hl.clone();
                    next.highlight = v;
                    on_saved.call(next);
                } }
            }
            SettingRow { label: t("novel.tts.test"), hint: t("novel.tts.test_hint"), stack: true,
                div { class: "tk-novel-test", "data-tts-state": "{test_state()}",
                    Button {
                        label: if test_state() == "playing" { t("novel.tts.stop") } else { t("novel.tts.test_run") },
                        icon: if test_state() == "playing" { "close".to_string() } else { "volume".to_string() },
                        variant: ButtonVariant::Tonal,
                        size: ButtonSize::Sm,
                        onclick: run_test,
                    }
                    if test_state() == "playing" {
                        LoadingIndicator { small: true }
                    }
                    if let Some(e) = test_err() {
                        ErrorState { error: e, compact: true, retry: false, title: t("novel.tts.failed") }
                    }
                }
            }
        }
    }
}

#[component]
fn KittenSettings(tts: TtsPrefs, on_change: EventHandler<TtsPrefs>) -> Element {
    let services = use_context::<Services>();
    let snack = use_snackbar();
    let mut status = use_resource({
        let backend = services.backend.clone();
        move || {
            let backend = backend.clone();
            async move { backend.kitten_status().await }
        }
    });
    let mut installing = use_signal(|| false);
    let mut progress = use_signal(|| None::<crate::model::novel::KittenProgressVm>);
    let mut action_err = use_signal(|| None::<UiError>);

    // Poll the download progress while the install call runs.
    use_future(move || async move {
        loop {
            tokio::time::sleep(Duration::from_millis(300)).await;
            if *installing.peek() {
                let p = kitten_progress();
                if *progress.peek() != p {
                    progress.set(p);
                }
            }
        }
    });

    let model_now = if tts.provider == TtsProviderVm::Kitten {
        tts.kitten_model()
    } else {
        crate::model::novel::KITTEN_DEFAULT_MODEL
    };
    let (_, model_mb) = kitten_model_info(model_now).unwrap_or(("", 0));
    let st = status.read().as_ref().and_then(|r| r.as_ref().ok()).cloned();
    let read_err = status.read().as_ref().and_then(|r| r.as_ref().err()).cloned();
    let loading = status.read().is_none();
    let state = kitten_state(st.as_ref(), installing());
    let runtime_attr = st
        .as_ref()
        .and_then(|s| s.runtime.clone())
        .unwrap_or_else(|| "missing".into());
    let phon_attr = st
        .as_ref()
        .and_then(|s| s.phonemizer.clone())
        .unwrap_or_else(|| "missing".into());
    let model_present = st.as_ref().is_some_and(|s| s.model_present);
    let phon_ok = phon_attr != "missing";
    let runtime_present = st.as_ref().is_some_and(|s| s.runtime.is_some());
    let need_mb =
        if runtime_present { 0 } else { KITTEN_RUNTIME_MB } + if model_present { 0 } else { model_mb };
    let pct = progress().and_then(|p| p.percent());
    let stage = progress().map(|p| p.stage).unwrap_or_default();
    let tts_pick = tts.clone();
    let tts_use = tts.clone();

    let install = {
        let backend = services.backend.clone();
        move |_| {
            if *installing.peek() {
                return;
            }
            let backend = backend.clone();
            installing.set(true);
            action_err.set(None);
            progress.set(None);
            spawn(async move {
                let res = backend.kitten_install().await;
                installing.set(false);
                progress.set(None);
                match res {
                    Ok(_) => {
                        snack.show(t("novel.kitten.installed"));
                        status.restart();
                    }
                    Err(e) => {
                        action_err.set(Some(e));
                        status.restart();
                    }
                }
            });
        }
    };
    let remove = {
        let backend = services.backend.clone();
        move |_| {
            let backend = backend.clone();
            action_err.set(None);
            spawn(async move {
                match backend.kitten_remove().await {
                    Ok(()) => {
                        snack.show(t("novel.kitten.removed"));
                        status.restart();
                    }
                    Err(e) => action_err.set(Some(e)),
                }
            });
        }
    };

    rsx! {
        div {
            class: "tk-novel-kitten",
            "data-kitten": "{state}",
            "data-kitten-runtime": "{runtime_attr}",
            "data-kitten-phonemizer": "{phon_attr}",
            "data-kitten-model": "{model_now}",
            p { class: "tk-panel__hint", {t("novel.kitten.hint")} }
            SettingRow { label: t("novel.kitten.model"), hint: t("novel.kitten.model_hint"),
                select {
                    "aria-label": t("novel.kitten.model"),
                    value: "{model_now}",
                    disabled: installing(),
                    onchange: move |e| {
                        let id = e.value();
                        if kitten_model_info(&id).is_some() {
                            let mut next = with_provider(&tts_pick, TtsProviderVm::Kitten);
                            next.model = Some(id);
                            on_change.call(next);
                            status.restart();
                        }
                    },
                    for (id , _ , mb) in KITTEN_MODELS {
                        option { key: "{id}", value: "{id}", selected: id == model_now, {tf(&format!("novel.kitten.model.{id}"), &[("mb", &mb.to_string())])} }
                    }
                }
            }
            if loading {
                LoadingIndicator { small: true }
            } else if let Some(e) = read_err {
                ErrorState { error: e, compact: true, title: t("novel.kitten.status_failed"), on_retry: move |_| status.restart() }
            } else {
                ul { class: "tk-novel-kitten__parts",
                    li { "data-part": "runtime", "data-ok": "{runtime_present}",
                        b { {t("novel.kitten.runtime")} }
                        span { class: "tk-muted",
                            if runtime_present { "{runtime_attr}" } else { {tf("novel.kitten.runtime_missing", &[("mb", &KITTEN_RUNTIME_MB.to_string())])} }
                        }
                    }
                    li { "data-part": "model", "data-ok": "{model_present}",
                        b { {t("novel.kitten.model")} }
                        span { class: "tk-muted",
                            if model_present { {t("novel.kitten.model_ready")} } else { {tf("novel.kitten.model_missing", &[("mb", &model_mb.to_string())])} }
                        }
                    }
                    li { "data-part": "phonemizer", "data-ok": "{phon_ok}",
                        b { {t("novel.kitten.phonemizer")} }
                        if phon_attr == "missing" {
                            span { class: "tk-muted", {t("novel.kitten.phonemizer_missing")} }
                            code { class: "tk-novel-kitten__cmd", {t(espeak_hint_key())} }
                        } else {
                            code { class: "tk-novel-kitten__cmd", "{phon_attr}" }
                        }
                    }
                }
                if let Some(dir) = st.as_ref().map(|s| s.model_dir.clone()) {
                    p { class: "tk-muted tk-novel-kitten__dir", {tf("novel.kitten.dir", &[("dir", &dir)])} }
                }
                if installing() {
                    div { class: "tk-novel-kitten__progress", role: "progressbar", "aria-valuemin": "0", "aria-valuemax": "100", "aria-valuenow": pct.map(|p| p.to_string()).unwrap_or_default(),
                        LoadingIndicator { small: true }
                        span { {t(&format!("novel.kitten.stage.{}", if stage.is_empty() { "start" } else { stage.as_str() }))} }
                        if let Some(p) = pct {
                            span { class: "tk-novel-kitten__pct", "{p}%" }
                            div { class: "tk-novel-reader__progress", i { style: "width:{p}%" } }
                        }
                    }
                }
                div { class: "tk-novel-kitten__actions",
                    if state != "ready" {
                        Button {
                            label: if need_mb > 0 { tf("novel.kitten.install_mb", &[("mb", &need_mb.to_string())]) } else { t("novel.kitten.install") },
                            icon: "download".to_string(),
                            variant: ButtonVariant::Filled,
                            size: ButtonSize::Sm,
                            disabled: installing(),
                            onclick: install,
                        }
                    }
                    if state == "ready" && tts.provider != TtsProviderVm::Kitten {
                        Button {
                            label: t("novel.kitten.use"),
                            icon: "check".to_string(),
                            variant: ButtonVariant::Tonal,
                            size: ButtonSize::Sm,
                            onclick: move |_| on_change.call(with_provider(&tts_use, TtsProviderVm::Kitten)),
                        }
                    }
                    if runtime_present || model_present {
                        Button {
                            label: t("novel.kitten.remove"),
                            icon: "trash".to_string(),
                            variant: ButtonVariant::Text,
                            size: ButtonSize::Sm,
                            disabled: installing(),
                            onclick: remove,
                        }
                    }
                }
                if let Some(err) = st.as_ref().and_then(|s| s.error.clone()) {
                    p { class: "tk-novel-kitten__error tk-muted", role: "status", {tf("novel.kitten.last_error", &[("detail", &err)])} }
                }
            }
            if let Some(e) = action_err() {
                ErrorState { error: e, compact: true, retry: false, title: t("novel.kitten.failed") }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kitten_states() {
        assert_eq!(kitten_state(None, false), "missing");
        assert_eq!(kitten_state(None, true), "installing");
        let ready = KittenStatusVm {
            available: true,
            runtime: Some("ONNX Runtime 1.22.0".into()),
            phonemizer: Some("/usr/bin/espeak-ng".into()),
            model_present: true,
            model_dir: "/d".into(),
            error: None,
        };
        assert_eq!(kitten_state(Some(&ready), false), "ready");
        let broken = KittenStatusVm {
            available: false,
            error: Some("x".into()),
            ..ready.clone()
        };
        assert_eq!(kitten_state(Some(&broken), false), "error");
        let half = KittenStatusVm {
            available: false,
            model_present: false,
            ..ready
        };
        assert_eq!(kitten_state(Some(&half), false), "missing");
    }

    #[test]
    fn switching_provider_clears_voice_and_model() {
        let p = TtsPrefs {
            voice: "alloy".into(),
            model: Some("tts-1".into()),
            ..TtsPrefs::default()
        };
        let n = with_provider(&p, TtsProviderVm::Kitten);
        assert_eq!(n.provider, TtsProviderVm::Kitten);
        assert!(n.voice.is_empty());
        assert_eq!(n.model, None);
        assert_eq!(with_provider(&p, TtsProviderVm::System), p);
    }
}
