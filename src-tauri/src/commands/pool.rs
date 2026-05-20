use crate::database::dao::PaginatedResult;
use crate::database::ApiEntry;
use crate::error::AppError;
use crate::services::pool_service;
use crate::AppState;
use serde::{Deserialize, Serialize};
use tauri::{Emitter, State};

#[derive(Serialize)]
pub struct TestResult {
    pub status: String,
    pub response_ms: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_detail: Option<String>,
}

#[derive(Deserialize)]
pub struct CreateEntryParams {
    pub channel_id: String,
    pub model: String,
    pub display_name: Option<String>,
    #[serde(default)]
    pub provider_logo: String,
    #[serde(default)]
    pub release_date: String,
    #[serde(default)]
    pub model_meta_zh: String,
    #[serde(default)]
    pub model_meta_en: String,
    #[serde(default)]
    pub group_name: Option<String>,
}

#[derive(Deserialize)]
pub struct EntryCatalogMetaUpdate {
    pub id: String,
    pub display_name: String,
    pub provider_logo: String,
    pub release_date: String,
    pub model_meta_zh: String,
    pub model_meta_en: String,
}

impl From<CreateEntryParams> for pool_service::CreateEntryParams {
    fn from(p: CreateEntryParams) -> Self {
        Self {
            channel_id: p.channel_id,
            model: p.model,
            display_name: p.display_name,
            provider_logo: p.provider_logo,
            release_date: p.release_date,
            model_meta_zh: p.model_meta_zh,
            model_meta_en: p.model_meta_en,
            group_name: p.group_name,
        }
    }
}

#[tauri::command]
pub fn list_entries(state: State<'_, AppState>) -> Result<Vec<ApiEntry>, AppError> {
    pool_service::list_entries(&state.db)
}

#[tauri::command]
pub fn list_entries_paginated(
    state: State<'_, AppState>,
    page: i32,
    page_size: i32,
    group_name: Option<String>,
    search: Option<String>,
    channel_id: Option<String>,
) -> Result<PaginatedResult<ApiEntry>, AppError> {
    pool_service::list_entries_paginated(
        &state.db,
        page,
        page_size,
        group_name.as_deref(),
        search.as_deref(),
        channel_id.as_deref(),
    )
}

#[tauri::command]
pub async fn toggle_entry(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    id: String,
    enabled: bool,
) -> Result<(), AppError> {
    pool_service::toggle_entry(&state.db, &state.failure_counts, &id, enabled)?;
    crate::state_version::bump("pool");
    let _ = app.emit("entries-changed", ());
    crate::state_version::bump("pool");
    crate::refresh_tray_if_enabled(&app);
    Ok(())
}

/// Batch toggle entries: single IPC call to toggle multiple entries.
/// Prevents Tauri IPC storm when user shift+clicks to toggle all visible entries.
#[tauri::command]
pub async fn batch_toggle_entries(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    ids: Vec<String>,
    enabled: bool,
) -> Result<(), AppError> {
    pool_service::batch_toggle_entries(&state.db, &state.failure_counts, &ids, enabled)?;
    crate::state_version::bump("pool");
    let _ = app.emit("entries-changed", ());
    crate::state_version::bump("pool");
    crate::refresh_tray_if_enabled(&app);
    Ok(())
}

#[tauri::command]
pub async fn reorder_entries(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    ordered_ids: Vec<String>,
) -> Result<(), AppError> {
    pool_service::reorder_entries(&state.db, &ordered_ids)?;
    crate::state_version::bump("pool");
    let _ = app.emit("entries-changed", ());
    crate::state_version::bump("pool");
    crate::refresh_tray_if_enabled(&app);
    Ok(())
}

#[tauri::command]
pub async fn delete_entry(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<(), AppError> {
    pool_service::delete_entry(&state.db, &id)?;
    crate::state_version::bump("pool");
    let _ = app.emit("entries-changed", ());
    crate::state_version::bump("pool");
    crate::refresh_tray_if_enabled(&app);
    Ok(())
}

#[tauri::command]
pub async fn create_entry(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    params: CreateEntryParams,
) -> Result<ApiEntry, AppError> {
    let entry = pool_service::create_entry(&state.db, params.into())?;
    crate::state_version::bump("pool");
    let _ = app.emit("entries-changed", ());
    crate::state_version::bump("pool");
    crate::refresh_tray_if_enabled(&app);
    Ok(entry)
}

#[tauri::command]
pub async fn backfill_entry_catalog_meta(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    items: Vec<EntryCatalogMetaUpdate>,
) -> Result<(), AppError> {
    let items: Vec<pool_service::CatalogMetaUpdate> = items
        .into_iter()
        .map(|item| pool_service::CatalogMetaUpdate {
            id: item.id,
            display_name: item.display_name,
            provider_logo: item.provider_logo,
            release_date: item.release_date,
            model_meta_zh: item.model_meta_zh,
            model_meta_en: item.model_meta_en,
        })
        .collect();
    pool_service::backfill_entry_catalog_meta(&state.db, items)?;
    crate::state_version::bump("pool");
    crate::refresh_tray_if_enabled(&app);
    Ok(())
}

#[tauri::command]
pub async fn test_entry_latency(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    entry_id: String,
) -> Result<TestResult, AppError> {
    let db = state.db.clone();
    let result = pool_service::test_entry_latency(&db, &entry_id).await?;
    let _ = app.emit("entries-changed", ());
    crate::state_version::bump("pool");
    crate::refresh_tray_if_enabled(&app);
    Ok(TestResult {
        status: result.status,
        response_ms: result.response_ms,
        error_detail: result.error_detail,
    })
}

#[tauri::command]
pub async fn update_entry_response_ms(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    entry_id: String,
    response_ms: String,
) -> Result<(), AppError> {
    pool_service::update_entry_response_ms(&state.db, &entry_id, &response_ms)?;
    crate::state_version::bump("pool");
    let _ = app.emit("entries-changed", ());
    crate::state_version::bump("pool");
    crate::refresh_tray_if_enabled(&app);
    Ok(())
}

#[tauri::command]
pub fn get_all_groups(state: State<'_, AppState>) -> Result<Vec<String>, AppError> {
    pool_service::get_all_groups(&state.db)
}

#[tauri::command]
pub async fn update_entry_display_name(
    app: tauri::AppHandle,
    state: tauri::State<'_, crate::AppState>,
    id: String,
    display_name: String,
) -> Result<(), AppError> {
    pool_service::update_entry_display_name(&state.db, &id, &display_name)?;
    crate::state_version::bump("pool");
    let _ = app.emit("entries-changed", ());
    crate::state_version::bump("pool");
    crate::refresh_tray_if_enabled(&app);
    Ok(())
}

#[tauri::command]
pub async fn update_entry_group(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    id: String,
    group_name: String,
) -> Result<(), AppError> {
    pool_service::update_entry_group(&state.db, &id, &group_name)?;
    crate::state_version::bump("pool");
    let _ = app.emit("entries-changed", ());
    crate::state_version::bump("pool");
    crate::refresh_tray_if_enabled(&app);
    Ok(())
}
