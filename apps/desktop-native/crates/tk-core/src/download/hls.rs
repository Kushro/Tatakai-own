//! HLS playlist helpers (worker **h**): master -> variant choice by `Settings::downloads` quality, media playlist ->
//! segment list with absolute URLs, `#EXT-X-KEY` AES-128 handling (download the key, decrypt per segment: h adds the
//! `aes` + `cbc` crates through `NOTES-deps.md` if the pure-Rust ones fit; without them encrypted playlists are
//! reported as `DownloadError::Playlist("encrypted")`). Parsing with `m3u8_rs` (already in the workspace).

use url::Url;

use crate::error::DownloadError;

/// Resolve a segment / variant URI against the playlist URL (pure; tested).
pub fn absolute(base: &str, uri: &str) -> Result<String, DownloadError> {
    let base = Url::parse(base).map_err(|e| DownloadError::Playlist(e.to_string()))?;
    base.join(uri).map(|u| u.to_string()).map_err(|e| DownloadError::Playlist(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::absolute;

    #[test]
    fn relative_and_absolute_uris() {
        assert_eq!(absolute("https://h/a/master.m3u8", "seg1.ts").unwrap(), "https://h/a/seg1.ts");
        assert_eq!(absolute("https://h/a/master.m3u8", "/root/seg1.ts").unwrap(), "https://h/root/seg1.ts");
        assert_eq!(absolute("https://h/a/master.m3u8", "https://cdn/x.ts").unwrap(), "https://cdn/x.ts");
    }
}
