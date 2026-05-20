use crate::admin::error::AdminError;
use crate::admin::state::{AdminState, SessionInfo};
use crate::database::AppSettings;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::Json;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

const SESSION_TTL_HOURS: i64 = 24;

pub async fn version() -> axum::Json<serde_json::Value> {
    axum::Json(serde_json::json!({
        "version": env!("CARGO_PKG_VERSION")
    }))
}

pub async fn state_version() -> axum::Json<crate::state_version::StatVersionsResponse> {
    let v = crate::state_version::all();
    axum::Json(crate::state_version::StatVersionsResponse {
        log: v["log"],
        pool: v["pool"],
        channel: v["channel"],
        token: v["token"],
    })
}

#[derive(Deserialize)]
pub struct LoginRequest {
    username: String,
    password: String,
}

#[derive(Serialize)]
pub struct LoginResponse {
    token: String,
    expires_at: i64,
}

#[derive(Serialize)]
pub struct AdminStatus {
    running: bool,
    port: i32,
    runtime_mode: Option<String>,
}

#[derive(Serialize)]
pub struct SettingsResponse {
    data: AppSettings,
    _version: i64,
}

#[derive(Serialize)]
pub struct RestartResponse {
    ok: bool,
    _version: i64,
    restart: Option<RestartInfo>,
}

#[derive(Serialize)]
pub struct RestartInfo {
    pub admin_relocated: bool,
    pub new_admin_base_url: Option<String>,
    pub proxy_restart_required: bool,
    pub proxy_restarted: bool,
}

#[derive(Deserialize)]
pub struct UpdateSettingsRequest {
    data: AppSettings,
    _version: i64,
}

fn invalidate_sessions_for_username(
    sessions: &mut std::collections::HashMap<String, SessionInfo>,
    username: &str,
) -> usize {
    let before = sessions.len();
    sessions.retain(|_, session| session.username != username);
    before.saturating_sub(sessions.len())
}

fn settings_credentials_changed(current: &AppSettings, next: &AppSettings) -> bool {
    next.web_admin_username != current.web_admin_username
        || next.web_admin_password != current.web_admin_password
}

pub async fn login(
    State(state): State<AdminState>,
    Json(payload): Json<LoginRequest>,
) -> Result<Json<LoginResponse>, AdminError> {
    let now = chrono::Utc::now();
    let key = payload.username.clone();
    {
        let mut failures = state.login_failures.lock().await;
        let entry = failures.entry(key.clone()).or_default();
        if let Some(locked_until) = entry.locked_until {
            if locked_until > now {
                let retry_after_seconds = (locked_until - now).num_seconds().max(1);
                return Err(AdminError::RateLimited {
                    retry_after_seconds,
                    remaining_attempts: 0,
                    locked_until: locked_until.timestamp(),
                });
            }
            entry.locked_until = None;
            entry.count = 0;
        }
    }

    let settings = state.settings.read().await.clone();
    if payload.username != settings.web_admin_username
        || payload.password != settings.web_admin_password
    {
        let mut failures = state.login_failures.lock().await;
        let entry = failures.entry(key).or_default();
        entry.count += 1;
        let remaining_attempts = 5_u32.saturating_sub(entry.count) as i64;
        let _ = state
            .db
            .add_audit_log("admin_login_failed", "invalid credentials");

        if entry.count >= 6 {
            let lock_expiry = now + chrono::Duration::minutes(5);
            entry.locked_until = Some(lock_expiry);
            entry.count = 0;
            return Err(AdminError::RateLimited {
                retry_after_seconds: (lock_expiry - now).num_seconds().max(1),
                remaining_attempts: 0,
                locked_until: lock_expiry.timestamp(),
            });
        }

        return Err(AdminError::InvalidCredentials {
            remaining_attempts,
            locked_until: entry.locked_until.map(|value| value.timestamp()),
        });
    }

    state.login_failures.lock().await.remove(&payload.username);
    state
        .login_failures
        .lock()
        .await
        .remove(&settings.web_admin_username);

    let token = Uuid::new_v4().to_string();
    let expires_at = chrono::Utc::now() + chrono::Duration::hours(SESSION_TTL_HOURS);
    state.login_sessions.write().await.insert(
        token.clone(),
        SessionInfo {
            username: settings.web_admin_username.clone(),
            expires_at,
        },
    );
    let _ = state
        .db
        .add_audit_log("admin_login_success", &payload.username);

    Ok(Json(LoginResponse {
        token,
        expires_at: expires_at.timestamp(),
    }))
}

pub async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "ok": true }))
}

pub async fn logout(
    State(state): State<AdminState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, AdminError> {
    if let Some(token) = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
    {
        state.login_sessions.write().await.remove(token);
        let _ = state.db.add_audit_log("admin_logout", "token_redacted");
    }

    Ok(Json(serde_json::json!({ "ok": true })))
}

pub async fn status(State(state): State<AdminState>) -> Json<AdminStatus> {
    let settings = state.settings.read().await.clone();
    let runtime_mode = state
        .runtime
        .as_ref()
        .map(|runtime| match runtime.runtime_mode {
            crate::runtime_mode::RuntimeMode::Combined => "combined".to_string(),
            crate::runtime_mode::RuntimeMode::Standalone => "standalone".to_string(),
        });
    Json(AdminStatus {
        running: true,
        port: settings.web_admin_port,
        runtime_mode,
    })
}

pub async fn audit_logs(
    State(state): State<AdminState>,
) -> Result<Json<Vec<crate::database::AuditLogItem>>, AdminError> {
    Ok(Json(state.db.list_audit_logs(100)?))
}

pub async fn get_settings(State(state): State<AdminState>) -> Json<SettingsResponse> {
    let mut settings = state.settings.read().await.clone();
    let version = settings.updated_at;
    settings.web_admin_password.clear();
    Json(SettingsResponse {
        _version: version,
        data: settings,
    })
}

pub async fn patch_settings(
    State(state): State<AdminState>,
    Json(patch): Json<serde_json::Value>,
) -> Result<Json<SettingsResponse>, AdminError> {
    // 1. 读取当前 settings
    let current = state.settings.read().await.clone();

    // 2. 将当前 settings 序列化为 serde_json::Value
    let mut settings_value =
        serde_json::to_value(&current).map_err(|e| AdminError::Internal(e.to_string()))?;

    // 3. 合并 patch 字段到 settings_value
    if let (Some(obj), Some(patch_obj)) = (settings_value.as_object_mut(), patch.as_object()) {
        for (key, value) in patch_obj {
            // 跳过 _version 字段，PATCH 不做版本号检查
            if key != "_version" {
                obj.insert(key.clone(), value.clone());
            }
        }
    }

    // 4. 反序列化为 AppSettings
    let merged: AppSettings = serde_json::from_value(settings_value)
        .map_err(|e| AdminError::BadRequest(e.to_string()))?;

    // 5. 保存到数据库
    state.db.update_settings(&merged)?;
    crate::state_version::bump("pool");

    // 6. 更新 L1 缓存
    *state.settings.write().await = merged.clone();

    // 7. 返回新 settings + 版本号
    let version = merged.updated_at;
    let mut response_settings = merged.clone();
    response_settings.web_admin_password.clear();
    Ok(Json(SettingsResponse {
        _version: version,
        data: response_settings,
    }))
}

pub async fn update_settings(
    State(state): State<AdminState>,
    Json(mut payload): Json<UpdateSettingsRequest>,
) -> Result<Json<RestartResponse>, AdminError> {
    let current = state.settings.read().await.clone();
    if payload._version != current.updated_at {
        return Err(AdminError::VersionMismatch {
            expected: payload._version,
            current: current.updated_at,
        });
    }

    let current_password = current.web_admin_password.clone();
    let current_username = current.web_admin_username.clone();

    if payload.data.web_admin_password.is_empty() {
        payload.data.web_admin_password = current_password.clone();
    }

    let credentials_changed = settings_credentials_changed(&current, &payload.data);
    let mut invalidated_session_count = 0usize;
    if credentials_changed {
        let mut sessions = state.login_sessions.write().await;
        invalidated_session_count +=
            invalidate_sessions_for_username(&mut sessions, &current_username);
    }
    let session_invalidated = credentials_changed;

    if let (Some(runtime), Some(app_handle)) = (state.runtime.clone(), state.app_handle.clone()) {
        let restart_info = crate::commands::config::apply_settings_update_with_restart(
            app_handle,
            &runtime,
            payload.data.clone(),
            true,
        )
        .await?;
        let refreshed = state.settings.read().await.clone();
        let _ = state.db.add_audit_log(
            "admin_settings_updated",
            &format!(
                "port={}, enabled={}, version={}, session_invalidated={}, invalidated_session_count={}, username_changed={}, password_changed={}",
                refreshed.web_admin_port,
                refreshed.web_admin_enabled,
                refreshed.updated_at,
                session_invalidated,
                invalidated_session_count,
                payload.data.web_admin_username != current_username,
                payload.data.web_admin_password != current_password
            ),
        );
        return Ok(Json(RestartResponse {
            ok: true,
            _version: refreshed.updated_at,
            restart: restart_info,
        }));
    }

    state.db.update_settings(&payload.data)?;
    crate::state_version::bump("pool");
    let refreshed = state.db.get_settings()?;
    *state.settings.write().await = refreshed;

    let refreshed = state.settings.read().await.clone();
    let _ = state.db.add_audit_log(
        "admin_settings_updated",
        &format!(
            "port={}, enabled={}, version={}, session_invalidated={}, invalidated_session_count={}, username_changed={}, password_changed={}",
            refreshed.web_admin_port,
            refreshed.web_admin_enabled,
            refreshed.updated_at,
            session_invalidated,
            invalidated_session_count,
            payload.data.web_admin_username != current_username,
            payload.data.web_admin_password != current_password
        ),
    );
    Ok(Json(RestartResponse {
        ok: true,
        _version: refreshed.updated_at,
        restart: None,
    }))
}
