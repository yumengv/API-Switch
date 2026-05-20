use crate::state_version;
use serde::Deserialize;

#[derive(Deserialize)]
pub struct TakeDirtyParams {
    pub module: String,
}

/// 取出指定模块的版本号。前端比对上次值，变化即表示有更新
#[tauri::command]
pub async fn take_dirty(params: TakeDirtyParams) -> Result<u64, crate::AppError> {
    match params.module.as_str() {
        "log" => Ok(state_version::current("log")),
        "pool" => Ok(state_version::current("pool")),
        "channel" => Ok(state_version::current("channel")),
        "token" => Ok(state_version::current("token")),
        _ => Err(crate::AppError::Validation(format!("Unknown module: {}", params.module))),
    }
}
