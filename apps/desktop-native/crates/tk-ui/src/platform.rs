//! Thin wrappers over the window / web view: native handle, JS bridges.

use dioxus::prelude::*;

/// HWND of the main window (0 off Windows). Must be called from the UI thread.
pub fn main_hwnd() -> isize {
    #[cfg(windows)]
    {
        use dioxus::desktop::tao::platform::windows::WindowExtWindows;
        dioxus::desktop::window().window.hwnd()
    }
    #[cfg(not(windows))]
    {
        0
    }
}

/// Seconds since the Unix epoch.
pub fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Milliseconds on a monotonic-ish clock (since the Unix epoch; only differences matter).
pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Set `data-palette` / `data-mode` / `data-contrast` on `<html>` (tokens.css selectors).
pub fn apply_theme_js(palette: &str, mode: &str, contrast: Option<&str>) -> String {
    // Values come from fixed enums / validated palette ids, but still go through JSON escaping.
    let q = |s: &str| serde_json::to_string(s).unwrap_or_else(|_| "\"\"".into());
    let contrast_js = match contrast {
        Some(c) => format!("r.setAttribute('data-contrast',{});", q(c)),
        None => "r.removeAttribute('data-contrast');".to_owned(),
    };
    format!(
        "const r=document.documentElement;r.setAttribute('data-palette',{});r.setAttribute('data-mode',{});{contrast_js}",
        q(palette),
        q(mode)
    )
}

pub fn apply_theme(palette: &str, mode: &str, contrast: Option<&str>) {
    let _ = document::eval(&apply_theme_js(palette, mode, contrast));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn theme_script_sets_and_clears_contrast() {
        let js = apply_theme_js("sakura", "amoled", Some("high"));
        assert!(js.contains("'data-palette',\"sakura\""));
        assert!(js.contains("'data-mode',\"amoled\""));
        assert!(js.contains("'data-contrast',\"high\""));
        assert!(apply_theme_js("sakura", "dark", None).contains("removeAttribute('data-contrast')"));
    }

    #[test]
    fn theme_script_escapes_quotes() {
        assert!(apply_theme_js("a\"b", "dark", None).contains("\\\""));
    }
}
