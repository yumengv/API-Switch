use crate::database::{AccessKey, AppSettings, Database};
use crate::error::AppError;
use crate::AppState;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tauri::State;
use tokio::sync::RwLock;

const CONNECTION_APPS_JSON: &str = include_str!("../../../link.json");
const AUTO_ACCESS_KEY_NAME: &str = "AUTO";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionAppItem {
    pub id: String,
    pub name: String,
    pub description: String,
    pub icon: String,
    pub config_mode: String,
    pub status: Option<String>,
    pub config: ConnectionAppConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionAppConfig {
    pub file: Option<String>,
    pub format: Option<String>,
    pub instructions: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfigResult {
    pub action: String,
    pub file_path: Option<String>,
    pub backup_path: Option<String>,
    pub content: Option<String>,
    pub instructions: Option<String>,
}

#[tauri::command]
pub fn list_connection_apps() -> Result<Vec<ConnectionAppItem>, AppError> {
    list_connection_apps_from_embedded()
}

#[tauri::command]
pub async fn execute_connection_app(
    state: State<'_, AppState>,
    id: String,
) -> Result<AppConfigResult, AppError> {
    execute_connection_app_from_parts(&state.db, &state.settings, &id, true).await
}

pub async fn execute_connection_app_from_parts(
    db: &Arc<Database>,
    settings: &Arc<RwLock<AppSettings>>,
    id: &str,
    allow_file_write: bool,
) -> Result<AppConfigResult, AppError> {
    let settings = settings.read().await.clone();
    let access_key = get_or_create_auto_access_key(db)?;
    execute_connection_app_with_context(id, settings.listen_port, &access_key, allow_file_write)
}

fn list_connection_apps_from_embedded() -> Result<Vec<ConnectionAppItem>, AppError> {
    serde_json::from_str(CONNECTION_APPS_JSON)
        .map_err(|e| AppError::Internal(format!("解析连接应用清单失败: {e}")))
}

fn execute_connection_app_with_context(
    id: &str,
    port: i32,
    access_key: &str,
    allow_file_write: bool,
) -> Result<AppConfigResult, AppError> {
    let apps = list_connection_apps_from_embedded()?;
    let app = apps
        .iter()
        .find(|item| item.id == id)
        .ok_or_else(|| AppError::NotFound(format!("未找到连接应用: {id}")))?;

    if app.status.as_deref() == Some("coming_soon") {
        return Err(AppError::Validation(format!("该应用暂不支持连接: {id}")));
    }

    let content = render_connection_content(id, port, access_key)?;
    let file_path = app.config.file.as_deref().map(expand_home_path).transpose()?;
    let instructions = build_instructions(app, file_path.as_ref());

    if should_write_file(app, allow_file_write) {
        let target_path = file_path.ok_or_else(|| AppError::Validation("缺少目标配置文件路径".to_string()))?;
        let backup_path = replace_config_file(&target_path, &content)?;
        return Ok(AppConfigResult {
            action: "write".to_string(),
            file_path: Some(target_path.display().to_string()),
            backup_path: backup_path.map(|path| path.display().to_string()),
            content: None,
            instructions: None,
        });
    }

    Ok(AppConfigResult {
        action: "clipboard".to_string(),
        file_path: file_path.map(|path| path.display().to_string()),
        backup_path: None,
        content: Some(content),
        instructions: Some(instructions),
    })
}

fn render_connection_content(id: &str, port: i32, access_key: &str) -> Result<String, AppError> {
    let base_v1 = format!("http://127.0.0.1:{port}/v1");
    let base_root = format!("http://127.0.0.1:{port}");

    match id {
        "opencode" => Ok(format!(
            r#"{{
  "provider": {{
    "{port}": {{
      "options": {{
        "baseURL": "{base_v1}",
        "apiKey": "{access_key}"
      }},
      "models": {{
        "auto": {{"name": "auto"}}
      }}
    }}
  }}
}}
"#
        )),
        "codex" => Ok(format!(
            r#"model = "auto"
model_provider = "api_switch"

[model_providers.api_switch]
name = "API-Switch"
base_url = "{base_v1}"
api_key = "{access_key}"
"#
        )),
        "claude-code" => Ok(format!(
            r#"{{
  "env": {{
    "ANTHROPIC_AUTH_TOKEN": "{access_key}",
    "ANTHROPIC_BASE_URL": "{base_root}",
    "ANTHROPIC_MODEL": "auto"
  }}
}}
"#
        )),
        "workbuddy" => Ok(format!(
            r#"[
  {{
    "id": "auto",
    "name": "auto",
    "vendor": "Custom",
    "url": "{base_v1}",
    "apiKey": "{access_key}",
    "supportsToolCall": true,
    "supportsImages": true,
    "supportsReasoning": true,
    "useCustomProtocol": false
  }}
]
"#
        )),
        _ => Err(AppError::NotFound(format!("未找到连接应用模板: {id}"))),
    }
}

fn get_or_create_auto_access_key(db: &Database) -> Result<String, AppError> {
    let keys = db.list_access_keys()?;
    if let Some(existing) = select_enabled_auto_access_key(&keys) {
        return Ok(existing.key.clone());
    }

    if let Some(disabled) = select_disabled_auto_access_key(&keys) {
        db.toggle_access_key(&disabled.id, true)?;
        crate::state_version::bump("token");
        return Ok(disabled.key.clone());
    }

    let created = db.create_access_key(AUTO_ACCESS_KEY_NAME)?;
    crate::state_version::bump("token");
    Ok(created.key)
}

fn select_enabled_auto_access_key(keys: &[AccessKey]) -> Option<&AccessKey> {
    keys.iter()
        .rev()
        .find(|key| key.name == AUTO_ACCESS_KEY_NAME && key.enabled)
}

fn select_disabled_auto_access_key(keys: &[AccessKey]) -> Option<&AccessKey> {
    keys.iter()
        .rev()
        .find(|key| key.name == AUTO_ACCESS_KEY_NAME && !key.enabled)
}

fn should_write_file(app: &ConnectionAppItem, allow_file_write: bool) -> bool {
    allow_file_write && app.config_mode == "write" && cfg!(target_os = "windows")
}

fn build_instructions(app: &ConnectionAppItem, file_path: Option<&PathBuf>) -> String {
    match file_path {
        Some(path) => format!("请手动将以下内容添加到 {}", path.display()),
        None => format!("请按 {} 的说明手动添加以下配置", app.name),
    }
}

fn expand_home_path(path: &str) -> Result<PathBuf, AppError> {
    if path == "~" {
        return home_dir();
    }

    if let Some(rest) = path.strip_prefix("~/") {
        return Ok(home_dir()?.join(rest));
    }

    Ok(PathBuf::from(path))
}

fn home_dir() -> Result<PathBuf, AppError> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .ok_or_else(|| AppError::Internal("获取用户目录失败".to_string()))
}

fn replace_config_file(path: &Path, content: &str) -> Result<Option<PathBuf>, AppError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| AppError::Internal(format!("创建配置目录失败: {e}")))?;
    }

    let temp_path = temporary_config_path(path)?;
    std::fs::write(&temp_path, content).map_err(|e| AppError::Internal(format!("写入临时配置失败: {e}")))?;

    let backup_path = if path.exists() {
        let backup_path = backup_path_for(path)?;
        std::fs::rename(path, &backup_path)
            .map_err(|e| AppError::Internal(format!("备份原配置失败: {e}")))?;
        Some(backup_path)
    } else {
        None
    };

    if let Err(err) = std::fs::rename(&temp_path, path) {
        if let Some(backup_path) = &backup_path {
            let _ = std::fs::rename(backup_path, path);
        }
        let _ = std::fs::remove_file(&temp_path);
        return Err(AppError::Internal(format!("配置写入失败: {err}")));
    }

    Ok(backup_path)
}

fn temporary_config_path(path: &Path) -> Result<PathBuf, AppError> {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| AppError::Internal(format!("无法读取配置文件名: {}", path.display())))?;
    let timestamp = Utc::now().format("%Y%m%d-%H%M%S");
    Ok(path.with_file_name(format!(".{file_name}.{timestamp}.tmp")))
}

fn backup_path_for(path: &Path) -> Result<PathBuf, AppError> {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| AppError::Internal(format!("无法读取配置文件名: {}", path.display())))?;
    let timestamp = Utc::now().format("%Y%m%d-%H%M%S");
    Ok(path.with_file_name(format!("{file_name}.{timestamp}.bak")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_connection_apps_returns_only_current_release_apps() {
        let apps = list_connection_apps_from_embedded().expect("应能解析内置连接应用清单");
        let ids: Vec<&str> = apps.iter().map(|app| app.id.as_str()).collect();

        assert_eq!(ids, vec!["opencode", "codex", "claude-code", "workbuddy"]);
        assert!(apps.iter().all(|app| app.config.file.is_some()));
    }

    #[test]
    fn render_connection_content_uses_exact_app_templates() {
        let opencode = render_connection_content("opencode", 19090, "sk-test").expect("OpenCode 应生成配置");
        assert!(opencode.contains("\"19090\":"));
        assert!(opencode.contains("\"baseURL\": \"http://127.0.0.1:19090/v1\""));
        assert!(opencode.contains("\"apiKey\": \"sk-test\""));
        assert!(opencode.contains("\"auto\": {\"name\": \"auto\"}"));

        let codex = render_connection_content("codex", 19090, "sk-test").expect("Codex 应生成配置");
        assert!(codex.contains("model_provider = \"api_switch\""));
        assert!(codex.contains("name = \"API-Switch\""));
        assert!(codex.contains("base_url = \"http://127.0.0.1:19090/v1\""));
        assert!(codex.contains("api_key = \"sk-test\""));
        assert!(!codex.contains("sandbox_mode"));
        assert!(!codex.contains("wire_api"));

        let claude = render_connection_content("claude-code", 19090, "sk-test").expect("Claude Code 应生成配置");
        assert!(claude.contains("\"ANTHROPIC_AUTH_TOKEN\": \"sk-test\""));
        assert!(claude.contains("\"ANTHROPIC_BASE_URL\": \"http://127.0.0.1:19090\""));
        assert!(claude.contains("\"ANTHROPIC_MODEL\": \"auto\""));

        let workbuddy = render_connection_content("workbuddy", 19090, "sk-test").expect("Workbuddy 应生成配置");
        assert!(workbuddy.contains("\"url\": \"http://127.0.0.1:19090/v1\""));
        assert!(workbuddy.contains("\"apiKey\": \"sk-test\""));
        assert!(!workbuddy.contains("/v1/chat/completions"));
    }

    #[test]
    fn select_enabled_auto_access_key_prefers_latest_enabled_auto() {
        let keys = vec![
            access_key("old", "AUTO", "sk-old", true, 1),
            access_key("disabled", "AUTO", "sk-disabled", false, 2),
            access_key("new", "AUTO", "sk-new", true, 3),
        ];

        let selected = select_enabled_auto_access_key(&keys).expect("应选择已启用 AUTO access key");
        assert_eq!(selected.key, "sk-new");
    }

    #[test]
    fn select_disabled_auto_access_key_uses_latest_disabled_when_no_enabled_exists() {
        let keys = vec![
            access_key("old-disabled", "AUTO", "sk-old-disabled", false, 1),
            access_key("other", "OTHER", "sk-other", true, 2),
            access_key("new-disabled", "AUTO", "sk-new-disabled", false, 3),
        ];

        let selected = select_disabled_auto_access_key(&keys).expect("应选择最新禁用 AUTO access key");
        assert_eq!(selected.key, "sk-new-disabled");
    }

    #[test]
    fn should_write_file_requires_explicit_write_permission() {
        let app = ConnectionAppItem {
            id: "opencode".to_string(),
            name: "OpenCode CLI".to_string(),
            description: String::new(),
            icon: "ExternalLink".to_string(),
            config_mode: "write".to_string(),
            status: None,
            config: ConnectionAppConfig {
                file: Some("~/.config/opencode/opencode.jsonc".to_string()),
                format: Some("jsonc".to_string()),
                instructions: None,
            },
        };

        assert!(!should_write_file(&app, false));
    }

    fn access_key(id: &str, name: &str, key: &str, enabled: bool, created_at: i64) -> AccessKey {
        AccessKey {
            id: id.to_string(),
            name: name.to_string(),
            key: key.to_string(),
            enabled,
            created_at,
        }
    }
}
