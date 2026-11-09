//! Torrent client > Settings: speed limits (KiB/s, 0 = unlimited), listen port + UPnP, max connections, DHT / PEX /
//! LSD, download folder (empty = the default folder, shown as placeholder), after watching (keep / delete) and
//! background downloads (disabled until the Downloads warning is accepted). One form with inline validation and a
//! save bar; `save_torrent_settings` persists and applies (limits live, the rest restarts the engine when idle).
//! Owner: wave 3 D.
//!
//! Wave A (worker i): custom trackers (one URL per line, `name="trackers"`), bandwidth schedule (`data-schedule`:
//! daily window with its own limits), torrent cache limit (`name="cache_limit_gib"`, 0 = none) and "preload the next
//! episode" (`data-preload-next`). All go through the same form and `save_torrent_settings`.

use dioxus::prelude::*;

use crate::app::{Route, Services};
use crate::backend::{AfterWatchVm, BandwidthScheduleVm, TORRENT_MAX_LIMIT_KIB, TORRENT_MAX_PEERS, TorrentSettingsVm, UiResult};
use crate::components::{
    Button, ButtonSize, ButtonVariant, ErrorState, Segmented, SkeletonLines, Switch, TextField, use_snackbar,
    use_uncontrolled_input,
};
use crate::i18n::{t, tf};
use crate::icons::Icon;
use crate::model::UiError;

/// Lowest non-zero listen port (no privileged ports), like the Link port.
const MIN_LISTEN_PORT: u16 = 1024;

/// Speed limit field: empty = 0 (unlimited), else `0..=TORRENT_MAX_LIMIT_KIB`.
fn parse_limit(text: &str) -> Option<u32> {
    let text = text.trim();
    if text.is_empty() {
        return Some(0);
    }
    text.parse::<u32>().ok().filter(|v| *v <= TORRENT_MAX_LIMIT_KIB)
}

/// Listen port field: empty / `0` = random, else `MIN_LISTEN_PORT..=65535`.
fn parse_listen_port(text: &str) -> Option<u16> {
    let text = text.trim();
    if text.is_empty() {
        return Some(0);
    }
    text.parse::<u16>().ok().filter(|p| *p == 0 || *p >= MIN_LISTEN_PORT)
}

/// Largest cache limit accepted (GiB).
const MAX_CACHE_GIB: u32 = 100_000;

/// Hour field: `0..=23`.
fn parse_hour(text: &str) -> Option<u8> {
    text.trim().parse::<u8>().ok().filter(|h| *h <= 23)
}

/// Cache limit field: empty = 0 (no limit), else `0..=MAX_CACHE_GIB`.
fn parse_cache(text: &str) -> Option<u32> {
    let text = text.trim();
    if text.is_empty() {
        return Some(0);
    }
    text.parse::<u32>().ok().filter(|v| *v <= MAX_CACHE_GIB)
}

/// Tracker list: one URL per line (commas and spaces also split); blank lines ignored, repeats dropped. `Err` with the
/// first line that is not a `udp://`, `http(s)://` or `wss://` URL.
fn parse_trackers(text: &str) -> Result<Vec<String>, String> {
    let mut out: Vec<String> = Vec::new();
    for line in text.split(['\n', ',', ' ', '\t']).map(str::trim).filter(|l| !l.is_empty()) {
        let lower = line.to_ascii_lowercase();
        let ok = ["udp://", "http://", "https://", "wss://"].iter().any(|p| lower.starts_with(p) && lower.len() > p.len());
        if !ok {
            return Err(line.to_owned());
        }
        if !out.iter().any(|o| o.eq_ignore_ascii_case(line)) {
            out.push(line.to_owned());
        }
    }
    Ok(out)
}

/// Max connections field: `1..=TORRENT_MAX_PEERS`.
fn parse_peers(text: &str) -> Option<u32> {
    text.trim().parse::<u32>().ok().filter(|v| (1..=TORRENT_MAX_PEERS).contains(v))
}

/// The text fields of the form, as typed.
#[derive(Debug, Clone, PartialEq, Default)]
struct Texts {
    download: String,
    upload: String,
    port: String,
    peers: String,
    folder: String,
    trackers: String,
    sched_start: String,
    sched_end: String,
    sched_down: String,
    sched_up: String,
    cache: String,
}

impl Texts {
    fn of(s: &TorrentSettingsVm) -> Self {
        Self {
            download: s.download_limit_kib.to_string(),
            upload: s.upload_limit_kib.to_string(),
            port: s.listen_port.to_string(),
            peers: s.max_peers.to_string(),
            folder: s.download_dir.clone(),
            trackers: s.trackers.join("\n"),
            sched_start: s.schedule.start_hour.to_string(),
            sched_end: s.schedule.end_hour.to_string(),
            sched_down: s.schedule.download_limit_kib.to_string(),
            sched_up: s.schedule.upload_limit_kib.to_string(),
            cache: s.cache_limit_gib.to_string(),
        }
    }
}

/// `edited` (switches / segments) with the text fields applied; `None` while a field is invalid.
fn build(edited: &TorrentSettingsVm, texts: &Texts) -> Option<TorrentSettingsVm> {
    Some(TorrentSettingsVm {
        download_limit_kib: parse_limit(&texts.download)?,
        upload_limit_kib: parse_limit(&texts.upload)?,
        listen_port: parse_listen_port(&texts.port)?,
        max_peers: parse_peers(&texts.peers)?,
        download_dir: texts.folder.trim().to_owned(),
        trackers: parse_trackers(&texts.trackers).ok()?,
        schedule: BandwidthScheduleVm {
            enabled: edited.schedule.enabled,
            start_hour: parse_hour(&texts.sched_start)?,
            end_hour: parse_hour(&texts.sched_end)?,
            download_limit_kib: parse_limit(&texts.sched_down)?,
            upload_limit_kib: parse_limit(&texts.sched_up)?,
        },
        cache_limit_gib: parse_cache(&texts.cache)?,
        ..edited.clone()
    })
}

#[component]
pub fn TorrentSettingsPage() -> Element {
    let services = use_context::<Services>();
    let navigator = use_navigator();
    let snack = use_snackbar();
    let backend = services.backend.clone();

    // What the backend has, what the switches / segments say, and what the text fields say.
    let mut loaded = use_signal(|| None::<UiResult<TorrentSettingsVm>>);
    let mut edited = use_signal(TorrentSettingsVm::default);
    let mut texts = use_signal(Texts::default);
    let mut saving = use_signal(|| false);
    let mut save_error = use_signal(|| None::<UiError>);

    let reload = use_callback({
        let backend = backend.clone();
        move |_: ()| {
            let backend = backend.clone();
            spawn(async move {
                let r = backend.torrent_settings().await;
                if let Ok(s) = &r {
                    edited.set(s.clone());
                    texts.set(Texts::of(s));
                }
                loaded.set(Some(r));
            });
        }
    });
    use_hook(|| reload.call(()));

    let save = use_callback({
        let backend = backend.clone();
        move |_: ()| {
            if saving() {
                return;
            }
            let Some(next) = build(&edited(), &texts()) else { return };
            saving.set(true);
            save_error.set(None);
            let backend = backend.clone();
            spawn(async move {
                match backend.save_torrent_settings(&next).await {
                    Ok(()) => {
                        snack.show(t("torrent.settings.saved"));
                        let r = backend.torrent_settings().await;
                        if let Ok(s) = &r {
                            edited.set(s.clone());
                            texts.set(Texts::of(s));
                        }
                        loaded.set(Some(r));
                    }
                    Err(e) => save_error.set(Some(e)),
                }
                saving.set(false);
            });
        }
    });

    let base = match &*loaded.read() {
        None => {
            return rsx! {
                section { class: "tk-tset", "aria-busy": "true",
                    SkeletonLines { widths: vec![60, 90, 80, 90, 70, 85] }
                }
            };
        }
        Some(Err(e)) => {
            let e = e.clone();
            return rsx! {
                section { class: "tk-tset",
                    ErrorState { error: e, title: Some(t("torrent.settings.error")), on_retry: move |_| reload.call(()) }
                }
            };
        }
        Some(Ok(s)) => s.clone(),
    };

    let s = edited();
    let tx = texts();
    let next = build(&s, &tx);
    let dirty = next.as_ref() != Some(&base);
    let invalid = next.is_none();
    let bad_down = parse_limit(&tx.download).is_none();
    let bad_up = parse_limit(&tx.upload).is_none();
    let bad_port = parse_listen_port(&tx.port).is_none();
    let bad_peers = parse_peers(&tx.peers).is_none();
    let bad_trackers = parse_trackers(&tx.trackers).err();
    let bad_start = parse_hour(&tx.sched_start).is_none();
    let bad_end = parse_hour(&tx.sched_end).is_none();
    let bad_sdown = parse_limit(&tx.sched_down).is_none();
    let bad_sup = parse_limit(&tx.sched_up).is_none();
    let bad_cache = parse_cache(&tx.cache).is_none();
    let max_cache = MAX_CACHE_GIB.to_string();
    let trackers_hint = match &bad_trackers {
        Some(line) => tf("torrent.settings.trackers_invalid", &[("line", line)]),
        None => t("torrent.settings.trackers_hint"),
    };
    let max_limit = TORRENT_MAX_LIMIT_KIB.to_string();
    let max_peers = TORRENT_MAX_PEERS.to_string();
    let after_watch: Vec<(String, String)> = AfterWatchVm::ALL
        .iter()
        .map(|a| (a.id().to_owned(), t(&format!("torrent.settings.after_watch.{}", a.id()))))
        .collect();
    let folder_hint = tf("torrent.settings.folder_hint", &[("dir", &s.default_download_dir)]);

    rsx! {
        section { class: "tk-tset", "aria-label": t("torrent.section.settings"),
            div { class: "tk-tlib__head",
                h2 { class: "tk-tlib__title", {t("torrent.section.settings")} }
                p { class: "tk-tlib__intro", {t("torrent.settings.intro")} }
            }

            SetPanel { icon: "speed", title: t("torrent.settings.speed"),
                SetRow { label: t("torrent.settings.download_limit"), hint: t("torrent.settings.limit_hint"),
                    TextField {
                        label: t("torrent.settings.kib"),
                        value: tx.download.clone(),
                        mono: true,
                        error: bad_down,
                        hint: if bad_down { tf("torrent.settings.limit_invalid", &[("max", &max_limit)]) } else { String::new() },
                        oninput: move |v: String| texts.with_mut(|x| x.download = v),
                        onsubmit: move |_| save.call(()),
                    }
                }
                SetRow { label: t("torrent.settings.upload_limit"), hint: t("torrent.settings.limit_hint"),
                    TextField {
                        label: t("torrent.settings.kib"),
                        value: tx.upload.clone(),
                        mono: true,
                        error: bad_up,
                        hint: if bad_up { tf("torrent.settings.limit_invalid", &[("max", &max_limit)]) } else { String::new() },
                        oninput: move |v: String| texts.with_mut(|x| x.upload = v),
                        onsubmit: move |_| save.call(()),
                    }
                }
            }

            SetPanel { icon: "link", title: t("torrent.settings.connection"),
                SetRow { label: t("torrent.settings.port"), hint: t("torrent.settings.port_hint"),
                    TextField {
                        label: t("torrent.settings.port_label"),
                        value: tx.port.clone(),
                        mono: true,
                        error: bad_port,
                        hint: if bad_port { t("torrent.settings.port_invalid") } else { String::new() },
                        oninput: move |v: String| texts.with_mut(|x| x.port = v),
                        onsubmit: move |_| save.call(()),
                    }
                }
                SetRow { label: t("torrent.settings.upnp"), hint: t("torrent.settings.upnp_hint"),
                    Switch { checked: s.upnp, label: t("torrent.settings.upnp"), on_toggle: move |v: bool| edited.with_mut(|x| x.upnp = v) }
                }
                SetRow { label: t("torrent.settings.max_peers"), hint: t("torrent.settings.max_peers_hint"),
                    TextField {
                        label: t("torrent.settings.peers_label"),
                        value: tx.peers.clone(),
                        mono: true,
                        error: bad_peers,
                        hint: if bad_peers { tf("torrent.settings.max_peers_invalid", &[("max", &max_peers)]) } else { String::new() },
                        oninput: move |v: String| texts.with_mut(|x| x.peers = v),
                        onsubmit: move |_| save.call(()),
                    }
                }
            }

            SetPanel { icon: "cast", title: t("torrent.settings.discovery"),
                SetRow { label: t("torrent.settings.dht"), hint: t("torrent.settings.dht_hint"),
                    Switch { checked: s.dht, label: t("torrent.settings.dht"), on_toggle: move |v: bool| edited.with_mut(|x| x.dht = v) }
                }
                SetRow { label: t("torrent.settings.pex"), hint: t("torrent.settings.pex_hint"),
                    Switch { checked: s.pex, label: t("torrent.settings.pex"), on_toggle: move |v: bool| edited.with_mut(|x| x.pex = v) }
                }
                SetRow { label: t("torrent.settings.lsd"), hint: t("torrent.settings.lsd_hint"),
                    Switch { checked: s.lsd, label: t("torrent.settings.lsd"), on_toggle: move |v: bool| edited.with_mut(|x| x.lsd = v) }
                }
            }

            SetPanel { icon: "storage", title: t("torrent.settings.storage"),
                SetRow { label: t("torrent.settings.folder"), hint: folder_hint, stack: true,
                    TextField {
                        label: t("torrent.settings.folder_label"),
                        value: tx.folder.clone(),
                        placeholder: s.default_download_dir.clone(),
                        icon: "folder".to_string(),
                        mono: true,
                        oninput: move |v: String| texts.with_mut(|x| x.folder = v),
                        onsubmit: move |_| save.call(()),
                    }
                }
                SetRow { label: t("torrent.settings.after_watch"), hint: t("torrent.settings.after_watch_hint"),
                    Segmented {
                        options: after_watch,
                        selected: s.after_watch.id().to_owned(),
                        label: t("torrent.settings.after_watch"),
                        on_select: move |id: String| {
                            if let Some(a) = AfterWatchVm::ALL.into_iter().find(|a| a.id() == id) {
                                edited.with_mut(|x| x.after_watch = a);
                            }
                        },
                    }
                }
            }

            SetPanel { icon: "cast", title: t("torrent.settings.trackers"),
                SetRow { label: t("torrent.settings.trackers_label"), hint: trackers_hint, stack: true,
                    NamedTextArea {
                        name: "trackers",
                        label: t("torrent.settings.trackers_label"),
                        value: tx.trackers.clone(),
                        placeholder: "udp://tracker.example.org:1337/announce".to_string(),
                        error: bad_trackers.is_some(),
                        oninput: move |v: String| texts.with_mut(|x| x.trackers = v),
                    }
                }
            }

            SetPanel { icon: "clock", title: t("torrent.settings.schedule"),
                div { class: "tk-tset__schedule", "data-schedule": "{s.schedule.enabled}",
                    SetRow { label: t("torrent.settings.schedule_switch"), hint: t("torrent.settings.schedule_hint"),
                        Switch {
                            checked: s.schedule.enabled,
                            label: t("torrent.settings.schedule_switch"),
                            on_toggle: move |v: bool| edited.with_mut(|x| x.schedule.enabled = v),
                        }
                    }
                    SetRow { label: t("torrent.settings.schedule_window"), hint: t("torrent.settings.schedule_window_hint"),
                        div { class: "tk-tset__pair",
                            NamedField {
                                name: "schedule_start_hour",
                                label: t("torrent.settings.schedule_from"),
                                value: tx.sched_start.clone(),
                                error: bad_start,
                                disabled: !s.schedule.enabled,
                                oninput: move |v: String| texts.with_mut(|x| x.sched_start = v),
                            }
                            NamedField {
                                name: "schedule_end_hour",
                                label: t("torrent.settings.schedule_to"),
                                value: tx.sched_end.clone(),
                                error: bad_end,
                                disabled: !s.schedule.enabled,
                                oninput: move |v: String| texts.with_mut(|x| x.sched_end = v),
                            }
                        }
                    }
                    SetRow { label: t("torrent.settings.schedule_limits"), hint: t("torrent.settings.limit_hint"),
                        div { class: "tk-tset__pair",
                            NamedField {
                                name: "schedule_download_kib",
                                label: t("torrent.settings.schedule_down"),
                                value: tx.sched_down.clone(),
                                error: bad_sdown,
                                disabled: !s.schedule.enabled,
                                oninput: move |v: String| texts.with_mut(|x| x.sched_down = v),
                            }
                            NamedField {
                                name: "schedule_upload_kib",
                                label: t("torrent.settings.schedule_up"),
                                value: tx.sched_up.clone(),
                                error: bad_sup,
                                disabled: !s.schedule.enabled,
                                oninput: move |v: String| texts.with_mut(|x| x.sched_up = v),
                            }
                        }
                    }
                    if bad_start || bad_end || bad_sdown || bad_sup {
                        p { class: "tk-tset__error", role: "alert", {tf("torrent.settings.schedule_invalid", &[("max", &max_limit)])} }
                    }
                }
            }

            SetPanel { icon: "storage", title: t("torrent.settings.cache"),
                SetRow { label: t("torrent.settings.cache_limit"), hint: t("torrent.settings.cache_hint"),
                    NamedField {
                        name: "cache_limit_gib",
                        label: t("torrent.settings.gib"),
                        value: tx.cache.clone(),
                        error: bad_cache,
                        hint: if bad_cache { tf("torrent.settings.cache_invalid", &[("max", &max_cache)]) } else { String::new() },
                        oninput: move |v: String| texts.with_mut(|x| x.cache = v),
                    }
                }
                div { "data-preload-next": "{s.preload_next}",
                    SetRow { label: t("torrent.settings.preload_next"), hint: t("torrent.settings.preload_next_hint"),
                        Switch {
                            checked: s.preload_next,
                            label: t("torrent.settings.preload_next"),
                            on_toggle: move |v: bool| edited.with_mut(|x| x.preload_next = v),
                        }
                    }
                }
            }

            SetPanel { icon: "download", title: t("torrent.settings.background"),
                SetRow {
                    label: t("torrent.settings.background_switch"),
                    hint: t(if s.background_ack { "torrent.settings.background_hint" } else { "torrent.settings.background_locked" }),
                    if !s.background_ack {
                        Button {
                            label: t("torrent.settings.background_review"),
                            variant: ButtonVariant::Text,
                            size: ButtonSize::Sm,
                            onclick: move |_| {
                                navigator.push(Route::TorrentDownloadsPage {});
                            },
                        }
                    }
                    Switch {
                        checked: s.background_downloads,
                        label: t("torrent.settings.background_switch"),
                        disabled: !s.background_ack,
                        on_toggle: move |v: bool| edited.with_mut(|x| x.background_downloads = v),
                    }
                }
            }

            if let Some(e) = save_error() {
                ErrorState { error: e, compact: true, title: Some(t("torrent.settings.save_failed")), on_retry: move |_| save.call(()) }
            }

            div { class: "tk-tset__bar", "data-dirty": "{dirty}", role: "region", "aria-label": t("torrent.settings.save_bar"),
                span { class: "tk-tset__status",
                    if invalid {
                        Icon { name: "warning".to_string(), size: 18 }
                        {t("torrent.settings.fix_fields")}
                    } else if dirty {
                        {t("torrent.settings.unsaved")}
                    } else {
                        {t("torrent.settings.all_saved")}
                    }
                }
                div { class: "tk-tset__bar-actions",
                    Button {
                        label: t("torrent.settings.discard"),
                        variant: ButtonVariant::Text,
                        size: ButtonSize::Sm,
                        disabled: !dirty || saving(),
                        onclick: {
                            let base = base.clone();
                            move |_| {
                                edited.set(base.clone());
                                texts.set(Texts::of(&base));
                                save_error.set(None);
                            }
                        },
                    }
                    Button {
                        label: t("common.save"),
                        icon: "check".to_string(),
                        size: ButtonSize::Sm,
                        disabled: !dirty || invalid || saving(),
                        onclick: move |_| save.call(()),
                    }
                }
            }
        }
    }
}

/// A one-line number field with a `name` attribute (the acceptance checks address the fields by name).
#[component]
fn NamedField(
    name: &'static str,
    label: String,
    value: String,
    #[props(default)] error: bool,
    #[props(default)] disabled: bool,
    #[props(default)] hint: String,
    #[props(default)] oninput: EventHandler<String>,
) -> Element {
    let (generation, record) = use_uncontrolled_input(&value);
    rsx! {
        label { class: "tk-field", "data-error": "{error}", "data-mono": "true",
            span { class: "tk-field__label", "{label}" }
            span { class: "tk-field__box",
                {std::iter::once(generation).map(|generation| {
                    let record = record.clone();
                    rsx! {
                        input {
                            key: "{generation}",
                            class: "tk-field__input",
                            r#type: "text",
                            inputmode: "numeric",
                            name: "{name}",
                            initial_value: "{value}",
                            autocomplete: "off",
                            spellcheck: "false",
                            disabled,
                            "aria-invalid": "{error}",
                            oninput: move |e| {
                                record(&e.value());
                                oninput.call(e.value())
                            },
                        }
                    }
                })}
            }
            if !hint.is_empty() {
                span { class: "tk-field__hint", "{hint}" }
            }
        }
    }
}

/// Multi-line text field with a `name` attribute (tracker list).
#[component]
fn NamedTextArea(
    name: &'static str,
    label: String,
    value: String,
    #[props(default)] placeholder: String,
    #[props(default)] error: bool,
    #[props(default)] oninput: EventHandler<String>,
) -> Element {
    let (generation, record) = use_uncontrolled_input(&value);
    rsx! {
        label { class: "tk-field tk-field--area", "data-error": "{error}", "data-mono": "true",
            span { class: "tk-field__label", "{label}" }
            span { class: "tk-field__box",
                {std::iter::once(generation).map(|generation| {
                    let record = record.clone();
                    rsx! {
                        textarea {
                            key: "{generation}",
                            class: "tk-field__input tk-tset__textarea",
                            name: "{name}",
                            rows: "4",
                            initial_value: "{value}",
                            placeholder: "{placeholder}",
                            spellcheck: "false",
                            "aria-invalid": "{error}",
                            oninput: move |e| {
                                record(&e.value());
                                oninput.call(e.value())
                            },
                        }
                    }
                })}
            }
        }
    }
}

/// One group of settings (same look as the app Settings panels).
#[component]
fn SetPanel(icon: &'static str, title: String, children: Element) -> Element {
    rsx! {
        div { class: "tk-panel tk-tset__panel",
            h3 { class: "tk-panel__title",
                Icon { name: icon.to_string(), size: 22 }
                "{title}"
            }
            {children}
        }
    }
}

/// Label + hint on the left, control on the right (`stack` puts the control below).
#[component]
fn SetRow(label: String, #[props(default)] hint: String, #[props(default)] stack: bool, children: Element) -> Element {
    rsx! {
        div { class: "tk-setting-row tk-tset__row", "data-stack": "{stack}",
            div { class: "tk-setting-row__text",
                div { class: "tk-setting-row__label", "{label}" }
                if !hint.is_empty() {
                    div { class: "tk-setting-row__hint", "{hint}" }
                }
            }
            div { class: "tk-setting-row__ctl", {children} }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn field_parsing() {
        assert_eq!(parse_limit(""), Some(0));
        assert_eq!(parse_limit(" 2048 "), Some(2048));
        assert_eq!(parse_limit("-1"), None);
        assert_eq!(parse_limit(&(TORRENT_MAX_LIMIT_KIB + 1).to_string()), None);
        assert_eq!(parse_listen_port(""), Some(0));
        assert_eq!(parse_listen_port("0"), Some(0));
        assert_eq!(parse_listen_port("51413"), Some(51413));
        assert_eq!(parse_listen_port("80"), None);
        assert_eq!(parse_listen_port("70000"), None);
        assert_eq!(parse_peers("0"), None);
        assert_eq!(parse_peers("50"), Some(50));
        assert_eq!(parse_peers(&(TORRENT_MAX_PEERS + 1).to_string()), None);
    }

    #[test]
    fn wave_a_fields() {
        assert_eq!(parse_hour("23"), Some(23));
        assert_eq!(parse_hour("24"), None);
        assert_eq!(parse_cache(""), Some(0));
        assert_eq!(parse_cache("50"), Some(50));
        assert_eq!(parse_cache("x"), None);
        assert_eq!(
            parse_trackers("udp://a:1/announce\n\n  https://b/announce, UDP://A:1/announce"),
            Ok(vec!["udp://a:1/announce".to_owned(), "https://b/announce".to_owned()])
        );
        assert_eq!(parse_trackers("ftp://nope"), Err("ftp://nope".to_owned()));
        assert_eq!(parse_trackers(""), Ok(Vec::new()));
        let base = TorrentSettingsVm::default();
        let texts = Texts { trackers: "udp://t:1/a".into(), cache: "20".into(), sched_start: "22".into(), ..Texts::of(&base) };
        let next = build(&base, &texts).unwrap();
        assert_eq!((next.trackers.len(), next.cache_limit_gib, next.schedule.start_hour), (1, 20, 22));
        assert_eq!(build(&base, &Texts { sched_end: "25".into(), ..Texts::of(&base) }), None);
    }

    #[test]
    fn build_applies_texts_and_rejects_invalid_fields() {
        let base = TorrentSettingsVm { default_download_dir: "/home/ana/Downloads/Tatakai".into(), ..TorrentSettingsVm::default() };
        assert_eq!(build(&base, &Texts::of(&base)), Some(base.clone()));
        let texts = Texts { download: "100".into(), folder: "  /data  ".into(), ..Texts::of(&base) };
        let next = build(&base, &texts).unwrap();
        assert_eq!((next.download_limit_kib, next.download_dir.as_str()), (100, "/data"));
        assert_eq!(build(&base, &Texts { port: "22".into(), ..Texts::of(&base) }), None);
    }
}
