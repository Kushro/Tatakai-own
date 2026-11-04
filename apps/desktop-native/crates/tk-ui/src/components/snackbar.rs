//! `snackbar.rs`: one global snackbar (M3: a single message at a time, newest wins).
//!
//! `App` provides a [`Snackbar`] through context and the shell renders [`SnackbarHost`]. Screens call
//! `use_snackbar().show(text)` or `.show_action(text, label, callback)`. Messages auto-dismiss after
//! [`SNACK_MS`]; the action closes the snackbar before running. Use it for confirmations ("Copied",
//! "Using Toko · Alpha"), never for errors that need attention (those are inline `ErrorState`s).

use std::time::Duration;

use dioxus::prelude::*;

use crate::components::button::IconButton;
use crate::i18n::t;

pub const SNACK_MS: u64 = 4000;

#[derive(Clone, PartialEq)]
pub struct SnackMsg {
    pub id: u64,
    pub text: String,
    pub action: Option<(String, Callback<()>)>,
}

#[derive(Clone, Copy, PartialEq)]
pub struct Snackbar {
    current: Signal<Option<SnackMsg>>,
    seq: Signal<u64>,
}

impl Snackbar {
    /// Create the signals in the calling component (call once, from `App`).
    pub fn new() -> Self {
        Self { current: Signal::new(None), seq: Signal::new(0) }
    }

    fn push(mut self, text: String, action: Option<(String, Callback<()>)>) {
        let id = *self.seq.peek() + 1;
        self.seq.set(id);
        self.current.set(Some(SnackMsg { id, text, action }));
    }

    pub fn show(self, text: impl Into<String>) {
        self.push(text.into(), None);
    }

    /// `action` must belong to a scope that outlives the message (a screen or the shell).
    pub fn show_action(self, text: impl Into<String>, label: impl Into<String>, action: Callback<()>) {
        self.push(text.into(), Some((label.into(), action)));
    }

    pub fn dismiss(mut self) {
        self.current.set(None);
    }

    pub fn current(&self) -> Option<SnackMsg> {
        self.current.read().clone()
    }
}

impl Default for Snackbar {
    fn default() -> Self {
        Self::new()
    }
}

pub fn use_snackbar() -> Snackbar {
    use_context::<Snackbar>()
}

/// Renders the current message. Place once, at the end of the shell.
#[component]
pub fn SnackbarHost() -> Element {
    let snack = use_snackbar();
    let mut current = snack.current;

    // Auto-dismiss: each new id starts its own timer; an older timer never clears a newer message.
    use_effect(move || {
        let Some(id) = current.read().as_ref().map(|m| m.id) else { return };
        spawn(async move {
            tokio::time::sleep(Duration::from_millis(SNACK_MS)).await;
            if current.peek().as_ref().is_some_and(|m| m.id == id) {
                current.set(None);
            }
        });
    });

    let Some(msg) = snack.current() else { return rsx! {} };
    rsx! {
        div { key: "{msg.id}", class: "tk-snack", role: "status", "aria-live": "polite",
            span { class: "tk-snack__text", "{msg.text}" }
            if let Some((label, action)) = msg.action.clone() {
                button {
                    class: "tk-snack__action",
                    r#type: "button",
                    onclick: move |_| {
                        snack.dismiss();
                        action.call(());
                    },
                    "{label}"
                }
            }
            IconButton { icon: "close".to_string(), label: t("common.close"), size: crate::components::IconButtonSize::Sm, onclick: move |_| snack.dismiss() }
        }
    }
}
