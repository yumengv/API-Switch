use crate::admin::error::AdminError;
use crate::admin::state::AdminState;
use axum::extract::{Request, State};
use axum::http::header::AUTHORIZATION;
use axum::middleware::Next;
use axum::response::Response;

const SESSION_TTL_HOURS: i64 = 24;

pub async fn require_auth(
    State(state): State<AdminState>,
    request: Request,
    next: Next,
) -> Result<Response, AdminError> {
    let token = request
        .headers()
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .map(str::to_string)
        .ok_or(AdminError::Unauthorized)?;

    let now = chrono::Utc::now();
    let current_username = state.settings.read().await.web_admin_username.clone();

    let mut sessions = state.login_sessions.write().await;
    sessions.retain(|_, session| session.expires_at > now);

    let session_valid = match sessions.get_mut(&token) {
        Some(session) if session.username == current_username => {
            session.expires_at = now + chrono::Duration::hours(SESSION_TTL_HOURS);
            true
        }
        _ => false,
    };

    if !session_valid {
        sessions.remove(&token);
        return Err(AdminError::Unauthorized);
    }

    Ok(next.run(request).await)
}
