use super::auth;
use super::forwarder;
use super::protocol::{claude_to_openai_request, openai_to_claude_response, ClaudeSSETransformer};
use super::router;
use super::server::ProxyState;
use axum::body::Body;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use bytes::Bytes;
use futures::StreamExt;
use serde_json::{json, Value};

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
    let access_key = auth::extract_access_key(headers, &state).await.map_err(|err| match err {
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
    let access_key = auth::extract_access_key(headers, &state).await.map_err(|err| match err {
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

    let requested_model = normalize_requested_model(openai_body.get("model").and_then(|m| m.as_str()));

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

        let transformed_stream = futures::stream::unfold(
            (upstream_stream, transformer, sse_buffer),
            |(mut stream, mut transformer, mut sse_buffer)| async move {
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
                                    (stream, transformer, sse_buffer),
                                ));
                            }

                            let events = transformer.transform_chunk(payload);
                            if !events.is_empty() {
                                let mut output = Vec::new();
                                for event in &events {
                                    output.extend_from_slice(
                                        format!("data: {event}\n\n").as_bytes(),
                                    );
                                }
                                return Some((
                                    Ok(Bytes::from(output)),
                                    (stream, transformer, sse_buffer),
                                ));
                            }
                        }
                        continue; // skip non-data lines or empty-transform chunks
                    }

                    // Need more data from upstream
                    match stream.next().await {
                        Some(Ok(chunk)) => {
                            sse_buffer.push_str(&String::from_utf8_lossy(&chunk));
                            // Continue loop to process buffered data
                        }
                        Some(Err(e)) => {
                            return Some((
                                Err(std::io::Error::new(
                                    std::io::ErrorKind::Other,
                                    format!("Stream read error: {e}"),
                                )),
                                (stream, transformer, sse_buffer),
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

    Ok(Json(json!({
        "object": "list",
        "data": models,
    })))
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
}

impl IntoResponse for ProxyError {
    fn into_response(self) -> axum::response::Response {
        let (status, message) = match &self {
            ProxyError::NoAvailableProvider(model) => (
                StatusCode::NOT_FOUND,
                format!("No available provider for model: {model}"),
            ),
            ProxyError::Unauthorized => (StatusCode::UNAUTHORIZED, "Unauthorized".to_string()),
            ProxyError::AllProvidersFailed => (
                StatusCode::BAD_GATEWAY,
                "All providers failed".to_string(),
            ),
            ProxyError::Internal(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg.clone()),
            ProxyError::Upstream { status, message } => {
                let code = StatusCode::from_u16(*status).unwrap_or(StatusCode::BAD_GATEWAY);
                (code, message.clone())
            }
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
