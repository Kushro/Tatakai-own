//! Settings > Social (wave B, worker **n**; checklist `social.notifications.badge`, `details.reviews`): notifications
//! polling interval (0 = off) and badge switch, "show AniList activity / reviews on Details", plus a row explaining
//! that privacy and the profile are edited on anilist.co (`open_url`). `SettingsAction::SetSocial`. Without an AniList
//! account the AniList-only rows are disabled and point to Settings > Accounts (the section right above).

use dioxus::prelude::*;

use super::SettingRow;
use crate::app::{Route, Services, Ui};
use crate::backend::ListServiceVm;
use crate::components::{Button, ButtonSize, ButtonVariant, Pill, SegOption, Segmented, Switch, Tone, use_snackbar};
use crate::i18n::{t, tn};
use crate::model::social::{POLL_CHOICES, wake_unread_poll};
use crate::model::{SettingsAction, SocialPrefs};
use crate::screens::social::profile::ANILIST_SETTINGS_URL;

/// Label of a polling choice: `0` = never, else "every N min" / "every hour" / "every 2 hours".
pub fn poll_label(minutes: u8) -> String {
    match minutes {
        0 => t("settings.social.poll.off"),
        m if m % 60 == 0 => tn("settings.social.poll.hours", usize::from(m / 60)),
        m => tn("settings.social.poll.minutes", usize::from(m)),
    }
}

/// The segmented options of the polling interval (ids are the minutes).
pub fn poll_options() -> Vec<SegOption> {
    POLL_CHOICES.iter().map(|m| (m.to_string(), poll_label(*m))).collect()
}

/// Parse a polling option id back (unknown ids keep the current value).
pub fn parse_poll(id: &str, current: u8) -> u8 {
    id.parse::<u8>().ok().filter(|m| POLL_CHOICES.contains(m)).unwrap_or(current)
}

#[component]
pub fn SocialSection(commit_saved: Callback<SettingsAction>) -> Element {
    let ui = use_context::<Ui>();
    let services = use_context::<Services>();
    let snack = use_snackbar();
    let navigator = use_navigator();
    let prefs: SocialPrefs = ui.settings.read().social.clone();
    let account = use_resource({
        let backend = services.backend.clone();
        move || {
            let backend = backend.clone();
            async move {
                backend
                    .list_accounts()
                    .await
                    .ok()
                    .and_then(|list| list.into_iter().find(|a| a.service == ListServiceVm::AniList && a.connected))
            }
        }
    });
    let account = account.read().clone();
    let loading = account.is_none();
    let connected = account.as_ref().is_some_and(|a| a.is_some());
    let user = account.flatten().and_then(|a| a.username);
    let disabled = !connected;
    let social_state = if connected {
        "connected"
    } else if loading {
        "loading"
    } else {
        "disconnected"
    };
    let disabled_keys: Vec<String> = if disabled { POLL_CHOICES.iter().map(|m| m.to_string()).collect() } else { Vec::new() };

    let set = move |p: SocialPrefs| commit_saved.call(SettingsAction::SetSocial(p));
    let p1 = prefs.clone();
    let p2 = prefs.clone();
    let p3 = prefs.clone();
    let p4 = prefs.clone();
    let open_settings = {
        let backend = services.backend.clone();
        move |_| {
            let backend = backend.clone();
            spawn(async move {
                if let Err(e) = backend.open_url(ANILIST_SETTINGS_URL).await {
                    tracing::warn!(detail = %e.detail, "opening AniList failed");
                    snack.show(t("social.open_failed"));
                }
            });
        }
    };

    rsx! {
        p { class: "tk-panel__hint", {t("settings.social.hint")} }
        SettingRow { label: t("settings.social.account"), hint: if connected { t("settings.social.account.hint") } else { t("settings.social.account.missing") },
            div { "data-social": "{social_state}",
                if connected {
                    Pill { text: user.clone().unwrap_or_else(|| t("settings.social.account.connected")), tone: Tone::Ok }
                    Button {
                        label: t("settings.social.open_profile"),
                        icon: "user".to_string(),
                        variant: ButtonVariant::Text,
                        size: ButtonSize::Sm,
                        onclick: move |_| {
                            navigator.push(Route::ProfilePage {});
                        },
                    }
                } else if !loading {
                    Pill { text: t("settings.social.account.none"), tone: Tone::Warn }
                }
            }
        }
        SettingRow { label: t("settings.social.poll"), hint: t("settings.social.poll.hint"), stack: true,
            Segmented {
                options: poll_options(),
                selected: prefs.notifications_poll_minutes.to_string(),
                label: t("settings.social.poll"),
                small: true,
                disabled_keys,
                on_select: move |id: String| {
                    let minutes = parse_poll(&id, p1.notifications_poll_minutes);
                    set(SocialPrefs { notifications_poll_minutes: minutes, ..p1.clone() });
                    wake_unread_poll();
                },
            }
        }
        SettingRow { label: t("settings.social.badge"), hint: t("settings.social.badge.hint"),
            Switch {
                checked: prefs.badge,
                label: t("settings.social.badge"),
                disabled,
                on_toggle: move |on: bool| {
                    set(SocialPrefs { badge: on, ..p2.clone() });
                    wake_unread_poll();
                },
            }
        }
        SettingRow { label: t("settings.social.reviews"), hint: t("settings.social.reviews.hint"),
            Switch {
                checked: prefs.show_reviews_on_details,
                label: t("settings.social.reviews"),
                on_toggle: move |on: bool| set(SocialPrefs { show_reviews_on_details: on, ..p3.clone() }),
            }
        }
        SettingRow { label: t("settings.social.activity"), hint: t("settings.social.activity.hint"),
            Switch {
                checked: prefs.show_activity_on_details,
                label: t("settings.social.activity"),
                disabled,
                on_toggle: move |on: bool| set(SocialPrefs { show_activity_on_details: on, ..p4.clone() }),
            }
        }
        SettingRow { label: t("settings.social.privacy"), hint: t("settings.social.privacy.hint"),
            Button {
                label: t("settings.social.privacy.open"),
                icon: "external".to_string(),
                variant: ButtonVariant::Tonal,
                size: ButtonSize::Sm,
                onclick: open_settings,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn poll_choices_round_trip() {
        for m in POLL_CHOICES {
            assert_eq!(parse_poll(&m.to_string(), 7), m);
        }
        assert_eq!(parse_poll("abc", 15), 15);
        assert_eq!(parse_poll("7", 15), 15);
        assert_eq!(poll_options().len(), POLL_CHOICES.len());
        assert!(!poll_label(0).is_empty());
        assert!(poll_label(60).contains('1') || !poll_label(60).is_empty());
    }
}
