//! Errors of the API, mapped from the server's `{code, detail, fields?}` body and status.
//!
//! `Display` texts are English and meant for logs; the UI maps each variant to a pt-BR message.

use std::collections::BTreeMap;
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::Deserialize;

use crate::auth::AuthFailure;

#[derive(Debug, Clone, thiserror::Error)]
pub enum ApiError {
    #[error("network error: {message}")]
    Network { message: String },
    #[error("request timed out")]
    Timeout,
    #[error("authentication failed: {0}")]
    Auth(AuthFailure),
    #[error("unauthorized ({code})")]
    Unauthorized {
        code: String,
        request_id: Option<String>,
    },
    #[error("forbidden ({code})")]
    Forbidden {
        code: String,
        detail: Option<String>,
        request_id: Option<String>,
    },
    #[error("not found")]
    NotFound { request_id: Option<String> },
    #[error("server refused the JSON response type")]
    NotAcceptable { request_id: Option<String> },
    #[error("validation failed ({code})")]
    Validation {
        code: String,
        detail: Option<String>,
        fields: BTreeMap<String, Vec<String>>,
        request_id: Option<String>,
    },
    #[error("conflict ({code})")]
    Conflict {
        code: String,
        detail: Option<String>,
        request_id: Option<String>,
    },
    #[error("the record changed since it was read")]
    PreconditionFailed { request_id: Option<String> },
    #[error("the request needs If-Match")]
    PreconditionRequired { request_id: Option<String> },
    #[error("request body too large")]
    PayloadTooLarge { request_id: Option<String> },
    #[error("rate limited")]
    RateLimited {
        retry_after: Option<Duration>,
        request_id: Option<String>,
    },
    #[error("server in maintenance")]
    Maintenance {
        retry_after: Option<Duration>,
        ends_at: Option<DateTime<Utc>>,
        detail: Option<String>,
        request_id: Option<String>,
    },
    #[error("server error (HTTP {status})")]
    Server {
        status: u16,
        request_id: Option<String>,
    },
    #[error("unexpected response: {message}")]
    Decode {
        message: String,
        request_id: Option<String>,
    },
    #[error("invalid request: {message}")]
    InvalidRequest { message: String },
}

impl ApiError {
    /// The `X-Request-ID` of the response, for support. Shown to people next to the message.
    pub fn request_id(&self) -> Option<&str> {
        match self {
            Self::Unauthorized { request_id, .. }
            | Self::Forbidden { request_id, .. }
            | Self::NotFound { request_id }
            | Self::NotAcceptable { request_id }
            | Self::Validation { request_id, .. }
            | Self::Conflict { request_id, .. }
            | Self::PreconditionFailed { request_id }
            | Self::PreconditionRequired { request_id }
            | Self::PayloadTooLarge { request_id }
            | Self::RateLimited { request_id, .. }
            | Self::Maintenance { request_id, .. }
            | Self::Server { request_id, .. }
            | Self::Decode { request_id, .. } => request_id.as_deref(),
            Self::Network { .. } | Self::Timeout | Self::Auth(_) | Self::InvalidRequest { .. } => {
                None
            }
        }
    }

    /// The server's machine-readable `code`, when there is one.
    pub fn code(&self) -> Option<&str> {
        match self {
            Self::Unauthorized { code, .. }
            | Self::Forbidden { code, .. }
            | Self::Validation { code, .. }
            | Self::Conflict { code, .. } => Some(code),
            Self::Maintenance { .. } => Some("maintenance"),
            _ => None,
        }
    }

    /// `true` when the person has to sign in again.
    pub fn needs_sign_in(&self) -> bool {
        matches!(
            self,
            Self::Unauthorized { .. }
                | Self::Auth(AuthFailure::NotSignedIn | AuthFailure::SessionExpired)
        )
    }

    /// `true` for failures where trying again later may succeed.
    pub fn is_transient(&self) -> bool {
        matches!(
            self,
            Self::Network { .. }
                | Self::Timeout
                | Self::RateLimited { .. }
                | Self::Maintenance { .. }
                | Self::Server { .. }
                | Self::Auth(AuthFailure::Renewal(_))
        )
    }
}

/// The error body of the server (`skeleton/api/errors.py`). Every field is optional so that an
/// unexpected body still maps to the status.
#[derive(Debug, Default, Deserialize)]
pub(crate) struct ErrorBody {
    pub code: Option<String>,
    pub detail: Option<serde_json::Value>,
    pub fields: Option<serde_json::Value>,
    pub ends_at: Option<DateTime<Utc>>,
}

impl ErrorBody {
    pub(crate) fn parse(bytes: &[u8]) -> Self {
        serde_json::from_slice(bytes).unwrap_or_default()
    }

    pub(crate) fn detail_text(&self) -> Option<String> {
        match &self.detail {
            Some(serde_json::Value::String(text)) => Some(text.clone()),
            Some(other) if !other.is_null() => Some(other.to_string()),
            _ => None,
        }
    }

    /// `fields` is `{field: [messages]}`; anything else is flattened as best as possible.
    pub(crate) fn field_errors(&self) -> BTreeMap<String, Vec<String>> {
        let Some(serde_json::Value::Object(map)) = &self.fields else {
            return BTreeMap::new();
        };
        map.iter()
            .map(|(field, value)| {
                let messages = match value {
                    serde_json::Value::Array(items) => items
                        .iter()
                        .map(|item| match item {
                            serde_json::Value::String(text) => text.clone(),
                            other => other.to_string(),
                        })
                        .collect(),
                    serde_json::Value::String(text) => vec![text.clone()],
                    other => vec![other.to_string()],
                };
                (field.clone(), messages)
            })
            .collect()
    }
}

/// Maps a non-success response to an [`ApiError`].
pub(crate) fn from_status(
    status: u16,
    body: &[u8],
    retry_after: Option<Duration>,
    request_id: Option<String>,
) -> ApiError {
    let parsed = ErrorBody::parse(body);
    let code = |fallback: &str| parsed.code.clone().unwrap_or_else(|| fallback.to_owned());
    match status {
        400 => ApiError::Validation {
            code: code("invalid_request"),
            detail: parsed.detail_text(),
            fields: parsed.field_errors(),
            request_id,
        },
        401 => ApiError::Unauthorized {
            code: code("invalid_token"),
            request_id,
        },
        403 => ApiError::Forbidden {
            code: code("permission_denied"),
            detail: parsed.detail_text(),
            request_id,
        },
        404 => ApiError::NotFound { request_id },
        406 => ApiError::NotAcceptable { request_id },
        409 => ApiError::Conflict {
            code: code("conflict"),
            detail: parsed.detail_text(),
            request_id,
        },
        412 => ApiError::PreconditionFailed { request_id },
        413 => ApiError::PayloadTooLarge { request_id },
        428 => ApiError::PreconditionRequired { request_id },
        429 => ApiError::RateLimited {
            retry_after,
            request_id,
        },
        503 if parsed.code.as_deref() == Some("maintenance") => ApiError::Maintenance {
            retry_after,
            ends_at: parsed.ends_at,
            detail: parsed.detail_text(),
            request_id,
        },
        _ => ApiError::Server { status, request_id },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validation_keeps_the_field_messages() {
        let body = r#"{"code":"validation_error","detail":"Verifique.","fields":{"name":["Obrigatório."]}}"#
            .as_bytes();
        match from_status(400, body, None, Some("r1".into())) {
            ApiError::Validation {
                code,
                fields,
                request_id,
                ..
            } => {
                assert_eq!(code, "validation_error");
                assert_eq!(fields["name"], vec!["Obrigatório.".to_owned()]);
                assert_eq!(request_id.as_deref(), Some("r1"));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn maintenance_needs_the_code() {
        let body = r#"{"code":"maintenance","detail":"Em manutenção.","ends_at":"2026-11-21T03:00:00+00:00"}"#
            .as_bytes();
        let error = from_status(503, body, Some(Duration::from_secs(120)), None);
        match error {
            ApiError::Maintenance {
                retry_after,
                ends_at,
                ..
            } => {
                assert_eq!(retry_after, Some(Duration::from_secs(120)));
                assert!(ends_at.is_some());
            }
            other => panic!("{other:?}"),
        }
        assert!(matches!(
            from_status(503, b"", None, None),
            ApiError::Server { status: 503, .. }
        ));
    }

    #[test]
    fn unparseable_body_still_maps_the_status() {
        assert!(matches!(
            from_status(403, b"<html>", None, None),
            ApiError::Forbidden { code, .. } if code == "permission_denied"
        ));
    }
}
