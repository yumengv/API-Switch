//! SERVER API facade：对核心状态变更的平台无关入口。
//!
//! `ServerApi` 持有 `AppState` 和 `AppEventHandle`，封装 proxy / config / channel /
//! pool / token 五个子 facade，使得非 Tauri-command 的调用方（如 admin HTTP handler、
//! 未来 mobile bridge、headless CLI）能以统一接口操作核心业务。

pub mod channel;
pub mod config;
pub mod pool;
pub mod proxy;
pub mod token;

use crate::AppState;

/// 平台无关的业务操作入口。
///
/// 在 GUI 模式下 `AppEventHandle = tauri::AppHandle`，
/// 在 headless 模式下 `AppEventHandle = ()`（零大小结构体，事件发送为空操作）。
///
/// 调用方（admin handler / future mobile bridge）通过 `ServerApi` 触发核心状态变更，
/// 而非直接依赖 `tauri::State` 或 `#[tauri::command]`。
#[derive(Clone)]
pub struct ServerApi {
    state: AppState,
    app: crate::AppEventHandle,
}

impl ServerApi {
    /// 创建新的 `ServerApi` 实例。
    ///
    /// - `state`: 共享应用状态
    /// - `app`: 事件句柄（GUI 模式下为 `tauri::AppHandle`，headless 下为零大小结构体）
    pub fn new(state: AppState, app: crate::AppEventHandle) -> Self {
        Self { state, app }
    }

    /// 获取内部 `AppState` 的引用。
    pub fn state(&self) -> &AppState {
        &self.state
    }

    /// 获取内部 `AppEventHandle` 的引用。
    pub fn app(&self) -> &crate::AppEventHandle {
        &self.app
    }
}
