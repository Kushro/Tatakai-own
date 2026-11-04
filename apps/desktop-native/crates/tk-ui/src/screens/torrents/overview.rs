//! Torrent client > Overview: name, state pill, info-hash (copyable), progress (`WavyProgress`), downloaded /
//! uploaded / size / pieces, speeds, remaining or "Streaming" / elapsed, seeders / leechers / wires / ratio,
//! protocol pills (`torrent.proto.*`) and the actions pause / resume / open folder / remove (confirmation with
//! "also delete the files"). `torrent_stats` is polled every second while the page is mounted. Owner: wave 3 C.

use dioxus::prelude::*;

use super::layout::common::{
    POLL_FAST, StatePill, TorrentPicker, format_bytes, format_duration, format_percent, format_speed, ratio,
    use_torrent_poll,
};
use crate::app::{Route, Services, Ui};
use crate::backend::{TorrentProtocolVm, TorrentStateVm, TorrentStatsVm};
use crate::components::{
    Banner, Button, ButtonVariant, ConfirmDialog, ErrorState, IconButton, IconButtonSize, Pill, SkeletonLines, Switch,
    Tone, WavyProgress, use_snackbar,
};
use crate::i18n::{t, tf};
use crate::icons::Icon;
use crate::model::{ErrorKind, UiError};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action {
    Pause,
    Resume,
    OpenFolder,
    Remove,
}

/// Protocol pills in display order: (`torrent.proto.<id>`, active).
fn protocol_pills(p: TorrentProtocolVm) -> [(&'static str, bool); 7] {
    [
        ("dht", p.dht),
        ("lsd", p.lsd),
        ("pex", p.pex),
        ("nat", p.nat),
        ("forwarding", p.forwarding),
        ("persisting", p.persisting),
        ("streaming", p.streaming),
    ]
}

/// "Remaining" cell: Streaming, complete, paused, the ETA or unknown.
fn remaining_text(s: &TorrentStatsVm) -> String {
    if s.streaming {
        t("torrent.overview.streaming")
    } else if s.progress >= 1.0 {
        t("torrent.overview.complete")
    } else if s.state == TorrentStateVm::Paused {
        t("torrent.state.paused")
    } else {
        s.eta_secs.map(format_duration).unwrap_or_else(|| t("torrent.overview.eta_unknown"))
    }
}

#[component]
pub fn TorrentOverviewPage() -> Element {
    let services = use_context::<Services>();
    let backend = services.backend.clone();
    let ui = use_context::<Ui>();
    let navigator = use_navigator();
    let snack = use_snackbar();
    let stats = use_torrent_poll(POLL_FAST, |b, id| async move { b.torrent_stats(&id).await });
    let mut busy = use_signal(|| None::<Action>);
    let mut action_error = use_signal(|| None::<UiError>);
    let mut confirm_remove = use_signal(|| false);
    let mut delete_files = use_signal(|| false);
    let mut selected_sig = ui.torrent;

    let run = use_callback({
        let backend = backend.clone();
        move |action: Action| {
            let Some(id) = ui.torrent.peek().clone() else { return };
            if busy.peek().is_some() {
                return;
            }
            busy.set(Some(action));
            action_error.set(None);
            let delete = *delete_files.peek();
            let backend = backend.clone();
            let mut selected = ui.torrent;
            spawn(async move {
                let result = match action {
                    Action::Pause => backend.torrent_pause(&id).await,
                    Action::Resume => backend.torrent_resume(&id).await,
                    Action::OpenFolder => backend.torrent_open_folder(&id).await,
                    Action::Remove => backend.torrent_remove(&id, delete).await,
                };
                busy.set(None);
                match result {
                    Ok(()) if action == Action::Remove => {
                        snack.show(t("torrent.overview.removed"));
                        if selected.peek().as_deref() == Some(id.as_str()) {
                            selected.set(None);
                        }
                        navigator.push(Route::TorrentLibraryPage {});
                    }
                    Ok(()) => stats.refresh(),
                    Err(e) => action_error.set(Some(e)),
                }
            });
        }
    });

    let selected = ui.torrent.read().clone();
    let Some(id) = selected else {
        return rsx! {
            TorrentPicker {}
        };
    };

    let body = match stats.for_id(&id) {
        None => rsx! {
            section { class: "tk-panel", "aria-busy": "true",
                SkeletonLines { widths: vec![55, 85, 40] }
            }
            section { class: "tk-panel", "aria-busy": "true",
                SkeletonLines { widths: vec![70, 60, 65, 50] }
            }
        },
        Some(Err(e)) => {
            let missing = e.kind == ErrorKind::NoSource;
            let cause = if missing { t("torrent.shell.missing") } else { String::new() };
            rsx! {
                ErrorState {
                    error: e.clone(),
                    title: Some(t("torrent.overview.error")),
                    cause: cause,
                    retry: e.kind != ErrorKind::NotWired,
                    on_retry: move |_| stats.refresh(),
                    if missing {
                        Button {
                            label: t("torrent.shell.choose"),
                            variant: ButtonVariant::Text,
                            onclick: move |_| selected_sig.set(None),
                        }
                    }
                }
            }
        }
        Some(Ok(s)) => {
            let busy_now = busy();
            let paused = s.state == TorrentStateVm::Paused;
            let complete = s.progress >= 1.0;
            let active = s.state == TorrentStateVm::Downloading && !complete;
            let pct = format_percent(s.progress);
            let down = format_speed(s.down_bps);
            let up = format_speed(s.up_bps);
            let size = if s.total_bytes > 0 && s.total_bytes < s.size_bytes {
                tf(
                    "torrent.overview.size_of",
                    &[("selected", &format_bytes(s.total_bytes)), ("total", &format_bytes(s.size_bytes))],
                )
            } else {
                format_bytes(s.size_bytes)
            };
            let pieces = tf(
                "torrent.overview.pieces_value",
                &[
                    ("have", &s.pieces_have.to_string()),
                    ("total", &s.pieces.to_string()),
                    ("size", &format_bytes(s.piece_bytes)),
                ],
            );
            let ratio_text = ratio(s.uploaded_bytes, s.downloaded_bytes).unwrap_or_else(|| t("torrent.fmt.none"));
            let hash = s.info_hash.clone();
            let (on_label, off_label) = (t("torrent.overview.proto_on"), t("torrent.overview.proto_off"));
            rsx! {
                section { class: "tk-panel tk-tov__hero",
                    div { class: "tk-tov__top",
                        div { class: "tk-tov__title",
                            h2 { class: "tk-tov__name tk-selectable", "{s.name}" }
                            StatePill { state: s.state }
                        }
                        div { class: "tk-tov__actions",
                            if paused {
                                Button {
                                    label: t("torrent.overview.resume"),
                                    icon: "play".to_string(),
                                    variant: ButtonVariant::Tonal,
                                    disabled: busy_now.is_some(),
                                    onclick: move |_| run.call(Action::Resume),
                                }
                            } else {
                                Button {
                                    label: t("torrent.overview.pause"),
                                    icon: "pause".to_string(),
                                    variant: ButtonVariant::Tonal,
                                    disabled: busy_now.is_some() || s.state == TorrentStateVm::Error,
                                    onclick: move |_| run.call(Action::Pause),
                                }
                            }
                            Button {
                                label: t("torrent.overview.open_folder"),
                                icon: "folder".to_string(),
                                variant: ButtonVariant::Outlined,
                                disabled: busy_now.is_some(),
                                onclick: move |_| run.call(Action::OpenFolder),
                            }
                            Button {
                                label: t("torrent.overview.remove"),
                                icon: "trash".to_string(),
                                variant: ButtonVariant::Text,
                                class: "tk-tov__remove".to_string(),
                                disabled: busy_now.is_some(),
                                onclick: move |_| {
                                    delete_files.set(false);
                                    confirm_remove.set(true);
                                },
                            }
                        }
                    }
                    div { class: "tk-tov__hash",
                        span { class: "tk-tov__label", {t("torrent.overview.info_hash")} }
                        code { class: "tk-tov__hashval tk-selectable", "{s.info_hash}" }
                        IconButton {
                            icon: "copy".to_string(),
                            label: t("torrent.overview.copy_hash"),
                            size: IconButtonSize::Sm,
                            onclick: move |_| {
                                let arg = serde_json::to_string(&hash).unwrap_or_else(|_| "\"\"".into());
                                let _ = document::eval(&format!("navigator.clipboard&&navigator.clipboard.writeText({arg});"));
                                snack.show(t("common.copied"));
                            },
                        }
                    }
                    if let Some(err) = s.error.clone() {
                        Banner { text: err, icon: "warning".to_string(), tone: Tone::Err }
                    }
                    div { class: "tk-tov__progress",
                        div { class: "tk-tov__progress-head",
                            span { class: "tk-tov__pct tk-num", "{pct}" }
                            span { class: "tk-tov__speeds tk-num",
                                span { class: "tk-tov__speed", title: t("torrent.overview.down_speed"),
                                    Icon { name: "download".to_string(), size: 18 }
                                    "{down}"
                                }
                                span { class: "tk-tov__speed", title: t("torrent.overview.up_speed"),
                                    Icon { name: "upload".to_string(), size: 18 }
                                    "{up}"
                                }
                            }
                        }
                        WavyProgress { fraction: s.progress, label: t("torrent.overview.progress"), active: active }
                    }
                }

                section { class: "tk-panel",
                    h2 { class: "tk-panel__title",
                        Icon { name: "storage".to_string(), size: 24 }
                        {t("torrent.overview.transfer")}
                    }
                    dl { class: "tk-tov__stats",
                        Stat { label: t("torrent.overview.downloaded"), value: format_bytes(s.downloaded_bytes) }
                        Stat { label: t("torrent.overview.uploaded"), value: format_bytes(s.uploaded_bytes) }
                        Stat { label: t("torrent.overview.size"), value: size }
                        Stat { label: t("torrent.overview.pieces"), value: pieces }
                        Stat { label: t("torrent.overview.down_speed"), value: down.clone() }
                        Stat { label: t("torrent.overview.up_speed"), value: up.clone() }
                        Stat { label: t("torrent.overview.remaining"), value: remaining_text(&s) }
                        Stat { label: t("torrent.overview.elapsed"), value: format_duration(s.elapsed_secs) }
                    }
                }

                section { class: "tk-panel",
                    h2 { class: "tk-panel__title",
                        Icon { name: "user".to_string(), size: 24 }
                        {t("torrent.overview.swarm")}
                    }
                    dl { class: "tk-tov__stats",
                        Stat { label: t("torrent.overview.seeders"), value: s.seeders.to_string() }
                        Stat { label: t("torrent.overview.leechers"), value: s.leechers.to_string() }
                        Stat { label: t("torrent.overview.wires"), value: s.peers.to_string() }
                        Stat { label: t("torrent.overview.ratio"), value: ratio_text }
                    }
                }

                section { class: "tk-panel",
                    h2 { class: "tk-panel__title",
                        Icon { name: "cast".to_string(), size: 24 }
                        {t("torrent.overview.protocol")}
                    }
                    ul { class: "tk-tov__proto", "aria-label": t("torrent.overview.protocol"),
                        for (key , on) in protocol_pills(s.protocol) {
                            {
                                let hint = if on { on_label.clone() } else { off_label.clone() };
                                let tone = if on { Tone::Ok } else { Tone::Neutral };
                                rsx! {
                                    li { key: "{key}", "data-on": "{on}", title: "{hint}",
                                        Pill { text: t(&format!("torrent.proto.{key}")), tone: tone, plain: !on }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    };

    rsx! {
        {body}
        if let Some(e) = action_error() {
            ErrorState {
                error: e.clone(),
                title: Some(t("torrent.overview.action_error")),
                compact: true,
                retry: false,
            }
        }
        if confirm_remove() {
            ConfirmDialog {
                title: t("torrent.overview.remove_title"),
                body: t("torrent.overview.remove_body"),
                confirm_label: t("torrent.overview.remove"),
                icon: "trash".to_string(),
                on_cancel: move |_| confirm_remove.set(false),
                on_confirm: move |_| {
                    confirm_remove.set(false);
                    run.call(Action::Remove);
                },
                div { class: "tk-tov__delete",
                    span { {t("torrent.overview.delete_files")} }
                    Switch {
                        checked: delete_files(),
                        label: t("torrent.overview.delete_files"),
                        on_toggle: move |v: bool| delete_files.set(v),
                    }
                }
            }
        }
    }
}

/// One figure of a stats list (`dt` label, `dd` value in tabular numerals).
#[component]
fn Stat(label: String, value: String) -> Element {
    rsx! {
        div { class: "tk-tov__stat",
            dt { class: "tk-tov__label", "{label}" }
            dd { class: "tk-tov__value tk-num", "{value}" }
        }
    }
}
