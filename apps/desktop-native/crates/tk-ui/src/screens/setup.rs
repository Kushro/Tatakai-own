//! First-run wizard (wave A, worker **f**; plan 14 section 0.1 #11, checklist `account.setup-wizard`). Route
//! `/setup`, drawn without the rail (`Route::is_bare`). The shell redirects here while `Settings::system.setup_done`
//! is false (`app.rs::Shell`, only after `Ui::settings_loaded`); "Skip for now" and "Start watching" both set
//! `setup_done = true` through `SettingsAction::SetSystem` and go to `Route::HomePage`. Settings > System offers
//! "Run the setup assistant again".
//!
//! Steps (classic `/setup` SetupPage, trimmed to what Next has; each exposes `data-setup-step="<id>"`):
//! 1. `appearance`: language (`Settings::language`), theme mode / palette (`SettingsAction` of Appearance), reduce motion.
//! 2. `downloads`: torrent download folder (`Backend::torrent_settings` / `save_torrent_settings`, field `download_dir`)
//!    and "keep after watching" (`after_watch`).
//! 3. `extensions`: explains `.kai` extensions with an "Install from file" that reuses `Backend::install_extension`
//!    (same `<input type="file">` flow as `screens/extensions.rs`) and a link to `Route::ExtensionsPage`.
//! 4. `accounts`: AniList / MAL connect buttons (worker g's `Backend::list_connect` / `list_accounts`, same error
//!    text when no client id is configured). Optional: the app works without an account.
//! 5. `system`: start with the system, tray icon, Discord presence (`Settings::flow.discord_presence`, e's field,
//!    f only toggles it), check for updates, open magnet links with Tatakai (`set_magnet_handler`, when
//!    `system_caps().magnet_handler`).
//! 6. `done`: summary + "Start watching".
//!
//! No step blocks: every control has a default, "Next" is always enabled. No terms / privacy screens (nothing is
//! collected: apps/README.md rule 3). Keys in the `system` namespace (`setup.*`); CSS in `assets/css/system.css`.
//!
//! Contract placeholder: the owner replaces the body. The two actions below are real so a fresh profile is never
//! trapped in the wizard before f lands.

use dioxus::prelude::*;

use crate::app::{Route, Services, Ui};
use crate::components::{Button, ButtonVariant};
use crate::i18n::t;
use crate::model::{SettingsAction, reduce_settings};

#[component]
pub fn SetupPage() -> Element {
    let services = use_context::<Services>();
    let ui = use_context::<Ui>();
    let navigator = use_navigator();
    let finish = use_callback(move |_: ()| {
        let mut system = ui.settings.peek().system.clone();
        system.setup_done = true;
        let next = reduce_settings(&ui.settings.peek(), SettingsAction::SetSystem(system));
        ui.commit_settings(&services.backend, next);
        navigator.replace(Route::HomePage {});
    });
    rsx! {
        div { class: "tk-page tk-page--setup", "data-setup-step": "done",
            div { class: "tk-page__head",
                div {
                    h1 { class: "tk-page__title", {t("setup.title")} }
                    p { class: "tk-page__intro", {t("setup.intro")} }
                }
            }
            div { class: "tk-panel__row",
                Button { label: t("setup.skip"), variant: ButtonVariant::Text, onclick: move |_| finish.call(()) }
                Button { label: t("setup.finish"), onclick: move |_| finish.call(()) }
            }
        }
    }
}
