//! Settings > Player: engine (embedded / enhanced / external mpv), upscaling preset and source selection; wave A adds auto-next, auto-skip, stable volume, Discord presence and the rebindable shortcuts (`Settings::flow`).
//!
//! Split out of `screens/settings.rs` by the wave-A contract (docs/redesign/15-parity-wave-a-contract.md
//! section 2). Owner: worker **e**. The page shell (`mod.rs`: index, scroll spy, `SettingsPanel`, `SettingRow`) is frozen.

use dioxus::prelude::*;
use tk_player::ShaderPreset;

use super::{SettingRow, lang_seg, mpv_placeholder, seg};
use crate::app::{Services, Ui};
use crate::backend::{AnimeBuildVm, SourceKindVm};
use crate::components::{
    Button, ButtonSize, ButtonVariant, ErrorState, IconButton, IconButtonSize, LoadingIndicator, Pill, Segmented,
    SkeletonLines, TextField, Tone, use_snackbar,
};
use crate::i18n::{t, tf};
use crate::model::{DUB_LANGS, EnhancedUiVm, PlatformVm, PlayerModeVm, QualityVm, SUB_LANGS, SettingsAction, SourceModeVm, UiError};

#[component]
pub fn PlayerSection(commit_saved: Callback<SettingsAction>) -> Element {
    let ui = use_context::<Ui>();
    let s = (ui.settings)();
    let prefer_id = s.sources.prefer.map(|k| k.id()).unwrap_or("any").to_owned();
    let mut prefer_options = vec![("any".to_owned(), t("src.prefer.any"))];
    prefer_options.extend(SourceKindVm::ALL.iter().map(|k| (k.id().to_owned(), t(&format!("src.{}", k.id())))));
    rsx! {
        PlayerEngineRows { on_action: commit_saved }
        SettingRow { label: t("settings.player.preset"), hint: t("settings.player.preset_hint_embedded"), stack: true,
            Segmented {
                options: ShaderPreset::ALL
                    .iter()
                    .map(|p| (p.id().to_owned(), t(&format!("preset.{}", p.id()))))
                    .collect::<Vec<_>>(),
                selected: s.shader_preset.clone(),
                label: t("settings.player.preset"),
                small: true,
                on_select: move |id: String| {
                    if let Some(p) = ShaderPreset::from_id(&id) {
                        commit_saved.call(SettingsAction::SetPreset(p));
                    }
                },
            }
        }
        SettingRow { label: t("src.mode"), hint: t("src.mode.hint"),
            Segmented {
                options: seg(&SourceModeVm::ALL, SourceModeVm::id, "src.mode"),
                selected: s.sources.mode.id().to_owned(),
                label: t("src.mode"),
                on_select: move |id: String| {
                    if let Some(m) = SourceModeVm::ALL.into_iter().find(|m| m.id() == id) {
                        commit_saved.call(SettingsAction::SetSourceMode(m));
                    }
                },
            }
        }
        SettingRow { label: t("src.prefer"), hint: t("src.prefer.hint"),
            Segmented {
                options: prefer_options,
                selected: prefer_id,
                label: t("src.prefer"),
                small: true,
                on_select: move |id: String| {
                    // "any" (or anything unknown) clears the preference.
                    let kind = SourceKindVm::ALL.into_iter().find(|k| k.id() == id);
                    commit_saved.call(SettingsAction::SetSourcePrefer(kind));
                },
            }
        }
        SettingRow { label: t("src.quality"),
            Segmented {
                options: QualityVm::OFFERED.iter().map(|q| (q.id().to_owned(), q.id().to_owned())).collect::<Vec<_>>(),
                selected: s.sources.min_quality.id().to_owned(),
                label: t("src.quality"),
                small: true,
                on_select: move |id: String| {
                    if let Some(q) = QualityVm::ALL.into_iter().find(|q| q.id() == id) {
                        commit_saved.call(SettingsAction::SetMinQuality(q));
                    }
                },
            }
        }
        SettingRow { label: t("src.sub_lang"),
            Segmented {
                options: lang_seg(&SUB_LANGS),
                selected: s.sources.sub_lang.clone(),
                label: t("src.sub_lang"),
                small: true,
                on_select: move |id: String| commit_saved.call(SettingsAction::SetSubLang(id)),
            }
        }
        SettingRow { label: t("src.dub_lang"),
            Segmented {
                options: lang_seg(&DUB_LANGS),
                selected: s.sources.dub_lang.clone(),
                label: t("src.dub_lang"),
                small: true,
                on_select: move |id: String| commit_saved.call(SettingsAction::SetDubLang(id)),
            }
        }
        // Wave A (worker e): auto-next + countdown seconds, auto-skip intro, stable volume, Discord Rich Presence,
        // shortcuts editor (`SettingsAction::SetFlow`). Keys under `settings.player.*` (namespace `settings_player`).
    }
}

/// Settings > Player > Engine (docs/redesign/09 section 4.3): Embedded / Embedded enhanced / External mpv with
/// a line per mode, then the enhanced rows (who draws the controls + mpv-anime-build status and Prepare) or
/// the external rows (mpv path + detection). Enhanced is disabled when this installation lacks the release.
#[component]
fn PlayerEngineRows(on_action: EventHandler<SettingsAction>) -> Element {
    let services = use_context::<Services>();
    let ui = use_context::<Ui>();
    let snack = use_snackbar();
    let backend = services.backend.clone();
    let mut build = use_resource({
        let backend = backend.clone();
        move || {
            let backend = backend.clone();
            async move { backend.anime_build_status().await }
        }
    });
    let mut preparing = use_signal(|| false);
    let mut prepare_error = use_signal(|| None::<UiError>);

    let prepare = move |_| {
        if preparing() {
            return;
        }
        preparing.set(true);
        prepare_error.set(None);
        let backend = backend.clone();
        spawn(async move {
            match backend.prepare_anime_build().await {
                Ok(_) => {
                    snack.show(t("settings.player.anime.prepared"));
                    build.restart();
                }
                Err(e) => prepare_error.set(Some(e)),
            }
            preparing.set(false);
        });
    };

    let (mode, enhanced_ui) = {
        let s = ui.settings.read();
        (s.player_mode, s.enhanced_ui)
    };
    let build_vm: Option<Result<AnimeBuildVm, UiError>> = build.read().clone();
    let unavailable = matches!(&build_vm, Some(Ok(b)) if !b.available);
    // A saved Enhanced choice stays selectable so the status row can explain why it cannot run.
    let disabled_keys = if unavailable && mode != PlayerModeVm::EmbeddedEnhanced {
        vec![PlayerModeVm::EmbeddedEnhanced.id().to_owned()]
    } else {
        Vec::new()
    };

    let build_block = match build_vm {
        None => rsx! {
            SkeletonLines { widths: vec![40] }
        },
        Some(Err(e)) => rsx! {
            ErrorState { error: e, compact: true, on_retry: move |_| build.restart() }
        },
        Some(Ok(b)) if !b.available => rsx! {
            div { class: "tk-engine-status",
                Pill { text: t("settings.player.anime.missing"), tone: Tone::Err }
                span { class: "tk-engine-status__note", {t("settings.player.anime.missing_hint")} }
            }
        },
        Some(Ok(b)) if b.extracted => {
            let label = tf("settings.player.anime.ready", &[("version", &b.version)]);
            let path = b.path.unwrap_or_default();
            rsx! {
                div { class: "tk-engine-status",
                    Pill { text: label, tone: Tone::Ok }
                    if !path.is_empty() {
                        code { class: "tk-engine-status__path", "{path}" }
                    }
                }
            }
        }
        Some(Ok(b)) => {
            let label = tf("settings.player.anime.not_prepared", &[("version", &b.version)]);
            rsx! {
                div { class: "tk-engine-status",
                    Pill { text: label, tone: Tone::Warn }
                    Button {
                        label: if preparing() { t("settings.player.anime.preparing") } else { t("settings.player.anime.prepare") },
                        icon: "download".to_string(),
                        variant: ButtonVariant::Tonal,
                        size: ButtonSize::Sm,
                        disabled: preparing(),
                        onclick: prepare,
                    }
                }
            }
        }
    };

    rsx! {
        SettingRow { label: t("settings.player.engine"), hint: t("settings.player.engine_hint"), stack: true,
            Segmented {
                options: seg(&PlayerModeVm::ALL, PlayerModeVm::id, "settings.player.mode"),
                selected: mode.id().to_owned(),
                label: t("settings.player.engine"),
                disabled_keys,
                on_select: move |id: String| {
                    if let Some(m) = PlayerModeVm::ALL.into_iter().find(|m| m.id() == id) {
                        on_action.call(SettingsAction::SetPlayerMode(m));
                    }
                },
            }
            ul { class: "tk-engine-modes",
                for m in PlayerModeVm::ALL {
                    {
                        let id = m.id();
                        let name = t(&format!("settings.player.mode.{id}"));
                        let hint = if m == PlayerModeVm::EmbeddedEnhanced && unavailable {
                            t("settings.player.anime.missing_hint")
                        } else {
                            t(&format!("settings.player.mode.{id}_hint"))
                        };
                        let selected = m == mode;
                        rsx! {
                            li { key: "{id}", "data-selected": "{selected}",
                                b { "{name}" }
                                span { "{hint}" }
                            }
                        }
                    }
                }
            }
        }
        if mode == PlayerModeVm::EmbeddedEnhanced {
            SettingRow {
                label: t("settings.player.enhanced_ui"),
                hint: t(&format!("settings.player.enhanced_ui.{}_hint", enhanced_ui.id())),
                Segmented {
                    options: seg(&EnhancedUiVm::ALL, EnhancedUiVm::id, "settings.player.enhanced_ui"),
                    selected: enhanced_ui.id().to_owned(),
                    label: t("settings.player.enhanced_ui"),
                    small: true,
                    on_select: move |id: String| {
                        if let Some(u) = EnhancedUiVm::ALL.into_iter().find(|u| u.id() == id) {
                            on_action.call(SettingsAction::SetEnhancedUi(u));
                        }
                    },
                }
            }
            SettingRow { label: t("settings.player.anime"), hint: t("settings.player.anime_hint"), stack: true,
                {build_block}
                if let Some(e) = prepare_error() {
                    ErrorState { error: e, compact: true, retry: false }
                }
            }
        }
        if mode == PlayerModeVm::External {
            ExternalMpvRows { on_action }
        }
    }
}

/// External mpv: path field (empty = autodetect) and the detection status (`tk_player::find_mpv` +
/// `mpv_version`, off the UI thread), re-run whenever the saved path changes.
#[component]
fn ExternalMpvRows(on_action: EventHandler<SettingsAction>) -> Element {
    let ui = use_context::<Ui>();
    let os = PlatformVm::current();
    let mut draft = use_signal(|| None::<String>);
    let mut detect = use_resource(move || {
        let custom = ui.settings.read().external_mpv_path.clone();
        async move {
            tokio::task::spawn_blocking(move || {
                let found = tk_player::find_mpv(custom.as_deref().map(std::path::Path::new))?;
                let version = tk_player::mpv_version(&found);
                Some((found.display().to_string(), version))
            })
            .await
            .ok()
            .flatten()
        }
    });

    let saved = ui.settings.read().external_mpv_path.clone();
    let save = use_callback(move |_: ()| {
        if let Some(d) = draft() {
            on_action.call(SettingsAction::SetExternalMpvPath(Some(d)));
            draft.set(None);
        }
    });
    let text = draft().unwrap_or_else(|| saved.clone().unwrap_or_default());
    let missing_hint = if saved.is_some() {
        t("settings.player.mpv.not_found_custom")
    } else {
        t(&format!("settings.player.mpv.install.{}", os.id()))
    };

    let detected: Option<Option<(String, Option<String>)>> = detect.read().clone();
    let status = match detected {
        None => rsx! {
            LoadingIndicator { small: true }
            span { class: "tk-engine-status__note", {t("settings.player.mpv.detecting")} }
        },
        Some(None) => rsx! {
            Pill { text: t("settings.player.mpv.not_found"), tone: Tone::Err }
            span { class: "tk-engine-status__note", "{missing_hint}" }
        },
        Some(Some((path, Some(v)))) => {
            // "mpv v0.40.0 Copyright © ..." -> "mpv v0.40.0".
            let short = v.split_whitespace().take(2).collect::<Vec<_>>().join(" ");
            rsx! {
                Pill { text: tf("settings.player.mpv.found", &[("version", &short)]), tone: Tone::Ok }
                code { class: "tk-engine-status__path", "{path}" }
            }
        }
        Some(Some((path, None))) => rsx! {
            Pill { text: t("settings.player.mpv.no_version"), tone: Tone::Warn }
            code { class: "tk-engine-status__path", "{path}" }
        },
    };

    rsx! {
        SettingRow { label: t("settings.player.mpv_path"), hint: t("settings.player.mpv_path_hint"), stack: true,
            div { class: "tk-settings__mpv",
                TextField {
                    label: t("settings.player.mpv_path"),
                    value: text,
                    mono: true,
                    placeholder: mpv_placeholder(os).to_string(),
                    hint: t("settings.player.mpv_path_auto"),
                    oninput: move |v: String| draft.set(Some(v)),
                    onsubmit: move |_| save.call(()),
                }
                Button {
                    label: t("common.save"),
                    variant: ButtonVariant::Tonal,
                    size: ButtonSize::Sm,
                    disabled: draft().is_none(),
                    onclick: move |_| save.call(()),
                }
            }
            div { class: "tk-engine-status", role: "status",
                {status}
                IconButton {
                    icon: "refresh".to_string(),
                    label: t("settings.player.mpv.redetect"),
                    size: IconButtonSize::Sm,
                    disabled: detect.read().is_none(),
                    onclick: move |_| detect.restart(),
                }
            }
        }
    }
}
