use medvita_platform::SecretStoreError;

#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error("network error: {0}")]
    Network(String),
    #[error("request timed out")]
    Timeout,
    #[error("OIDC discovery failed: {0}")]
    Discovery(String),
    #[error("issuer mismatch: expected {expected}, server says {found}")]
    IssuerMismatch { expected: String, found: String },
    #[error("endpoint not allowed: {0}")]
    UntrustedEndpoint(String),
    #[error("could not open the local sign-in listener: {0}")]
    LoopbackUnavailable(String),
    #[error("sign-in refused by the server: {error}")]
    Denied {
        error: String,
        description: Option<String>,
    },
    #[error("sign-in timed out")]
    LoginTimedOut,
    #[error("token endpoint answered {status}: {error}")]
    TokenEndpoint { status: u16, error: String },
    /// The refresh token is no longer valid: the person must sign in again.
    #[error("session expired")]
    InvalidGrant,
    #[error("invalid ID token: {0}")]
    InvalidIdToken(String),
    #[error("unexpected token response: {0}")]
    InvalidResponse(String),
    #[error(transparent)]
    Vault(#[from] SecretStoreError),
}

impl AuthError {
    pub(crate) fn from_reqwest(error: reqwest::Error) -> Self {
        if error.is_timeout() {
            Self::Timeout
        } else {
            Self::Network(error.without_url().to_string())
        }
    }
}
