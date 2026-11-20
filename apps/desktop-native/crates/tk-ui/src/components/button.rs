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
    /// Tooltip (`title`), e.g. when CSS shows the button as an icon only; empty = none.
    #[props(default)] title: String,
    #[props(default)] onclick: EventHandler<MouseEvent>,
) -> Element {
    let block = if block { "tk-btn--block" } else { "" };
    let title = (!title.is_empty()).then_some(title);
    let px = size.icon_px();
    rsx! {
        button {
            class: "tk-btn tk-btn--{variant.css()} tk-btn--{size.css()} {block} {class}",
            r#type: "button",
            title,
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
    /// Only the main action is disabled (the chevron still opens its menu).
    #[props(default)] main_disabled: bool,
    #[props(default)] onclick: EventHandler<MouseEvent>,
    #[props(default)] on_menu: EventHandler<MouseEvent>,
    /// Accessible name of the main action when the visible label is shorter ("Continue" in a card that already
    /// names the episode): it must contain the visible label (WCAG 2.5.3). Empty = the label.
    #[props(default)] aria_label: String,
    /// A second, smaller line under the label (`small`, e.g. "3:39 of 24 min"). Empty = one line.
    #[props(default)] sub_label: String,
    /// Watched fraction (0..1) drawn as a bar along the main action's bottom edge (`i.tk-split__prog`).
    #[props(default)] progress: Option<f64>,
    /// Extra classes on the wrapper (`tk-split {class}`).
    #[props(default)] class: String,
) -> Element {
    let v = variant.css();
    let s = size.css();
    let px = size.icon_px();
    // With a second line the name reads "Continue episode 4, 3:39 of 24 min", never the two run together.
    let aria = if !aria_label.is_empty() {
        Some(aria_label)
    } else if !sub_label.is_empty() {
        Some(format!("{label}, {sub_label}"))
    } else {
        None
    };
    let prog = progress.map(|f| format!("{:.1}%", f.clamp(0.0, 1.0) * 100.0));
    rsx! {
        div { class: "tk-split {class}",
            button {
                class: "tk-btn tk-btn--{v} tk-btn--{s}",
                r#type: "button",
                "aria-label": aria,
                disabled: disabled || main_disabled,
                onclick: move |e| onclick.call(e),
                if !icon.is_empty() {
                    Icon { name: icon.clone(), size: px }
                }
                if sub_label.is_empty() {
                    span { "{label}" }
                } else {
                    span { class: "tk-split__lbl",
                        "{label}"
                        small { "{sub_label}" }
                    }
                }
                if let Some(w) = prog {
                    i { class: "tk-split__prog", "aria-hidden": "true", style: "width: {w}" }
                }
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
    /// `Some(-1)`: out of the Tab order (reached another way, e.g. arrow keys from its row).
    #[props(default)] tabindex: Option<i32>,
    #[props(default)] onclick: EventHandler<MouseEvent>,
) -> Element {
    let (size_css, px) = size.css();
    let tabindex = tabindex.map(|i| i.to_string());
    let pressed = (toggle || toggled).then_some(if toggled { "true" } else { "false" });
    rsx! {
        button {
            class: "tk-iconbtn {variant.css()} {size_css} {class}",
            r#type: "button",
            "aria-label": "{label}",
            title: "{label}",
            "aria-pressed": pressed,
            "data-toggled": "{toggled}",
            tabindex,
            disabled,
            onclick: move |e| onclick.call(e),
            Icon { name: icon.clone(), size: px }
        }
    }
}
