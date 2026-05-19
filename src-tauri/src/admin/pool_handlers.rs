use crate::admin::error::AdminError;
use crate::admin::state::AdminState;
use crate::database::dao::PaginatedResult;
use crate::database::ApiEntry;
use crate::services::pool_service;
use axum::extract::{Json, Path, Query, State};
use serde::Deserialize;

// ---------- Request/Response Types -----------------------------------------

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
#[serde(rename_all = "camelCase")]
pub struct CatalogMetaUpdate {
    pub id: String,
    #[serde(default)]
    pub display_name: String,
    pub provider_logo: String,
    pub release_date: String,
    pub model_meta_zh: String,
    pub model_meta_en: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReorderParams {
    pub ordered_ids: Vec<String>,
}

// ---------- Handlers -------------------------------------------------------

/// GET /admin/pool - List all API entries
pub async fn list(State(state): State<AdminState>) -> Result<Json<Vec<ApiEntry>>, AdminError> {
    let entries = pool_service::list_entries(&state.db)?;
    Ok(Json(entries))
}

#[derive(Deserialize)]
pub struct PoolPageParams {
    pub page: Option<i32>,
    pub page_size: Option<i32>,
    pub group_name: Option<String>,
    pub search: Option<String>,
    pub channel_id: Option<String>,
}

/// GET /admin/pool/paginated - List API entries with pagination
pub async fn list_paginated(
    State(state): State<AdminState>,
    Query(params): Query<PoolPageParams>,
) -> Result<Json<PaginatedResult<ApiEntry>>, AdminError> {
    let entries = pool_service::list_entries_paginated(
        &state.db,
        params.page.unwrap_or(1),
        params.page_size.unwrap_or(20),
        params.group_name.as_deref(),
        params.search.as_deref(),
        params.channel_id.as_deref(),
    )?;
    Ok(Json(entries))
}

/// POST /admin/pool - Create a new API entry
pub async fn create(
    State(state): State<AdminState>,
    Json(payload): Json<CreateEntryParams>,
) -> Result<Json<ApiEntry>, AdminError> {
    let entry = pool_service::create_entry(
        &state.db,
        pool_service::CreateEntryParams {
            channel_id: payload.channel_id,
            model: payload.model,
            display_name: payload.display_name,
            provider_logo: payload.provider_logo,
            release_date: payload.release_date,
            model_meta_zh: payload.model_meta_zh,
            model_meta_en: payload.model_meta_en,
            group_name: payload.group_name,
        },
    )?;
    state.mark_pool_dirty();
    Ok(Json(entry))
}

/// PUT /admin/pool/:id/toggle - Toggle an entry's enabled state
pub async fn toggle(
    State(state): State<AdminState>,
    Path(id): Path<String>,
    Json(enabled): Json<bool>,
) -> Result<Json<serde_json::Value>, AdminError> {
    pool_service::toggle_entry(
        &state.db,
        &state
            .runtime
            .as_ref()
            .ok_or_else(|| {
                AdminError::Internal("Runtime state not available for toggle operation".to_string())
            })?
            .failure_counts,
        &id,
        enabled,
    )?;
    state.mark_pool_dirty();
    Ok(Json(serde_json::json!({"ok": true})))
}

/// DELETE /admin/pool/:id - Delete an entry by ID
pub async fn delete(
    State(state): State<AdminState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, AdminError> {
    pool_service::delete_entry(&state.db, &id)?;
    state.mark_pool_dirty();
    Ok(Json(serde_json::json!({"ok": true})))
}

/// POST /admin/pool/reorder - Reorder entries
pub async fn reorder(
    State(state): State<AdminState>,
    Json(payload): Json<ReorderParams>,
) -> Result<Json<serde_json::Value>, AdminError> {
    pool_service::reorder_entries(&state.db, &payload.ordered_ids)?;
    state.mark_pool_dirty();
    Ok(Json(serde_json::json!({"ok": true})))
}

/// POST /admin/pool/:id/test-latency - Test latency for a specific entry
pub async fn test_latency(
    State(state): State<AdminState>,
    Path(id): Path<String>,
) -> Result<Json<pool_service::TestLatencyResult>, AdminError> {
    let result = pool_service::test_entry_latency(
        &state.db,
        &id,
        state.runtime.as_ref().map(|runtime| runtime.dirty.as_ref()),
    )
    .await?;
    Ok(Json(result))
}

/// POST /admin/pool/backfill-catalog-meta - Backfill catalog metadata for multiple entries
pub async fn backfill_catalog_meta(
    State(state): State<AdminState>,
    Json(payload): Json<Vec<CatalogMetaUpdate>>,
) -> Result<Json<serde_json::Value>, AdminError> {
    let updates: Vec<pool_service::CatalogMetaUpdate> = payload
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

    pool_service::backfill_entry_catalog_meta(&state.db, updates)?;
    state.mark_pool_dirty();
    Ok(Json(serde_json::json!({"ok": true})))
}

/// GET /admin/pool/groups - Get all distinct group names
pub async fn get_groups(State(state): State<AdminState>) -> Result<Json<Vec<String>>, AdminError> {
    let groups = pool_service::get_all_groups(&state.db)?;
    Ok(Json(groups))
}


/// PUT /admin/pool/:id/display-name - Update the display_name (alias) for an entry
pub async fn update_display_name(
    State(state): State<AdminState>,
    Path(id): Path<String>,
    Json(payload): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, AdminError> {
    let display_name = payload.get("display_name").and_then(|v| v.as_str()).unwrap_or("");
    pool_service::update_entry_display_name(&state.db, &id, display_name)?;
    state.mark_pool_dirty();
    Ok(Json(serde_json::json!({"ok": true})))
}

/// PUT /admin/pool/:id/group - Update the group_name for an entry
pub async fn update_group(
    State(state): State<AdminState>,
    Path(id): Path<String>,
    Json(group_name): Json<String>,
) -> Result<Json<serde_json::Value>, AdminError> {
    pool_service::update_entry_group(&state.db, &id, &group_name)?;
    state.mark_pool_dirty();
    Ok(Json(serde_json::json!({"ok": true})))
}
