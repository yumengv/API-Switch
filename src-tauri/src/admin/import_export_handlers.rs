use crate::admin::error::AdminError;
use crate::admin::state::AdminState;
use crate::services::import_export_service::{
    build_import_preview, build_transfer_json, validate_transfer_payload, ImportPreview,
    ImportResult,
};
use axum::extract::State;
use axum::Json;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportResponse {
    pub payload: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportPayloadRequest {
    pub payload: String,
}

pub async fn export_channel_model_transfer(
    State(state): State<AdminState>,
) -> Result<Json<ExportResponse>, AdminError> {
    let channels = state
        .db
        .list_channels()
        .map_err(|e| AdminError::Internal(e.to_string()))?;
    let entries = state
        .db
        .list_entries()
        .map_err(|e| AdminError::Internal(e.to_string()))?;
    let payload = build_transfer_json(&channels, &entries)
        .map_err(|e| AdminError::Internal(e.to_string()))?;

    Ok(Json(ExportResponse { payload }))
}

pub async fn preview_channel_model_transfer(
    State(state): State<AdminState>,
    Json(request): Json<ImportPayloadRequest>,
) -> Result<Json<ImportPreview>, AdminError> {
    let transfer = validate_transfer_payload(&request.payload)
        .map_err(|e| AdminError::BadRequest(e.to_string()))?;
    let current_channels = state
        .db
        .list_channels()
        .map_err(|e| AdminError::Internal(e.to_string()))?
        .len();
    let current_models = state
        .db
        .list_entries()
        .map_err(|e| AdminError::Internal(e.to_string()))?
        .len();

    Ok(Json(build_import_preview(
        &transfer,
        current_channels,
        current_models,
    )))
}

pub async fn import_channel_model_transfer(
    State(state): State<AdminState>,
    Json(request): Json<ImportPayloadRequest>,
) -> Result<Json<ImportResult>, AdminError> {
    let transfer = validate_transfer_payload(&request.payload)
        .map_err(|e| AdminError::BadRequest(e.to_string()))?;
    let (channel_count, model_count) = state
        .db
        .replace_channels_and_models_from_transfer(&transfer)
        .map_err(|e| AdminError::Internal(e.to_string()))?;

    crate::state_version::bump("channel");
    crate::state_version::bump("pool");
    state.mark_channel_dirty();
    state.mark_pool_dirty();

    Ok(Json(ImportResult {
        success: true,
        message: format!("导入成功，已重建 {channel_count} 个渠道和 {model_count} 个模型。"),
        channel_count,
        model_count,
    }))
}
