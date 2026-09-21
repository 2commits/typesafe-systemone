use std::time::Duration;

/// Result alias for this crate.
pub type Result<T> = std::result::Result<T, Error>;

/// Errors returned by [`Client`](crate::Client).
///
/// HTTP-status variants carry the response body (truncated) as `message`.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// Missing API key or otherwise unusable client configuration.
    #[error("configuration error: {0}")]
    Config(String),

    /// The request was not sendable as built (no state, no questions, bad option set).
    #[error("invalid request: {0}")]
    InvalidRequest(String),

    /// The response has no answer of the expected type under this id.
    #[error("no {expected} answer for question `{id}`")]
    MissingAnswer {
        /// The question id that was looked up.
        id: String,
        /// The answer type that was expected: `noul`, `choice` or `score`.
        expected: &'static str,
    },

    /// 400.
    #[error("bad request: {message}")]
    BadRequest {
        /// Response body, truncated.
        message: String,
    },

    /// 401: missing or invalid API key.
    #[error("authentication failed: {message}")]
    Authentication {
        /// Response body, truncated.
        message: String,
    },

    /// 403.
    #[error("permission denied: {message}")]
    PermissionDenied {
        /// Response body, truncated.
        message: String,
    },

    /// 404.
    #[error("not found: {message}")]
    NotFound {
        /// Response body, truncated.
        message: String,
    },

    /// 422: the request body failed validation.
    #[error("unprocessable entity: {message}")]
    UnprocessableEntity {
        /// Response body, truncated; names the offending field.
        message: String,
    },

    /// 429: rate limit exceeded after all retries.
    #[error("rate limited: {message}")]
    RateLimit {
        /// The server's `retry-after`, when it sent one.
        retry_after: Option<Duration>,
        /// Response body, truncated.
        message: String,
    },

    /// 529: TypeSafe is temporarily overloaded, after all retries.
    #[error("overloaded: {message}")]
    Overloaded {
        /// The server's `retry-after`, when it sent one.
        retry_after: Option<Duration>,
        /// Response body, truncated.
        message: String,
    },

    /// Any other 5xx, after all retries.
    #[error("server error {status}: {message}")]
    Server {
        /// HTTP status code.
        status: u16,
        /// Response body, truncated.
        message: String,
    },

    /// Any other unexpected status.
    #[error("unexpected status {status}: {message}")]
    UnexpectedStatus {
        /// HTTP status code.
        status: u16,
        /// Response body, truncated.
        message: String,
    },

    /// The request never produced a response (DNS, connect, TLS, reset).
    #[error("connection error: {0}")]
    Connection(#[source] reqwest::Error),

    /// The request exceeded its timeout.
    #[error("request timed out")]
    Timeout(#[source] reqwest::Error),

    /// The response body did not match the documented schema.
    #[error("response validation failed: {0}")]
    ResponseValidation(#[source] serde_json::Error),

    /// Failed to serialize the request body.
    #[error("request serialization failed: {0}")]
    RequestSerialization(#[source] serde_json::Error),
}

impl Error {
    /// The HTTP status behind this error, when there is one.
    #[must_use]
    pub const fn status(&self) -> Option<u16> {
        match self {
            Self::BadRequest { .. } => Some(400),
            Self::Authentication { .. } => Some(401),
            Self::PermissionDenied { .. } => Some(403),
            Self::NotFound { .. } => Some(404),
            Self::UnprocessableEntity { .. } => Some(422),
            Self::RateLimit { .. } => Some(429),
            Self::Overloaded { .. } => Some(529),
            Self::Server { status, .. } | Self::UnexpectedStatus { status, .. } => Some(*status),
            _ => None,
        }
    }

    pub(crate) fn from_status(status: u16, retry_after: Option<Duration>, message: String) -> Self {
        match status {
            400 => Self::BadRequest { message },
            401 => Self::Authentication { message },
            403 => Self::PermissionDenied { message },
            404 => Self::NotFound { message },
            422 => Self::UnprocessableEntity { message },
            429 => Self::RateLimit { retry_after, message },
            529 => Self::Overloaded { retry_after, message },
            500..=599 => Self::Server { status, message },
            _ => Self::UnexpectedStatus { status, message },
        }
    }

    pub(crate) fn from_transport(err: reqwest::Error) -> Self {
        if err.is_timeout() {
            Self::Timeout(err)
        } else {
            Self::Connection(err)
        }
    }
}
