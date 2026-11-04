//! `menu.rs`: menu anchored to a trigger. A transparent backdrop closes it on an outside click, Esc
//! closes it, focus moves into it on open and Up/Down move between items. Items with `checked` are
//! `menuitemradio` (player subtitle/audio/quality/speed, enhance presets); a hint shows the shortcut.
//!
//! ```ignore
//! Menu { open: open(), on_close: move |_| open.set(false), label: t("player.speed"),
//!     trigger: rsx! { IconButton { icon: "speed", label: t("player.speed"), onclick: move |_| open.toggle() } },
//!     MenuItem { label: "1x", checked: Some(true), on_select: move |_| { .. } }
//! }
//! ```

use dioxus::prelude::*;

use crate::icons::Icon;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MenuAlign {
    Start,
    #[default]
    End,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MenuPlacement {
    #[default]
    Below,
    /// Opens upwards (player bottom bar).
    Above,
}

/// Script run on Up/Down inside an open menu: moves focus between enabled items (wrapping).
const MOVE_FOCUS_JS: &str = r#"
const dir = await dioxus.recv();
const menu = document.activeElement && document.activeElement.closest('.tk-menu');
if (menu) {
  const items = [...menu.querySelectorAll('.tk-menu__item:not(:disabled)')];
  if (items.length) {
    const i = items.indexOf(document.activeElement);
    const next = i < 0 ? (dir > 0 ? 0 : items.length - 1) : (i + dir + items.length) % items.length;
    items[next].focus();
  }
}
"#;

#[component]
pub fn Menu(
    open: bool,
    /// Accessible name of the menu.
    label: String,
    trigger: Element,
    #[props(default)] title: String,
    #[props(default)] align: MenuAlign,
    #[props(default)] placement: MenuPlacement,
    #[props(default)] on_close: EventHandler<()>,
    children: Element,
) -> Element {
    let align = match align {
        MenuAlign::Start => "start",
        MenuAlign::End => "end",
    };
    let placement = match placement {
        MenuPlacement::Below => "below",
        MenuPlacement::Above => "above",
    };
    rsx! {
        div { class: "tk-menu-anchor",
            {trigger}
            if open {
                div { class: "tk-menu__backdrop", onclick: move |_| on_close.call(()) }
                div {
                    class: "tk-menu",
                    role: "menu",
                    "aria-label": "{label}",
                    tabindex: "-1",
                    "data-align": "{align}",
                    "data-placement": "{placement}",
                    onmounted: move |e| async move {
                        let _ = e.set_focus(true).await;
                    },
                    onkeydown: move |e| {
                        let dir = match e.key() {
                            Key::Escape | Key::Tab => {
                                e.stop_propagation();
                                on_close.call(());
                                return;
                            }
                            Key::ArrowDown => 1,
                            Key::ArrowUp => -1,
                            _ => return,
                        };
                        e.prevent_default();
                        let eval = document::eval(MOVE_FOCUS_JS);
                        let _ = eval.send(dir);
                    },
                    if !title.is_empty() {
                        div { class: "tk-menu__title", "{title}" }
                    }
                    {children}
                }
            }
        }
    }
}

#[component]
pub fn MenuItem(
    label: String,
    #[props(default)] icon: String,
    /// Shortcut or secondary value on the right ("Ctrl+1", "1080p").
    #[props(default)] hint: String,
    /// `Some` makes it a radio item.
    #[props(default)] checked: Option<bool>,
    #[props(default)] disabled: bool,
    #[props(default)] on_select: EventHandler<()>,
) -> Element {
    let role = if checked.is_some() { "menuitemradio" } else { "menuitem" };
    let aria_checked = checked.map(|c| if c { "true" } else { "false" });
    rsx! {
        button {
            class: "tk-menu__item",
            r#type: "button",
            role,
            "aria-checked": aria_checked,
            disabled,
            onclick: move |_| on_select.call(()),
            span { class: "tk-menu__icon",
                if checked == Some(true) && icon.is_empty() {
                    Icon { name: "check".to_string(), size: 20 }
                } else if !icon.is_empty() {
                    Icon { name: icon.clone(), size: 20 }
                }
            }
            span { class: "tk-menu__label", "{label}" }
            if !hint.is_empty() {
                span { class: "tk-menu__hint", "{hint}" }
            }
        }
    }
}

#[component]
pub fn MenuSeparator() -> Element {
    rsx! { div { class: "tk-menu__sep", role: "separator" } }
}
