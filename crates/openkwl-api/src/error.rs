//! Error bodies. Every non-2xx response carries an [`ApiError`].

use serde::{Deserialize, Serialize};

/// Machine-readable error kind; each maps to one HTTP status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    /// 400: body is not valid JSON or does not match the schema.
    BadRequest,
    /// 422: body parsed but a value is out of range; `field` names it.
    Invalid,
    /// 401: missing, unknown or expired bearer token, or wrong password.
    Unauthorized,
    /// 403: no password set yet; only `/setup` is available.
    SetupRequired,
    /// 404
    NotFound,
    /// 405
    MethodNotAllowed,
    /// 409: e.g. another firmware update is being uploaded.
    Conflict,
    /// 413: body larger than the device accepts.
    PayloadTooLarge,
    /// 429: login throttled; `retry_after_s` says how long (also sent as `Retry-After`).
    TooManyRequests,
    /// 500
    Internal,
    /// 503: e.g. SD card missing while saving settings.
    Unavailable,
}

impl ErrorCode {
    /// HTTP status code to send with this error.
    pub const fn http_status(self) -> u16 {
        match self {
            ErrorCode::BadRequest => 400,
            ErrorCode::Unauthorized => 401,
            ErrorCode::SetupRequired => 403,
            ErrorCode::NotFound => 404,
            ErrorCode::MethodNotAllowed => 405,
            ErrorCode::Conflict => 409,
            ErrorCode::PayloadTooLarge => 413,
            ErrorCode::Invalid => 422,
            ErrorCode::TooManyRequests => 429,
            ErrorCode::Internal => 500,
            ErrorCode::Unavailable => 503,
        }
    }
}

/// Result of [`crate::Validate::validate`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FieldError {
    /// JSON field name.
    pub field: &'static str,
    /// Short English reason, e.g. `out of range`.
    pub reason: &'static str,
}

/// Body of every non-2xx response.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApiError<'a> {
    /// Error kind.
    pub error: ErrorCode,
    /// Short English description for logs and UI fallback.
    pub message: &'a str,
    /// Offending field for `invalid`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub field: Option<&'a str>,
    /// Seconds until the next login attempt for `too_many_requests`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry_after_s: Option<u64>,
}

impl<'a> ApiError<'a> {
    /// Error without field or retry hint.
    pub const fn new(error: ErrorCode, message: &'a str) -> Self {
        Self { error, message, field: None, retry_after_s: None }
    }

    /// 422 from a failed [`crate::Validate`] check.
    pub const fn invalid(e: FieldError) -> ApiError<'static> {
        ApiError { error: ErrorCode::Invalid, message: e.reason, field: Some(e.field), retry_after_s: None }
    }

    /// 429 from the login throttle.
    pub const fn throttled(retry_after_s: u64) -> ApiError<'static> {
        ApiError {
            error: ErrorCode::TooManyRequests,
            message: "too many failed logins",
            field: None,
            retry_after_s: Some(retry_after_s),
        }
    }
}

/// Upper bound of an encoded [`ApiError`] with the built-in messages.
pub const API_ERROR_JSON_MAX: usize = 192;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serialises_compactly() {
        let e = ApiError::invalid(FieldError { field: "level", reason: "out of range" });
        let json: heapless::String<API_ERROR_JSON_MAX> = serde_json_core::to_string(&e).unwrap();
        assert_eq!(json, r#"{"error":"invalid","message":"out of range","field":"level"}"#);
        let (back, _): (ApiError, _) = serde_json_core::from_str(&json).unwrap();
        assert_eq!(back, e);
        assert_eq!(back.error.http_status(), 422);
    }

    #[test]
    fn throttled_carries_retry_after() {
        let e = ApiError::throttled(900);
        let json: heapless::String<API_ERROR_JSON_MAX> = serde_json_core::to_string(&e).unwrap();
        assert_eq!(json, r#"{"error":"too_many_requests","message":"too many failed logins","retry_after_s":900}"#);
        let (back, _): (ApiError, _) = serde_json_core::from_str(&json).unwrap();
        assert_eq!(back, e);
    }
}
