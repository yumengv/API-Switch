//! Proxy facade：代理服务器的启动 / 停止 / 状态查询。
//!
//! 直接操作 AppState 中的 ProxyServer，不依赖 proxy_cmd helpers（后者需要 tauri::AppHandle）。

use crate::commands::config::refresh_settings_l1;
use crate::error::AppError;
use crate::proxy::ProxyStatus;

use super::ServerApi;

impl ServerApi {
    /// 启动代理服务器并绑定 admin router。
    pub async fn start_proxy(&self) -> Result<ProxyStatus, AppError> {
        let state = self.state();
        let settings = state.settings.read().await.clone();
        let port = settings.listen_port;

        let mut proxy_guard = state.proxy.write().await;
        if proxy_guard.is_some() {
            return Err(AppError::Proxy("Proxy already running".to_string()));
        }

        let server = crate::proxy::ProxyServer::new(
            port,
            state.db.clone(),
            state.settings.clone(),
            Some(self.app().clone()),
            state.failure_counts.clone(),
        );
        let admin_router = crate::admin::build_combined_router(
            &settings,
            crate::admin::AdminState::new_runtime(state.clone(), self.app().clone()),
        );
        server
            .start_with_admin(admin_router)
            .await
            .map_err(|e| AppError::Proxy(e.to_string()))?;

        let status = ProxyStatus {
            running: true,
            address: "127.0.0.1".to_string(),
            port,
        };

        *proxy_guard = Some(server);
        state.db.set_config_value("proxy_enabled", "1")?;
        refresh_settings_l1(state).await?;

        Ok(status)
    }

    /// 停止正在运行的代理服务器。
    pub async fn stop_proxy(&self) -> Result<(), AppError> {
        let state = self.state();
        let mut proxy_guard = state.proxy.write().await;
        if let Some(server) = proxy_guard.take() {
            server
                .stop()
                .await
                .map_err(|e| AppError::Proxy(e.to_string()))?;
        }
        state.db.set_config_value("proxy_enabled", "0")?;
        refresh_settings_l1(state).await?;
        Ok(())
    }

    /// 查询代理当前运行状态（不改变任何状态）。
    pub async fn proxy_status(&self) -> Result<ProxyStatus, AppError> {
        let state = self.state();
        let proxy_guard = state.proxy.read().await;
        let settings = state.settings.read().await.clone();

        Ok(match proxy_guard.as_ref() {
            Some(server) => server.get_status(),
            None => ProxyStatus {
                running: false,
                address: "127.0.0.1".to_string(),
                port: settings.listen_port,
            },
        })
    }
}
