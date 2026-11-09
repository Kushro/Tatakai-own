//! JSON envelope and error catalogue of `/api/server/v1` (docs/redesign/13 section 3.2; classic
//! `server/src/api-response.mjs`). Every error is `{ "error": "<code>", "message"?: string }` with the HTTP status the
//! classic server uses, so the Android client's error mapping keeps working.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

/// `GET /health` `protocolVersion` (unchanged since the classic server).
pub const PROTOCOL_VERSION: u32 = 1;

/// Error codes the classic server emits (k keeps the list in sync with `server/src/core.mjs`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApiError {
    /// 401: no token and the server requires one (desktop without a viewer password).
    AuthenticationRequired,
    /// 401: `POST /auth/login` viewer without a configured password on desktop.
    PairingRequired,
    /// 401: code unknown, used or expired.
    PairingExpired,
    /// 401: token revoked / unknown.
    DeviceNotAuthorized,
    /// 403: viewer called an owner route.
    Forbidden,
    /// 409: 50 live pairing codes.
    PairingLimit,
    /// 404.
    NotFound,
    /// 400 with a message.
    BadRequest(String),
    /// 502: the extension / upstream failed.
    Upstream(String),
    /// 500.
    Internal,
}

impl ApiError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::AuthenticationRequired => "authentication_required",
            Self::PairingRequired => "pairing_required",
            Self::PairingExpired => "pairing_expired",
            Self::DeviceNotAuthorized => "device_not_authorized",
            Self::Forbidden => "forbidden",
            Self::PairingLimit => "pairing_limit",
            Self::NotFound => "not_found",
            Self::BadRequest(_) => "bad_request",
            Self::Upstream(_) => "upstream_failed",
            Self::Internal => "internal",
        }
    }

    pub fn status(&self) -> StatusCode {
        match self {
            Self::AuthenticationRequired | Self::PairingRequired | Self::PairingExpired | Self::DeviceNotAuthorized => {
                StatusCode::UNAUTHORIZED
            }
            Self::Forbidden => StatusCode::FORBIDDEN,
            Self::PairingLimit => StatusCode::CONFLICT,
            Self::NotFound => StatusCode::NOT_FOUND,
            Self::BadRequest(_) => StatusCode::BAD_REQUEST,
            Self::Upstream(_) => StatusCode::BAD_GATEWAY,
            Self::Internal => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    fn message(&self) -> Option<&str> {
        match self {
            Self::BadRequest(m) | Self::Upstream(m) => Some(m.as_str()),
            _ => None,
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let mut body = serde_json::json!({ "error": self.code() });
        if let Some(m) = self.message() {
            body["message"] = serde_json::Value::String(m.to_owned());
        }
        (self.status(), axum::Json(body)).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_and_statuses_match_the_classic_server() {
        assert_eq!(ApiError::PairingExpired.code(), "pairing_expired");
        assert_eq!(ApiError::PairingExpired.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(ApiError::PairingLimit.status(), StatusCode::CONFLICT);
    }
}
