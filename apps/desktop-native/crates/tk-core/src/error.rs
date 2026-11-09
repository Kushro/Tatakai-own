use thiserror::Error;

/// Failure of one metadata provider call. The router decides what trips the cool-down.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ProviderError {
    #[error("{provider} rate limited")]
    RateLimited { provider: String },
    #[error("http status {0}")]
    Http(u16),
    #[error("network error: {0}")]
    Network(String),
    #[error("unexpected payload: {0}")]
    Parse(String),
    /// The provider does not implement this capability; never trips a cool-down.
    #[error("unsupported")]
    Unsupported,
}

impl ProviderError {
    /// Transport / server failures take the provider out of rotation; a bad request or an unsupported
    /// capability does not (a mapper bug on one entry must not disable the provider for everything else).
    pub fn trips_cooldown(&self) -> bool {
        match self {
            Self::RateLimited { .. } | Self::Network(_) => true,
            Self::Http(code) => matches!(*code, 401 | 403 | 429) || *code >= 500,
            Self::Parse(_) | Self::Unsupported => false,
        }
    }
}

impl From<reqwest::Error> for ProviderError {
    fn from(e: reqwest::Error) -> Self {
        if let Some(s) = e.status() {
            Self::Http(s.as_u16())
        } else {
            Self::Network(e.without_url().to_string())
        }
    }
}

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("store lock poisoned")]
    Poisoned,
}

// ---- wave A (docs/redesign/15-parity-wave-a-contract.md section 3): one error type per new area ----

/// `secrets.rs` (worker f): OS keyring or the encrypted-at-rest fallback file.
#[derive(Debug, Error)]
pub enum SecretError {
    #[error("keyring: {0}")]
    Keyring(String),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    /// No keyring backend and no fallback directory configured.
    #[error("no secret store available")]
    Unavailable,
}

/// `translate/` (worker d).
#[derive(Debug, Error)]
pub enum TranslateError {
    #[error("translation is off")]
    Off,
    #[error("provider {0} is not configured (key / endpoint)")]
    NotConfigured(String),
    #[error("http status {0}")]
    Http(u16),
    #[error("network error: {0}")]
    Network(String),
    #[error("unexpected payload: {0}")]
    Parse(String),
    #[error("rate limited")]
    RateLimited,
    /// Contract stub answer: the body is not implemented yet.
    #[error("unsupported")]
    Unsupported,
}

impl From<reqwest::Error> for TranslateError {
    fn from(e: reqwest::Error) -> Self {
        match e.status() {
            Some(s) if s.as_u16() == 429 => Self::RateLimited,
            Some(s) => Self::Http(s.as_u16()),
            None => Self::Network(e.without_url().to_string()),
        }
    }
}

/// `lists/` (worker g): AniList / MAL OAuth and GraphQL / REST.
#[derive(Debug, Error)]
pub enum ListError {
    #[error("not connected")]
    NotConnected,
    /// The token was rejected: the UI asks to connect again.
    #[error("token expired or revoked")]
    Unauthorized,
    #[error("oauth: {0}")]
    OAuth(String),
    #[error("http status {0}")]
    Http(u16),
    #[error("network error: {0}")]
    Network(String),
    #[error("unexpected payload: {0}")]
    Parse(String),
    #[error("rate limited")]
    RateLimited,
    #[error("store: {0}")]
    Store(#[from] StoreError),
    #[error("secrets: {0}")]
    Secret(#[from] SecretError),
    #[error("unsupported")]
    Unsupported,
}

impl From<reqwest::Error> for ListError {
    fn from(e: reqwest::Error) -> Self {
        match e.status() {
            Some(s) if s.as_u16() == 401 => Self::Unauthorized,
            Some(s) if s.as_u16() == 429 => Self::RateLimited,
            Some(s) => Self::Http(s.as_u16()),
            None => Self::Network(e.without_url().to_string()),
        }
    }
}

/// `debrid/` (worker i).
#[derive(Debug, Error)]
pub enum DebridError {
    #[error("no api key for {0}")]
    NoKey(String),
    #[error("api key rejected")]
    Unauthorized,
    #[error("not cached on the service")]
    NotCached,
    #[error("no video file matched the episode")]
    NoFile,
    #[error("http status {0}")]
    Http(u16),
    #[error("network error: {0}")]
    Network(String),
    #[error("unexpected payload: {0}")]
    Parse(String),
    #[error("unsupported")]
    Unsupported,
}

impl From<reqwest::Error> for DebridError {
    fn from(e: reqwest::Error) -> Self {
        match e.status() {
            Some(s) if s.as_u16() == 401 || s.as_u16() == 403 => Self::Unauthorized,
            Some(s) => Self::Http(s.as_u16()),
            None => Self::Network(e.without_url().to_string()),
        }
    }
}

// ---- wave B (docs/redesign/16-parity-wave-b-contract.md section 3.5): one error type per new area ----

/// `download/**` (worker h): HLS / MP4 engine, local library.
#[derive(Debug, Error)]
pub enum DownloadError {
    #[error("http status {0}")]
    Http(u16),
    #[error("network error: {0}")]
    Network(String),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("playlist: {0}")]
    Playlist(String),
    #[error("store: {0}")]
    Store(#[from] StoreError),
    #[error("storage limit reached ({limit_bytes} bytes)")]
    StorageLimit { limit_bytes: u64 },
    #[error("download {0} not found")]
    NotFound(i64),
    #[error("file missing or truncated: {0}")]
    Broken(String),
    #[error("cancelled")]
    Cancelled,
}

impl From<reqwest::Error> for DownloadError {
    fn from(e: reqwest::Error) -> Self {
        match e.status() {
            Some(s) => Self::Http(s.as_u16()),
            None => Self::Network(e.without_url().to_string()),
        }
    }
}

/// `manga/**`, `novel/**` (workers l, m): chapter lists, pages, text, offline copies.
#[derive(Debug, Error)]
pub enum ReadError {
    #[error("provider {0}")]
    Provider(#[from] ProviderError),
    #[error("extension: {0}")]
    Extension(String),
    #[error("chapter not found")]
    ChapterNotFound,
    #[error("no pages")]
    NoPages,
    #[error("store: {0}")]
    Store(#[from] StoreError),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

/// `extserver.rs`, `stremio.rs` (worker j): extension-server client and Stremio add-ons.
#[derive(Debug, Error)]
pub enum ExtServerError {
    #[error("http status {0}")]
    Http(u16),
    #[error("network error: {0}")]
    Network(String),
    /// `pairing_required`, `pairing_expired`, `authentication_required`, `device_not_authorized` (docs/redesign/13 section 3.2).
    #[error("server refused: {0}")]
    Refused(String),
    #[error("unexpected payload: {0}")]
    Parse(String),
    #[error("not paired: {0}")]
    NotPaired(String),
}

impl From<reqwest::Error> for ExtServerError {
    fn from(e: reqwest::Error) -> Self {
        match e.status() {
            Some(s) => Self::Http(s.as_u16()),
            None => Self::Network(e.without_url().to_string()),
        }
    }
}
