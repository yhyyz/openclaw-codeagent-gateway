//! Request/response middleware layers.

use axum::body::Body;
use axum::extract::State;
use axum::http::Request;
use axum::middleware::Next;
use axum::response::Response;

use crate::app::AppState;
use crate::auth::identity::extract_bearer_token;
use crate::error::Error;

pub async fn auth_middleware(
    State(state): State<AppState>,
    mut request: Request<Body>,
    next: Next,
) -> Result<Response, Error> {
    let auth_header = request
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .ok_or(Error::Unauthorized)?;

    let token = extract_bearer_token(auth_header)?;

    let tenant = state
        .tenant_registry
        .resolve(token)
        .ok_or(Error::Unauthorized)?
        .clone();

    request.extensions_mut().insert(tenant);

    Ok(next.run(request).await)
}
