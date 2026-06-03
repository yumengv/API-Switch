//! Config facade：设置的读取 / 完整更新 / 局部补丁。
//!
//! 调用 `commands::config` 中的可复用 helper，底层使用 `AppEventHandle`
//! 因此在所有构建模式下均可用。

use crate::commands::config;
use crate::database::AppSettings;
use crate::error::AppError;

use super::ServerApi;

impl ServerApi {
    /// 从内存缓存获取当前设置。
    pub async fn get_settings(&self) -> Result<AppSettings, AppError> {
        config::get_settings_from_state(self.state()).await
    }

    /// 完整更新设置，触发 proxy/admin 重载等副作用。
    pub async fn update_settings(&self, settings: AppSettings) -> Result<AppSettings, AppError> {
        config::update_settings_from_state(self.app.clone(), self.state(), settings).await
    }

    /// 完整更新设置（异步重载），返回重启信息。
    ///
    /// 用于 admin HTTP handler 等需要 `RestartInfo` 的场景。
    pub async fn update_settings_with_restart(
        &self,
        settings: AppSettings,
        restart_async: bool,
    ) -> Result<Option<crate::admin::RestartInfo>, AppError> {
        config::apply_settings_update_with_restart(
            self.app.clone(),
            self.state(),
            settings,
            restart_async,
        )
        .await
    }

    /// 局部补丁更新设置，返回更新后的设置。
    ///
    /// 注意：`web_admin_password` 保护逻辑（空密码时保持原值）由调用方负责。
    pub async fn patch_settings(
        &self,
        patch: serde_json::Value,
    ) -> Result<AppSettings, AppError> {
        config::patch_settings_from_state(self.app.clone(), self.state(), patch).await
    }
}
