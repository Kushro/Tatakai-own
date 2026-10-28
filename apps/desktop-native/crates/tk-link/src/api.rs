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
}

impl ErrorCode {
    pub const ALL: [Self; 19] = [
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
            Self::SessionGone => StatusCode::GONE,
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
        (self.0.status(), Json(body)).into_response()
    }
}

pub type ApiResult<T> = Result<T, ApiError>;
