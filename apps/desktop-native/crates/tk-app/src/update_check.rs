//! New-version notice (wave A, worker **f**; docs/redesign/15 section 4): `GET https://api.github.com/repos/
//! Kushro/Tatakai-own/releases/latest` (`User-Agent: TatakaiNext/<version>`, `Accept: application/vnd.github+json`),
//! compare `tag_name` (strip a leading `v`) with `CARGO_PKG_VERSION` through `semver`, never download anything.
//! `main.rs` runs it once at start-up when `Settings::system.check_updates` and the result is not the
//! `skipped_version`; the UI shows a snackbar with "Open release". Contract: types final, body a stub.

pub const RELEASES_API: &str = "https://api.github.com/repos/Kushro/Tatakai-own/releases/latest";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Latest {
    /// Release tag as published (`v0.2.0`).
    pub tag: String,
    pub url: String,
    pub notes: Option<String>,
}

/// `true` when `tag` (with or without a leading `v`) is a newer semver than `current`. Pure.
pub fn is_newer(current: &str, tag: &str) -> bool {
    let parse = |s: &str| semver::Version::parse(s.trim().trim_start_matches(['v', 'V'])).ok();
    match (parse(current), parse(tag)) {
        (Some(c), Some(t)) => t > c,
        _ => false,
    }
}

/// Contract stub (worker f).
pub async fn latest(client: &reqwest::Client) -> Result<Option<Latest>, String> {
    let _ = client;
    Err("update check is not wired in this build".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newer_compares_semver_and_ignores_the_v_prefix() {
        assert!(is_newer("0.1.0", "v0.2.0"));
        assert!(!is_newer("0.2.0", "v0.2.0"));
        assert!(!is_newer("0.3.0", "0.2.9"));
        assert!(!is_newer("0.1.0", "latest"));
    }
}
