//! Switch and outlined text field. (The segmented button lives in `segmented.rs`.)

use dioxus::prelude::*;

use crate::icons::Icon;

#[component]
pub fn Switch(
    checked: bool,
    label: String,
    #[props(default)] disabled: bool,
    #[props(default)] on_toggle: EventHandler<bool>,
) -> Element {
    rsx! {
        button {
            class: "tk-switch",
            r#type: "button",
            role: "switch",
            "aria-checked": "{checked}",
            "aria-label": "{label}",
            title: "{label}",
            "data-checked": "{checked}",
            disabled,
            onclick: move |_| on_toggle.call(!checked),
            span { class: "tk-switch__thumb" }
        }
    }
}

/// Outlined text field with optional leading icon, supporting text and error state.
#[component]
pub fn TextField(
    label: String,
    value: String,
    #[props(default = "text".to_string())] kind: String,
    #[props(default)] placeholder: String,
    #[props(default)] icon: String,
    /// Supporting text under the box; error-colored when `error` is set.
    #[props(default)] hint: String,
    #[props(default)] error: bool,
    /// Monospace input (codes, paths, hashes).
    #[props(default)] mono: bool,
    #[props(default)] disabled: bool,
    #[props(default)] oninput: EventHandler<String>,
    #[props(default)] onsubmit: EventHandler<()>,
) -> Element {
    rsx! {
        label { class: "tk-field", "data-error": "{error}", "data-mono": "{mono}",
            span { class: "tk-field__label", "{label}" }
            span { class: "tk-field__box",
                if !icon.is_empty() {
                    Icon { name: icon.clone(), size: 20 }
                }
                input {
                    class: "tk-field__input",
                    r#type: "{kind}",
                    value: "{value}",
                    placeholder: "{placeholder}",
                    autocomplete: "off",
                    spellcheck: "false",
                    disabled,
                    "aria-invalid": "{error}",
                    oninput: move |e| oninput.call(e.value()),
                    onkeydown: move |e| {
                        if e.key() == Key::Enter {
                            onsubmit.call(());
                        }
                    },
                }
            }
            if !hint.is_empty() {
                span { class: "tk-field__hint", "{hint}" }
            }
        }
    }
}
