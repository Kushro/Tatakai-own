//! Settings > Player: engine (embedded / enhanced / external mpv), upscaling preset and source selection; wave A adds auto-next, auto-skip, stable volume, Discord presence and the rebindable shortcuts (`Settings::flow`).
//!
//! Split out of `screens/settings.rs` by the wave-A contract (docs/redesign/15-parity-wave-a-contract.md
//! section 2). Owner: worker **e**. The page shell (`mod.rs`: index, scroll spy, `SettingsPanel`, `SettingRow`) is frozen.

use dioxus::prelude::*;
use tk_player::ShaderPreset;

use super::{SettingRow, lang_seg, mpv_placeholder, seg};
use crate::app::{Services, Ui};
use crate::backend::{AnimeBuildVm, PresenceVm, SourceKindVm};
use crate::components::{
    Button, ButtonSize, ButtonVariant, ErrorState, IconButton, IconButtonSize, Kbd, LoadingIndicator, Pill, Segmented,
    SkeletonLines, Switch, TextField, Tone, use_snackbar,
};
use crate::i18n::{t, tf};
use crate::model::{
    DUB_LANGS, EnhancedUiVm, PlatformVm, PlayerFlowPrefs, PlayerModeVm, QualityVm, SUB_LANGS, SettingsAction,
    ShortcutMapVm, SourceModeVm, UiError,
};
use crate::screens::player::presence;
use crate::shortcuts::{Shortcut, assign, binding, conflicts, display_spec, is_reserved, spec_from_event, unassign};

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
        PlayerFlowRows { on_action: commit_saved }
        ShortcutEditor { on_action: commit_saved }
    }
}

/// Countdown lengths offered for the next-episode card (`PlayerFlowPrefs::auto_next_secs`, classic 3 to 30 s).
pub const AUTO_NEXT_CHOICES: [u8; 6] = [3, 5, 10, 15, 20, 30];

/// Auto-next (+ countdown), auto-skip intro, stable volume and Discord Rich Presence (`Settings::flow`).
#[component]
fn PlayerFlowRows(on_action: EventHandler<SettingsAction>) -> Element {
    let services = use_context::<Services>();
    let ui = use_context::<Ui>();
    let flow = ui.settings.read().flow.clone();
    let set = move |f: PlayerFlowPrefs| on_action.call(SettingsAction::SetFlow(f));
    let secs_options: Vec<(String, String)> = AUTO_NEXT_CHOICES
        .iter()
        .map(|s| (s.to_string(), tf("settings.player.auto_next_secs.value", &[("n", &s.to_string())])))
        .collect();
    let backend = services.backend.clone();
    rsx! {
        SettingRow { label: t("settings.player.auto_next"), hint: t("settings.player.auto_next_hint"),
            div { "data-auto-next-setting": "{flow.auto_next}",
                Switch {
                    checked: flow.auto_next,
                    label: t("settings.player.auto_next"),
                    on_toggle: move |v: bool| {
                        let mut f = ui.settings.peek().flow.clone();
                        f.auto_next = v;
                        set(f);
                    },
                }
            }
        }
        SettingRow { label: t("settings.player.auto_next_secs"), hint: t("settings.player.auto_next_secs_hint"),
            Segmented {
                options: secs_options,
                selected: flow.auto_next_secs.to_string(),
                label: t("settings.player.auto_next_secs"),
                small: true,
                disabled_keys: if flow.auto_next { Vec::new() } else { AUTO_NEXT_CHOICES.iter().map(u8::to_string).collect() },
                on_select: move |id: String| {
                    if let Ok(n) = id.parse::<u8>() {
                        let mut f = ui.settings.peek().flow.clone();
                        f.auto_next_secs = n;
                        set(f);
                    }
                },
            }
        }
        SettingRow { label: t("settings.player.auto_skip"), hint: t("settings.player.auto_skip_hint"),
            div { "data-auto-skip-setting": "{flow.auto_skip_intro}",
                Switch {
                    checked: flow.auto_skip_intro,
                    label: t("settings.player.auto_skip"),
                    on_toggle: move |v: bool| {
                        let mut f = ui.settings.peek().flow.clone();
                        f.auto_skip_intro = v;
                        set(f);
                    },
                }
            }
        }
        SettingRow { label: t("settings.player.stable_volume"), hint: t("settings.player.stable_volume_hint"),
            div { "data-stable-volume-setting": "{flow.stable_volume}",
                Switch {
                    checked: flow.stable_volume,
                    label: t("settings.player.stable_volume"),
                    on_toggle: move |v: bool| {
                        let mut f = ui.settings.peek().flow.clone();
                        f.stable_volume = v;
                        set(f);
                    },
                }
            }
        }
        SettingRow { label: t("settings.player.discord"), hint: t("settings.player.discord_hint"),
            div { "data-discord": "{flow.discord_presence}",
                Switch {
                    checked: flow.discord_presence,
                    label: t("settings.player.discord"),
                    on_toggle: move |v: bool| {
                        let mut f = ui.settings.peek().flow.clone();
                        f.discord_presence = v;
                        set(f);
                        // Turning it off clears Discord at once; turning it on shows this page as browsing.
                        let state = if v { PresenceVm::Browsing } else { PresenceVm::Clear };
                        presence::send(&backend, state);
                    },
                }
            }
        }
    }
}

/// Settings > Player > Shortcuts (classic `keybindings.ts`): one row per rebindable action with its key; "Change"
/// captures the next key press (Esc cancels), clashes are marked on both rows, reserved keys are refused, and
/// "Reset" goes back to the defaults. Saved in `Settings::flow.shortcuts`.
#[component]
fn ShortcutEditor(on_action: EventHandler<SettingsAction>) -> Element {
    let ui = use_context::<Ui>();
    let map = ui.settings.read().flow.shortcuts.clone();
    let mut capturing = use_signal(|| None::<&'static str>);
    let mut refused = use_signal(|| None::<(&'static str, String)>);
    let clashes = conflicts(&map);
    let customized = !map.0.is_empty();
    let save = move |next: ShortcutMapVm| {
        let mut f = ui.settings.peek().flow.clone();
        f.shortcuts = next;
        on_action.call(SettingsAction::SetFlow(f));
    };
    let rows: Vec<(Shortcut, &'static str, String, String, bool, bool)> = Shortcut::REBINDABLE
        .into_iter()
        .map(|a| {
            let id = a.id();
            let spec = binding(&map, &a).unwrap_or_default();
            let shown = if spec.is_empty() { t("player.key.none") } else { display_spec(&spec) };
            let custom = map.get(id).is_some();
            let clash = clashes.contains(id);
            (a, id, spec, shown, custom, clash)
        })
        .collect();
    let conflict_count = clashes.len();
    let row_count = rows.len();
    rsx! {
        SettingRow { label: t("settings.player.shortcuts"), hint: t("settings.player.shortcuts_hint"), stack: true,
            div {
                class: "tk-keymap",
                "data-shortcuts": "{row_count}",
                "data-conflicts": "{conflict_count}",
                "data-customized": "{customized}",
                for (action , id , spec , shown , custom , clash) in rows {
                    {
                        let editing = capturing() == Some(id);
                        let refusal = refused().filter(|(rid, _)| *rid == id).map(|(_, text)| text);
                        let label = t(&format!("player.shortcut.{id}"));
                        let reset_action = action.clone();
                        rsx! {
                            div {
                                key: "{id}",
                                class: "tk-keymap__row",
                                "data-shortcut-id": "{id}",
                                "data-key": "{spec}",
                                "data-conflict": "{clash}",
                                "data-custom": "{custom}",
                                "data-editing": "{editing}",
                                span { class: "tk-keymap__label", "{label}" }
                                div { class: "tk-keymap__keys",
                                    if editing {
                                        input {
                                            class: "tk-keymap__capture",
                                            r#type: "text",
                                            readonly: true,
                                            "aria-label": tf("settings.player.shortcuts.press_for", &[("action", &label)]),
                                            placeholder: t("settings.player.shortcuts.press"),
                                            onmounted: move |e| {
                                                spawn(async move {
                                                    let _ = e.set_focus(true).await;
                                                });
                                            },
                                            onblur: move |_| capturing.set(None),
                                            onkeydown: {
                                                let action = action.clone();
                                                move |e: KeyboardEvent| {
                                                    e.prevent_default();
                                                    let key = e.key().to_string();
                                                    let m = e.modifiers();
                                                    if key == "Escape" {
                                                        capturing.set(None);
                                                        return;
                                                    }
                                                    let Some(spec) = spec_from_event(&key, m.ctrl() || m.meta()) else { return };
                                                    if is_reserved(&spec) {
                                                        refused.set(Some((id, tf("settings.player.shortcuts.reserved", &[("key", &display_spec(&spec))]))));
                                                        return;
                                                    }
                                                    refused.set(None);
                                                    capturing.set(None);
                                                    let current = ui.settings.peek().flow.shortcuts.clone();
                                                    save(assign(&current, &action, &spec));
                                                }
                                            },
                                        }
                                    } else {
                                        Kbd { keys: shown }
                                    }
                                    if clash {
                                        Pill { text: t("settings.player.shortcuts.conflict"), tone: Tone::Warn }
                                    }
                                    Button {
                                        label: if editing { t("common.cancel") } else { t("settings.player.shortcuts.change") },
                                        variant: ButtonVariant::Text,
                                        size: ButtonSize::Sm,
                                        onclick: move |_| {
                                            refused.set(None);
                                            capturing.set(if editing { None } else { Some(id) });
                                        },
                                    }
                                    if custom {
                                        IconButton {
                                            icon: "replay".to_string(),
                                            label: t("settings.player.shortcuts.default_one"),
                                            size: IconButtonSize::Sm,
                                            onclick: move |_| {
                                                let current = ui.settings.peek().flow.shortcuts.clone();
                                                save(unassign(&current, &reset_action));
                                            },
                                        }
                                    }
                                }
                                if let Some(text) = refusal {
                                    div { class: "tk-keymap__refused", role: "alert", "data-reserved": "true", "{text}" }
                                }
                            }
                        }
                    }
                }
            }
            div { class: "tk-keymap__foot",
                if conflict_count > 0 {
                    span { class: "tk-keymap__note", role: "status", {t("settings.player.shortcuts.conflict_hint")} }
                }
                Button {
                    label: t("settings.player.shortcuts.reset"),
                    icon: "replay".to_string(),
                    variant: ButtonVariant::Tonal,
                    size: ButtonSize::Sm,
                    disabled: !customized,
                    onclick: move |_| {
                        capturing.set(None);
                        refused.set(None);
                        save(ShortcutMapVm::default());
                    },
                }
            }
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::flow::{AUTO_NEXT_MAX_SECS, AUTO_NEXT_MIN_SECS};

    #[test]
    fn countdown_choices_are_within_the_saved_bounds_and_include_the_default() {
        assert!(AUTO_NEXT_CHOICES.iter().all(|s| (AUTO_NEXT_MIN_SECS..=AUTO_NEXT_MAX_SECS).contains(s)));
        assert!(AUTO_NEXT_CHOICES.contains(&PlayerFlowPrefs::default().auto_next_secs));
        assert!(AUTO_NEXT_CHOICES.windows(2).all(|w| w[0] < w[1]));
    }
}
