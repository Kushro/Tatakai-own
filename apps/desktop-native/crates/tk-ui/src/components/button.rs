//! `button.rs`: M3E buttons (filled / tonal / elevated / outlined / text / danger, sizes XS..XL, shape
//! morph on press in CSS), the split button and icon buttons (aria-label is required).

use dioxus::prelude::*;

use crate::icons::Icon;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ButtonVariant {
    #[default]
    Filled,
    Tonal,
    Elevated,
    Outlined,
    Text,
    /// Destructive confirmation (error colors).
    Danger,
}

impl ButtonVariant {
    pub fn css(self) -> &'static str {
        match self {
            Self::Filled => "filled",
            Self::Tonal => "tonal",
            Self::Elevated => "elevated",
            Self::Outlined => "outlined",
            Self::Text => "text",
            Self::Danger => "danger",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ButtonSize {
    Xs,
    Sm,
    #[default]
    Md,
    Lg,
    Xl,
}

impl ButtonSize {
    pub fn css(self) -> &'static str {
        match self {
            Self::Xs => "xs",
            Self::Sm => "sm",
            Self::Md => "md",
            Self::Lg => "lg",
            Self::Xl => "xl",
        }
    }

    fn icon_px(self) -> u32 {
        match self {
            Self::Xs => 16,
            Self::Sm | Self::Md => 20,
            Self::Lg => 24,
            Self::Xl => 28,
        }
    }
}

#[component]
pub fn Button(
    label: String,
    #[props(default)] variant: ButtonVariant,
    #[props(default)] size: ButtonSize,
    #[props(default)] icon: String,
    /// Icon after the label (e.g. a chevron).
    #[props(default)] trailing_icon: String,
    #[props(default)] disabled: bool,
    /// Full width.
    #[props(default)] block: bool,
    #[props(default)] class: String,
    #[props(default)] onclick: EventHandler<MouseEvent>,
) -> Element {
    let block = if block { "tk-btn--block" } else { "" };
    let px = size.icon_px();
    rsx! {
        button {
            class: "tk-btn tk-btn--{variant.css()} tk-btn--{size.css()} {block} {class}",
            r#type: "button",
            disabled,
            onclick: move |e| onclick.call(e),
            if !icon.is_empty() {
                Icon { name: icon.clone(), size: px }
            }
            span { "{label}" }
            if !trailing_icon.is_empty() {
                Icon { name: trailing_icon.clone(), size: px }
            }
        }
    }
}

/// M3E split button: a main action plus a menu chevron. The caller renders the menu, usually by
/// wrapping this component in [`crate::components::Menu`].
#[component]
pub fn SplitButton(
    label: String,
    /// Accessible name of the chevron ("More options", "Choose source"...).
    menu_label: String,
    #[props(default)] icon: String,
    #[props(default)] variant: ButtonVariant,
    #[props(default)] size: ButtonSize,
    #[props(default)] expanded: bool,
    #[props(default)] disabled: bool,
    #[props(default)] onclick: EventHandler<MouseEvent>,
    #[props(default)] on_menu: EventHandler<MouseEvent>,
) -> Element {
    let v = variant.css();
    let s = size.css();
    let px = size.icon_px();
    rsx! {
        div { class: "tk-split",
            button {
                class: "tk-btn tk-btn--{v} tk-btn--{s}",
                r#type: "button",
                disabled,
                onclick: move |e| onclick.call(e),
                if !icon.is_empty() {
                    Icon { name: icon.clone(), size: px }
                }
                span { "{label}" }
            }
            button {
                class: "tk-btn tk-btn--{v} tk-btn--{s}",
                r#type: "button",
                "aria-label": "{menu_label}",
                title: "{menu_label}",
                "aria-haspopup": "menu",
                "aria-expanded": "{expanded}",
                disabled,
                onclick: move |e| on_menu.call(e),
                Icon { name: "chevron-down".to_string(), size: px }
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum IconButtonVariant {
    #[default]
    Standard,
    Filled,
    Tonal,
    Outlined,
}

impl IconButtonVariant {
    fn css(self) -> &'static str {
        match self {
            Self::Standard => "",
            Self::Filled => "tk-iconbtn--filled",
            Self::Tonal => "tk-iconbtn--tonal",
            Self::Outlined => "tk-iconbtn--outlined",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum IconButtonSize {
    Sm,
    #[default]
    Md,
    Lg,
}

impl IconButtonSize {
    fn css(self) -> (&'static str, u32) {
        match self {
            Self::Sm => ("tk-iconbtn--sm", 20),
            Self::Md => ("", 22),
            Self::Lg => ("tk-iconbtn--lg", 28),
        }
    }
}

/// Icon-only button. `label` becomes the accessible name and the tooltip. `toggled` turns it into a
/// toggle button (aria-pressed) whose shape morphs to a rounded square when on.
#[component]
pub fn IconButton(
    icon: String,
    label: String,
    #[props(default)] variant: IconButtonVariant,
    #[props(default)] size: IconButtonSize,
    #[props(default)] toggled: bool,
    /// Set for toggle buttons so `aria-pressed` is emitted even when off.
    #[props(default)] toggle: bool,
    #[props(default)] disabled: bool,
    #[props(default)] class: String,
    #[props(default)] onclick: EventHandler<MouseEvent>,
) -> Element {
    let (size_css, px) = size.css();
    let pressed = (toggle || toggled).then_some(if toggled { "true" } else { "false" });
    rsx! {
        button {
            class: "tk-iconbtn {variant.css()} {size_css} {class}",
            r#type: "button",
            "aria-label": "{label}",
            title: "{label}",
            "aria-pressed": pressed,
            "data-toggled": "{toggled}",
            disabled,
            onclick: move |e| onclick.call(e),
            Icon { name: icon.clone(), size: px }
        }
    }
}
