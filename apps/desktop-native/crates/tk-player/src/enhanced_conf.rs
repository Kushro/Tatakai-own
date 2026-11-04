//! Pure rules for the "Embedded enhanced" engine (mpv-anime-build, docs/redesign/09 section 3.3, owner B). No
//! libmpv here, so `cargo test` covers them without the `mpv` feature.
//!
//! * [`sanitize_mpv_conf`]: the build's `mpv.conf` is written for a standalone mpv with its own window. tk-app runs
//!   it through this once, at extraction time (the upstream file is kept as [`UPSTREAM_CONF`]). Options that would
//!   fight the embedding are commented out in every section, including the `profile-cond` ones that mpv's
//!   auto-profiles apply asynchronously after start-up (so overriding them from code is not enough): video output and
//!   GPU API / context, window geometry and fullscreen, IPC servers, watch-later resume (it would beat our `start`).
//! * [`select_scripts`]: `load-scripts=no` + this explicit list. Always left out: scripts that open IPC endpoints,
//!   check for updates over the network, resume / save positions, touch the audio device or the mpv window, or run
//!   PowerShell; without uosc also uosc itself and thumbfast (it only feeds uosc's timeline).

use std::path::{Path, PathBuf};

use crate::engine::EngineMode;

/// Bump when [`sanitize_mpv_conf`] or the extraction layout changes: tk-app re-extracts when the marker differs.
pub const SANITIZE_REV: u32 = 1;
/// The build's original `mpv.conf`, kept next to the sanitized one.
pub const UPSTREAM_CONF: &str = "mpv.conf.upstream";
/// Prefix of a line commented out by [`sanitize_mpv_conf`].
pub const DISABLED_PREFIX: &str = "# tatakai (embedded): ";

/// Options (without `--` / `no-`) that the embedding owns.
const BLOCKED_OPTIONS: &[&str] = &[
    "vo",
    "gpu-api",
    "gpu-context",
    "hwdec",
    "wid",
    "input-ipc-server",
    "input-default-bindings",
    "input-vo-keyboard",
    "osc",
    "fullscreen",
    "fs",
    "border",
    "geometry",
    "autofit",
    "autofit-larger",
    "autofit-smaller",
    "force-window",
    "ontop",
    "idle",
    "keep-open",
    "save-position-on-quit",
    "resume-playback",
];
/// Option name prefixes the embedding owns (Windows-only D3D11 tuning).
const BLOCKED_PREFIXES: &[&str] = &["d3d11-"];

/// Scripts never loaded embedded (file or directory names under `scripts/`).
const EXCLUDED_SCRIPTS: &[&str] = &[
    "mpvSockets.lua",        // IPC endpoints per instance
    "update_check.lua",      // network version check, opens the browser
    "youtube-download.lua",  // yt-dlp downloads
    "power_manager.lua",     // PowerShell power plan checks
    "auto-audio-device.lua", // switches the system audio device (PowerShell)
    "autosave.lua",          // watch-later saves: the app records progress
    "auto-save-state.lua",
    "autoload.lua",          // directory playlists: meaningless for streams
    "pip.lua",               // resizes / pins the mpv window
    "mpv-scripts.disable",
];
/// Scripts that only make sense with uosc's own UI.
const UOSC_ONLY_SCRIPTS: &[&str] = &["uosc", "thumbfast.lua"];

/// Our Anime4K / FSR presets apply only to the plain embedded engine; the build brings its own shader profiles.
pub fn shader_presets_apply(mode: &EngineMode) -> bool {
    !mode.is_enhanced()
}

/// The option name a config line sets, without `--` and `no-` (`None` for blanks, comments and `[profile]` headers).
fn option_name(line: &str) -> Option<&str> {
    let t = line.trim();
    if t.is_empty() || t.starts_with('#') || t.starts_with('[') {
        return None;
    }
    let t = t.trim_start_matches("--");
    let name = t.split(['=', ' ', '\t', '#']).next().unwrap_or("").trim();
    if name.is_empty() {
        return None;
    }
    Some(name.strip_prefix("no-").unwrap_or(name))
}

fn is_blocked(name: &str) -> bool {
    BLOCKED_OPTIONS.contains(&name) || BLOCKED_PREFIXES.iter().any(|p| name.starts_with(p))
}

/// Comment out the options the embedding owns; everything else (profiles, shaders, subtitles, audio) is kept as is.
pub fn sanitize_mpv_conf(src: &str) -> String {
    let mut out = String::with_capacity(src.len() + 512);
    for line in src.lines() {
        if option_name(line).is_some_and(is_blocked) {
            out.push_str(DISABLED_PREFIX);
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

/// Whether the entry `name` of `scripts/` loads in this engine.
pub fn script_allowed(name: &str, uosc: bool) -> bool {
    if EXCLUDED_SCRIPTS.contains(&name) {
        return false;
    }
    uosc || !UOSC_ONLY_SCRIPTS.contains(&name)
}

/// mpv's own auto-load rule: `*.lua` / `*.js` files and directories holding `main.lua` / `main.js`.
fn is_script(path: &Path) -> bool {
    if path.is_dir() {
        return path.join("main.lua").is_file() || path.join("main.js").is_file();
    }
    matches!(path.extension().and_then(|e| e.to_str()), Some("lua" | "js"))
}

/// The scripts of `<config_dir>/scripts` to load, sorted by name like mpv's auto-load.
pub fn select_scripts(config_dir: &Path, uosc: bool) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(config_dir.join("scripts")) else {
        return Vec::new();
    };
    let mut out: Vec<PathBuf> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| script_allowed(n, uosc)))
        .filter(|p| is_script(p))
        .collect();
    out.sort();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONF: &str = "\
[Platform-Linux-Base]
profile-cond=platform ~= \"windows\"
vo=gpu-next
gpu-api=vulkan,auto
fbo-format=rgba16hf
hwdec=nvdec,vulkan,auto

[SVP-Unix-Patch]
input-ipc-server=/tmp/mpvsocket
[default]
--osd-level=1
save-position-on-quit=yes
fullscreen=yes
no-border
osc=no
fs
d3d11-flip=yes
sub-font=\"Hey Comic\"
glsl-shaders-append=\"~~/shaders/KrigBilateral.glsl\"
# vo=gpu
";

    #[test]
    fn sanitize_comments_out_only_what_the_embedding_owns() {
        let out = sanitize_mpv_conf(CONF);
        for kept in [
            "[Platform-Linux-Base]",
            "profile-cond=platform ~= \"windows\"",
            "fbo-format=rgba16hf",
            "--osd-level=1",
            "sub-font=\"Hey Comic\"",
            "glsl-shaders-append=\"~~/shaders/KrigBilateral.glsl\"",
            "# vo=gpu",
        ] {
            assert!(out.lines().any(|l| l == kept), "{kept} should be kept");
        }
        for dropped in [
            "vo=gpu-next",
            "gpu-api=vulkan,auto",
            "hwdec=nvdec,vulkan,auto",
            "input-ipc-server=/tmp/mpvsocket",
            "save-position-on-quit=yes",
            "fullscreen=yes",
            "no-border",
            "osc=no",
            "fs",
            "d3d11-flip=yes",
        ] {
            assert!(!out.lines().any(|l| l == dropped), "{dropped} should be commented out");
            let commented = format!("{DISABLED_PREFIX}{dropped}");
            assert!(out.lines().any(|l| l == commented), "{dropped} kept as a comment");
        }
        assert_eq!(out.lines().count(), CONF.lines().count());
    }

    #[test]
    fn option_names_ignore_dashes_negation_and_comments() {
        assert_eq!(option_name("--osd-level=1"), Some("osd-level"));
        assert_eq!(option_name("no-border"), Some("border"));
        assert_eq!(option_name("temporal-dither=yes   # comment"), Some("temporal-dither"));
        assert_eq!(option_name("interpolation-preserve"), Some("interpolation-preserve"));
        assert_eq!(option_name("  # x=1"), None);
        assert_eq!(option_name("[default]"), None);
        assert_eq!(option_name(""), None);
    }

    #[test]
    fn uosc_and_thumbfast_load_only_with_uosc() {
        assert!(!script_allowed("uosc", false));
        assert!(!script_allowed("thumbfast.lua", false));
        assert!(script_allowed("uosc", true));
        assert!(script_allowed("thumbfast.lua", true));
        assert!(script_allowed("anime_profile_controller.lua", false));
        for name in ["mpvSockets.lua", "update_check.lua", "autosave.lua", "pip.lua"] {
            assert!(!script_allowed(name, true), "{name}");
        }
    }

    #[test]
    fn select_scripts_follows_mpv_autoload_rules() {
        let dir = std::env::temp_dir().join(format!("tk-enhanced-conf-{}", std::process::id()));
        let scripts = dir.join("scripts");
        std::fs::create_dir_all(scripts.join("uosc")).unwrap();
        std::fs::create_dir_all(scripts.join("mpv-scripts.disable")).unwrap();
        std::fs::create_dir_all(scripts.join("empty-dir")).unwrap();
        std::fs::write(scripts.join("uosc").join("main.lua"), "").unwrap();
        std::fs::write(scripts.join("b.lua"), "").unwrap();
        std::fs::write(scripts.join("a.js"), "").unwrap();
        std::fs::write(scripts.join("notes.txt"), "").unwrap();
        std::fs::write(scripts.join("mpvSockets.lua"), "").unwrap();

        let names = |uosc| -> Vec<String> {
            select_scripts(&dir, uosc)
                .iter()
                .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
                .collect()
        };
        assert_eq!(names(false), ["a.js", "b.lua"]);
        assert_eq!(names(true), ["a.js", "b.lua", "uosc"]);
        assert!(select_scripts(&dir.join("missing"), true).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn presets_apply_only_to_the_plain_engine() {
        assert!(shader_presets_apply(&EngineMode::Embedded));
        assert!(!shader_presets_apply(&EngineMode::Enhanced { config_dir: PathBuf::from("x"), uosc: false }));
    }
}
