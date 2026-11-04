//! Torrent client > Settings: speed limits (KiB/s, 0 = unlimited), listen port + UPnP, max connections, DHT / PEX /
//! LSD, download folder (empty = the default folder, shown as placeholder), after watching (keep / delete) and
//! background downloads (disabled until the Downloads warning is accepted). One form with inline validation and a
//! save bar; `save_torrent_settings` persists and applies (limits live, the rest restarts the engine when idle).
//! Owner: wave 3 D.

use dioxus::prelude::*;

use crate::app::{Route, Services};
use crate::backend::{AfterWatchVm, TORRENT_MAX_LIMIT_KIB, TORRENT_MAX_PEERS, TorrentSettingsVm, UiResult};
use crate::components::{
    Button, ButtonSize, ButtonVariant, ErrorState, Segmented, SkeletonLines, Switch, TextField, use_snackbar,
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
}

impl Texts {
    fn of(s: &TorrentSettingsVm) -> Self {
        Self {
            download: s.download_limit_kib.to_string(),
            upload: s.upload_limit_kib.to_string(),
            port: s.listen_port.to_string(),
            peers: s.max_peers.to_string(),
            folder: s.download_dir.clone(),
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
    fn build_applies_texts_and_rejects_invalid_fields() {
        let base = TorrentSettingsVm { default_download_dir: "/home/ana/Downloads/Tatakai".into(), ..TorrentSettingsVm::default() };
        assert_eq!(build(&base, &Texts::of(&base)), Some(base.clone()));
        let texts = Texts { download: "100".into(), folder: "  /data  ".into(), ..Texts::of(&base) };
        let next = build(&base, &texts).unwrap();
        assert_eq!((next.download_limit_kib, next.download_dir.as_str()), (100, "/data"));
        assert_eq!(build(&base, &Texts { port: "22".into(), ..Texts::of(&base) }), None);
    }
}
