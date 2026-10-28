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
pub enum SupabaseError {
    #[error("not signed in")]
    NotSignedIn,
    #[error("invalid credentials")]
    InvalidCredentials,
    #[error("session expired")]
    SessionExpired,
    #[error("http status {0}")]
    Http(u16),
    #[error("network error: {0}")]
    Network(String),
    #[error("unexpected payload: {0}")]
    Parse(String),
    #[error("invalid supabase url: {0}")]
    BadUrl(String),
}

impl From<reqwest::Error> for SupabaseError {
    fn from(e: reqwest::Error) -> Self {
        Self::Network(e.without_url().to_string())
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
