use crate::database::dao::PaginatedResult;
use crate::database::AccessKey;
use crate::error::AppError;
use crate::server_api::ServerApi;
use crate::services::token_service;
use crate::AppState;
use tauri::{AppHandle, State};

#[tauri::command]
pub fn list_access_keys(state: State<'_, AppState>) -> Result<Vec<AccessKey>, AppError> {
    token_service::list_access_keys(&state.db)
}

#[tauri::command]
pub fn list_access_keys_paginated(
    state: State<'_, AppState>,
    page: i32,
    page_size: i32,
) -> Result<PaginatedResult<AccessKey>, AppError> {
    token_service::list_access_keys_paginated(&state.db, page, page_size)
}

#[tauri::command]
pub fn create_access_key(app: AppHandle, state: State<'_, AppState>, name: String) -> Result<AccessKey, AppError> {
    let api = ServerApi::new(state.inner().clone(), app);
    api.create_access_key(&name)
}

#[tauri::command]
pub async fn delete_access_key(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<(), AppError> {
    let api = ServerApi::new(state.inner().clone(), app);
    api.delete_access_key(&id)
}

#[tauri::command]
pub async fn toggle_access_key(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    enabled: bool,
) -> Result<(), AppError> {
    let api = ServerApi::new(state.inner().clone(), app);
    api.toggle_access_key(&id, enabled)
}
