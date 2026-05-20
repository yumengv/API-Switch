use crate::database::{AppSettings, Database};
use crate::AppState;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};

#[derive(Default)]
pub struct LoginFailureState {
    pub count: u32,
    pub locked_until: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Clone)]
pub struct SessionInfo {
    pub username: String,
    pub expires_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Clone)]
pub struct AdminState {
    pub db: Arc<Database>,
    pub settings: Arc<RwLock<AppSettings>>,
    pub login_sessions: Arc<RwLock<HashMap<String, SessionInfo>>>,
    pub login_failures: Arc<Mutex<HashMap<String, LoginFailureState>>>,
    pub runtime: Option<AppState>,
    pub app_handle: Option<tauri::AppHandle>,
}

impl AdminState {
    pub fn new_runtime(runtime: AppState, app_handle: tauri::AppHandle) -> Self {
        Self {
            db: runtime.db.clone(),
            settings: runtime.settings.clone(),
            login_sessions: Arc::new(RwLock::new(HashMap::new())),
            login_failures: Arc::new(Mutex::new(HashMap::new())),
            runtime: Some(runtime),
            app_handle: Some(app_handle),
        }
    }

    pub fn mark_channel_dirty(&self) {
        if let Some(handle) = &self.app_handle {
            let _ = tauri::Emitter::emit(handle, "channels-changed", ());
        }
        crate::state_version::bump("channel");
    }

    pub fn mark_pool_dirty(&self) {
        if let Some(handle) = &self.app_handle {
            let _ = tauri::Emitter::emit(handle, "entries-changed", ());
            crate::refresh_tray_if_enabled(handle);
        }
        crate::state_version::bump("pool");
    }

    pub fn mark_token_dirty(&self) {
        crate::state_version::bump("token");
    }

    pub fn mark_log_dirty(&self) {
        crate::state_version::bump("log");
    }
}
