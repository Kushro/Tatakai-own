use thiserror::Error;

/// Install-time failures (invalid package / manifest). Messages name the failing field.
#[derive(Debug, Error)]
pub enum InstallError {
    #[error("{0}")]
    Invalid(String),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

impl InstallError {
    pub(crate) fn invalid(msg: impl Into<String>) -> Self {
        Self::Invalid(msg.into())
    }
}

/// Failures of `ExtensionHost::call` / `invoke`.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ExtError {
    /// Compat level `unsupported`: the extension is installed but cannot run.
    #[error("extension is unsupported by the native runtime")]
    Unsupported,
    #[error("extension '{0}' is not installed")]
    NotInstalled(String),
    #[error("extension '{0}' is disabled")]
    Disabled(String),
    #[error("unknown route '{0}'")]
    RouteNotFound(String),
    #[error("route '{0}' is not enabled by the extension manifest")]
    RouteNotAllowed(String),
    /// HTTP 400 in the legacy host.
    #[error("bad request: {0}")]
    BadRequest(String),
    /// HTTP 501: the bundle does not implement the method.
    #[error("bundle does not implement '{0}'")]
    NotImplemented(String),
    /// Unknown provider / entry (HTTP 404).
    #[error("not found: {0}")]
    NotFound(String),
    #[error("extension call timed out")]
    Timeout,
    #[error("extension ran out of memory")]
    OutOfMemory,
    /// The bundle threw (HTTP 502).
    #[error("extension error: {0}")]
    Js(String),
    #[error("runtime error: {0}")]
    Runtime(String),
}
