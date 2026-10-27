//! QR rendering for the Link pairing payload (`tatakai-link:v1?...`). Inline SVG, black on white with a
//! quiet zone: scanners need real contrast regardless of the app palette.
//! Display approach after Enigmata-Temple `temple-ui` (same author; see NOTICES-B6.md).

use qrcode::QrCode;
use qrcode::render::svg;

/// SVG markup for `data`, or `None` if it does not fit in a QR code.
pub fn qr_svg(data: &str) -> Option<String> {
    let code = QrCode::new(data.as_bytes()).ok()?;
    let svg = code
        .render::<svg::Color>()
        .min_dimensions(256, 256)
        .quiet_zone(true)
        .dark_color(svg::Color("#000000"))
        .light_color(svg::Color("#ffffff"))
        .build();
    // Drop the XML prolog so the markup can be inlined in HTML.
    svg.find("<svg").map(|i| svg[i..].to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_inline_svg_without_prolog() {
        let s = qr_svg("tatakai-link:v1?h=192.168.1.20&p=47321&ca=abc&c=ABCDEFGHJK&x=1&n=PC").unwrap();
        assert!(s.starts_with("<svg"));
        assert!(!s.contains("<?xml"));
        assert!(s.contains("#000000") && s.contains("#ffffff"));
    }

    #[test]
    fn oversized_payload_is_rejected() {
        assert!(qr_svg(&"x".repeat(10_000)).is_none());
    }
}
