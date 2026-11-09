//! JSON envelope and error catalogue of `/api/server/v1` (docs/redesign/13 section 3.2; classic
//! `server/src/api-response.mjs`, `core.mjs::handle`). Success is `{ "success": true, "data": ... }` ([`ok`]); every error
//! is `{ "success": false, "error": { "code", "message" } }` ([`Failure`]) with the HTTP status the classic server uses,
//! so the Android client's error mapping keeps working.

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
        Failure::from(self).into_response()
    }
}

/// Any error answer of the API: the classic envelope `{ "success": false, "error": { "code", "message" } }` (what the
/// Android client `ExtensionServerClient.errorCode` and the TS client read). [`ApiError`] covers the catalogue of
/// docs/redesign/13; the few extra codes the classic server also emits (`invalid_password`, `login_throttled`,
/// `proxy_disabled`, `media_expired`, ...) go through [`Failure::new`] with the same status the classic uses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    pub status: StatusCode,
    pub code: &'static str,
    pub message: Option<String>,
}

impl Failure {
    pub fn new(status: StatusCode, code: &'static str) -> Self {
        Self { status, code, message: None }
    }

    pub fn with_message(mut self, message: impl Into<String>) -> Self {
        self.message = Some(message.into());
        self
    }

    /// The JSON body (without the status).
    pub fn body(&self) -> serde_json::Value {
        serde_json::json!({
            "success": false,
            "error": { "code": self.code, "message": self.message.clone().unwrap_or_else(|| self.code.to_owned()) },
        })
    }
}

impl From<ApiError> for Failure {
    fn from(e: ApiError) -> Self {
        Self { status: e.status(), code: e.code(), message: e.message().map(str::to_owned) }
    }
}

impl IntoResponse for Failure {
    fn into_response(self) -> Response {
        let mut r = (self.status, axum::Json(self.body())).into_response();
        r.headers_mut().insert(axum::http::header::CACHE_CONTROL, axum::http::HeaderValue::from_static("no-store"));
        r
    }
}

/// The success envelope `{ "success": true, "data": <value> }`.
pub fn ok<T: serde::Serialize>(value: T) -> Response {
    axum::Json(serde_json::json!({ "success": true, "data": value })).into_response()
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

    #[test]
    fn errors_use_the_classic_envelope() {
        let f = Failure::from(ApiError::BadRequest("missing code".into()));
        assert_eq!(f.status, StatusCode::BAD_REQUEST);
        assert_eq!(f.body(), serde_json::json!({ "success": false, "error": { "code": "bad_request", "message": "missing code" } }));
        let g = Failure::new(StatusCode::TOO_MANY_REQUESTS, "login_throttled");
        assert_eq!(g.body()["error"]["message"], "login_throttled");
    }
    // ---- F6 (parity wave B) ----

    #[test]
    fn whole_catalogue_matches_the_classic_server() {
        let table = [
            (ApiError::AuthenticationRequired, "authentication_required", StatusCode::UNAUTHORIZED),
            (ApiError::PairingRequired, "pairing_required", StatusCode::UNAUTHORIZED),
            (ApiError::PairingExpired, "pairing_expired", StatusCode::UNAUTHORIZED),
            (ApiError::DeviceNotAuthorized, "device_not_authorized", StatusCode::UNAUTHORIZED),
            (ApiError::Forbidden, "forbidden", StatusCode::FORBIDDEN),
            (ApiError::PairingLimit, "pairing_limit", StatusCode::CONFLICT),
            (ApiError::NotFound, "not_found", StatusCode::NOT_FOUND),
            (ApiError::BadRequest("x".into()), "bad_request", StatusCode::BAD_REQUEST),
            (ApiError::Upstream("x".into()), "upstream_failed", StatusCode::BAD_GATEWAY),
            (ApiError::Internal, "internal", StatusCode::INTERNAL_SERVER_ERROR),
        ];
        for (e, code, status) in table {
            assert_eq!(e.code(), code);
            assert_eq!(e.status(), status, "{code}");
            let f = Failure::from(e);
            assert_eq!(f.code, code);
            assert_eq!(f.status, status);
        }
    }

    #[test]
    fn only_bad_request_and_upstream_carry_a_message() {
        assert_eq!(Failure::from(ApiError::Upstream("boom".into())).message.as_deref(), Some("boom"));
        assert_eq!(Failure::from(ApiError::Forbidden).message, None);
        // without a message the code doubles as the message
        assert_eq!(Failure::from(ApiError::Forbidden).body()["error"]["message"], "forbidden");
        let f = Failure::new(StatusCode::GONE, "media_expired").with_message("ticket expired");
        assert_eq!(f.body(), serde_json::json!({ "success": false, "error": { "code": "media_expired", "message": "ticket expired" } }));
    }

    #[tokio::test]
    async fn error_responses_are_not_cached_and_keep_the_status() {
        let r = ApiError::PairingLimit.into_response();
        assert_eq!(r.status(), StatusCode::CONFLICT);
        assert_eq!(r.headers().get(axum::http::header::CACHE_CONTROL).unwrap(), "no-store");
        let bytes = axum::body::to_bytes(r.into_body(), 64 * 1024).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["success"], false);
        assert_eq!(v["error"]["code"], "pairing_limit");
    }

    #[tokio::test]
    async fn ok_wraps_data_in_the_success_envelope() {
        let r = ok(serde_json::json!({ "a": 1 }));
        assert_eq!(r.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(r.into_body(), 64 * 1024).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v, serde_json::json!({ "success": true, "data": { "a": 1 } }));
        let bytes = axum::body::to_bytes(ok(Vec::<u32>::new()).into_body(), 1024).await.unwrap();
        assert_eq!(serde_json::from_slice::<serde_json::Value>(&bytes).unwrap()["data"], serde_json::json!([]));
    }
}
