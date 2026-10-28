//! Helpers shared by the offline and live backends.

use tk_ui::model::{ErrorKind, UiError};

/// UAC firewall flow on a blocking thread.
pub async fn allow_private_network() -> Result<(), UiError> {
    let exe = std::env::current_exe().map_err(|e| UiError::new(ErrorKind::Other, e.to_string()))?;
    tokio::task::spawn_blocking(move || crate::firewall::allow_private_network(&exe, crate::firewall::DEFAULT_PORT))
        .await
        .map_err(|e| UiError::new(ErrorKind::Other, e.to_string()))?
        .map_err(|e| UiError::new(ErrorKind::Other, e))
}

#[cfg_attr(not(feature = "live"), allow(dead_code))]
/// Parse a UI media id: `"1234"` (AniList), `"mal:55"` or `"kitsu:7"`.
pub fn parse_media_id(id: &str) -> Option<(IdKind, u32)> {
    let (kind, num) = match id.split_once(':') {
        Some(("mal", n)) => (IdKind::Mal, n),
        Some(("kitsu", n)) => (IdKind::Kitsu, n),
        Some(_) => return None,
        None => (IdKind::AniList, id),
    };
    num.trim().parse::<u32>().ok().filter(|n| *n > 0).map(|n| (kind, n))
}

#[cfg_attr(not(feature = "live"), allow(dead_code))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdKind {
    AniList,
    Mal,
    Kitsu,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn media_ids() {
        assert_eq!(parse_media_id("21"), Some((IdKind::AniList, 21)));
        assert_eq!(parse_media_id("mal:5114"), Some((IdKind::Mal, 5114)));
        assert_eq!(parse_media_id("kitsu:7"), Some((IdKind::Kitsu, 7)));
        assert_eq!(parse_media_id("0"), None);
        assert_eq!(parse_media_id("abc"), None);
        assert_eq!(parse_media_id("x:1"), None);
        assert_eq!(parse_media_id("mal:"), None);
    }
}
