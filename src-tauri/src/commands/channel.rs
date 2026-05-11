use crate::database::{Channel, ModelInfo};
use crate::error::AppError;
use crate::services::channel_service::{self, FetchModelsResult, ProbeResult, TestChannelResult};
use crate::AppState;
use serde::Deserialize;
use tauri::{Emitter, State};

#[derive(Deserialize)]
pub struct ModelCatalogMetaInput {
    pub model: String,
    pub provider_logo: String,
    pub release_date: String,
    pub model_meta_zh: String,
    pub model_meta_en: String,
}

#[derive(Clone)]
#[allow(dead_code)]
struct ProbeSuccess {
    models: Vec<ModelInfo>,
    corrected_base_url: String,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
enum ModelsEndpointError {
    Network(String),
    Timeout(String),
    Auth(u16),
    Http(u16),
    Parse(String),
    Empty,
}

#[allow(dead_code)]
impl ModelsEndpointError {
    fn is_blocking(&self) -> bool {
        matches!(self, Self::Network(_) | Self::Timeout(_) | Self::Auth(_))
    }

    fn message(&self) -> String {
        match self {
            Self::Network(msg) => format!("Network error: {msg}"),
            Self::Timeout(msg) => format!("Network timeout: {msg}"),
            Self::Auth(status) => format!("Authentication failed: HTTP {status}"),
            Self::Http(status) => format!("HTTP {status}"),
            Self::Parse(msg) => format!("Invalid model list response: {msg}"),
            Self::Empty => "Empty model list".to_string(),
        }
    }
}

#[derive(Clone)]
#[allow(dead_code)]
struct EndpointGuess {
    detected_type: String,
    corrected_base_url: String,
}

#[derive(Deserialize)]
pub struct CreateChannelParams {
    pub name: String,
    pub api_type: String,
    pub base_url: String,
    pub api_key: String,
    pub notes: Option<String>,
}

#[derive(Deserialize)]
pub struct UpdateChannelParams {
    pub id: String,
    pub name: Option<String>,
    pub api_type: Option<String>,
    pub base_url: Option<String>,
    pub api_key: Option<String>,
    pub enabled: Option<bool>,
    pub notes: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct UpdateResponseMsParams {
    #[serde(rename = "channelId")]
    channel_id: String,
    #[serde(rename = "responseMs")]
    response_ms: String,
}

#[tauri::command]
pub fn update_channel_response_ms(
    state: State<'_, AppState>,
    params: UpdateResponseMsParams,
) -> Result<(), AppError> {
    channel_service::update_channel_response_ms(
        &state.db,
        channel_service::UpdateResponseMsParams {
            channel_id: params.channel_id,
            response_ms: params.response_ms,
        },
    )
}

#[tauri::command]
pub fn list_channels(state: State<'_, AppState>) -> Result<Vec<Channel>, AppError> {
    channel_service::list_channels(&state.db)
}

#[tauri::command]
pub fn create_channel(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    params: CreateChannelParams,
) -> Result<Channel, AppError> {
    let channel = channel_service::create_channel(
        &state.db,
        channel_service::CreateChannelParams {
            name: params.name,
            api_type: params.api_type,
            base_url: params.base_url,
            api_key: params.api_key,
            notes: params.notes,
        },
    )?;
    let _ = app.emit("channels-changed", ());
    Ok(channel)
}

#[tauri::command]
pub fn update_channel(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    params: UpdateChannelParams,
) -> Result<Channel, AppError> {
    channel_service::update_channel(
        &state.db,
        Some(&app),
        channel_service::UpdateChannelParams {
            id: params.id,
            name: params.name,
            api_type: params.api_type,
            base_url: params.base_url,
            api_key: params.api_key,
            enabled: params.enabled,
            notes: params.notes,
        },
    )
}

#[tauri::command]
pub fn delete_channel(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<(), AppError> {
    channel_service::delete_channel(&state.db, Some(&app), id)
}

#[tauri::command]
pub async fn probe_url(url: String) -> Result<ProbeResult, AppError> {
    channel_service::probe_url(url).await
}

#[tauri::command]
pub async fn test_channel(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    channel_id: String,
) -> Result<TestChannelResult, AppError> {
    let channel = state.db.get_channel(&channel_id)?;

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
        let _ =
            state
                .db
                .update_channel_response_ms(&channel_id, &result.latency_ms.to_string());
        let _ = channel_service::update_channel(
            &state.db,
            Some(&app),
            channel_service::UpdateChannelParams {
                id: channel_id.clone(),
                name: None,
                api_type: None,
                base_url: None,
                api_key: None,
                enabled: Some(true),
                notes: None,
            },
        );
    } else {
        let _ = state.db.disable_channel(&channel_id);
    }

    Ok(result)
}

#[tauri::command]
pub async fn fetch_models_direct(
    _state: State<'_, AppState>,
    api_type: String,
    base_url: String,
    api_key: String,
    verified: Option<bool>,
) -> Result<FetchModelsResult, AppError> {
    channel_service::fetch_models_direct(api_type, base_url, api_key, verified).await
}

#[tauri::command]
pub async fn fetch_models(
    state: State<'_, AppState>,
    channel_id: String,
) -> Result<FetchModelsResult, AppError> {
    channel_service::fetch_models(&state.db, channel_id).await
}

#[tauri::command]
pub fn select_models(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    channel_id: String,
    model_names: Vec<String>,
    available_models: Vec<ModelInfo>,
    catalog_meta: Vec<ModelCatalogMetaInput>,
) -> Result<(), AppError> {
    state
        .db
        .update_channel_models(&channel_id, &available_models, &model_names)?;
    let catalog_meta: Vec<crate::database::ModelCatalogMetaInput> = catalog_meta
        .into_iter()
        .map(|item| crate::database::ModelCatalogMetaInput {
            model: item.model,
            provider_logo: item.provider_logo,
            release_date: item.release_date,
            model_meta_zh: item.model_meta_zh,
            model_meta_en: item.model_meta_en,
        })
        .collect();
    state
        .db
        .sync_entries_for_channel_with_meta(&channel_id, &model_names, &catalog_meta)?;
    let _ = app.emit("entries-changed", ());
    let _ = app.emit("channels-changed", ());
    crate::refresh_tray_if_enabled(&app);
    Ok(())
}

/// Generate model list URL candidates: adapter standard + common variants
#[allow(dead_code)]
fn build_models_url_variants(
    adapter: &(dyn crate::proxy::protocol::ProtocolAdapter + Send + Sync),
    base_url: &str,
    api_key: &str,
) -> Vec<String> {
    let mut urls = vec![adapter.build_models_url(base_url, api_key)];
    let base = base_url.trim_end_matches('/');
    for v in &[
        "/models",
        "/v1/models",
        "/api/models",
        "/api/v1/models",
        "/v2/models",
    ] {
        let u = format!("{base}{v}");
        if !urls.contains(&u) {
            urls.push(u);
        }
    }
    urls
}

/// Try a single models endpoint, return parsed models or a structured endpoint error.
#[allow(dead_code)]
async fn try_models_endpoint(
    client: &reqwest::Client,
    adapter: &(dyn crate::proxy::protocol::ProtocolAdapter + Send + Sync),
    url: &str,
    api_key: &str,
) -> Result<Vec<ModelInfo>, ModelsEndpointError> {
    let resp = adapter
        .apply_auth(client.get(url), api_key)
        .send()
        .await
        .map_err(|e| {
            if e.is_timeout() {
                ModelsEndpointError::Timeout(e.to_string())
            } else if e.is_connect() || e.is_request() {
                ModelsEndpointError::Network(e.to_string())
            } else {
                ModelsEndpointError::Network(e.to_string())
            }
        })?;
    let status = resp.status();
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        return Err(ModelsEndpointError::Auth(status.as_u16()));
    }
    if status != reqwest::StatusCode::OK {
        return Err(ModelsEndpointError::Http(status.as_u16()));
    }
    let body: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| ModelsEndpointError::Parse(e.to_string()))?;
    let models: Vec<ModelInfo> = adapter
        .parse_models_response(&body)
        .into_iter()
        .map(|(id, owned_by)| ModelInfo {
            name: id.clone(),
            id,
            owned_by,
        })
        .collect();
    if models.is_empty() {
        Err(ModelsEndpointError::Empty)
    } else {
        Ok(models)
    }
}

/// Try to extract model list from a JSON body (even error responses)
#[allow(dead_code)]
fn extract_models_from_json(body: &str) -> Option<Vec<ModelInfo>> {
    let json: serde_json::Value = serde_json::from_str(body).ok()?;
    let arr = json.get("data")?.as_array()?;
    let models: Vec<ModelInfo> = arr
        .iter()
        .filter_map(|m| {
            let id = m.get("id")?.as_str()?.to_string();
            if id.eq_ignore_ascii_case("auto") {
                return None;
            }
            let owned = m
                .get("owned_by")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();
            Some(ModelInfo {
                name: id.clone(),
                id,
                owned_by: Some(owned),
            })
        })
        .collect();
    if models.is_empty() {
        None
    } else {
        Some(models)
    }
}

/// Chat probe: send a tiny request to verify the API works
#[allow(dead_code)]
async fn try_chat_probe(
    client: &reqwest::Client,
    adapter: &(dyn crate::proxy::protocol::ProtocolAdapter + Send + Sync),
    base_url: &str,
    api_key: &str,
    api_type: &str,
) -> Option<ProbeSuccess> {
    let test_model = match api_type {
        "claude" => "claude-3-5-sonnet-20241022",
        "gemini" => "gemini-2.0-flash",
        _ => "gpt-4o-mini",
    };
    let chat_url = adapter.build_chat_url(base_url, test_model);
    let body = serde_json::json!({"model": test_model, "messages": [{"role":"user","content":"hi"}], "max_tokens": 1});
    let req = adapter.apply_auth(
        client
            .post(&chat_url)
            .header("Content-Type", "application/json"),
        api_key,
    );
    match req.json(&body).send().await {
        Ok(resp) => {
            let s = resp.status().as_u16();
            if s < 500 {
                let corrected_base_url =
                    crate::services::channel_service::canonical_base_url_for_success(
                        api_type, base_url, &chat_url,
                    );
                // Server responded → API works, return known models
                if let Ok(text) = resp.text().await {
                    if let Some(m) = extract_models_from_json(&text) {
                        return Some(ProbeSuccess {
                            models: m,
                            corrected_base_url,
                        });
                    }
                }
                return Some(ProbeSuccess {
                    models: known_models_for_type(api_type),
                    corrected_base_url,
                });
            }
            None
        }
        Err(_) => None,
    }
}

/// Commonly known models per API type
#[allow(dead_code)]
fn known_models_for_type(api_type: &str) -> Vec<ModelInfo> {
    let list: &[(&str, &str)] = match api_type {
        "openai" => &[
            ("gpt-4o", "openai"),
            ("gpt-4o-mini", "openai"),
            ("gpt-4-turbo", "openai"),
            ("gpt-3.5-turbo", "openai"),
            ("o1", "openai"),
            ("o1-mini", "openai"),
            ("o1-preview", "openai"),
            ("o3-mini", "openai"),
            ("o4-mini", "openai"),
        ],
        "claude" => &[
            ("claude-sonnet-4-20250514", "anthropic"),
            ("claude-3-5-sonnet-20241022", "anthropic"),
            ("claude-3-5-haiku-20241022", "anthropic"),
            ("claude-3-opus-20240229", "anthropic"),
        ],
        "gemini" => &[
            ("gemini-2.5-pro-preview-05-06", "google"),
            ("gemini-2.0-flash", "google"),
            ("gemini-1.5-pro", "google"),
            ("gemini-1.5-flash", "google"),
        ],
        "azure" => &[
            ("gpt-4o", "azure"),
            ("gpt-4o-mini", "azure"),
            ("gpt-4-turbo", "azure"),
        ],
        _ => &[
            ("gpt-4o", "openai"),
            ("gpt-4o-mini", "openai"),
            ("gpt-3.5-turbo", "openai"),
            ("claude-3-5-sonnet-20241022", "anthropic"),
            ("claude-3-5-haiku-20241022", "anthropic"),
            ("gemini-2.0-flash", "google"),
            ("deepseek-chat", "deepseek"),
            ("deepseek-reasoner", "deepseek"),
            ("qwen-turbo", "alibaba"),
            ("glm-4-flash", "zhipu"),
        ],
    };
    list.iter()
        .map(|&(name, owner)| ModelInfo {
            name: name.into(),
            id: name.into(),
            owned_by: Some(owner.into()),
        })
        .collect()
}

#[allow(dead_code)]
fn dedup_models(models: Vec<ModelInfo>) -> Vec<ModelInfo> {
    let mut seen = std::collections::HashSet::new();
    models
        .into_iter()
        .filter(|m| !m.id.eq_ignore_ascii_case("auto") && !m.name.eq_ignore_ascii_case("auto"))
        .filter(|m| seen.insert(m.name.clone()))
        .collect()
}
