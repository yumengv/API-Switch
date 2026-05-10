use super::auth;
use super::forwarder;
use super::protocol::{
    azure_to_openai_request, claude_to_openai_request, gemini_to_openai_request,
    openai_to_azure_response, openai_to_claude_response, openai_to_gemini_response,
    transform_azure_error, transform_claude_error, transform_gemini_error, AzureSSETransformer,
    ClaudeSSETransformer, GeminiSSETransformer,
};
use super::router;
use super::server::ProxyState;
use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use bytes::Bytes;
use futures::StreamExt;
use serde_json::{json, Value};
use std::collections::HashSet;

fn normalize_requested_model(model: Option<&str>) -> String {
    let trimmed = model.unwrap_or("auto").trim();
    if trimmed.is_empty() {
        "auto".to_string()
    } else {
        trimmed.to_string()
    }
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

    let body: Value = serde_json::from_slice(&body_bytes)
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
    forwarder::forward_with_retry(
        &state,
        &resolved,
        &body,
        headers,
        &requested_model,
        access_key.as_ref(),
        is_stream,
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
    let openai_body = claude_to_openai_request(&body);

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

    // Forward with retry
    let response = forwarder::forward_with_retry(
        &state,
        &resolved,
        &openai_body,
        headers,
        &requested_model,
        access_key.as_ref(),
        is_stream,
    )
    .await?;

    // Convert the response from OpenAI format back to Claude format
    if is_stream {
        // Streaming: transform each SSE chunk from OpenAI to Claude format
        let upstream_stream = response.into_body().into_data_stream();

        let message_id = format!("msg_{}", chrono::Utc::now().timestamp());
        let transformer = ClaudeSSETransformer::new(message_id, requested_model.clone());
        let sse_buffer = String::new();
        let sse_utf8_remainder: Vec<u8> = Vec::new();

        let transformed_stream = futures::stream::unfold(
            (upstream_stream, transformer, sse_buffer, sse_utf8_remainder),
            |(mut stream, mut transformer, mut sse_buffer, mut sse_utf8_remainder)| async move {
                loop {
                    // Process any buffered SSE lines first
                    if let Some(line_end) = sse_buffer.find('\n') {
                        let mut line = sse_buffer.drain(..=line_end).collect::<String>();
                        if line.ends_with('\n') {
                            line.pop();
                        }
                        if line.ends_with('\r') {
                            line.pop();
                        }

                        if let Some(payload) = line.strip_prefix("data: ") {
                            if payload == "[DONE]" {
                                let output = Bytes::from("data: [DONE]\n\n");
                                return Some((
                                    Ok::<_, std::io::Error>(output),
                                    (stream, transformer, sse_buffer, sse_utf8_remainder),
                                ));
                            }

                            let events = transformer.transform_chunk(payload);
                            if !events.is_empty() {
                                let mut output = Vec::new();
                                for event in &events {
                                    output
                                        .extend_from_slice(format!("data: {event}\n\n").as_bytes());
                                }
                                return Some((
                                    Ok(Bytes::from(output)),
                                    (stream, transformer, sse_buffer, sse_utf8_remainder),
                                ));
                            }
                        }
                        continue; // skip non-data lines or empty-transform chunks
                    }

                    // Need more data from upstream
                    match stream.next().await {
                        Some(Ok(chunk)) => {
                            super::sse::append_utf8_safe(&mut sse_buffer, &mut sse_utf8_remainder, &chunk);
                            // Continue loop to process buffered data
                        }
                        Some(Err(e)) => {
                            return Some((
                                Err(std::io::Error::new(
                                    std::io::ErrorKind::Other,
                                    format!("Stream read error: {e}"),
                                )),
                                (stream, transformer, sse_buffer, sse_utf8_remainder),
                            ));
                        }
                        None => {
                            return None; // stream ended
                        }
                    }
                }
            },
        );

        Ok(axum::http::Response::builder()
            .status(StatusCode::OK)
            .header("content-type", "text/event-stream")
            .header("cache-control", "no-cache")
            .header("connection", "keep-alive")
            .header("x-accel-buffering", "no")
            .body(Body::from_stream(transformed_stream))
            .unwrap())
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
    let mut entries = state.db.get_entries_for_routing()?;
    let sort_mode = state.settings.read().await.default_sort_mode.clone();
    router::apply_sort_mode(&mut entries, &sort_mode);

    // Collect unique non-empty group names as models
    let mut group_set: HashSet<String> = HashSet::new();
    for e in &entries {
        if let Some(name) = &e.group_name {
            if !name.is_empty() {
                group_set.insert(name.clone());
            }
        }
    }
    // Convert groups to model objects, owned_by "group"
    let mut group_models: Vec<Value> = group_set
        .iter()
        .map(|g| {
            json!({
                "id": g,
                "object": "model",
                "owned_by": "group",
            })
        })
        .collect();
    // Sort groups for deterministic order (optional)
    group_models.sort_by(|a, b| {
        a["id"]
            .as_str()
            .unwrap_or("")
            .cmp(b["id"].as_str().unwrap_or(""))
    });

    let models: Vec<Value> = entries
        .iter()
        .map(|e| {
            json!({
                "id": e.model,
                "object": "model",
                "owned_by": e.channel_name,
            })
        })
        .collect();

    // Combine group models first, then regular models
    let mut data = group_models;
    data.extend(models);

    Ok(Json(json!({
        "object": "list",
        "data": data,
    })))
}

/// Handle /v1beta/models/{model}:generateContent (Gemini native)
pub async fn handle_gemini_native(
    State(state): State<ProxyState>,
    Path(rest): Path<String>,
    request: axum::extract::Request,
) -> Result<axum::response::Response, ProxyError> {
    let Some((model, action)) = rest.rsplit_once(':') else {
        return Err(ProxyError::Internal("Invalid Gemini path".to_string()));
    };

    if action != "generateContent" {
        return Err(ProxyError::Internal(format!(
            "Unsupported Gemini action: {action}"
        )));
    }

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

    let response = forwarder::forward_with_retry(
        &state,
        &resolved,
        &openai_body,
        headers,
        &requested_model,
        None,
        false,
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

    let body: Value = serde_json::from_slice(&body_bytes)
        .map_err(|e| ProxyError::Internal(format!("Failed to parse JSON: {e}")))?;

    // Convert Azure format to OpenAI format (mostly passthrough)
    let openai_body = azure_to_openai_request(&body, &deployment);

    let requested_model =
        normalize_requested_model(openai_body.get("model").and_then(|m| m.as_str()));

    let is_stream = openai_body
        .get("stream")
        .and_then(|s| s.as_bool())
        .unwrap_or(false);

    // Resolve target entries
    let all_entries = state.db.get_entries_for_routing()?;
    let auto_entries = state.db.get_enabled_entries_for_auto()?;
    let sort_mode = state.settings.read().await.default_sort_mode.clone();

    // Azure native semantics: try deployment exact-match first, then fallback to normal resolve.
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

    // Forward with retry
    let response = forwarder::forward_with_retry(
        &state,
        &resolved,
        &openai_body,
        headers,
        &requested_model,
        access_key.as_ref(),
        is_stream,
    )
    .await?;

    // Azure format is same as OpenAI, so just pass through
    Ok(response)
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
