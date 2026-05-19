use crate::admin::error::{
    AdminError, ERROR_CODE_BAD_REQUEST, ERROR_CODE_EMPTY_MODEL_LIST,
    ERROR_CODE_ENDPOINT_CORRECTION_FAILED, ERROR_CODE_ENDPOINT_UNREACHABLE,
    ERROR_CODE_INVALID_CREDENTIALS, ERROR_CODE_INVALID_URL, ERROR_CODE_RATE_LIMITED,
    ERROR_CODE_TIMEOUT, ERROR_CODE_UNSUPPORTED_PROVIDER,
};
use crate::admin::state::AdminState;
use crate::database::dao::PaginatedResult;
use crate::database::{Channel, ModelInfo};
use crate::services::channel_service;
use crate::services::channel_service::{
    ChannelOperationError, FetchModelsResult, ProbeResult, TestChannelResult,
};
use axum::extract::{Json, Path, Query, State};
use serde::Deserialize;

// Types for request bodies 鈥?reuse the same definitions as in the Tauri commands
#[derive(Deserialize)]
pub struct CreateChannelParams {
    pub name: String,
    pub api_type: String,
    pub base_url: String,
    pub api_key: String,
    pub notes: Option<String>,
}

#[derive(Deserialize, Default)]
pub struct UpdateChannelParams {
    pub name: Option<String>,
    pub api_type: Option<String>,
    pub base_url: Option<String>,
    pub api_key: Option<String>,
    pub enabled: Option<bool>,
    pub notes: Option<String>,
}

#[derive(Deserialize)]
pub struct UpdateResponseMsParams {
    #[serde(rename = "channelId")]
    pub channel_id: String,
    #[serde(rename = "responseMs")]
    pub response_ms: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FetchModelsDirectParams {
    pub api_type: String,
    pub base_url: String,
    pub api_key: String,
    pub verified: Option<bool>,
}

#[derive(Deserialize)]
pub struct TestChannelDirectParams {
    pub api_type: String,
    pub base_url: String,
    pub api_key: String,
    pub model: String,
}

#[derive(Deserialize)]
pub struct ProbeUrlParams {
    pub url: String,
    pub api_type: Option<String>,
    pub api_key: Option<String>,
}

#[derive(Deserialize)]
pub struct SelectModelsParams {
    #[serde(rename = "modelNames")]
    pub model_names: Vec<String>,
    #[serde(rename = "availableModels", default)]
    pub available_models: Vec<ModelInfo>,
    #[serde(rename = "catalogMeta", default)]
    pub catalog_meta: Vec<crate::database::ModelCatalogMetaInput>,
}

fn channel_operation_error_to_admin(error: &ChannelOperationError) -> AdminError {
    match error.code.as_str() {
        ERROR_CODE_INVALID_CREDENTIALS => AdminError::InvalidCredentials {
            remaining_attempts: 0,
            locked_until: None,
        },
        ERROR_CODE_TIMEOUT => AdminError::Timeout { url: None },
        ERROR_CODE_ENDPOINT_UNREACHABLE => AdminError::EndpointUnreachable {
            url: error
                .details
                .as_ref()
                .and_then(|v| v.get("url"))
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string(),
        },
        ERROR_CODE_RATE_LIMITED => AdminError::RateLimited {
            retry_after_seconds: error
                .details
                .as_ref()
                .and_then(|v| v.get("retry_after_seconds"))
                .and_then(|v| v.as_i64())
                .unwrap_or(0),
            remaining_attempts: 0,
            locked_until: 0,
        },
        ERROR_CODE_INVALID_URL
        | ERROR_CODE_BAD_REQUEST
        | ERROR_CODE_UNSUPPORTED_PROVIDER
        | ERROR_CODE_EMPTY_MODEL_LIST => AdminError::BadRequest(error.message.clone()),
        ERROR_CODE_ENDPOINT_CORRECTION_FAILED => AdminError::Conflict {
            code: ERROR_CODE_ENDPOINT_CORRECTION_FAILED,
            message: error.message.clone(),
            details: error.details.clone(),
        },
        _ => AdminError::Internal(error.message.clone()),
    }
}

fn ensure_fetch_models_result(result: FetchModelsResult) -> Result<FetchModelsResult, AdminError> {
    if let Some(error) = &result.error {
        return Err(channel_operation_error_to_admin(error));
    }
    Ok(result)
}

fn ensure_probe_result(result: ProbeResult) -> Result<ProbeResult, AdminError> {
    if let Some(error) = &result.error {
        return Err(channel_operation_error_to_admin(error));
    }
    Ok(result)
}

// ---------- Handlers -------------------------------------------------------

pub async fn list(State(state): State<AdminState>) -> Result<Json<Vec<Channel>>, AdminError> {
    let res = channel_service::list_channels(&state.db)?;
    Ok(Json(res))
}

#[derive(Deserialize)]
pub struct SimplePageParams {
    pub page: Option<i32>,
    pub page_size: Option<i32>,
}

pub async fn list_paginated(
    State(state): State<AdminState>,
    Query(params): Query<SimplePageParams>,
) -> Result<Json<PaginatedResult<Channel>>, AdminError> {
    let res = channel_service::list_channels_paginated(
        &state.db,
        params.page.unwrap_or(1),
        params.page_size.unwrap_or(20),
    )?;
    Ok(Json(res))
}

pub async fn create(
    State(state): State<AdminState>,
    Json(payload): Json<CreateChannelParams>,
) -> Result<Json<Channel>, AdminError> {
    let res = channel_service::create_channel(
        &state.db,
        channel_service::CreateChannelParams {
            name: payload.name,
            api_type: payload.api_type,
            base_url: payload.base_url,
            api_key: payload.api_key,
            notes: payload.notes,
        },
    )?;
    state.mark_channel_dirty();
    Ok(Json(res))
}

pub async fn update(
    State(state): State<AdminState>,
    Path(id): Path<String>,
    Json(payload): Json<UpdateChannelParams>,
) -> Result<Json<Channel>, AdminError> {
    let params = channel_service::UpdateChannelParams {
        id,
        name: payload.name,
        api_type: payload.api_type,
        base_url: payload.base_url,
        api_key: payload.api_key,
        enabled: payload.enabled,
        notes: payload.notes,
    };
    let chan = channel_service::update_channel(&state.db, None, params)?;
    state.mark_channel_dirty();
    Ok(Json(chan))
}

pub async fn delete(
    State(state): State<AdminState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, AdminError> {
    channel_service::delete_channel(&state.db, None, id)?;
    state.mark_channel_dirty();
    state.mark_pool_dirty();
    Ok(Json(serde_json::json!({"ok": true})))
}

pub async fn fetch_models(
    State(state): State<AdminState>,
    Path(id): Path<String>,
) -> Result<Json<FetchModelsResult>, AdminError> {
    let res = channel_service::fetch_models(&state.db, id).await?;
    Ok(Json(ensure_fetch_models_result(res)?))
}

pub async fn fetch_models_direct(
    State(_state): State<AdminState>,
    Json(payload): Json<FetchModelsDirectParams>,
) -> Result<Json<FetchModelsResult>, AdminError> {
    let res = channel_service::fetch_models_direct(
        payload.api_type,
        payload.base_url,
        payload.api_key,
        payload.verified,
    )
    .await?;
    Ok(Json(ensure_fetch_models_result(res)?))
}

pub async fn probe_url(
    State(_): State<AdminState>,
    Json(payload): Json<ProbeUrlParams>,
) -> Result<Json<ProbeResult>, AdminError> {
    let res = channel_service::probe_url(payload.url, payload.api_type, payload.api_key).await?;
    Ok(Json(ensure_probe_result(res)?))
}

pub async fn select_models(
    State(state): State<AdminState>,
    Path(id): Path<String>,
    Json(payload): Json<SelectModelsParams>,
) -> Result<Json<serde_json::Value>, AdminError> {
    state
        .db
        .update_channel_models(&id, &payload.available_models, &payload.model_names)?;
    state.db.sync_entries_for_channel_with_meta(
        &id,
        &payload.model_names,
        &payload.catalog_meta,
    )?;
    state.mark_channel_dirty();
    state.mark_pool_dirty();
    Ok(Json(serde_json::json!({"ok": true})))
}

pub async fn update_response_ms(
    State(state): State<AdminState>,
    Json(payload): Json<UpdateResponseMsParams>,
) -> Result<Json<serde_json::Value>, AdminError> {
    channel_service::update_channel_response_ms(
        &state.db,
        channel_service::UpdateResponseMsParams {
            channel_id: payload.channel_id,
            response_ms: payload.response_ms,
        },
    )?;
    state.mark_channel_dirty();
    Ok(Json(serde_json::json!({"ok": true})))
}

pub async fn test_channel(
    State(state): State<AdminState>,
    Path(id): Path<String>,
) -> Result<Json<TestChannelResult>, AdminError> {
    let channel = state.db.get_channel(&id)?;
    let model = channel
        .selected_models
        .first()
        .or_else(|| channel.available_models.first().map(|m| &m.name))
        .cloned()
        .unwrap_or_else(|| "gpt-3.5-turbo".to_string());
    let result = channel_service::test_channel_chat(
        &channel.base_url,
        &channel.api_key,
        &channel.api_type,
        &model,
    )
    .await;
    if result.success && result.status_code == Some(200) {
        let _ = state
            .db
            .update_channel_response_ms(&channel.id, &result.latency_ms.to_string());
        let _ = channel_service::update_channel(
            &state.db,
            None,
            channel_service::UpdateChannelParams {
                id: channel.id.clone(),
                name: None,
                api_type: None,
                base_url: None,
                api_key: None,
                enabled: Some(true),
                notes: None,
            },
        );
    } else {
        let _ = state.db.disable_channel(&channel.id);
    }
    state.mark_channel_dirty();
    Ok(Json(result))
}

pub async fn test_channel_direct(
    State(_state): State<AdminState>,
    Json(payload): Json<TestChannelDirectParams>,
) -> Result<Json<TestChannelResult>, AdminError> {
    let result = channel_service::test_channel_direct(channel_service::TestChannelDirectParams {
        api_type: payload.api_type,
        base_url: payload.base_url,
        api_key: payload.api_key,
        model: payload.model,
    })
    .await;
    Ok(Json(result))
}

pub async fn save_with_models(
    State(state): State<AdminState>,
    Json(params): Json<channel_service::SaveChannelWithModelsParams>,
) -> Result<Json<channel_service::SaveChannelWithModelsResult>, AdminError> {
    let result =
        channel_service::save_channel_with_models(&state.db, state.app_handle.as_ref(), params)?;
    state.mark_channel_dirty();
    state.mark_pool_dirty();
    Ok(Json(result))
}
