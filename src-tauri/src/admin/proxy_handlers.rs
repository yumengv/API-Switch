// Proxy administration handlers (admin side)
// Provides status, start, and stop endpoints for the HTTP proxy.

use crate::admin::{error::AdminError, state::AdminState};
use crate::server_api::ServerApi;
use axum::{extract::State, Json};
use serde::Serialize;

#[derive(Serialize)]
pub struct ProxyStatusResponse {
    pub running: bool,
    pub port: i32,
}

/// 从 AdminState 构建 ServerApi 实例。
/// 当 runtime 或 app_handle 缺失时返回错误。
fn server_api_from_admin(state: &AdminState) -> Result<ServerApi, AdminError> {
    let runtime = state
        .runtime
        .as_ref()
        .ok_or_else(|| AdminError::Internal("AdminState missing runtime".to_string()))?
        .clone();
    let app_handle = state
        .app_handle
        .as_ref()
        .ok_or_else(|| AdminError::Internal("AdminState missing app handle".to_string()))?
        .clone();
    Ok(ServerApi::new(runtime, app_handle))
}

/// Get the current proxy status.
///
/// Returns JSON with `running` flag and the configured listen `port`.
pub async fn get_status(
    State(state): State<AdminState>,
) -> Result<Json<ProxyStatusResponse>, AdminError> {
    let api = server_api_from_admin(&state)?;
    let status = api
        .proxy_status()
        .await
        .map_err(|e| AdminError::Internal(e.to_string()))?;
    Ok(Json(ProxyStatusResponse {
        running: status.running,
        port: status.port,
    }))
}

/// Start the proxy server.
///
/// Fails with an error if the proxy is already running.
pub async fn start(
    State(state): State<AdminState>,
) -> Result<Json<ProxyStatusResponse>, AdminError> {
    let api = server_api_from_admin(&state)?;
    let status = api
        .start_proxy()
        .await
        .map_err(|e| AdminError::Internal(e.to_string()))?;
    Ok(Json(ProxyStatusResponse {
        running: status.running,
        port: status.port,
    }))
}

/// Stop the proxy server.
///
/// Fails if the proxy is not currently running.
pub async fn stop(
    State(state): State<AdminState>,
) -> Result<Json<ProxyStatusResponse>, AdminError> {
    let api = server_api_from_admin(&state)?;
    let runtime = state
        .runtime
        .as_ref()
        .ok_or_else(|| AdminError::Internal("AdminState missing runtime".to_string()))?;
    let settings = runtime.settings.read().await.clone();
    api.stop_proxy()
        .await
        .map_err(|e| AdminError::Internal(e.to_string()))?;
    Ok(Json(ProxyStatusResponse {
        running: false,
        port: settings.listen_port,
    }))
}
