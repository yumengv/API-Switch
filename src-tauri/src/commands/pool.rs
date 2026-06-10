use crate::database::dao::PaginatedResult;
use crate::database::{ApiEntry, ModelGroupConfig};
use crate::error::AppError;
use crate::server_api::ServerApi;
use crate::services::pool_service;
use crate::AppState;
use serde::{Deserialize, Serialize};
use tauri::State;

#[derive(Serialize)]
pub struct TestResult {
    pub status: String,
    pub response_ms: String,
    pub score: f64,
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

#[derive(Deserialize)]
pub struct UpsertModelGroupParams {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default = "default_group_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub priority: i32,
}

#[derive(Deserialize)]
pub struct ModelGroupEnabledParams {
    pub name: String,
    pub enabled: bool,
}

#[derive(Deserialize)]
pub struct ReplaceModelGroupEntriesParams {
    pub name: String,
    pub entry_ids: Vec<String>,
}

#[derive(Deserialize)]
pub struct BatchSortIndexParams {
    pub items: Vec<pool_service::SortIndexUpdate>,
}

fn default_group_enabled() -> bool {
    true
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
pub fn list_entries(
    app: crate::AppEventHandle,
    state: State<'_, AppState>,
) -> Result<Vec<ApiEntry>, AppError> {
    let api = ServerApi::new(state.inner().clone(), app);
    api.list_entries()
}

#[tauri::command]
pub fn list_entries_paginated(
    app: crate::AppEventHandle,
    state: State<'_, AppState>,
    page: i32,
    page_size: i32,
    group_name: Option<String>,
    search: Option<String>,
    channel_id: Option<String>,
) -> Result<PaginatedResult<ApiEntry>, AppError> {
    let api = ServerApi::new(state.inner().clone(), app);
    api.list_entries_paginated(
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
    pin_to_top: Option<bool>,
) -> Result<(), AppError> {
    let api = ServerApi::new(state.inner().clone(), app);
    api.toggle_entry(&id, enabled, pin_to_top.unwrap_or(false))
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
    let api = ServerApi::new(state.inner().clone(), app);
    api.batch_toggle_entries(&ids, enabled)
}

#[tauri::command]
pub async fn reorder_entries(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    ordered_ids: Vec<String>,
) -> Result<(), AppError> {
    let api = ServerApi::new(state.inner().clone(), app);
    api.reorder_entries(&ordered_ids)
}

#[tauri::command]
pub async fn update_entry_sort_index(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    id: String,
    sort_index: i32,
) -> Result<(), AppError> {
    let api = ServerApi::new(state.inner().clone(), app);
    api.update_entry_sort_index(&id, sort_index)
}

#[tauri::command]
pub async fn batch_update_entry_sort_indexes(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    params: BatchSortIndexParams,
) -> Result<(), AppError> {
    let api = ServerApi::new(state.inner().clone(), app);
    api.batch_update_entry_sort_indexes(&params.items)
}

#[tauri::command]
pub async fn delete_entry(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<(), AppError> {
    let api = ServerApi::new(state.inner().clone(), app);
    api.delete_entry(&id)
}

#[tauri::command]
pub async fn create_entry(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    params: CreateEntryParams,
) -> Result<ApiEntry, AppError> {
    let api = ServerApi::new(state.inner().clone(), app);
    api.create_entry(params.into())
}

#[tauri::command]
pub async fn backfill_entry_catalog_meta(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    items: Vec<EntryCatalogMetaUpdate>,
) -> Result<(), AppError> {
    let api = ServerApi::new(state.inner().clone(), app);
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
    api.backfill_entry_catalog_meta(items)
}

#[tauri::command]
pub async fn test_entry_latency(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    entry_id: String,
    model_score: f64,
) -> Result<TestResult, AppError> {
    let api = ServerApi::new(state.inner().clone(), app);
    let result = api.test_entry_latency(&entry_id, model_score).await?;
    Ok(TestResult {
        status: result.status,
        response_ms: result.response_ms,
        score: result.score,
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
    let api = ServerApi::new(state.inner().clone(), app);
    api.update_entry_response_ms(&entry_id, &response_ms)
}

#[tauri::command]
pub fn get_all_groups(
    app: crate::AppEventHandle,
    state: State<'_, AppState>,
) -> Result<Vec<String>, AppError> {
    let api = ServerApi::new(state.inner().clone(), app);
    api.get_all_groups()
}

#[tauri::command]
pub fn list_model_groups(
    app: crate::AppEventHandle,
    state: State<'_, AppState>,
) -> Result<Vec<ModelGroupConfig>, AppError> {
    let api = ServerApi::new(state.inner().clone(), app);
    api.list_model_groups()
}

#[tauri::command]
pub fn list_model_group_entry_ids(
    app: crate::AppEventHandle,
    state: State<'_, AppState>,
    name: String,
) -> Result<Vec<String>, AppError> {
    let api = ServerApi::new(state.inner().clone(), app);
    api.list_model_group_entry_ids(&name)
}

#[tauri::command]
pub async fn upsert_model_group(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    params: UpsertModelGroupParams,
) -> Result<ModelGroupConfig, AppError> {
    let api = ServerApi::new(state.inner().clone(), app);
    api.upsert_model_group(pool_service::UpsertModelGroupParams {
        name: params.name,
        description: params.description,
        enabled: params.enabled,
        priority: params.priority,
    })
}

#[tauri::command]
pub async fn update_model_group_enabled(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    params: ModelGroupEnabledParams,
) -> Result<(), AppError> {
    let api = ServerApi::new(state.inner().clone(), app);
    api.update_model_group_enabled(&params.name, params.enabled)
}

#[tauri::command]
pub async fn delete_model_group(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    name: String,
) -> Result<(), AppError> {
    let api = ServerApi::new(state.inner().clone(), app);
    api.delete_model_group(&name)
}

#[tauri::command]
pub async fn replace_model_group_entries(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    params: ReplaceModelGroupEntriesParams,
) -> Result<(), AppError> {
    let api = ServerApi::new(state.inner().clone(), app);
    api.replace_model_group_entries(pool_service::ReplaceModelGroupEntriesParams {
        name: params.name,
        entry_ids: params.entry_ids,
    })
}

#[tauri::command]
pub async fn update_entry_display_name(
    app: tauri::AppHandle,
    state: tauri::State<'_, crate::AppState>,
    id: String,
    display_name: String,
) -> Result<(), AppError> {
    let api = ServerApi::new(state.inner().clone(), app);
    api.update_entry_display_name(&id, &display_name)
}

#[tauri::command]
pub async fn update_entry_group(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    id: String,
    group_name: String,
) -> Result<(), AppError> {
    let api = ServerApi::new(state.inner().clone(), app);
    api.update_entry_group(&id, &group_name)
}
