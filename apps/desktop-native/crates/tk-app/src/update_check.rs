//! New-version notice (wave A, worker **f**; docs/redesign/15 section 4; classic `desktop/services/update-manager.cjs`,
//! notice only): `GET https://api.github.com/repos/Kushro/Tatakai-own/releases/latest`
//! (`User-Agent: TatakaiNext/<version>`, `Accept: application/vnd.github+json`), compare `tag_name` (strip a leading
//! `v`) with `CARGO_PKG_VERSION` through `semver`, never download anything. `live/system.rs::on_start` runs it once
//! per launch when `Settings::system.check_updates`; the shell's snackbar (`tk_ui::tray`) offers "Open" and "Skip
//! this version" (`Settings::system.skipped_version`).

pub const RELEASES_API: &str = "https://api.github.com/repos/Kushro/Tatakai-own/releases/latest";
/// Release notes longer than this are cut (the Licenses / changelog sheet shows them as plain text).
pub const MAX_NOTES_CHARS: usize = 8_000;

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

/// Parse the body of `releases/latest`. Drafts and pre-releases are not offered; `html_url` must be an https
/// GitHub page (the UI opens it in the browser). Pure.
pub fn parse_latest(body: &str) -> Result<Option<Latest>, String> {
    #[derive(serde::Deserialize)]
    struct Release {
        tag_name: Option<String>,
        html_url: Option<String>,
        body: Option<String>,
        #[serde(default)]
        draft: bool,
        #[serde(default)]
        prerelease: bool,
    }
    let r: Release = serde_json::from_str(body).map_err(|e| format!("invalid release JSON: {e}"))?;
    if r.draft || r.prerelease {
        return Ok(None);
    }
    let Some(tag) = r.tag_name.map(|t| t.trim().to_owned()).filter(|t| !t.is_empty()) else { return Ok(None) };
    let url = r
        .html_url
        .filter(|u| u.starts_with("https://github.com/"))
        .unwrap_or_else(|| format!("https://github.com/Kushro/Tatakai-own/releases/tag/{tag}"));
    let notes = r.body.map(|b| b.trim().to_owned()).filter(|b| !b.is_empty()).map(|b| {
        if b.chars().count() > MAX_NOTES_CHARS { b.chars().take(MAX_NOTES_CHARS).collect::<String>() + "…" } else { b }
    });
    Ok(Some(Latest { tag, url, notes }))
}

/// The latest published release. `Ok(None)` = the repository has no (stable) release yet (GitHub answers 404).
#[cfg(feature = "live")]
pub async fn latest(client: &reqwest::Client) -> Result<Option<Latest>, String> {
    let resp = client
        .get(RELEASES_API)
        .header(reqwest::header::USER_AGENT, concat!("TatakaiNext/", env!("CARGO_PKG_VERSION")))
        .header(reqwest::header::ACCEPT, "application/vnd.github+json")
        .send()
        .await
        .map_err(|e| e.without_url().to_string())?;
    let status = resp.status();
    if status == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    if !status.is_success() {
        return Err(format!("GitHub answered {status}"));
    }
    let body = resp.text().await.map_err(|e| e.without_url().to_string())?;
    parse_latest(&body)
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
        assert!(is_newer("0.1.0", "V0.1.1"));
    }

    #[test]
    fn release_json_is_parsed_and_filtered() {
        let body = r#"{"tag_name":"v7.1.0","html_url":"https://github.com/Kushro/Tatakai-own/releases/tag/v7.1.0","body":"  Fixes\n- a  ","draft":false,"prerelease":false}"#;
        let l = parse_latest(body).unwrap().unwrap();
        assert_eq!(l.tag, "v7.1.0");
        assert_eq!(l.url, "https://github.com/Kushro/Tatakai-own/releases/tag/v7.1.0");
        assert_eq!(l.notes.as_deref(), Some("Fixes\n- a"));
        // pre-releases / drafts are not offered
        assert_eq!(parse_latest(r#"{"tag_name":"v8.0.0-rc1","prerelease":true}"#).unwrap(), None);
        assert_eq!(parse_latest(r#"{"tag_name":"v8.0.0","draft":true}"#).unwrap(), None);
        // a missing or foreign html_url falls back to the tag page
        let l = parse_latest(r#"{"tag_name":"v1.0.0","html_url":"http://evil.example/x"}"#).unwrap().unwrap();
        assert_eq!(l.url, "https://github.com/Kushro/Tatakai-own/releases/tag/v1.0.0");
        assert_eq!(l.notes, None);
        assert!(parse_latest("{nope").is_err());
        assert_eq!(parse_latest("{}").unwrap(), None);
    }

    #[test]
    fn long_notes_are_cut() {
        let long = "x".repeat(MAX_NOTES_CHARS + 50);
        let body = serde_json::json!({ "tag_name": "v1.0.0", "body": long }).to_string();
        let notes = parse_latest(&body).unwrap().unwrap().notes.unwrap();
        assert_eq!(notes.chars().count(), MAX_NOTES_CHARS + 1);
    }
}
