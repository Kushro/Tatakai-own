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
        "chevron-left" => ("M15 6l-6 6 6 6", false),
        "chevron-right" => ("M9 6l6 6-6 6", false),
        "library" => ("M4 4h4v16H4zM10 4h4v16h-4zM16 5l4 1-3 14-4-1", false),
        "history" => ("M4 12a8 8 0 1 0 2.3-5.7M4 4v4h4M12 8v4l3 2", false),
        "tune" => ("M4 6h9M17 6h3M4 12h3M11 12h9M4 18h11M19 18h1M15 4v4M9 10v4M17 16v4", false),
        "language" => ("M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18zM3 12h18M12 3c3 3 3 15 0 18M12 3c-3 3-3 15 0 18", false),
        "download" => ("M12 4v11M7 10l5 5 5-5M5 20h14", false),
        "upload" => ("M12 20V9M7 14l5-5 5 5M5 4h14", false),
        "cc" => ("M4 6h16v12H4zM10.5 10.5a2 2 0 1 0 0 3M17 10.5a2 2 0 1 0 0 3", false),
        "audio" => ("M9 18V6l10-2v12M9 18a3 3 0 1 1-6 0 3 3 0 0 1 6 0zM19 16a3 3 0 1 1-6 0 3 3 0 0 1 6 0z", false),
        "hd" => ("M4 6h16v12H4zM8 9v6M11 9v6M8 12h3M14 9h2a3 3 0 0 1 0 6h-2z", false),
        "star" => ("M12 3l2.7 5.6 6.1.9-4.4 4.3 1 6.1L12 17l-5.4 2.9 1-6.1-4.4-4.3 6.1-.9z", true),
        "copy" => ("M9 9h11v11H9zM5 15H4V4h11v1", false),
        "qr" => ("M4 4h6v6H4zM14 4h6v6h-6zM4 14h6v6H4zM14 14h2v2h-2zM18 18h2v2h-2zM14 18h2M18 14h2", false),
        "camera" => ("M4 8h3l2-3h6l2 3h3v11H4zM12 10a3.5 3.5 0 1 0 0 7 3.5 3.5 0 0 0 0-7z", false),
        "desktop" => ("M3 4h18v12H3zM8 20h8M12 16v4", false),
        "phone" => ("M7 2h10v20H7zM11 18h2", false),
        "gpu" => ("M3 7h18v10H3zM7 17v3M17 17v3M8 10h3v4H8zM14 12h3", false),
        "cast" => ("M3 8V5h18v14h-7M3 12a7 7 0 0 1 7 7M3 16a3 3 0 0 1 3 3", false),
        "info" => ("M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18zM12 11v6M12 7.5v.5", false),
        "warning" => ("M12 4l9 16H3zM12 10v4M12 17v.5", false),
        "sync" => ("M20 8a8 8 0 0 0-14.3-2M4 4v4h4M4 16a8 8 0 0 0 14.3 2M20 20v-4h-4", false),
        "filter" => ("M4 5h16l-6 8v6l-4-2v-4z", false),
        "sort" => ("M7 4v16M3 16l4 4 4-4M17 20V4M13 8l4-4 4 4", false),
        "pip" => ("M3 5h18v14H3zM12 12h7v5h-7z", false),
        "screenshot" => ("M4 8V4h4M16 4h4v4M20 16v4h-4M8 20H4v-4M12 9a3 3 0 1 0 0 6 3 3 0 0 0 0-6z", false),
        "speed" => ("M4 17a8 8 0 1 1 16 0M12 17l4-6", false),
        "skip" => ("M5 5l8 7-8 7zM13 5l8 7-8 7z", false),
        "more" => ("M12 6v.5M12 12v.5M12 18v.5", false),
        "edit" => ("M4 20h4L19 9l-4-4L4 16zM13 7l4 4", false),
        "folder" => ("M3 6h6l2 2h10v11H3z", false),
        "keyboard" => ("M3 6h18v12H3zM7 10h.5M11 10h.5M15 10h.5M7 14h10", false),
        "external" => ("M14 4h6v6M20 4l-9 9M18 14v6H4V6h6", false),
        "clock" => ("M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18zM12 7v5l3 2", false),
        "calendar" => ("M4 6h16v14H4zM4 10h16M8 3v4M16 3v4", false),
        "bookmark" => ("M6 3h12v18l-6-4-6 4z", false),
        "grid" => ("M4 4h7v7H4zM13 4h7v7h-7zM4 13h7v7H4zM13 13h7v7h-7z", false),
        "palette" => ("M12 3a9 9 0 0 0 0 18c1.5 0 2-1 2-2s-1-2 0-3h3a4 4 0 0 0 4-4c0-5-4-9-9-9zM7.5 11.5h.5M10 7.5h.5M15 8h.5", false),
        // wave 1 D (Link, Settings, title bar chips)
        "offline" => ("M8.5 7A6 6 0 0 1 18 10.1 4 4 0 0 1 19.5 17.6M16 18H6a4.5 4.5 0 0 1-.5-9M3 3l18 18", false),
        "storage" => ("M4 4h16v5H4zM4 10h16v5H4zM4 16h16v4H4zM7 6.5h.5M7 12.5h.5M7 18h.5", false),
        "terminal" => ("M3 5h18v14H3zM7 9l3 3-3 3M12 15h5", false),
        "update" => ("M20 12a8 8 0 1 1-2.3-5.7M20 4v5h-5M12 8v4l3 2", false),
        "logout" => ("M10 4H4v16h6M14 8l4 4-4 4M18 12H9", false),
        "tablet" => ("M5 3h14v18H5zM11 18h2", false),
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
            "enhance", "shield", "chevron-up", "chevron-down", "user", "fullscreen", "film", "chevron-left", "chevron-right", "library", "history", "tune",
            "language", "download", "upload", "cc", "audio", "hd", "star", "copy", "qr", "camera", "desktop", "phone",
            "gpu", "cast", "info", "warning", "sync", "filter", "sort", "pip", "screenshot", "speed", "skip", "more",
            "edit", "folder", "keyboard", "external", "clock", "calendar", "bookmark", "grid", "palette",
        ] {
            assert!(glyph(n).is_some(), "{n}");
        }
        for n in ["offline", "storage", "terminal", "update", "logout", "tablet"] {
            assert!(glyph(n).is_some(), "{n}");
        }
        assert!(glyph("nope").is_none());
    }
}
