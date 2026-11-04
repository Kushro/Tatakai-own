//! Switch and outlined text field. (The segmented button lives in `segmented.rs`.)

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use dioxus::prelude::*;

use crate::icons::Icon;

/// Keeps a text input uncontrolled while the user types. Writing `value` back on every keystroke races the
/// webview round trip and drops characters typed in between, so the input only gets `initial_value`. The
/// returned key changes when the caller sets a value the input did not report itself (clear, reset), which
/// remounts the input with it; call the returned recorder from `oninput` with what the input reported.
pub fn use_uncontrolled_input(value: &str) -> (u32, Rc<dyn Fn(&str)>) {
    let typed = use_hook(|| Rc::new(RefCell::new(value.to_owned())));
    let generation = use_hook(|| Rc::new(Cell::new(0u32)));
    if *typed.borrow() != value {
        *typed.borrow_mut() = value.to_owned();
        generation.set(generation.get().wrapping_add(1));
    }
    let record: Rc<dyn Fn(&str)> = Rc::new(move |v: &str| *typed.borrow_mut() = v.to_owned());
    (generation.get(), record)
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
    let (generation, record) = use_uncontrolled_input(&value);
    rsx! {
        label { class: "tk-field", "data-error": "{error}", "data-mono": "{mono}",
            span { class: "tk-field__label", "{label}" }
            span { class: "tk-field__box",
                if !icon.is_empty() {
                    Icon { name: icon.clone(), size: 20 }
                }
                // A one-item keyed list: Dioxus only remounts on a key change inside a list.
                {std::iter::once(generation).map(|generation| {
                    let record = record.clone();
                    rsx! {
                        input {
                            key: "{generation}",
                            class: "tk-field__input",
                            r#type: "{kind}",
                            initial_value: "{value}",
                            placeholder: "{placeholder}",
                            autocomplete: "off",
                            spellcheck: "false",
                            disabled,
                            "aria-invalid": "{error}",
                            oninput: move |e| {
                                record(&e.value());
                                oninput.call(e.value())
                            },
                            onkeydown: move |e| {
                                if e.key() == Key::Enter {
                                    onsubmit.call(());
                                }
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
