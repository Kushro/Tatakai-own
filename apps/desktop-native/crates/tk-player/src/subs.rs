//! Subtitle style -> mpv `sub-*` options (pure, tested without libmpv). Wave A, worker **d** owns this file
//! (docs/redesign/15-parity-wave-a-contract.md section 4): add fields and mappings here; `Player::set_sub_style`
//! applies whatever `sub_options` returns. The UI keeps its own `SubtitleStyleVm` and maps it to [`SubStyle`].

/// What the player applies. Plain numbers in mpv's own units so the mapping stays obvious.
#[derive(Debug, Clone, PartialEq)]
pub struct SubStyle {
    /// `sub-scale` (1.0 = mpv default).
    pub scale: f64,
    /// `sub-font`; `None` keeps mpv's default font.
    pub font: Option<String>,
    /// `sub-color` as `#RRGGBB`.
    pub color: String,
    /// Background box alpha 0..=100 (`sub-back-color` `#AARRGGBB`, black); 0 = no box.
    pub back_alpha: u8,
    /// `sub-border-size` in px.
    pub border_px: f64,
    /// `sub-border-color` as `#RRGGBB` (the outline; mpv's default is black).
    pub border_color: String,
    /// `sub-pos` 0..=100 (100 = default bottom position).
    pub pos: u8,
    /// `sub-bold`
    pub bold: bool,
}

impl Default for SubStyle {
    fn default() -> Self {
        Self { scale: 1.0, font: None, color: "#FFFFFF".to_owned(), back_alpha: 0, border_px: 2.0, border_color: "#000000".to_owned(), pos: 100, bold: false }
    }
}

/// `(mpv option, value)` pairs for `Player::set_sub_style` / `set_option_str`.
pub fn sub_options(style: &SubStyle) -> Vec<(&'static str, String)> {
    let scale = if style.scale.is_finite() { style.scale.clamp(0.5, 2.0) } else { 1.0 };
    let alpha = (u32::from(style.back_alpha.min(100)) * 255).div_ceil(100);
    let mut out = vec![
        ("sub-scale", format!("{scale:.2}")),
        ("sub-color", style.color.clone()),
        ("sub-back-color", format!("#{alpha:02X}000000")),
        ("sub-border-size", format!("{:.1}", style.border_px.clamp(0.0, 5.0))),
        ("sub-border-color", style.border_color.clone()),
        ("sub-pos", style.pos.min(100).to_string()),
        ("sub-bold", if style.bold { "yes" } else { "no" }.to_owned()),
    ];
    if let Some(font) = style.font.as_deref().map(str::trim).filter(|f| !f.is_empty()) {
        out.push(("sub-font", font.to_owned()));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_style_maps_to_mpv_defaults_and_alpha_is_hex() {
        let opts = sub_options(&SubStyle::default());
        let get = |k: &str| opts.iter().find(|(n, _)| *n == k).map(|(_, v)| v.as_str());
        assert_eq!(get("sub-scale"), Some("1.00"));
        assert_eq!(get("sub-back-color"), Some("#00000000"));
        assert_eq!(get("sub-pos"), Some("100"));
        assert_eq!(get("sub-font"), None);
        let boxed = sub_options(&SubStyle { back_alpha: 100, font: Some(" Outfit ".into()), bold: true, ..SubStyle::default() });
        let get = |k: &str| boxed.iter().find(|(n, _)| *n == k).map(|(_, v)| v.as_str());
        assert_eq!(get("sub-back-color"), Some("#FF000000"));
        assert_eq!(get("sub-font"), Some("Outfit"));
        assert_eq!(get("sub-bold"), Some("yes"));
    }

    #[test]
    fn outline_colour_and_bounds() {
        let opts = sub_options(&SubStyle { border_color: "#FF0000".into(), border_px: 9.0, scale: 7.0, pos: 250, ..SubStyle::default() });
        let get = |k: &str| opts.iter().find(|(n, _)| *n == k).map(|(_, v)| v.as_str());
        assert_eq!(get("sub-border-color"), Some("#FF0000"));
        assert_eq!(get("sub-border-size"), Some("5.0"));
        assert_eq!(get("sub-scale"), Some("2.00"));
        assert_eq!(get("sub-pos"), Some("100"));
    }
}
