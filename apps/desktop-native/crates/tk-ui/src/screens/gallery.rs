//! Component gallery (debug builds only): every Tatami Expressive component with its states, so the
//! wave 1-2 screens are built from parts that were checked once. Release builds render nothing.

use dioxus::prelude::*;

use crate::components::{
    Banner, Button, ButtonSize, ButtonVariant, Chip, ChipKind, ConfirmDialog, EmptyState, ErrorState, FloatingToolbar,
    IconButton, IconButtonSize, IconButtonVariant, InfoChip, Kbd, LoadingIndicator, Menu, MenuItem, MenuSeparator, Pill,
    Segmented, Sheet, SkeletonLines, SkeletonRow, SplitButton, Switch, TextField, Tone, ToolbarSeparator, WavyProgress,
    use_snackbar,
};
use crate::i18n::t;
use crate::model::{ErrorKind, UiError};

#[component]
pub fn GalleryPage() -> Element {
    if !cfg!(debug_assertions) {
        return rsx! {};
    }
    rsx! { Gallery {} }
}

#[component]
fn Gallery() -> Element {
    let snack = use_snackbar();
    let mut seg = use_signal(|| "web".to_string());
    let mut sw = use_signal(|| true);
    let mut text = use_signal(String::new);
    let mut chip_on = use_signal(|| true);
    let mut menu_open = use_signal(|| false);
    let mut split_open = use_signal(|| false);
    let mut speed = use_signal(|| "1".to_string());
    let mut dialog = use_signal(|| false);
    let mut sheet = use_signal(|| false);
    let mut fraction = use_signal(|| 0.42_f64);

    let seg_opts = vec![
        ("all".to_string(), t("src.filter.all")),
        ("web".to_string(), t("src.web")),
        ("torrent".to_string(), t("src.torrent")),
    ];
    let port_err = UiError::new(ErrorKind::Other, "io: os error 10048 (address already in use) 0.0.0.0:47321");
    let invalid = !text().is_empty() && !text().starts_with("tatakai-link:");

    rsx! {
        div { class: "tk-page tk-page--gallery",
            div { class: "tk-page__head",
                div {
                    h1 { class: "tk-page__title", {t("gallery.title")} }
                    p { class: "tk-page__intro", {t("gallery.intro")} }
                }
            }
            div { class: "tk-gallery__grid",
                section { class: "tk-panel",
                    h2 { class: "tk-panel__title", "Button" }
                    div { class: "tk-panel__row",
                        for v in [ButtonVariant::Filled, ButtonVariant::Tonal, ButtonVariant::Elevated, ButtonVariant::Outlined, ButtonVariant::Text, ButtonVariant::Danger] {
                            Button { key: "{v.css()}", label: v.css().to_string(), variant: v }
                        }
                    }
                    div { class: "tk-panel__row",
                        for s in [ButtonSize::Xs, ButtonSize::Sm, ButtonSize::Md, ButtonSize::Lg, ButtonSize::Xl] {
                            Button { key: "{s.css()}", label: s.css().to_uppercase(), size: s, icon: "play".to_string() }
                        }
                    }
                    div { class: "tk-panel__row",
                        Menu {
                            open: split_open(),
                            label: t("details.choose_source"),
                            align: crate::components::MenuAlign::Start,
                            on_close: move |_| split_open.set(false),
                            trigger: rsx! {
                                SplitButton {
                                    label: t("details.watch"),
                                    menu_label: t("details.choose_source"),
                                    icon: "play".to_string(),
                                    expanded: split_open(),
                                    onclick: move |_| snack.show(t("details.watch")),
                                    on_menu: move |_| split_open.toggle(),
                                }
                            },
                            MenuItem { label: "Toko · Alpha".to_string(), hint: "1080p".to_string(), icon: "language".to_string(), on_select: move |_| split_open.set(false) }
                            MenuItem { label: "Nyaa · SubsPlease".to_string(), hint: "1080p".to_string(), icon: "download".to_string(), on_select: move |_| split_open.set(false) }
                        }
                        Button { label: t("common.see_all"), variant: ButtonVariant::Tonal, trailing_icon: "chevron-right".to_string(), disabled: true }
                    }
                }
                section { class: "tk-panel",
                    h2 { class: "tk-panel__title", "IconButton" }
                    div { class: "tk-panel__row",
                        for v in [IconButtonVariant::Standard, IconButtonVariant::Filled, IconButtonVariant::Tonal, IconButtonVariant::Outlined] {
                            IconButton { key: "{v:?}", icon: "enhance".to_string(), label: format!("{v:?}"), variant: v }
                        }
                        IconButton { icon: "cc".to_string(), label: t("player.subs"), toggle: true, toggled: chip_on(), onclick: move |_| chip_on.toggle() }
                        IconButton { icon: "play".to_string(), label: t("player.play"), size: IconButtonSize::Lg, variant: IconButtonVariant::Filled }
                        IconButton { icon: "close".to_string(), label: t("common.close"), size: IconButtonSize::Sm }
                    }
                }
                section { class: "tk-panel",
                    h2 { class: "tk-panel__title", "Segmented · Switch · Field" }
                    Segmented { options: seg_opts.clone(), selected: seg(), label: t("src.title"), on_select: move |v| seg.set(v) }
                    Segmented { options: seg_opts, selected: seg(), label: t("src.title"), small: true, disabled_keys: vec!["torrent".to_string()], on_select: move |v| seg.set(v) }
                    div { class: "tk-panel__row",
                        Switch { checked: sw(), label: t("ext.enabled"), on_toggle: move |v| sw.set(v) }
                        Switch { checked: false, label: t("ext.enabled"), disabled: true }
                    }
                    TextField {
                        label: t("link.code"),
                        value: text(),
                        icon: "qr".to_string(),
                        mono: true,
                        placeholder: "tatakai-link:v1?h=…".to_string(),
                        error: invalid,
                        hint: if invalid { t("ext.install.invalid") } else { String::new() },
                        oninput: move |v| text.set(v),
                    }
                }
                section { class: "tk-panel",
                    h2 { class: "tk-panel__title", "Chip · Pill · Banner" }
                    div { class: "tk-chips",
                        Chip { label: t("src.web"), on: chip_on(), onclick: move |_| chip_on.toggle() }
                        Chip { label: t("src.torrent"), icon: "download".to_string() }
                        Chip { label: t("common.copy"), kind: ChipKind::Assist, icon: "copy".to_string() }
                        Chip { label: "1080p".to_string(), on: true, on_dismiss: move |_| chip_on.set(false), dismiss_label: t("common.remove") }
                        InfoChip { text: "TV · 2024".to_string() }
                        InfoChip { text: "8.4".to_string(), icon: "star".to_string() }
                    }
                    div { class: "tk-chips",
                        for tone in [Tone::Neutral, Tone::Ok, Tone::Warn, Tone::Err, Tone::Primary] {
                            Pill { key: "{tone.css()}", text: tone.css().to_string(), tone }
                        }
                        Pill { text: t("src.recommended"), tone: Tone::Primary, plain: true }
                    }
                    Banner { text: t("src.failover"), icon: "enhance".to_string(), tone: Tone::Primary,
                        Button { label: t("common.details"), variant: ButtonVariant::Text, size: ButtonSize::Xs }
                    }
                    Banner { text: t("ext.compat.partial_hint"), icon: "warning".to_string(), tone: Tone::Warn }
                    p { class: "tk-muted", Kbd { keys: "Ctrl".to_string() } " + " Kbd { keys: "K".to_string() } }
                }
                section { class: "tk-panel",
                    h2 { class: "tk-panel__title", "Menu · Dialog · Sheet · Snackbar" }
                    div { class: "tk-panel__row",
                        Menu {
                            open: menu_open(),
                            label: t("player.speed"),
                            title: t("player.speed"),
                            align: crate::components::MenuAlign::Start,
                            on_close: move |_| menu_open.set(false),
                            trigger: rsx! {
                                Button { label: t("player.speed"), variant: ButtonVariant::Outlined, icon: "speed".to_string(), onclick: move |_| menu_open.toggle() }
                            },
                            for s in ["0.5", "1", "1.25", "1.5", "2"] {
                                MenuItem {
                                    key: "{s}",
                                    label: format!("{s}x"),
                                    checked: Some(speed() == s),
                                    on_select: move |_| {
                                        speed.set(s.to_string());
                                        menu_open.set(false);
                                    },
                                }
                            }
                            MenuSeparator {}
                            MenuItem { label: t("player.smoothing"), icon: "enhance".to_string(), hint: "Ctrl+1".to_string(), disabled: true }
                        }
                        Button { label: t("common.remove"), variant: ButtonVariant::Tonal, onclick: move |_| dialog.set(true) }
                        Button { label: t("search.filters"), variant: ButtonVariant::Tonal, icon: "filter".to_string(), onclick: move |_| sheet.set(true) }
                        Button { label: t("gallery.snack"), variant: ButtonVariant::Text, onclick: move |_| snack.show(t("gallery.snack_text")) }
                    }
                }
                section { class: "tk-panel",
                    h2 { class: "tk-panel__title", "Loading · Progress · Skeleton" }
                    div { class: "tk-panel__row",
                        LoadingIndicator {}
                        LoadingIndicator { contained: true }
                        LoadingIndicator { small: true }
                    }
                    WavyProgress { fraction: fraction(), label: t("player.buffering") }
                    WavyProgress { fraction: fraction(), label: t("player.buffering"), active: false, thin: true }
                    input {
                        r#type: "range",
                        min: "0",
                        max: "100",
                        value: "{(fraction() * 100.0).round()}",
                        "aria-label": "fraction",
                        oninput: move |e| fraction.set(e.value().parse::<f64>().unwrap_or(0.0) / 100.0),
                    }
                    SkeletonLines {}
                    SkeletonRow { count: 3 }
                }
            }
            section { class: "tk-section",
                h2 { class: "tk-section__title", "EmptyState · ErrorState" }
                EmptyState { icon: "library".to_string(), title: t("lib.empty.title"), body: t("lib.empty.body"), action_label: t("nav.search"), on_action: move |_| {} }
                ErrorState { error: port_err, title: t("link.err.port"), cause: t("link.err.port_fix"), on_retry: move |_| {},
                    Button { label: t("link.err.port_fix"), variant: ButtonVariant::Text }
                }
            }
            FloatingToolbar { label: t("details.watch"),
                Button { label: t("details.continue"), icon: "play".to_string() }
                ToolbarSeparator {}
                IconButton { icon: "bookmark".to_string(), label: t("details.add_list") }
                IconButton { icon: "cast".to_string(), label: t("details.send_device") }
                IconButton { icon: "extensions".to_string(), label: t("details.choose_source") }
            }
            if dialog() {
                ConfirmDialog {
                    title: t("ext.remove.title"),
                    body: t("ext.remove.body"),
                    confirm_label: t("common.remove"),
                    icon: "trash".to_string(),
                    on_confirm: move |_| dialog.set(false),
                    on_cancel: move |_| dialog.set(false),
                }
            }
            if sheet() {
                Sheet {
                    title: t("search.filters"),
                    on_close: move |_| sheet.set(false),
                    footer: rsx! {
                        Button { label: t("search.reset"), variant: ButtonVariant::Text, onclick: move |_| sheet.set(false) }
                        Button { label: t("search.apply"), onclick: move |_| sheet.set(false) }
                    },
                    div { class: "tk-chips",
                        for g in ["Action", "Drama", "Fantasy", "Romance", "Sci-Fi"] {
                            Chip { key: "{g}", label: g.to_string() }
                        }
                    }
                }
            }
        }
    }
}
