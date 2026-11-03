//! `title_bar.rs`: frameless window title bar (drag region + window controls + status chips).
//! Adapted from Miko-Kagura `src/ui/title_bar.rs` (same author; see THIRD_PARTY_NOTICES.md).

use std::time::Duration;

use dioxus::desktop::use_window;
use dioxus::prelude::*;

use crate::app::Services;
use crate::backend::LinkStatusVm;
use crate::i18n::{t, tf};
use crate::icons::Icon;

/// How often the shell re-reads the Link status for the title bar chip.
const LINK_POLL_SECS: u64 = 20;

/// Sends `navigator.onLine` now and on every `online` / `offline` event.
const ONLINE_JS: &str =
    "const s=()=>dioxus.send(navigator.onLine);window.addEventListener('online',s);window.addEventListener('offline',s);s();";

/// Data behind the title bar chips. Provided by the shell through context so screens that change the
/// Link server (Link page restart) can refresh the chip right away with `link.set(Some(..))`.
#[derive(Clone, Copy)]
pub struct ShellStatus {
    pub online: Signal<bool>,
    /// Last `Backend::link_status()`; `None` until the first answer or when it failed.
    pub link: Signal<Option<LinkStatusVm>>,
}

impl ShellStatus {
    /// Port for the `Link :<port>` chip, only while the server runs.
    pub fn link_port(&self) -> Option<u16> {
        match &*self.link.read() {
            Some(s) if s.running => Some(s.port),
            _ => None,
        }
    }
}

/// Call once from the shell: provides [`ShellStatus`] and keeps it fed (web view online state and a
/// periodic `link_status()` poll).
pub fn use_shell_status() -> ShellStatus {
    let services = use_context::<Services>();
    let mut online = use_signal(|| true);
    let mut link = use_signal(|| None::<LinkStatusVm>);
    let status = use_context_provider(|| ShellStatus { online, link });

    use_future(move || async move {
        let mut eval = document::eval(ONLINE_JS);
        while let Ok(v) = eval.recv::<bool>().await {
            if *online.peek() != v {
                online.set(v);
            }
        }
    });

    let backend = services.backend.clone();
    use_future(move || {
        let backend = backend.clone();
        async move {
            loop {
                let next = backend.link_status().await.ok();
                if *link.peek() != next {
                    link.set(next);
                }
                tokio::time::sleep(Duration::from_secs(LINK_POLL_SECS)).await;
            }
        }
    });

    status
}

#[component]
pub fn TitleBar(
    #[props(default)] on_toggle_nav: EventHandler<()>,
    /// Shows the "Offline" chip.
    #[props(default)] offline: bool,
    /// Shows the `Link :<port>` chip (the Link server is running on that port).
    #[props(default)] link_port: Option<u16>,
) -> Element {
    let win = use_window();
    let mut maximized = use_signal(|| win.window.is_maximized());

    let (drag, dbl, min, max, close) = (win.clone(), win.clone(), win.clone(), win.clone(), win.clone());
    let max_label = if maximized() { t("window.restore") } else { t("window.maximize") };
    let max_icon = if maximized() { "restore" } else { "maximize" };
    let (min_label, close_label, nav_label) = (t("window.minimize"), t("window.close"), t("nav.expand"));

    rsx! {
        header { class: "tk-titlebar",
            button {
                class: "tk-titlebar__btn tk-titlebar__menu",
                r#type: "button",
                "aria-label": "{nav_label}",
                title: "{nav_label} (Ctrl+B)",
                onclick: move |_| on_toggle_nav.call(()),
                Icon { name: "menu".to_string(), size: 18 }
            }
            div {
                class: "tk-titlebar__drag",
                onmousedown: move |_| drag.drag(),
                ondoubleclick: move |_| {
                    dbl.toggle_maximized();
                    maximized.set(dbl.window.is_maximized());
                },
                span { class: "tk-titlebar__name", {t("app.name")} }
                span { class: "tk-titlebar__channel", {t("app.channel")} }
                div { class: "tk-titlebar__status", role: "status",
                    if offline {
                        span { class: "tk-titlebar__pill tk-titlebar__pill--offline",
                            Icon { name: "offline".to_string(), size: 14 }
                            {t("common.offline")}
                        }
                    }
                    if let Some(port) = link_port {
                        span { class: "tk-titlebar__pill tk-titlebar__pill--link",
                            Icon { name: "link".to_string(), size: 14 }
                            {tf("titlebar.link", &[("port", &port.to_string())])}
                        }
                    }
                }
            }
            div { class: "tk-titlebar__controls",
                button {
                    class: "tk-titlebar__btn",
                    r#type: "button",
                    "aria-label": "{min_label}",
                    title: "{min_label}",
                    onclick: move |_| min.window.set_minimized(true),
                    Icon { name: "minimize".to_string(), size: 16 }
                }
                button {
                    class: "tk-titlebar__btn",
                    r#type: "button",
                    "aria-label": "{max_label}",
                    title: "{max_label}",
                    onclick: move |_| {
                        max.toggle_maximized();
                        maximized.set(max.window.is_maximized());
                    },
                    Icon { name: max_icon.to_string(), size: 16 }
                }
                button {
                    class: "tk-titlebar__btn tk-titlebar__btn--close",
                    r#type: "button",
                    "aria-label": "{close_label}",
                    title: "{close_label}",
                    onclick: move |_| close.close(),
                    Icon { name: "close".to_string(), size: 16 }
                }
            }
        }
    }
}
