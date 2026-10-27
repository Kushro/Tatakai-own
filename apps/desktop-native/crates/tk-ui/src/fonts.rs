//! Bundled fonts (never a CDN: offline + privacy). The files live in `assets/fonts/` next to the exe
//! (`packaging/fetch-deps.ps1` downloads Space Grotesk and Outfit against `fonts.lock.json`; JetBrains
//! Mono ships in the repo with its OFL text). Missing files are skipped and the `system-ui` fallback
//! from tokens.css applies.

use std::path::Path;

use base64::Engine;

/// `(css family, file name, weight range)`; all variable fonts.
pub const FONTS: [(&str, &str, &str); 3] = [
    ("Space Grotesk", "SpaceGrotesk.woff2", "300 700"),
    ("Outfit", "Outfit.woff2", "100 900"),
    ("JetBrains Mono", "JetBrainsMono.woff2", "100 800"),
];

/// `@font-face` rules with the font data inlined as data URIs (no asset-protocol dependency).
pub fn font_face_css(dir: &Path) -> String {
    let mut css = String::new();
    for (family, file, weights) in FONTS {
        let Ok(bytes) = std::fs::read(dir.join(file)) else {
            tracing::warn!(family, "font file missing, falling back to system-ui");
            continue;
        };
        let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
        css.push_str(&format!(
            "@font-face{{font-family:\"{family}\";font-weight:{weights};font-style:normal;font-display:swap;\
             src:url(data:font/woff2;base64,{b64}) format(\"woff2\");}}\n"
        ));
    }
    css
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emits_a_rule_per_existing_file_only() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Outfit.woff2"), [1u8, 2, 3]).unwrap();
        let css = font_face_css(dir.path());
        assert_eq!(css.matches("@font-face").count(), 1);
        assert!(css.contains("font-family:\"Outfit\""));
        assert!(css.contains("base64,AQID"));
    }

    #[test]
    fn missing_directory_yields_no_css() {
        assert!(font_face_css(&std::env::temp_dir().join("tk-ui-no-such-fonts-dir")).is_empty());
    }

    #[test]
    fn repo_ships_jetbrains_mono_with_its_license() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/fonts");
        assert!(dir.join("JetBrainsMono.woff2").is_file());
        assert!(dir.join("OFL-JetBrainsMono.txt").is_file());
    }
}
