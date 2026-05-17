use crate::AppState;
use serde::Deserialize;
use tauri::State;

#[derive(Deserialize)]
pub struct TakeDirtyParams {
    pub module: String,
}

/// 取出指定模块的 dirty 标记并清零。返回 `true` 表示有更新。
#[tauri::command]
pub async fn take_dirty(state: State<'_, AppState>, params: TakeDirtyParams) -> Result<bool, crate::AppError> {
    let dirty = state.dirty.clone();
    match params.module.as_str() {
        "log" => Ok(dirty.take_log()),
        "pool" => Ok(dirty.take_pool()),
        "channel" => Ok(dirty.take_channel()),
        "token" => Ok(dirty.take_token()),
        _ => Err(crate::AppError::Validation(format!("Unknown module: {}", params.module))),
    }
}
