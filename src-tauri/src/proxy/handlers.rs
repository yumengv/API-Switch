use super::auth;
use super::forwarder;
use super::protocol::{
    azure_to_openai_request, claude_to_openai_request, gemini_to_openai_request,
    openai_to_claude_response, openai_to_gemini_response, transform_openai_sse_to_claude_stream,
    transform_openai_sse_to_gemini_stream,
};
use super::router;
use super::server::ProxyState;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::sync::Arc;

/// Gemini 单模型格式（用于 /v1beta/models/{model}）
fn gemini_single_model_item(entry: &crate::database::ApiEntry) -> Value {
    let token_limit = parse_context_limit(entry);
    json!({
        "name": format!("models/{}", entry.model),
        "version": gemini_version(entry),
        "displayName": gemini_display_name(entry),
        "description": gemini_description(entry),
        "inputTokenLimit": token_limit,
        "outputTokenLimit": token_limit,
        "supportedGenerationMethods": ["generateContent", "streamGenerateContent"],
        "group_name": entry.group_name,
    })
}

fn normalize_requested_model(model: Option<&str>) -> String {
    let trimmed = model.unwrap_or("auto").trim();
    if trimmed.is_empty() {
        "auto".to_string()
    } else {
        trimmed.to_string()
    }
}

async fn load_sorted_entries(
    state: &ProxyState,
) -> Result<Vec<crate::database::ApiEntry>, ProxyError> {
    let mut entries = state.db.get_entries_for_routing()?;
    entries.retain(|entry| !entry.model.trim().is_empty());
    let sort_mode = state.settings.read().await.default_sort_mode.clone();
    router::apply_sort_mode(&mut entries, &sort_mode);
    Ok(entries)
}

fn dedup_models_by_name(
    mut entries: Vec<crate::database::ApiEntry>,
) -> Vec<crate::database::ApiEntry> {
    // 按分组+模型名校验去重，确保相同模型在不同分组下分别保留
    let mut seen = HashSet::new();
    entries.retain(|entry| {
        let group = entry
            .group_name
            .as_deref()
            .unwrap_or("")
            .to_ascii_lowercase();
        let key = format!("{}::{}", group, entry.model.to_ascii_lowercase());
        seen.insert(key)
    });
    entries
}

fn group_created_at(entries: &[crate::database::ApiEntry], group: &str) -> i64 {
    entries
        .iter()
        .find(|entry| entry.group_name.as_deref() == Some(group))
        .map(entry_created_at)
        .unwrap_or_else(|| chrono::Utc::now().timestamp())
}

fn openai_model_item(entry: &crate::database::ApiEntry) -> Value {
    let alias = if entry.display_name.trim().is_empty() {
        entry.model.clone()
    } else {
        entry.display_name.clone()
    };
    json!({
        "id": entry.model,
        "object": "model",
        "created": entry_created_at(entry),
        "owned_by": entry_owned_by(entry, "openai"),
        "display_name": alias,
        "group_name": entry.group_name,
    })
}

fn claude_model_item(entry: &crate::database::ApiEntry) -> Value {
    json!({
        "type": "model",
        "id": entry.model,
        "display_name": claude_display_name(entry),
        "created_at": entry_created_at_rfc3339(entry),
        "group_name": entry.group_name,
        "object": "model",
    })
}

fn gemini_model_item(entry: &crate::database::ApiEntry) -> Value {
    let token_limit = parse_context_limit(entry);
    json!({
        "name": format!("models/{}", entry.model),
        "version": gemini_version(entry),
        "displayName": gemini_display_name(entry),
        "description": gemini_description(entry),
        "inputTokenLimit": token_limit,
        "outputTokenLimit": token_limit,
        "supportedGenerationMethods": ["generateContent", "streamGenerateContent"],
        "group_name": entry.group_name,
    })
}

fn azure_deployment_item(entry: &crate::database::ApiEntry) -> Value {
    let alias = if entry.display_name.trim().is_empty() {
        entry.model.clone()
    } else {
        entry.display_name.clone()
    };
    json!({
        "id": entry.model,
        "object": "deployment",
        "created": entry_created_at(entry),
        "owned_by": entry_owned_by(entry, "openai"),
        "model": alias,
        "display_name": alias,
        "group_name": entry.group_name,
    })
}

fn entry_created_at(entry: &crate::database::ApiEntry) -> i64 {
    if entry.created_at > 0 {
        entry.created_at
    } else {
        chrono::Utc::now().timestamp()
    }
}

fn entry_created_at_rfc3339(entry: &crate::database::ApiEntry) -> String {
    let ts = entry_created_at(entry);
    chrono::DateTime::<chrono::Utc>::from_timestamp(ts, 0)
        .unwrap_or_else(|| chrono::Utc::now())
        .to_rfc3339()
}

fn entry_owned_by(entry: &crate::database::ApiEntry, default_owned_by: &str) -> String {
    entry
        .owned_by
        .clone()
        .unwrap_or_else(|| default_owned_by.to_string())
}

fn claude_display_name(entry: &crate::database::ApiEntry) -> String {
    if !entry.display_name.trim().is_empty() {
        entry.display_name.clone()
    } else {
        entry.model.clone()
    }
}

fn gemini_display_name(entry: &crate::database::ApiEntry) -> String {
    if !entry.display_name.trim().is_empty() {
        entry.display_name.clone()
    } else {
        entry.model.clone()
    }
}

fn gemini_description(entry: &crate::database::ApiEntry) -> String {
    entry
        .model_meta_en
        .clone()
        .or_else(|| entry.model_meta_zh.clone())
        .unwrap_or_default()
}

fn gemini_version(entry: &crate::database::ApiEntry) -> String {
    entry
        .release_date
        .clone()
        .unwrap_or_else(|| "stable".to_string())
}

fn parse_context_limit(entry: &crate::database::ApiEntry) -> i64 {
    let mut haystacks = Vec::new();
    if let Some(text) = &entry.model_meta_en {
        haystacks.push(text.as_str());
    }
    if let Some(text) = &entry.model_meta_zh {
        haystacks.push(text.as_str());
    }

    for text in haystacks {
        for token in text.split(|c: char| !(c.is_ascii_alphanumeric() || c == 'k' || c == 'K')) {
            let lower = token.to_ascii_lowercase();
            if let Some(stripped) = lower.strip_suffix('k') {
                if let Ok(value) = stripped.parse::<i64>() {
                    return value * 1024;
                }
            }
            if let Ok(value) = lower.parse::<i64>() {
                if value >= 1024 {
                    return value;
                }
            }
        }
    }

    8192
}

/// Health check endpoint
pub async fn health_check() -> (StatusCode, Json<Value>) {
    (
        StatusCode::OK,
        Json(json!({
            "status": "healthy",
            "timestamp": chrono::Utc::now().to_rfc3339(),
        })),
    )
}

/// Handle /v1/chat/completions
pub async fn handle_chat_completions(
    State(state): State<ProxyState>,
    request: axum::extract::Request,
) -> Result<axum::response::Response, ProxyError> {
    let (parts, body) = request.into_parts();
    let headers = &parts.headers;

    // Extract Access Key
    let access_key = auth::extract_access_key(headers, &state)
        .await
        .map_err(|err| match err {
            crate::error::AppError::Validation(_) => ProxyError::Unauthorized,
            other => ProxyError::from(other),
        })?;

    // Read request body
    let body_bytes = axum::body::to_bytes(body, 32 * 1024 * 1024)
        .await
        .map_err(|e| ProxyError::Internal(format!("Failed to read body: {e}")))?;

    let mut body: Value = serde_json::from_slice(&body_bytes)
        .map_err(|e| ProxyError::Internal(format!("Failed to parse JSON: {e}")))?;


    let requested_model = normalize_requested_model(body.get("model").and_then(|m| m.as_str()));

    let is_stream = body
        .get("stream")
        .and_then(|s| s.as_bool())
        .unwrap_or(false);

    // Resolve target entries
    // - AUTO: only enabled entries enter the auto pool
    // - named routes: resolution is based on group/model matching before AUTO fallback
    let all_entries = state.db.get_entries_for_routing()?;
    let auto_entries = state.db.get_enabled_entries_for_auto()?;
    let sort_mode = state.settings.read().await.default_sort_mode.clone();
    let resolved = router::resolve(
        &requested_model,
        &all_entries,
        &auto_entries,
        &state.circuit_breakers,
        &sort_mode,
    )
    .await;

    if resolved.is_empty() {
        return Err(ProxyError::NoAvailableProvider(requested_model));
    }

    // Forward with retry
    let middleware: Vec<Arc<dyn super::middleware::ForwarderMiddleware>> =
        vec![Arc::new(super::middleware::StreamOptionsMiddleware)];
    let caller_kind = super::middleware::CallerKind::OpenAiChat;

    forwarder::forward_with_retry(
        &state,
        &resolved,
        &body,
        headers,
        &requested_model,
        access_key.as_ref(),
        is_stream,
        &middleware,
        caller_kind,
    )
    .await
}

/// Handle /v1/messages (Claude format)
///
/// Accepts requests in Claude protocol format, converts to OpenAI format
/// for internal routing, forwards to the resolved provider, and converts
/// the response back to Claude format.
pub async fn handle_messages(
    State(state): State<ProxyState>,
    request: axum::extract::Request,
) -> Result<axum::response::Response, ProxyError> {
    let (parts, body) = request.into_parts();
    let headers = &parts.headers;

    // Extract Access Key (same as OpenAI)
    let access_key = auth::extract_access_key(headers, &state)
        .await
        .map_err(|err| match err {
            crate::error::AppError::Validation(_) => ProxyError::Unauthorized,
            other => ProxyError::from(other),
        })?;

    // Read request body
    let body_bytes = axum::body::to_bytes(body, 32 * 1024 * 1024)
        .await
        .map_err(|e| ProxyError::Internal(format!("Failed to read body: {e}")))?;

    let body: Value = serde_json::from_slice(&body_bytes)
        .map_err(|e| ProxyError::Internal(format!("Failed to parse JSON: {e}")))?;

    // Convert Claude format to OpenAI format for internal routing
    let mut openai_body = claude_to_openai_request(&body);


    let requested_model =
        normalize_requested_model(openai_body.get("model").and_then(|m| m.as_str()));

    let is_stream = openai_body
        .get("stream")
        .and_then(|s| s.as_bool())
        .unwrap_or(false);

    // Resolve target entries (same logic as chat completions)
    let all_entries = state.db.get_entries_for_routing()?;
    let auto_entries = state.db.get_enabled_entries_for_auto()?;
    let sort_mode = state.settings.read().await.default_sort_mode.clone();
    let resolved = router::resolve(
        &requested_model,
        &all_entries,
        &auto_entries,
        &state.circuit_breakers,
        &sort_mode,
    )
    .await;

    if resolved.is_empty() {
        return Err(ProxyError::NoAvailableProvider(requested_model));
    }

    // Forward with retry - handle_messages (Claude)
    let middleware: Vec<Arc<dyn super::middleware::ForwarderMiddleware>> =
        vec![Arc::new(super::middleware::StreamOptionsMiddleware)];
    let caller_kind = super::middleware::CallerKind::ClaudeMessages;

    let response = forwarder::forward_with_retry(
        &state,
        &resolved,
        &openai_body,
        headers,
        &requested_model,
        access_key.as_ref(),
        is_stream,
        &middleware,
        caller_kind,
    )
    .await?;

    // Convert the response from OpenAI format back to Claude format
    if is_stream {
        transform_openai_sse_to_claude_stream(response, requested_model.clone())
            .map_err(ProxyError::from)
    } else {
        // Non-streaming: read the body, convert JSON from OpenAI to Claude format
        let body_bytes = axum::body::to_bytes(response.into_body(), 32 * 1024 * 1024)
            .await
            .map_err(|e| ProxyError::Internal(format!("Failed to read response: {e}")))?;

        let body: Value = serde_json::from_slice(&body_bytes)
            .map_err(|e| ProxyError::Internal(format!("Failed to parse response: {e}")))?;

        let claude_response = openai_to_claude_response(&body);

        Ok(Json(claude_response).into_response())
    }
}

/// Handle /v1/models - list ALL models from the pool (including disabled).
/// disabled only means "skip in AUTO", the model is still usable when requested by name.
pub async fn handle_list_models(
    State(state): State<ProxyState>,
) -> Result<Json<Value>, ProxyError> {
    let entries = dedup_models_by_name(load_sorted_entries(&state).await?);

    let mut group_set: HashSet<String> = HashSet::new();
    for e in &entries {
        if let Some(name) = &e.group_name {
            if !name.is_empty() {
                group_set.insert(name.clone());
            }
        }
    }
    let mut group_names: Vec<String> = group_set.into_iter().collect();
    group_names.sort();

    let group_models: Vec<Value> = group_names
        .iter()
        .map(|g| {
            json!({
                "id": g,
                "object": "model",
                "created": group_created_at(&entries, g),
                "owned_by": "group",
            })
        })
        .collect();

    let models: Vec<Value> = entries.iter().map(openai_model_item).collect();

    let mut data = group_models;
    data.extend(models);

    Ok(Json(json!({
        "object": "list",
        "data": data,
    })))
}

/// Handle /anthropic/v1/models - Anthropic native model list format.
pub async fn handle_list_models_claude(
    State(state): State<ProxyState>,
) -> Result<Json<Value>, ProxyError> {
    let entries = dedup_models_by_name(load_sorted_entries(&state).await?);
    let data: Vec<Value> = entries.iter().map(claude_model_item).collect();

    let first_id = data
        .first()
        .and_then(|item| item.get("id"))
        .and_then(|value| value.as_str())
        .map(str::to_string);
    let last_id = data
        .last()
        .and_then(|item| item.get("id"))
        .and_then(|value| value.as_str())
        .map(str::to_string);

    Ok(Json(json!({
        "data": data,
        "first_id": first_id,
        "last_id": last_id,
        "has_more": false,
    })))
}

/// Handle /v1beta/models - Gemini native model list format.
pub async fn handle_list_models_gemini(
    State(state): State<ProxyState>,
) -> Result<Json<Value>, ProxyError> {
    let entries = dedup_models_by_name(load_sorted_entries(&state).await?);
    let models: Vec<Value> = entries.iter().map(gemini_model_item).collect();

    Ok(Json(json!({
        "models": models,
    })))
}

/// Handle /openai/deployments - Azure deployment list format.
pub async fn handle_list_models_azure(
    State(state): State<ProxyState>,
    Query(_query): Query<super::server::AzureDeploymentsQuery>,
) -> Result<Json<Value>, ProxyError> {
    let entries = dedup_models_by_name(load_sorted_entries(&state).await?);
    let data: Vec<Value> = entries.iter().map(azure_deployment_item).collect();

    Ok(Json(json!({
        "object": "list",
        "data": data,
    })))
}

/// Handle /v1beta/models/{model}:{action} (Gemini native)
///
/// 支持 action:
/// - `generateContent`        — 非流式内容生成（已有）
/// - `streamGenerateContent`  — 流式内容生成（新增，OpenAI SSE → Gemini SSE 转换）
pub async fn handle_gemini_native(
    State(state): State<ProxyState>,
    Path(rest): Path<String>,
    request: axum::extract::Request,
) -> Result<axum::response::Response, ProxyError> {
    let Some((model, action)) = rest.rsplit_once(':') else {
        return Err(ProxyError::Internal("Invalid Gemini path".to_string()));
    };

    match action {
        "generateContent" => handle_gemini_generate_content(State(state), model, request).await,
        "streamGenerateContent" => {
            handle_gemini_stream_generate_content(State(state), model, request).await
        }
        _ => Err(ProxyError::Internal(format!(
            "Unsupported Gemini action: {action}"
        ))),
    }
}

/// 非流式 Gemini 内容生成（:generateContent）
async fn handle_gemini_generate_content(
    State(state): State<ProxyState>,
    model: &str,
    request: axum::extract::Request,
) -> Result<axum::response::Response, ProxyError> {
    let (parts, body) = request.into_parts();
    let headers = &parts.headers;

    let body_bytes = axum::body::to_bytes(body, 32 * 1024 * 1024)
        .await
        .map_err(|e| ProxyError::Internal(format!("Failed to read body: {e}")))?;

    let body: Value = serde_json::from_slice(&body_bytes)
        .map_err(|e| ProxyError::Internal(format!("Failed to parse JSON: {e}")))?;

    let openai_body = gemini_to_openai_request(&body);
    let mut openai_body = openai_body;
    openai_body["model"] = json!(model);

    let requested_model = normalize_requested_model(Some(model));

    let all_entries = state.db.get_entries_for_routing()?;
    let auto_entries = state.db.get_enabled_entries_for_auto()?;
    let sort_mode = state.settings.read().await.default_sort_mode.clone();
    let resolved = router::resolve(
        &requested_model,
        &all_entries,
        &auto_entries,
        &state.circuit_breakers,
        &sort_mode,
    )
    .await;

    if resolved.is_empty() {
        return Err(ProxyError::NoAvailableProvider(requested_model));
    }

    let middleware: Vec<Arc<dyn super::middleware::ForwarderMiddleware>> =
        vec![Arc::new(super::middleware::StreamOptionsMiddleware)];
    let caller_kind = super::middleware::CallerKind::GeminiNative;

    let response = forwarder::forward_with_retry(
        &state,
        &resolved,
        &openai_body,
        headers,
        &requested_model,
        None,
        false,
        &middleware,
        caller_kind,
    )
    .await?;

    let body_bytes = axum::body::to_bytes(response.into_body(), 32 * 1024 * 1024)
        .await
        .map_err(|e| ProxyError::Internal(format!("Failed to read response: {e}")))?;

    let body: Value = serde_json::from_slice(&body_bytes)
        .map_err(|e| ProxyError::Internal(format!("Failed to parse response: {e}")))?;

    let gemini_response = openai_to_gemini_response(&body);
    Ok(Json(gemini_response).into_response())
}

/// 流式 Gemini 内容生成（:streamGenerateContent）
///
/// 流程:
/// 1. Gemini 原生请求 → OpenAI 格式
/// 2. 强制 stream=true，通过 forwarder 转发
/// 3. forwarder 返回 OpenAI SSE 流 (chat.completion.chunk)
/// 4. 逐行转换为 Gemini 原生 SSE 格式 (candidates)
/// 5. 返回流式响应给客户端
async fn handle_gemini_stream_generate_content(
    State(state): State<ProxyState>,
    model: &str,
    request: axum::extract::Request,
) -> Result<axum::response::Response, ProxyError> {
    let (parts, body) = request.into_parts();
    let headers = &parts.headers;

    let body_bytes = axum::body::to_bytes(body, 32 * 1024 * 1024)
        .await
        .map_err(|e| ProxyError::Internal(format!("Failed to read body: {e}")))?;

    let body: Value = serde_json::from_slice(&body_bytes)
        .map_err(|e| ProxyError::Internal(format!("Failed to parse JSON: {e}")))?;

    let openai_body = gemini_to_openai_request(&body);
    let mut openai_body = openai_body;
    openai_body["model"] = json!(model);
    openai_body["stream"] = json!(true);

    let requested_model = normalize_requested_model(Some(model));

    let all_entries = state.db.get_entries_for_routing()?;
    let auto_entries = state.db.get_enabled_entries_for_auto()?;
    let sort_mode = state.settings.read().await.default_sort_mode.clone();
    let resolved = router::resolve(
        &requested_model,
        &all_entries,
        &auto_entries,
        &state.circuit_breakers,
        &sort_mode,
    )
    .await;

    if resolved.is_empty() {
        return Err(ProxyError::NoAvailableProvider(requested_model));
    }

    let middleware: Vec<Arc<dyn super::middleware::ForwarderMiddleware>> =
        vec![Arc::new(super::middleware::StreamOptionsMiddleware)];
    let caller_kind = super::middleware::CallerKind::GeminiNative;

    let response = forwarder::forward_with_retry(
        &state,
        &resolved,
        &openai_body,
        headers,
        &requested_model,
        None,
        true, // is_stream = true
        &middleware,
        caller_kind,
    )
    .await?;

    // 将 forwarder 返回的 OpenAI SSE 流转换为 Gemini 原生 SSE 流
    transform_openai_sse_to_gemini_stream(response).map_err(ProxyError::from)
}

/// Handle GET /v1beta/models/{model} — Gemini 单模型详情
pub async fn handle_gemini_model_detail(
    State(state): State<ProxyState>,
    Path(model): Path<String>,
) -> Result<Json<Value>, ProxyError> {
    // 从 DB 查找该模型
    let entries = state.db.get_entries_for_routing()?;
    let model_lower = model.to_ascii_lowercase();

    if let Some(entry) = entries
        .iter()
        .find(|e| e.model.to_ascii_lowercase() == model_lower)
    {
        return Ok(Json(gemini_single_model_item(entry)));
    }

    // 没找到 → 返回 Gemini 格式的 404
    Err(ProxyError::Internal(format!("Model '{}' not found", model)))
}

/// Handle /openai/deployments/{deployment}/chat/completions (Azure native)
pub async fn handle_azure_chat(
    State(state): State<ProxyState>,
    Path(rest): Path<String>,
    request: axum::extract::Request,
) -> Result<axum::response::Response, ProxyError> {
    let deployment = rest
        .strip_suffix("/chat/completions")
        .ok_or_else(|| ProxyError::Internal("Invalid Azure path".to_string()))?
        .to_string();

    let (parts, body) = request.into_parts();
    let headers = &parts.headers;

    let access_key = auth::extract_access_key(headers, &state)
        .await
        .map_err(|err| match err {
            crate::error::AppError::Validation(_) => ProxyError::Unauthorized,
            other => ProxyError::from(other),
        })?;

    let body_bytes = axum::body::to_bytes(body, 32 * 1024 * 1024)
        .await
        .map_err(|e| ProxyError::Internal(format!("Failed to read body: {e}")))?;

    let body: Value = serde_json::from_slice(&body_bytes)
        .map_err(|e| ProxyError::Internal(format!("Failed to parse JSON: {e}")))?;

    let openai_body = azure_to_openai_request(&body, &deployment);

    let requested_model =
        normalize_requested_model(openai_body.get("model").and_then(|m| m.as_str()));

    let is_stream = openai_body
        .get("stream")
        .and_then(|s| s.as_bool())
        .unwrap_or(false);

    let all_entries = state.db.get_entries_for_routing()?;
    let auto_entries = state.db.get_enabled_entries_for_auto()?;
    let sort_mode = state.settings.read().await.default_sort_mode.clone();

    let mut resolved: Vec<crate::database::ApiEntry> = {
        let deployment_lower = deployment.to_ascii_lowercase();
        all_entries
            .iter()
            .filter(|e| e.enabled)
            .filter(|e| e.model.to_ascii_lowercase() == deployment_lower)
            .cloned()
            .collect()
    };

    if resolved.is_empty() {
        resolved = router::resolve(
            &requested_model,
            &all_entries,
            &auto_entries,
            &state.circuit_breakers,
            &sort_mode,
        )
        .await;
    }

    if resolved.is_empty() {
        return Err(ProxyError::NoAvailableProvider(requested_model));
    }

    let middleware: Vec<Arc<dyn super::middleware::ForwarderMiddleware>> =
        vec![Arc::new(super::middleware::StreamOptionsMiddleware)];
    let caller_kind = super::middleware::CallerKind::AzureChat;

    let response = forwarder::forward_with_retry(
        &state,
        &resolved,
        &openai_body,
        headers,
        &requested_model,
        access_key.as_ref(),
        is_stream,
        &middleware,
        caller_kind,
    )
    .await?;

    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::ApiEntry;

    fn sample_entry(
        id: &str,
        model: &str,
        display_name: &str,
        group_name: Option<&str>,
    ) -> ApiEntry {
        ApiEntry {
            id: id.to_string(),
            channel_id: format!("channel-{id}"),
            model: model.to_string(),
            display_name: display_name.to_string(),
            sort_index: 0,
            enabled: true,
            cooldown_until: None,
            circuit_state: "closed".to_string(),
            created_at: 1_700_000_000,
            updated_at: 1_700_000_000,
            channel_name: Some("channel-a".to_string()),
            channel_api_type: Some("openai".to_string()),
            owned_by: Some("openai".to_string()),
            response_ms: None,
            provider_logo: None,
            release_date: Some("2024-01-02".to_string()),
            model_meta_zh: Some("上下文 32k".to_string()),
            model_meta_en: Some("Context 32k".to_string()),
            group_name: group_name.map(str::to_string),
            score: 0.0,
        }
    }

    #[test]
    fn openai_model_item_includes_created_and_owned_by() {
        let entry = sample_entry("1", "gpt-4o", "GPT-4o", None);
        let value = openai_model_item(&entry);
        assert_eq!(value["id"], "gpt-4o");
        assert_eq!(value["object"], "model");
        assert_eq!(value["created"], 1_700_000_000);
        assert_eq!(value["owned_by"], "openai");
    }

    #[test]
    fn claude_model_item_uses_rfc3339_created_at() {
        let entry = sample_entry("1", "claude-3-5-sonnet", "Claude Sonnet", None);
        let value = claude_model_item(&entry);
        assert_eq!(value["type"], "model");
        assert_eq!(value["id"], "claude-3-5-sonnet");
        assert_eq!(value["display_name"], "Claude Sonnet");
        assert_eq!(value["created_at"], "2023-11-14T22:13:20+00:00");
    }

    #[test]
    fn gemini_model_item_uses_native_shape() {
        let entry = sample_entry("1", "gemini-2.0-flash", "Gemini Flash", None);
        let value = gemini_model_item(&entry);
        assert_eq!(value["name"], "models/gemini-2.0-flash");
        assert_eq!(value["displayName"], "Gemini Flash");
        assert_eq!(
            value["supportedGenerationMethods"],
            json!(["generateContent", "streamGenerateContent"])
        );
    }

    #[test]
    fn azure_deployment_item_uses_deployment_object_and_model_fallback() {
        let entry = sample_entry("1", "deployment-1", "", None);
        let value = azure_deployment_item(&entry);
        assert_eq!(value["object"], "deployment");
        assert_eq!(value["model"], "deployment-1");
    }

    #[test]
    fn dedup_models_by_name_keeps_first_case_insensitive_match() {
        let first = sample_entry("1", "gpt-4o", "First", None);
        let second = sample_entry("2", "GPT-4O", "Second", None);
        let items = dedup_models_by_name(vec![first.clone(), second]);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id, first.id);
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ProxyError {
    #[error("No available provider for model: {0}")]
    NoAvailableProvider(String),

    #[error("Unauthorized")]
    Unauthorized,

    #[error("Internal error: {0}")]
    Internal(String),

    #[error("All providers failed")]
    AllProvidersFailed,

    #[error("Upstream error {status}: {message}")]
    Upstream { status: u16, message: String },

    #[allow(dead_code)]
    #[error("Bad request: {0}")]
    BadRequest(String),
}

impl IntoResponse for ProxyError {
    fn into_response(self) -> axum::response::Response {
        let (status, message) = match &self {
            ProxyError::NoAvailableProvider(model) => (
                StatusCode::NOT_FOUND,
                format!("No available provider for model: {model}"),
            ),
            ProxyError::Unauthorized => (StatusCode::UNAUTHORIZED, "Unauthorized".to_string()),
            ProxyError::AllProvidersFailed => {
                (StatusCode::BAD_GATEWAY, "All providers failed".to_string())
            }
            ProxyError::Internal(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg.clone()),
            ProxyError::Upstream { status, message } => {
                let code = StatusCode::from_u16(*status).unwrap_or(StatusCode::BAD_GATEWAY);
                (code, message.clone())
            }
            ProxyError::BadRequest(msg) => (StatusCode::BAD_REQUEST, msg.clone()),
        };

        let body = json!({
            "error": {
                "message": message,
                "type": "proxy_error",
                "code": status.as_u16(),
            }
        });

        (status, Json(body)).into_response()
    }
}

impl From<crate::error::AppError> for ProxyError {
    fn from(e: crate::error::AppError) -> Self {
        ProxyError::Internal(e.to_string())
    }
}
