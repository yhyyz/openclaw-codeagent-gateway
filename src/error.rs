//! Unified error types.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("unauthorized")]
    Unauthorized,
    #[error("forbidden: {0}")]
    Forbidden(String),
    #[error("quota exceeded: {0}")]
    QuotaExceeded(String),
    #[error("rate limited")]
    RateLimited,
    #[error("agent not found: {0}")]
    AgentNotFound(String),
    #[error("agent crashed: {0}")]
    AgentCrashed(String),
    #[error("pool exhausted: {0}")]
    PoolExhausted(String),
    #[error("job not found: {0}")]
    JobNotFound(String),
    #[error("prompt too long: {len} > {limit}")]
    PromptTooLong { len: usize, limit: usize },
    #[error("callback url not allowed: {0}")]
    CallbackUrlDenied(String),
    #[error("callback channel not allowed: {0}")]
    CallbackChannelDenied(String),
    #[error("timeout after {0}s")]
    Timeout(u64),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Db(#[from] rusqlite::Error),
    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}

impl IntoResponse for Error {
    fn into_response(self) -> Response {
        let status = match &self {
            Error::Unauthorized => StatusCode::UNAUTHORIZED,
            Error::Forbidden(_)
            | Error::CallbackUrlDenied(_)
            | Error::CallbackChannelDenied(_) => StatusCode::FORBIDDEN,
            Error::AgentNotFound(_) | Error::JobNotFound(_) => StatusCode::NOT_FOUND,
            Error::PromptTooLong { .. } => StatusCode::UNPROCESSABLE_ENTITY,
            Error::QuotaExceeded(_) | Error::RateLimited | Error::PoolExhausted(_) => {
                StatusCode::TOO_MANY_REQUESTS
            }
            Error::Timeout(_) => StatusCode::GATEWAY_TIMEOUT,
            Error::AgentCrashed(_) | Error::Io(_) | Error::Db(_) | Error::Internal(_) => {
                StatusCode::INTERNAL_SERVER_ERROR
            }
        };

        let body = serde_json::json!({ "error": self.to_string() });
        (status, Json(body)).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status_of(err: Error) -> StatusCode {
        err.into_response().status()
    }

    #[test]
    fn unauthorized_is_401() {
        assert_eq!(status_of(Error::Unauthorized), StatusCode::UNAUTHORIZED);
    }

    #[test]
    fn forbidden_is_403() {
        assert_eq!(
            status_of(Error::Forbidden("nope".into())),
            StatusCode::FORBIDDEN
        );
    }

    #[test]
    fn callback_url_denied_is_403() {
        assert_eq!(
            status_of(Error::CallbackUrlDenied("http://evil".into())),
            StatusCode::FORBIDDEN
        );
    }

    #[test]
    fn callback_channel_denied_is_403() {
        assert_eq!(
            status_of(Error::CallbackChannelDenied("slack".into())),
            StatusCode::FORBIDDEN
        );
    }

    #[test]
    fn agent_not_found_is_404() {
        assert_eq!(
            status_of(Error::AgentNotFound("foo".into())),
            StatusCode::NOT_FOUND
        );
    }

    #[test]
    fn job_not_found_is_404() {
        assert_eq!(
            status_of(Error::JobNotFound("abc".into())),
            StatusCode::NOT_FOUND
        );
    }

    #[test]
    fn prompt_too_long_is_422() {
        assert_eq!(
            status_of(Error::PromptTooLong {
                len: 5000,
                limit: 4096
            }),
            StatusCode::UNPROCESSABLE_ENTITY
        );
    }

    #[test]
    fn quota_exceeded_is_429() {
        assert_eq!(
            status_of(Error::QuotaExceeded("daily".into())),
            StatusCode::TOO_MANY_REQUESTS
        );
    }

    #[test]
    fn rate_limited_is_429() {
        assert_eq!(status_of(Error::RateLimited), StatusCode::TOO_MANY_REQUESTS);
    }

    #[test]
    fn pool_exhausted_is_429() {
        assert_eq!(
            status_of(Error::PoolExhausted("gpu".into())),
            StatusCode::TOO_MANY_REQUESTS
        );
    }

    #[test]
    fn timeout_is_504() {
        assert_eq!(
            status_of(Error::Timeout(30)),
            StatusCode::GATEWAY_TIMEOUT
        );
    }

    #[test]
    fn agent_crashed_is_500() {
        assert_eq!(
            status_of(Error::AgentCrashed("segfault".into())),
            StatusCode::INTERNAL_SERVER_ERROR
        );
    }

    #[test]
    fn io_error_is_500() {
        let io_err = std::io::Error::new(std::io::ErrorKind::Other, "disk full");
        assert_eq!(status_of(Error::Io(io_err)), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[test]
    fn internal_error_is_500() {
        let err = Error::Internal(anyhow::anyhow!("something broke"));
        assert_eq!(status_of(err), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[test]
    fn response_body_is_json_with_error_field() {
        let resp = Error::Unauthorized.into_response();
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        let body_bytes = rt.block_on(async {
            axum::body::to_bytes(resp.into_body(), 1024).await.unwrap()
        });
        let json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
        assert_eq!(json["error"], "unauthorized");
    }
}
