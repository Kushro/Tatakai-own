//! Small line icons drawn on a 24x24 grid (original artwork, stroke-based, `currentColor`).

use dioxus::prelude::*;

/// `(path data, filled)`; unknown names render nothing.
fn glyph(name: &str) -> Option<(&'static str, bool)> {
    Some(match name {
        "home" => ("M3 11l9-8 9 8M5 10v10h5v-6h4v6h5V10", false),
        "search" => ("M11 4a7 7 0 1 0 0 14 7 7 0 0 0 0-14zM21 21l-5-5", false),
        "extensions" => ("M12 3l8 4.5v9L12 21l-8-4.5v-9L12 3zM12 12l8-4.5M12 12v9M12 12L4 7.5", false),
        "link" => ("M10 14a4 4 0 0 0 5.7 0l3-3a4 4 0 0 0-5.7-5.7l-1 1M14 10a4 4 0 0 0-5.7 0l-3 3a4 4 0 0 0 5.7 5.7l1-1", false),
        "settings" => ("M4 7h10M18 7h2M4 17h2M10 17h10M14 4v6M6 14v6", false),
        "play" => ("M8 5v14l11-7z", true),
        "pause" => ("M7 5h3v14H7zM14 5h3v14h-3z", true),
        "back" => ("M15 5l-7 7 7 7", false),
        "close" => ("M6 6l12 12M18 6L6 18", false),
        "minimize" => ("M5 12h14", false),
        "maximize" => ("M6 6h12v12H6z", false),
        "restore" => ("M8 8h10v10H8zM6 16V6h10", false),
        "menu" => ("M4 6h16M4 12h16M4 18h16", false),
        "volume" => ("M4 9v6h4l5 4V5L8 9H4zM16 9a4 4 0 0 1 0 6", false),
        "mute" => ("M4 9v6h4l5 4V5L8 9H4zM17 9l4 6M21 9l-4 6", false),
        "plus" => ("M12 5v14M5 12h14", false),
        "trash" => ("M5 7h14M9 7V4h6v3M7 7l1 13h8l1-13", false),
        "refresh" => ("M20 12a8 8 0 1 1-2.3-5.7M20 4v5h-5", false),
        "check" => ("M5 12l5 5 9-10", false),
        "next" => ("M6 5l9 7-9 7zM18 5v14", false),
        "enhance" => ("M12 3l2 6 6 2-6 2-2 6-2-6-6-2 6-2z", false),
        "shield" => ("M12 3l8 3v6c0 5-3.5 8-8 9-4.5-1-8-4-8-9V6z", false),
        "chevron-up" => ("M6 15l6-6 6 6", false),
        "chevron-down" => ("M6 9l6 6 6-6", false),
        "user" => ("M12 12a4 4 0 1 0 0-8 4 4 0 0 0 0 8zM4 21c0-4 4-6 8-6s8 2 8 6", false),
        "fullscreen" => ("M4 9V4h5M20 9V4h-5M4 15v5h5M20 15v5h-5", false),
        "film" => ("M4 5h16v14H4zM8 5v14M16 5v14M4 9h4M4 15h4M16 9h4M16 15h4", false),
        _ => return None,
    })
}

#[component]
pub fn Icon(name: String, #[props(default = 24)] size: u32) -> Element {
    let Some((d, filled)) = glyph(&name) else {
        return rsx! {};
    };
    rsx! {
        svg {
            class: "tk-icon",
            view_box: "0 0 24 24",
            width: "{size}",
            height: "{size}",
            fill: if filled { "currentColor" } else { "none" },
            stroke: "currentColor",
            "stroke-width": "2",
            "stroke-linecap": "round",
            "stroke-linejoin": "round",
            "aria-hidden": "true",
            path { d: "{d}" }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::glyph;

    #[test]
    fn every_icon_used_by_the_ui_exists() {
        for n in [
            "home", "search", "extensions", "link", "settings", "play", "pause", "back", "close", "minimize",
            "maximize", "restore", "menu", "volume", "mute", "plus", "trash", "refresh", "check", "next",
            "enhance", "shield", "chevron-up", "chevron-down", "user", "fullscreen", "film",
        ] {
            assert!(glyph(n).is_some(), "{n}");
        }
        assert!(glyph("nope").is_none());
    }
}
