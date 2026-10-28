//! `button.rs`: filled / tonal / outlined / text buttons and icon buttons (aria-label is required).

use dioxus::prelude::*;

use crate::icons::Icon;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ButtonVariant {
    #[default]
    Filled,
    Tonal,
    Outlined,
    Text,
}

impl ButtonVariant {
    fn css(self) -> &'static str {
        match self {
            Self::Filled => "filled",
            Self::Tonal => "tonal",
            Self::Outlined => "outlined",
            Self::Text => "text",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ButtonSize {
    Sm,
    #[default]
    Md,
    Lg,
}

impl ButtonSize {
    fn css(self) -> &'static str {
        match self {
            Self::Sm => "sm",
            Self::Md => "md",
            Self::Lg => "lg",
        }
    }
}

#[component]
pub fn Button(
    label: String,
    #[props(default)] variant: ButtonVariant,
    #[props(default)] size: ButtonSize,
    #[props(default)] icon: String,
    #[props(default)] disabled: bool,
    #[props(default)] class: String,
    #[props(default)] onclick: EventHandler<MouseEvent>,
) -> Element {
    rsx! {
        button {
            class: "tk-btn tk-btn--{variant.css()} tk-btn--{size.css()} {class}",
            r#type: "button",
            disabled,
            onclick: move |e| onclick.call(e),
            if !icon.is_empty() {
                Icon { name: icon.clone(), size: 20 }
            }
            span { "{label}" }
        }
    }
}

/// Icon-only button. `label` becomes the accessible name and the tooltip.
#[component]
pub fn IconButton(
    icon: String,
    label: String,
    #[props(default)] toggled: bool,
    #[props(default)] disabled: bool,
    #[props(default)] class: String,
    #[props(default)] onclick: EventHandler<MouseEvent>,
) -> Element {
    rsx! {
        button {
            class: "tk-iconbtn {class}",
            r#type: "button",
            "aria-label": "{label}",
            title: "{label}",
            "aria-pressed": if toggled { "true" } else { "false" },
            "data-toggled": "{toggled}",
            disabled,
            onclick: move |e| onclick.call(e),
            Icon { name: icon.clone(), size: 22 }
        }
    }
}
