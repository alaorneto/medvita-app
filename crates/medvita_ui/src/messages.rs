//! pt-BR messages for errors, always with the request id for support when there is one.

use chrono::Local;
use medvita_api::{ApiError, AuthFailure};
use medvita_auth::AuthError;
use rust_i18n::t;

fn with_request_id(message: String, request_id: Option<&str>) -> String {
    match request_id {
        Some(id) => format!("{message} {}", t!("errors.request_id", id = id)),
        None => message,
    }
}

pub fn api_error(error: &ApiError) -> String {
    let message = match error {
        ApiError::Network { .. } => t!("errors.network").to_string(),
        ApiError::Timeout => t!("errors.timeout").to_string(),
        ApiError::Auth(AuthFailure::Renewal(_)) => t!("errors.network").to_string(),
        ApiError::Auth(_) | ApiError::Unauthorized { .. } => {
            t!("errors.session_expired").to_string()
        }
        ApiError::Forbidden { code, .. } => match code.as_str() {
            "mfa_required" => t!("errors.mfa_required").to_string(),
            "organization_suspended" => t!("errors.organization_suspended").to_string(),
            "token_not_allowed" => t!("errors.token_not_allowed").to_string(),
            _ => t!("errors.forbidden").to_string(),
        },
        ApiError::NotFound { .. } => t!("errors.not_found").to_string(),
        ApiError::Validation { detail, .. } | ApiError::Conflict { detail, .. } => detail
            .clone()
            .unwrap_or_else(|| t!("errors.invalid").to_string()),
        ApiError::PreconditionFailed { .. } => t!("errors.changed").to_string(),
        ApiError::RateLimited { .. } => t!("errors.rate_limited").to_string(),
        ApiError::Maintenance { ends_at, .. } => match ends_at {
            Some(end) => t!(
                "errors.maintenance_until",
                time = end.with_timezone(&Local).format("%H:%M").to_string()
            )
            .to_string(),
            None => t!("errors.maintenance").to_string(),
        },
        ApiError::Server { .. } => t!("errors.server").to_string(),
        ApiError::NotAcceptable { .. }
        | ApiError::PreconditionRequired { .. }
        | ApiError::PayloadTooLarge { .. }
        | ApiError::Decode { .. }
        | ApiError::InvalidRequest { .. } => t!("errors.unexpected").to_string(),
    };
    with_request_id(message, error.request_id())
}

pub fn auth_error(error: &AuthError) -> String {
    match error {
        AuthError::Network(_) => t!("errors.network").to_string(),
        AuthError::Timeout => t!("errors.timeout").to_string(),
        AuthError::Discovery(_)
        | AuthError::IssuerMismatch { .. }
        | AuthError::UntrustedEndpoint(_) => t!("errors.no_sign_in").to_string(),
        AuthError::LoopbackUnavailable(_) => t!("errors.loopback").to_string(),
        AuthError::Denied { .. } => t!("errors.denied").to_string(),
        AuthError::LoginTimedOut => t!("errors.login_timeout").to_string(),
        AuthError::InvalidGrant => t!("errors.session_expired").to_string(),
        AuthError::Vault(_) => t!("errors.vault").to_string(),
        AuthError::TokenEndpoint { .. }
        | AuthError::InvalidIdToken(_)
        | AuthError::InvalidResponse(_) => t!("errors.sign_in_failed").to_string(),
    }
}
