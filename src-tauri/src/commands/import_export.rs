use crate::error::AppError;
use crate::services::import_export_service::{
    build_import_preview, build_transfer_json, validate_transfer_payload, ImportPreview,
    ImportResult,
};
use crate::AppState;
use tauri::{Emitter, State};

#[tauri::command]
pub fn export_channel_model_transfer(state: State<'_, AppState>) -> Result<String, AppError> {
    let channels = state.db.list_channels()?;
    let entries = state.db.list_entries()?;
    build_transfer_json(&channels, &entries)
}

#[tauri::command]
pub fn preview_channel_model_transfer(
    state: State<'_, AppState>,
    payload: String,
) -> Result<ImportPreview, AppError> {
    let transfer = validate_transfer_payload(&payload)?;
    let current_channels = state.db.list_channels()?.len();
    let current_models = state.db.list_entries()?.len();
    Ok(build_import_preview(
        &transfer,
        current_channels,
        current_models,
    ))
}

#[tauri::command]
pub fn import_channel_model_transfer(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    payload: String,
) -> Result<ImportResult, AppError> {
    let transfer = validate_transfer_payload(&payload)?;
    let (channel_count, model_count) = state
        .db
        .replace_channels_and_models_from_transfer(&transfer)?;

    crate::state_version::bump("channel");
    crate::state_version::bump("pool");
    let _ = app.emit("channels-changed", ());
    let _ = app.emit("entries-changed", ());
    crate::refresh_tray_if_enabled(&app);

    Ok(ImportResult {
        success: true,
        message: format!("导入成功，已重建 {channel_count} 个渠道和 {model_count} 个模型。"),
        channel_count,
        model_count,
    })
}
