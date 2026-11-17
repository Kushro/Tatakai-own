//! Link v1 error catalogue (contracts/link/openapi.v1.json `x-errors`). Messages are FIXED: they never carry
//! paths, secrets, tokens or pairing codes.

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::json;

pub const CONTRACT_HEADER: &str = "Tatakai-Link";
pub const CONTRACT_VERSION: &str = "v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCode {
    ClientCertificateRequired,
    DeviceNotAuthorized,
    PairingClosed,
    PairingCodeInvalid,
    PairingAttemptsExceeded,
    CsrInvalid,
    RequestInvalid,
    BodyTooLarge,
    BodyTimeout,
    ProfileUnavailable,
    LibraryItemNotFound,
    SourceUnavailable,
    SessionNotFound,
    SessionGone,
    SessionNotReady,
    SegmentWaitTimeout,
    ServerBusy,
    Internal,
    UrlSourceDenied,
    /// 0.4.1: a media file of a failed session (its engine failed, or stalled and its automatic restarts ran out). Final
    /// for the session: the phone opens a new one at its position instead of retrying a 503 forever.
    SegmentUnavailable,
}

impl ErrorCode {
    pub const ALL: [Self; 20] = [
        Self::ClientCertificateRequired,
        Self::DeviceNotAuthorized,
        Self::PairingClosed,
        Self::PairingCodeInvalid,
        Self::PairingAttemptsExceeded,
        Self::CsrInvalid,
        Self::RequestInvalid,
        Self::BodyTooLarge,
        Self::BodyTimeout,
        Self::ProfileUnavailable,
        Self::LibraryItemNotFound,
        Self::SourceUnavailable,
        Self::SessionNotFound,
        Self::SessionGone,
        Self::SessionNotReady,
        Self::SegmentWaitTimeout,
        Self::ServerBusy,
        Self::Internal,
        Self::UrlSourceDenied,
        Self::SegmentUnavailable,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::ClientCertificateRequired => "client_certificate_required",
            Self::DeviceNotAuthorized => "device_not_authorized",
            Self::PairingClosed => "pairing_closed",
            Self::PairingCodeInvalid => "pairing_code_invalid",
            Self::PairingAttemptsExceeded => "pairing_attempts_exceeded",
            Self::CsrInvalid => "csr_invalid",
            Self::RequestInvalid => "request_invalid",
            Self::BodyTooLarge => "body_too_large",
            Self::BodyTimeout => "body_timeout",
            Self::ProfileUnavailable => "profile_unavailable",
            Self::LibraryItemNotFound => "library_item_not_found",
            Self::SourceUnavailable => "source_unavailable",
            Self::SessionNotFound => "session_not_found",
            Self::SessionGone => "session_gone",
            Self::SessionNotReady => "session_not_ready",
            Self::SegmentWaitTimeout => "segment_wait_timeout",
            Self::ServerBusy => "server_busy",
            Self::Internal => "internal",
            Self::UrlSourceDenied => "url_source_denied",
            Self::SegmentUnavailable => "segment_unavailable",
        }
    }

    pub fn status(self) -> StatusCode {
        match self {
            Self::ClientCertificateRequired => StatusCode::UNAUTHORIZED,
            Self::DeviceNotAuthorized | Self::PairingCodeInvalid | Self::UrlSourceDenied => StatusCode::FORBIDDEN,
            Self::PairingClosed | Self::SessionNotReady => StatusCode::CONFLICT,
            Self::PairingAttemptsExceeded => StatusCode::TOO_MANY_REQUESTS,
            Self::CsrInvalid | Self::RequestInvalid => StatusCode::BAD_REQUEST,
            Self::BodyTooLarge => StatusCode::PAYLOAD_TOO_LARGE,
            Self::BodyTimeout => StatusCode::REQUEST_TIMEOUT,
            Self::ProfileUnavailable => StatusCode::UNPROCESSABLE_ENTITY,
            Self::LibraryItemNotFound | Self::SessionNotFound => StatusCode::NOT_FOUND,
            Self::SourceUnavailable | Self::SegmentWaitTimeout | Self::ServerBusy => StatusCode::SERVICE_UNAVAILABLE,
            Self::SessionGone | Self::SegmentUnavailable => StatusCode::GONE,
            Self::Internal => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    pub fn message(self) -> &'static str {
        match self {
            Self::ClientCertificateRequired => "a client certificate is required",
            Self::DeviceNotAuthorized => "this device is not authorized",
            Self::PairingClosed => "no pairing window is open",
            Self::PairingCodeInvalid => "the pairing code is invalid",
            Self::PairingAttemptsExceeded => "too many failed attempts; the pairing window is closed",
            Self::CsrInvalid => "the certificate signing request is invalid",
            Self::RequestInvalid => "the request is invalid",
            Self::BodyTooLarge => "the request body is too large",
            Self::BodyTimeout => "the request body was not received in time",
            Self::ProfileUnavailable => "the requested profile is not available",
            Self::LibraryItemNotFound => "the library item was not found",
            Self::SourceUnavailable => "the source is not available",
            Self::SessionNotFound => "session or resource not found",
            Self::SessionGone => "the session is closed",
            Self::SessionNotReady => "the session is still being prepared",
            Self::SegmentWaitTimeout => "the segment is not ready yet",
            Self::ServerBusy => "the server is busy",
            Self::Internal => "internal server error",
            Self::UrlSourceDenied => "the url source is not allowed",
            Self::SegmentUnavailable => "the segment cannot be produced; open the session again",
        }
    }
}

/// An error answer: `{"error":{"code","message"}}` with the fixed message of the code.
#[derive(Debug, Clone, Copy)]
pub struct ApiError(pub ErrorCode);

impl From<ErrorCode> for ApiError {
    fn from(c: ErrorCode) -> Self {
        Self(c)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = json!({ "error": { "code": self.0.as_str(), "message": self.0.message() } });
        let mut r = (self.0.status(), Json(body)).into_response();
        if matches!(self.0, ErrorCode::SegmentWaitTimeout) {
            // the engine is still producing it: ask the player to come back soon instead of giving up
            r.headers_mut().insert(axum::http::header::RETRY_AFTER, axum::http::HeaderValue::from_static("1"));
        }
        r
    }
}

pub type ApiResult<T> = Result<T, ApiError>;

#[cfg(test)]
mod contract_tests {
    use super::ErrorCode;

    /// Every code the PC answers is in the contract's `x-errors` table with the same status and fixed message.
    #[test]
    fn every_error_code_matches_the_contract() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../../../contracts/link/openapi.v1.json");
        let spec: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        let rows = spec["x-errors"]["codes"].as_array().unwrap();
        for code in ErrorCode::ALL {
            let row = rows.iter().find(|r| r["code"] == code.as_str()).unwrap_or_else(|| panic!("{} missing", code.as_str()));
            assert_eq!(row["http"].as_u64(), Some(u64::from(code.status().as_u16())), "{}", code.as_str());
            assert_eq!(row["message"].as_str(), Some(code.message()), "{}", code.as_str());
        }
    }
}
