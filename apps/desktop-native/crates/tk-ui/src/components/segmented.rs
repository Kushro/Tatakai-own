//! `segmented.rs`: M3E connected button group with single selection (radiogroup semantics).
//! The selected item morphs to a full pill; Left/Right/Home/End move the selection.

use dioxus::prelude::*;

/// One segment: `(id, already-translated label)`.
pub type SegOption = (String, String);

/// Next enabled index from `from` in direction `step` (wrapping); `None` if nothing is enabled.
pub fn step_enabled(enabled: &[bool], from: usize, step: isize) -> Option<usize> {
    let n = enabled.len();
    if n == 0 {
        return None;
    }
    let mut i = from;
    for _ in 0..n {
        i = (i as isize + step).rem_euclid(n as isize) as usize;
        if enabled[i] {
            return Some(i);
        }
    }
    None
}

#[component]
pub fn Segmented(
    options: Vec<SegOption>,
    selected: String,
    /// Accessible name of the group.
    label: String,
    #[props(default)] small: bool,
    /// Ids that render disabled (e.g. "M1" features).
    #[props(default)] disabled_keys: Vec<String>,
    #[props(default)] on_select: EventHandler<String>,
) -> Element {
    let enabled: Vec<bool> = options.iter().map(|(id, _)| !disabled_keys.contains(id)).collect();
    let current = options.iter().position(|(id, _)| *id == selected).unwrap_or(0);
    let ids: Vec<String> = options.iter().map(|(id, _)| id.clone()).collect();
    let small = if small { "tk-seg--sm" } else { "" };
    rsx! {
        div {
            class: "tk-seg {small}",
            role: "radiogroup",
            "aria-label": "{label}",
            onkeydown: move |e| {
                let step = match e.key() {
                    Key::ArrowRight | Key::ArrowDown => Some(1),
                    Key::ArrowLeft | Key::ArrowUp => Some(-1),
                    _ => None,
                };
                let target = match (step, e.key()) {
                    (Some(s), _) => step_enabled(&enabled, current, s),
                    (None, Key::Home) => step_enabled(&enabled, enabled.len().saturating_sub(1), 1),
                    (None, Key::End) => step_enabled(&enabled, 0, -1),
                    _ => None,
                };
                if let Some(i) = target {
                    e.prevent_default();
                    on_select.call(ids[i].clone());
                }
            },
            for (i , (id , text)) in options.into_iter().enumerate() {
                button {
                    key: "{id}",
                    class: "tk-seg__item",
                    r#type: "button",
                    role: "radio",
                    "aria-checked": if id == selected { "true" } else { "false" },
                    tabindex: if i == current { "0" } else { "-1" },
                    disabled: disabled_keys.contains(&id),
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

#[cfg(test)]
mod tests {
    use super::step_enabled;

    #[test]
    fn arrow_keys_skip_disabled_and_wrap() {
        let e = [true, false, true, true];
        assert_eq!(step_enabled(&e, 0, 1), Some(2));
        assert_eq!(step_enabled(&e, 3, 1), Some(0));
        assert_eq!(step_enabled(&e, 0, -1), Some(3));
        assert_eq!(step_enabled(&e, 2, -1), Some(0));
        assert_eq!(step_enabled(&[false, false], 0, 1), None);
        assert_eq!(step_enabled(&[], 0, 1), None);
    }
}
