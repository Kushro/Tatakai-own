//! Segmented buttons, switch and text field.

use dioxus::prelude::*;

/// One segment: `(id, already-translated label)`.
pub type SegOption = (String, String);

/// Single-select segmented button group (radiogroup semantics).
#[component]
pub fn Segmented(
    options: Vec<SegOption>,
    selected: String,
    label: String,
    #[props(default)] on_select: EventHandler<String>,
) -> Element {
    rsx! {
        div { class: "tk-seg", role: "radiogroup", "aria-label": "{label}",
            for (id , text) in options {
                button {
                    key: "{id}",
                    class: "tk-seg__item",
                    r#type: "button",
                    role: "radio",
                    "aria-checked": if id == selected { "true" } else { "false" },
                    "data-selected": "{id == selected}",
                    onclick: {
                        let id = id.clone();
                        move |_| on_select.call(id.clone())
                    },
                    "{text}"
                }
            }
        }
    }
}

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

#[component]
pub fn TextField(
    label: String,
    value: String,
    #[props(default = "text".to_string())] kind: String,
    #[props(default)] placeholder: String,
    #[props(default)] oninput: EventHandler<String>,
    #[props(default)] onsubmit: EventHandler<()>,
) -> Element {
    rsx! {
        label { class: "tk-field",
            span { class: "tk-field__label", "{label}" }
            input {
                class: "tk-field__input",
                r#type: "{kind}",
                value: "{value}",
                placeholder: "{placeholder}",
                autocomplete: "off",
                spellcheck: "false",
                oninput: move |e| oninput.call(e.value()),
                onkeydown: move |e| {
                    if e.key() == Key::Enter {
                        onsubmit.call(());
                    }
                },
            }
        }
    }
}
