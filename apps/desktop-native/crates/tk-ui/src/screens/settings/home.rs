//! Settings > Home & discover (wave B, worker **b**; checklist `home.*` toggles, `home.calendar.reminders`,
//! `settings.privacy.mature-content`): one `Switch` per Home block (`model::home::HOME_BLOCKS`,
//! `data-home-block="<field>"`), airing reminders on / off + minutes before (`data-reminders`), the mature-content
//! `Segmented` (`MatureContentVm::ALL`, `data-mature="show|blur|warn"`) and shortcuts to the calendar and the
//! recommendations. Every change dispatches `SettingsAction::SetHome(prefs)` through `commit_saved`.

use dioxus::prelude::*;

use super::{SettingRow, seg};
use crate::app::{Route, Ui};
use crate::components::{Button, ButtonSize, ButtonVariant, Segmented, Switch, TextField};
use crate::i18n::t;
use crate::model::SettingsAction;
use crate::model::home::{HOME_BLOCKS, MatureContentVm, REMINDER_MAX, REMINDER_MIN};

/// Minutes typed in the field -> value, `None` unless a whole number in range.
pub fn parse_minutes(text: &str) -> Option<u16> {
    text.trim()
        .parse::<u16>()
        .ok()
        .filter(|m| (REMINDER_MIN..=REMINDER_MAX).contains(m))
}

#[component]
pub fn HomeSection(commit_saved: Callback<SettingsAction>) -> Element {
    let ui = use_context::<Ui>();
    let navigator = use_navigator();
    let prefs = ui.settings.read().home.clone();
    let mature = prefs.mature_content;
    let minutes_text = use_signal(|| prefs.reminder_minutes.to_string());
    let minutes_bad = parse_minutes(&minutes_text()).is_none();

    rsx! {
        p { class: "tk-panel__hint", {t("settings.home.hint")} }
        SettingRow { label: t("settings.home.mature"), hint: t("settings.home.mature.hint"),
            div { "data-mature": "{mature.id()}",
                Segmented {
                    options: seg(&MatureContentVm::ALL, MatureContentVm::id, "home.mature"),
                    selected: mature.id().to_owned(),
                    label: t("settings.home.mature"),
                    on_select: {
                        let prefs = prefs.clone();
                        move |id: String| {
                            if let Some(m) = MatureContentVm::ALL.into_iter().find(|m| m.id() == id) {
                                let mut next = prefs.clone();
                                next.mature_content = m;
                                commit_saved.call(SettingsAction::SetHome(next));
                            }
                        }
                    },
                }
            }
        }
        SettingRow { label: t("settings.home.reminders"), hint: t("settings.home.reminders.hint"),
            span { "data-reminders": "{prefs.airing_reminders}",
                Switch {
                    checked: prefs.airing_reminders,
                    label: t("settings.home.reminders"),
                    on_toggle: {
                        let prefs = prefs.clone();
                        move |v: bool| {
                            let mut next = prefs.clone();
                            next.airing_reminders = v;
                            commit_saved.call(SettingsAction::SetHome(next));
                        }
                    },
                }
            }
        }
        SettingRow { label: t("settings.home.reminder_minutes"), hint: t("settings.home.reminder_minutes.hint"),
            div { "data-reminder-minutes": "{prefs.reminder_minutes}",
                TextField {
                    label: t("settings.home.reminder_minutes"),
                    value: minutes_text(),
                    kind: "number".to_string(),
                    mono: true,
                    error: minutes_bad,
                    disabled: !prefs.airing_reminders,
                    oninput: {
                        let prefs = prefs.clone();
                        let mut minutes_text = minutes_text;
                        move |v: String| {
                            minutes_text.set(v.clone());
                            if let Some(m) = parse_minutes(&v) {
                                let mut next = prefs.clone();
                                next.reminder_minutes = m;
                                commit_saved.call(SettingsAction::SetHome(next));
                            }
                        }
                    },
                }
            }
        }
        h3 { class: "tk-settings__subhead", {t("settings.home.blocks")} }
        for id in HOME_BLOCKS {
            {
                let on = prefs.block(id);
                let prefs = prefs.clone();
                rsx! {
                    SettingRow { key: "{id}", label: t(&format!("settings.home.block.{id}")),
                        span { "data-home-block": "{id}", "data-on": "{on}",
                            Switch {
                                checked: on,
                                label: t(&format!("settings.home.block.{id}")),
                                on_toggle: move |v: bool| commit_saved.call(SettingsAction::SetHome(prefs.clone().with_block(id, v))),
                            }
                        }
                    }
                }
            }
        }
        div { class: "tk-settings__links",
            Button {
                label: t("settings.home.open_calendar"),
                variant: ButtonVariant::Tonal,
                size: ButtonSize::Sm,
                icon: "calendar".to_string(),
                onclick: move |_| {
                    navigator.push(Route::CalendarPage {});
                },
            }
            Button {
                label: t("settings.home.open_recs"),
                variant: ButtonVariant::Tonal,
                size: ButtonSize::Sm,
                icon: "star".to_string(),
                onclick: move |_| {
                    navigator.push(Route::RecommendationsPage {});
                },
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minutes_must_be_in_range() {
        assert_eq!(parse_minutes(" 30 "), Some(30));
        assert_eq!(parse_minutes("0"), Some(0));
        assert_eq!(parse_minutes("1440"), Some(1440));
        assert_eq!(parse_minutes("1441"), None);
        assert_eq!(parse_minutes("-1"), None);
        assert_eq!(parse_minutes("abc"), None);
    }
}
