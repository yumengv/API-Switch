//! POST /v1/responses — OpenAI Responses API compatibility layer.
//!
//! Converts a subset of Responses API requests (text and function tools)
//! to Chat Completions format. Non-streaming responses are returned as
//! Responses JSON objects; streaming responses are converted from upstream
//! Chat Completions SSE to Responses-style SSE events in real time.

use super::auth;
use super::forwarder;
use super::handlers::ProxyError;
use super::protocol::responses::{
    convert_tools, input_to_messages, is_function_tool_call, merge_tool_delta,
    passthrough_output_item,
};
use super::router;
use super::server::ProxyState;
use axum::body::Body;
use axum::extract::State;
use axum::http::{header, StatusCode};
use axum::response::IntoResponse;
use bytes::Bytes;
use futures::StreamExt;
use serde_json::{json, Value};
use uuid::Uuid;

fn sse_line(obj: &Value) -> Bytes {
    let line = format!(
        "data: {}\n\n",
        serde_json::to_string(obj).unwrap_or_default()
    );
    Bytes::from(line)
}

fn sse_done() -> Bytes {
    Bytes::from("data: [DONE]\n\n")
}

fn append_utf8_safe(buffer: &mut String, remainder: &mut Vec<u8>, bytes: &[u8]) {
    super::sse::append_utf8_safe(buffer, remainder, bytes)
}

fn sse_data_payload(line: &str) -> Option<&str> {
    super::sse::sse_data_payload(line)
}

// ─── Handler ─────────────────────────────────────────────────────────

/// POST /v1/responses — Responses API compatibility endpoint.
///
/// Flow:
/// 1. Authenticate (reuse existing access key logic)
/// 2. Parse Responses API request
/// 3. Convert `input` → Chat messages, `tools` → Chat tools
/// 4. Forward non-streaming to upstream via existing forwarder
/// 5. Wrap result as SSE events in Responses API format
pub async fn handle_responses(
    State(state): State<ProxyState>,
    request: axum::extract::Request,
) -> Result<axum::response::Response, ProxyError> {
    let (parts, body) = request.into_parts();
    let headers = &parts.headers;

    // 1. Auth
    let access_key = auth::extract_access_key(headers, &state)
        .await
        .map_err(|err| match err {
            crate::error::AppError::Validation(_) => ProxyError::Unauthorized,
            other => ProxyError::from(other),
        })?;

    // 2. Parse request body
    let body_bytes = axum::body::to_bytes(body, 32 * 1024 * 1024)
        .await
        .map_err(|e| ProxyError::Internal(format!("Failed to read body: {e}")))?;

    let req_body: Value = match serde_json::from_slice(&body_bytes) {
        Ok(v) => v,
        Err(e) => {
            return Ok((
                StatusCode::BAD_REQUEST,
                axum::Json(json!({
                    "error": {
                        "message": format!("Invalid JSON: {e}"),
                        "type": "invalid_request_error",
                        "code": "invalid_json"
                    }
                })),
            )
                .into_response());
        }
    };

    // ── Hosted tool types: passed through as-is ──
    //
    // As a pure relay, we pass all tool types through to the upstream unchanged.
    // Function tools are converted to Chat Completions format; all other tool
    // types (web_search, image_generation, local_shell, etc.) are forwarded as-is.
    // The upstream decides how to handle them.
    //
    // Affected tool types: web_search, image_generation, tool_search,
    //                      local_shell, custom (and any future hosted tools)

    // 3. Determine stream mode BEFORE building chat body
    let is_stream = req_body
        .get("stream")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let response_id = format!("resp_{}", Uuid::new_v4().to_string().replace('-', ""));
    let model = req_body
        .get("model")
        .and_then(|m| m.as_str())
        .unwrap_or("auto");

    // 4. Convert to Chat Completions format
    let messages = input_to_messages(
        req_body.get("input").unwrap_or(&Value::Null),
        req_body.get("instructions").and_then(|v| v.as_str()),
    );

    let mut chat_body = json!({
        "model": model,
        "messages": messages,
        "stream": is_stream,
    });

    // Passthrough temperature, top_p, max_output_tokens to upstream
    if let Some(temp) = req_body.get("temperature") {
        chat_body["temperature"] = temp.clone();
    }
    if let Some(top_p) = req_body.get("top_p") {
        chat_body["top_p"] = top_p.clone();
    }
    if let Some(max_tokens) = req_body.get("max_output_tokens") {
        chat_body["max_tokens"] = max_tokens.clone();
    }

    // Passthrough reasoning config (gpt-5 / o-series)
    if let Some(reasoning) = req_body.get("reasoning") {
        chat_body["reasoning"] = reasoning.clone();
    }

    // Passthrough service_tier
    if let Some(service_tier) = req_body.get("service_tier") {
        chat_body["service_tier"] = service_tier.clone();
    }

    // Passthrough text config (structured output)
    if let Some(text) = req_body.get("text") {
        chat_body["text"] = text.clone();
    }

    // Passthrough top_logprobs
    if let Some(top_logprobs) = req_body.get("top_logprobs") {
        chat_body["top_logprobs"] = top_logprobs.clone();
    }

    // Passthrough stream_options
    if let Some(stream_options) = req_body.get("stream_options") {
        chat_body["stream_options"] = stream_options.clone();
    }

    // Passthrough max_tool_calls
    if let Some(max_tool_calls) = req_body.get("max_tool_calls") {
        chat_body["max_tool_calls"] = max_tool_calls.clone();
    }

    // Passthrough include (request extra output data)
    if let Some(include) = req_body.get("include") {
        chat_body["include"] = include.clone();
    }

    // Passthrough prompt config
    if let Some(prompt) = req_body.get("prompt") {
        chat_body["prompt"] = prompt.clone();
    }
    if let Some(prompt_cache_key) = req_body.get("prompt_cache_key") {
        chat_body["prompt_cache_key"] = prompt_cache_key.clone();
    }
    if let Some(prompt_cache_retention) = req_body.get("prompt_cache_retention") {
        chat_body["prompt_cache_retention"] = prompt_cache_retention.clone();
    }

    // Passthrough safety_identifier
    if let Some(safety_identifier) = req_body.get("safety_identifier") {
        chat_body["safety_identifier"] = safety_identifier.clone();
    }

    // Convert tools if present — pure translation, no filtering.
    // We convert function tools to Chat Completions format and pass all other
    // tool types (web_search, local_shell, etc.) through as-is. Whatever the
    // upstream returns, we forward back unchanged.
    if let Some(tools) = req_body.get("tools").and_then(|v| v.as_array()) {
        if let Some(converted) = convert_tools(tools) {
            chat_body["tools"] = converted;
        }
    }
    // Always passthrough tool_choice and parallel_tool_calls regardless of tools presence
    if let Some(tool_choice) = req_body.get("tool_choice") {
        chat_body["tool_choice"] = tool_choice.clone();
    }
    if let Some(parallel_tool_calls) = req_body.get("parallel_tool_calls") {
        chat_body["parallel_tool_calls"] = parallel_tool_calls.clone();
    }

    // Pass through all remaining fields (pure relay)
    // This ensures fields like background, conversation, metadata,
    // previous_response_id, store, truncation, user are forwarded
    if let (Some(req_obj), Some(chat_obj)) = (req_body.as_object(), chat_body.as_object_mut()) {
        for (key, value) in req_obj {
            if !chat_obj.contains_key(key) {
                chat_obj.insert(key.clone(), value.clone());
            }
        }
    }

    // 4. Route and forward (non-streaming)
    let requested_model = if model == "auto" || model.is_empty() {
        "auto".to_string()
    } else {
        model.to_string()
    };

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

    // Forward with retry - handle_responses (Responses API)
    // Note: No ModelAnnotationMiddleware for Responses handler per requirements
    let middleware: Vec<Box<dyn super::middleware::ForwarderMiddleware>> = vec![
        Box::new(super::middleware::StreamOptionsMiddleware),
    ];
    let caller_kind = super::middleware::CallerKind::Responses;

    let upstream_response = forwarder::forward_with_retry(
        &state,
        &resolved,
        &chat_body,
        headers,
        &requested_model,
        access_key.as_ref(),
        is_stream,
        &middleware,
        caller_kind,
    )
    .await;

    // 6. Build response based on stream mode
    let item_id = format!(
        "msg_{}",
        Uuid::new_v4().to_string().replace('-', "")[..16].to_string()
    );
    let created_at = chrono::Utc::now().timestamp();

    // Collect all SSE frames into a Vec for streaming
    let mut frames: Vec<Bytes> = Vec::new();

    // Build base response object
    let base_response = json!({
        "id": &response_id,
        "object": "response",
        "created_at": created_at,
        "status": "in_progress",
        "error": null,
        "incomplete_details": null,
        "instructions": req_body.get("instructions"),
        "max_output_tokens": req_body.get("max_output_tokens"),
        "model": model,
        "output": [],
        "parallel_tool_calls": req_body.get("parallel_tool_calls").unwrap_or(&json!(true)),
        "reasoning": req_body.get("reasoning").cloned().unwrap_or(json!({"effort": null, "summary": null})),
        "temperature": req_body.get("temperature").unwrap_or(&json!(1.0)),
        "text": req_body.get("text").cloned().unwrap_or(json!({"format": {"type": "text"}})),
        "tool_choice": req_body.get("tool_choice").unwrap_or(&json!("auto")),
        "tools": req_body.get("tools").unwrap_or(&json!([])),
        "top_p": req_body.get("top_p").unwrap_or(&json!(1.0)),
        "truncation": req_body.get("truncation").unwrap_or(&json!("disabled")),
        "previous_response_id": null,
        "store": req_body.get("store").unwrap_or(&json!(true)),
        "usage": null,
        "user": req_body.get("user"),
        "metadata": req_body.get("metadata").unwrap_or(&json!({}))
    });

    if is_stream {
        frames.push(sse_line(&json!({
            "type": "response.created",
            "response": &base_response
        })));
    }

    match upstream_response {
        Ok(resp) => {
            let status = resp.status().as_u16();

            if status != 200 {
                let body_bytes = axum::body::to_bytes(resp.into_body(), 32 * 1024 * 1024)
                    .await
                    .unwrap_or_default();
                let err_text = String::from_utf8_lossy(&body_bytes)
                    .chars()
                    .take(2000)
                    .collect::<String>();
                let error_event = json!({
                    "type": "response.failed",
                    "response": {
                        "id": &response_id,
                        "object": "response",
                        "created_at": created_at,
                        "status": "failed",
                        "error": { "message": err_text, "type": "upstream_error" }
                    }
                });
                if is_stream {
                    frames.push(sse_line(&error_event));
                    frames.push(sse_done());
                    return build_sse_response(frames);
                } else {
                    let non_stream_error = json!({
                        "id": &response_id,
                        "object": "response",
                        "created_at": created_at,
                        "status": "failed",
                        "error": { "message": err_text, "type": "upstream_error" }
                    });
                    return Ok(
                        (StatusCode::BAD_GATEWAY, axum::Json(non_stream_error)).into_response()
                    );
                }
            }

            if is_stream {
                // ── TRUE STREAMING: mpsc channel for incremental delivery ──
                // Create channel before the upstream processing loop
                let (tx, rx) = tokio::sync::mpsc::channel::<Result<Bytes, std::io::Error>>(256);

                // Drain initial events from frames into the channel output
                let initial_frames: Vec<Bytes> = frames.drain(..).collect();
                let upstream_body = resp.into_body();

                // Clone values for the async task
                let response_id_task = response_id.clone();
                let item_id_task = item_id.clone();
                let model_task = model.to_string();
                let req_body_task = req_body.clone();
                let created_at_task = created_at;

                // Spawn upstream processing in a background task for true streaming
                tokio::spawn(async move {
                    use std::collections::HashMap;

                    // Helper macro to send a frame, abort if receiver dropped
                    macro_rules! send {
                        ($frame:expr) => {
                            if tx.send(Ok($frame)).await.is_err() {
                                return;
                            }
                        };
                    }

                    // Send initial events first (response.created, response.in_progress)
                    for frame in initial_frames {
                        send!(frame);
                    }

                    let upstream_stream = upstream_body.into_data_stream();
                    let mut buffer = String::new();
                    let mut utf8_remainder: Vec<u8> = Vec::new();
                    let mut full_content = String::new();
                    let mut usage = json!({});
                    let mut finish_reason: Option<String> = None;
                    let mut upstream_model: Option<String> = None;
                    let mut content_len: usize = 0;

                    // Index tracking for output items
                    // Start at 1 because we assume text will exist at index 0
                    // If no text is present, we'll adjust in response.completed
                    let mut next_content_index: u32 = 1;
                    let mut index_by_key: HashMap<String, u32> = HashMap::new();
                    let mut tool_index_by_item_id: HashMap<String, u32> = HashMap::new();
                    let mut last_tool_index: Option<u32> = None;

                    // ToolCallAccum: index → accumulated state across chunks
                    struct ToolCallEntry {
                        id: String,
                        name: String,
                        arguments: String,
                        passthrough_item: Option<Value>,
                        added_emitted: bool,
                        assigned_index: u32,
                    }
                    let mut tool_accum: HashMap<usize, ToolCallEntry> = HashMap::new();
                    const MAX_CONTENT_LEN: usize = 10 * 1024 * 1024; // 10MB cap

                    let mut stream = upstream_stream;

                    // TODO: move to protocol::responses in a future refactor
                    // 这段 SSE 重包装代码与 state/usage 累积高度耦合，
                    // 搬迁成本很高。保留原位，后续版本再考虑抽离。
                    while let Some(chunk_result) = stream.next().await {
                        let bytes = match chunk_result {
                            Ok(b) => b,
                            Err(_) => break,
                        };

                        // Enforce 10MB stream content cap
                        content_len += bytes.len();
                        if content_len > MAX_CONTENT_LEN {
                            send!(sse_line(&json!({
                                "type": "error",
                                "code": "content_too_large",
                                "message": "Stream content exceeds 10MB limit"
                            })));
                            break;
                        }

                        append_utf8_safe(&mut buffer, &mut utf8_remainder, &bytes);

                        // Process complete SSE lines in buffer
                        while let Some(newline_pos) = buffer.find('\n') {
                            let line = buffer[..newline_pos].to_string();
                            buffer = buffer[newline_pos + 1..].to_string();
                            let line = line.trim();

                            if line.is_empty() {
                                continue;
                            }

                            if let Some(data) = sse_data_payload(line) {
                                if data == "[DONE]" {
                                    // ── Stream complete: emit finalization events ──

                                    // Emit output_item.done for text if present
                                    if !full_content.is_empty() {
                                        send!(sse_line(&json!({
                                            "type": "response.output_item.done",
                                            "response_id": &response_id_task,
                                            "output_index": 0,
                                            "item": {
                                                "type": "message",
                                                "role": "assistant",
                                                "id": &item_id_task,
                                                "status": "completed",
                                                "content": [{ "type": "output_text", "text": &full_content, "annotations": [] }]
                                            }
                                        })));
                                    }

                                    // Emit final events for each accumulated tool call
                                    let mut sorted_indices: Vec<usize> =
                                        tool_accum.keys().copied().collect();
                                    sorted_indices.sort();
                                    for &idx in &sorted_indices {
                                        let entry = &tool_accum[&idx];
                                        if let Some(item) = &entry.passthrough_item {
                                            send!(sse_line(&json!({
                                                "type": "response.output_item.done",
                                                "response_id": &response_id_task,
                                                "output_index": entry.assigned_index,
                                                "item": passthrough_output_item(item, Some("completed"))
                                            })));
                                        } else {
                                            send!(sse_line(&json!({
                                                "type": "response.function_call_arguments.done",
                                                "response_id": &response_id_task,
                                                "item_id": entry.id,
                                                "output_index": entry.assigned_index,
                                                "arguments": entry.arguments
                                            })));
                                            send!(sse_line(&json!({
                                                "type": "response.output_item.done",
                                                "response_id": &response_id_task,
                                                "output_index": entry.assigned_index,
                                                "item": {
                                                    "id": entry.id,
                                                    "type": "function_call",
                                                    "call_id": entry.id,
                                                    "name": entry.name,
                                                    "arguments": entry.arguments,
                                                    "status": "completed"
                                                }
                                            })));
                                        }
                                    }

                                    // Determine status based on finish_reason
                                    let streaming_incomplete = match finish_reason.as_deref() {
                                        Some("length") | Some("content_filter") => {
                                            json!({ "reason": finish_reason.as_ref().unwrap() })
                                        }
                                        _ => json!(null),
                                    };
                                    let final_status = match finish_reason.as_deref() {
                                        Some("length") | Some("content_filter") => "incomplete",
                                        _ => "completed",
                                    };
                                    let resolved_model =
                                        upstream_model.as_deref().unwrap_or(&model_task);

                                    // Build output items for response.completed (text + tool calls)
                                    let mut final_items: Vec<Value> = Vec::new();
                                    if !full_content.is_empty() {
                                        final_items.push(json!({
                                            "type": "message",
                                            "role": "assistant",
                                            "id": &item_id_task,
                                            "status": final_status,
                                            "content": [{ "type": "output_text", "text": &full_content, "annotations": [] }]
                                        }));
                                    }
                                    for &idx in &sorted_indices {
                                        let entry = &tool_accum[&idx];
                                        if let Some(item) = &entry.passthrough_item {
                                            final_items.push(passthrough_output_item(
                                                item,
                                                Some("completed"),
                                            ));
                                        } else {
                                            final_items.push(json!({
                                                "id": entry.id,
                                                "type": "function_call",
                                                "call_id": entry.id,
                                                "name": entry.name,
                                                "arguments": entry.arguments,
                                                "status": "completed"
                                            }));
                                        }
                                    }

                                    // Usage
                                    let input_tokens = usage
                                        .get("prompt_tokens")
                                        .and_then(|v| v.as_i64())
                                        .unwrap_or(0);
                                    let output_tokens = usage
                                        .get("completion_tokens")
                                        .and_then(|v| v.as_i64())
                                        .unwrap_or(0);
                                    let total = usage
                                        .get("total_tokens")
                                        .and_then(|v| v.as_i64())
                                        .unwrap_or(input_tokens + output_tokens);
                                    let cached = usage
                                        .get("prompt_tokens_details")
                                        .and_then(|d| d.get("cached_tokens"))
                                        .and_then(|v| v.as_i64())
                                        .unwrap_or(0);
                                    let reasoning = usage
                                        .get("completion_tokens_details")
                                        .and_then(|d| d.get("reasoning_tokens"))
                                        .and_then(|v| v.as_i64())
                                        .unwrap_or(0);

                                    send!(sse_line(&json!({
                                        "type": "response.completed",
                                        "response": {
                                            "id": &response_id_task,
                                            "object": "response",
                                            "created_at": created_at_task,
                                            "status": final_status,
                                            "error": null,
                                            "incomplete_details": streaming_incomplete,
                                            "instructions": req_body_task.get("instructions"),
                                            "max_output_tokens": req_body_task.get("max_output_tokens"),
                                            "model": resolved_model,
                                            "output": final_items,
                                            "output_text": if !full_content.is_empty() { Some(&full_content) } else { None },
                                            "parallel_tool_calls": req_body_task.get("parallel_tool_calls").unwrap_or(&json!(true)),
                                            "reasoning": req_body_task.get("reasoning").cloned().unwrap_or(json!({"effort": null, "summary": null})),
                                            "temperature": req_body_task.get("temperature").unwrap_or(&json!(1.0)),
                                            "text": req_body_task.get("text").cloned().unwrap_or(json!({"format": {"type": "text"}})),
                                            "tool_choice": req_body_task.get("tool_choice").unwrap_or(&json!("auto")),
                                            "tools": req_body_task.get("tools").unwrap_or(&json!([])),
                                            "top_p": req_body_task.get("top_p").unwrap_or(&json!(1.0)),
                                            "truncation": req_body_task.get("truncation").unwrap_or(&json!("disabled")),
                                            "previous_response_id": null,
                                            "store": req_body_task.get("store").unwrap_or(&json!(true)),
                                            "usage": {
                                                "input_tokens": input_tokens,
                                                "input_tokens_details": { "cached_tokens": cached },
                                                "output_tokens": output_tokens,
                                                "output_tokens_details": { "reasoning_tokens": reasoning },
                                                "total_tokens": total
                                            },
                                            "user": req_body_task.get("user"),
                                            "metadata": req_body_task.get("metadata").unwrap_or(&json!({}))
                                        }
                                    })));
                                    send!(sse_done());
                                    return;
                                }

                                // Parse upstream Chat Completions chunk
                                if let Ok(chunk_obj) = serde_json::from_str::<Value>(data) {
                                    // Capture upstream model from first chunk that has it
                                    if upstream_model.is_none() {
                                        if let Some(m) =
                                            chunk_obj.get("model").and_then(|m| m.as_str())
                                        {
                                            upstream_model = Some(m.to_string());
                                        }
                                    }

                                    if let Some(u) = chunk_obj.get("usage") {
                                        usage = u.clone();
                                    }

                                    // Extract finish_reason
                                    if let Some(fr) = chunk_obj
                                        .get("choices")
                                        .and_then(|c| c.as_array())
                                        .and_then(|a| a.first())
                                        .and_then(|c| c.get("finish_reason"))
                                        .and_then(|f| f.as_str())
                                    {
                                        if !fr.is_empty() {
                                            finish_reason = Some(fr.to_string());
                                        }
                                    }

                                    // Parse streaming tool_calls into accumulated state
                                    if let Some(tool_calls_delta) = chunk_obj
                                        .get("choices")
                                        .and_then(|c| c.as_array())
                                        .and_then(|a| a.first())
                                        .and_then(|c| c.get("delta"))
                                        .and_then(|d| d.get("tool_calls"))
                                        .and_then(|t| t.as_array())
                                    {
                                        for tc_delta in tool_calls_delta {
                                            let tc_idx = tc_delta
                                                .get("index")
                                                .and_then(|v| v.as_u64())
                                                .unwrap_or(0)
                                                as usize;
                                            let tc_id_new = tc_delta
                                                .get("id")
                                                .and_then(|v| v.as_str())
                                                .unwrap_or("");
                                            let tc_fn = tc_delta
                                                .get("function")
                                                .cloned()
                                                .unwrap_or_else(|| json!({}));
                                            let tc_name_delta = tc_fn
                                                .get("name")
                                                .and_then(|v| v.as_str())
                                                .unwrap_or("");
                                            let is_function_delta =
                                                tc_delta.get("type").and_then(|v| v.as_str())
                                                    == Some("function")
                                                    || (tc_delta.get("type").is_none()
                                                        && tc_delta.get("function").is_some());
                                            // Handle arguments as both string and object (for streaming deltas)
                                            let tc_args_delta = match tc_fn.get("arguments") {
                                                Some(Value::String(s)) => s.clone(),
                                                Some(Value::Object(_)) | Some(Value::Array(_)) => {
                                                    serde_json::to_string(
                                                        tc_fn.get("arguments").unwrap(),
                                                    )
                                                    .unwrap_or_else(|_| String::new())
                                                }
                                                _ => String::new(),
                                            };

                                            let entry =
                                                tool_accum.entry(tc_idx).or_insert_with(|| {
                                                    ToolCallEntry {
                                                        id: String::new(),
                                                        name: String::new(),
                                                        arguments: String::new(),
                                                        passthrough_item: None,
                                                        added_emitted: false,
                                                        assigned_index: 0,
                                                    }
                                                });

                                            if !is_function_delta
                                                || entry.passthrough_item.is_some()
                                            {
                                                let item =
                                                    entry.passthrough_item.get_or_insert_with(
                                                        || passthrough_output_item(tc_delta, None),
                                                    );
                                                merge_tool_delta(item, tc_delta);
                                                if !tc_id_new.is_empty() {
                                                    entry.id = tc_id_new.to_string();
                                                }
                                                if !entry.added_emitted {
                                                    send!(sse_line(&json!({
                                                        "type": "response.output_item.added",
                                                        "response_id": &response_id_task,
                                                        "output_index": tc_idx + 1,
                                                        "item": passthrough_output_item(item, Some("in_progress"))
                                                    })));
                                                    entry.added_emitted = true;
                                                }
                                                continue;
                                            }

                                            if !tc_id_new.is_empty() {
                                                entry.id = tc_id_new.to_string();
                                            }
                                            if !tc_name_delta.is_empty() {
                                                entry.name = tc_name_delta.to_string();
                                            }
                                            if !tc_args_delta.is_empty() {
                                                entry.arguments.push_str(&tc_args_delta);
                                            }

                                            // Calculate stable index for this tool call
                                            let tool_key = if !entry.id.is_empty() {
                                                Some(format!("tool:{}", entry.id))
                                            } else {
                                                None
                                            };

                                            let assigned_index = if let Some(ref k) = tool_key {
                                                if let Some(existing) = index_by_key.get(k).copied()
                                                {
                                                    existing
                                                } else {
                                                    let assigned = next_content_index;
                                                    next_content_index += 1;
                                                    index_by_key.insert(k.clone(), assigned);
                                                    assigned
                                                }
                                            } else {
                                                let assigned = next_content_index;
                                                next_content_index += 1;
                                                assigned
                                            };

                                            // Store index by item_id for later lookups
                                            entry.assigned_index = assigned_index;
                                            if !entry.id.is_empty() {
                                                tool_index_by_item_id
                                                    .insert(entry.id.clone(), assigned_index);
                                                last_tool_index = Some(assigned_index);
                                            }

                                            // Emit output_item.added only on first occurrence
                                            if !entry.added_emitted && !entry.name.is_empty() {
                                                send!(sse_line(&json!({
                                                    "type": "response.output_item.added",
                                                    "response_id": &response_id_task,
                                                    "output_index": assigned_index,
                                                    "item": {
                                                        "id": entry.id,
                                                        "type": "function_call",
                                                        "call_id": entry.id,
                                                        "name": entry.name,
                                                        "arguments": "",
                                                        "status": "in_progress"
                                                    }
                                                })));
                                                entry.added_emitted = true;
                                            }

                                            // Emit argument deltas incrementally
                                            if !tc_args_delta.is_empty() {
                                                let delta_index = tool_index_by_item_id
                                                    .get(&entry.id)
                                                    .copied()
                                                    .or(last_tool_index)
                                                    .unwrap_or(assigned_index);

                                                send!(sse_line(&json!({
                                                    "type": "response.function_call_arguments.delta",
                                                    "response_id": &response_id_task,
                                                    "item_id": entry.id,
                                                    "output_index": delta_index,
                                                    "delta": tc_args_delta
                                                })));
                                            }
                                        }
                                    }

                                    // Parse content delta incrementally
                                    if let Some(content) = chunk_obj
                                        .get("choices")
                                        .and_then(|c| c.as_array())
                                        .and_then(|a| a.first())
                                        .and_then(|c| c.get("delta"))
                                        .and_then(|d| d.get("content"))
                                        .and_then(|c| c.as_str())
                                    {
                                        if !content.is_empty() {
                                            // Emit output_item.added for text message on first content
                                            if full_content.is_empty() {
                                                send!(sse_line(&json!({
                                                    "type": "response.output_item.added",
                                                    "response_id": &response_id_task,
                                                    "output_index": 0,
                                                    "item": {
                                                        "type": "message",
                                                        "role": "assistant",
                                                        "id": &item_id_task,
                                                        "status": "in_progress",
                                                        "content": []
                                                    }
                                                })));
                                            }
                                            full_content.push_str(content);
                                            send!(sse_line(&json!({
                                                "type": "response.output_text.delta",
                                                "response_id": &response_id_task,
                                                "item_id": &item_id_task,
                                                "output_index": 0,
                                                "content_index": 0,
                                                "delta": content
                                            })));
                                        }
                                    }
                                }
                            }
                        }
                    }

                    // Stream ended without [DONE] — emit [DONE] anyway
                    send!(sse_done());
                });

                // Return streaming response immediately via channel
                let stream = futures::stream::unfold(rx, |mut rx| async move {
                    rx.recv().await.map(|item| (item, rx))
                });

                let body = Body::from_stream(stream);

                return Ok(axum::http::Response::builder()
                    .status(StatusCode::OK)
                    .header(header::CONTENT_TYPE, "text/event-stream; charset=utf-8")
                    .header(header::CACHE_CONTROL, "no-cache")
                    .header(header::CONNECTION, "close")
                    .body(body)
                    .map_err(|e| {
                        ProxyError::Internal(format!("Failed to build response: {e}"))
                    })?);
            }

            // ── NON-STREAMING: buffer entire response, parse JSON ──
            let body_bytes = match axum::body::to_bytes(resp.into_body(), 32 * 1024 * 1024).await {
                Ok(b) => b,
                Err(_) => {
                    frames.push(sse_line(&json!({
                        "type": "response.failed",
                        "response": {
                            "id": &response_id,
                            "object": "response",
                            "created_at": created_at,
                            "status": "failed",
                            "error": { "message": "Failed to read upstream body", "type": "upstream_error" }
                        }
                    })));
                    frames.push(sse_done());
                    return build_sse_response(frames);
                }
            };

            // Parse upstream Chat Completions response
            let obj: Value = serde_json::from_slice(&body_bytes).unwrap_or_else(|_| {
                json!({ "choices": [{ "message": { "content": String::from_utf8_lossy(&body_bytes) } }] })
            });

            let msg = obj
                .get("choices")
                .and_then(|c| c.as_array())
                .and_then(|a| a.first())
                .and_then(|c| c.get("message"))
                .cloned()
                .unwrap_or_else(|| json!({}));

            let content = msg.get("content").and_then(|v| v.as_str()).unwrap_or("");

            let tool_calls = msg.get("tool_calls").and_then(|v| v.as_array());

            // Build output items array for the final Response object
            let mut output_items: Vec<Value> = Vec::new();

            // Text output
            if !content.is_empty() {
                frames.push(sse_line(&json!({
                    "type": "response.output_text.delta",
                    "response_id": &response_id,
                    "item_id": &item_id,
                    "output_index": 0,
                    "content_index": 0,
                    "delta": content
                })));
                frames.push(sse_line(&json!({
                    "type": "response.output_item.done",
                    "response_id": &response_id,
                    "output_index": 0,
                    "item": {
                        "type": "message",
                        "role": "assistant",
                        "id": &item_id,
                        "status": "completed",
                        "content": [{ "type": "output_text", "text": content, "annotations": [] }]
                    }
                })));
                output_items.push(json!({
                    "type": "message",
                    "role": "assistant",
                    "id": &item_id,
                    "status": "completed",
                    "content": [{ "type": "output_text", "text": content, "annotations": [] }]
                }));
            }

            // Tool calls (function_call output or passthrough output items)
            if let Some(tc_array) = tool_calls {
                for (idx, tc) in tc_array.iter().enumerate() {
                    let output_index = if content.is_empty() {
                        idx as u32
                    } else {
                        (idx + 1) as u32
                    };

                    if !is_function_tool_call(tc) {
                        let item = passthrough_output_item(tc, Some("completed"));
                        frames.push(sse_line(&json!({
                            "type": "response.output_item.added",
                            "response_id": &response_id,
                            "output_index": output_index,
                            "item": passthrough_output_item(tc, Some("in_progress"))
                        })));
                        frames.push(sse_line(&json!({
                            "type": "response.output_item.done",
                            "response_id": &response_id,
                            "output_index": output_index,
                            "item": item
                        })));
                        output_items.push(item);
                        continue;
                    }

                    let tc_id = tc.get("id").and_then(|v| v.as_str()).unwrap_or("");
                    let tc_fn = tc.get("function").cloned().unwrap_or_else(|| json!({}));
                    let tc_name = tc_fn.get("name").and_then(|v| v.as_str()).unwrap_or("");
                    // Handle arguments as both string and object
                    let tc_args = match tc_fn.get("arguments") {
                        Some(Value::String(s)) => s.clone(),
                        Some(Value::Object(_)) | Some(Value::Array(_)) => {
                            serde_json::to_string(tc_fn.get("arguments").unwrap())
                                .unwrap_or_else(|_| "{}".to_string())
                        }
                        _ => "{}".to_string(),
                    };

                    frames.push(sse_line(&json!({
                        "type": "response.output_item.added",
                        "response_id": &response_id,
                        "output_index": output_index,
                        "item": {
                            "id": tc_id,
                            "type": "function_call",
                            "call_id": tc_id,
                            "name": tc_name,
                            "arguments": "",
                            "status": "in_progress"
                        }
                    })));
                    frames.push(sse_line(&json!({
                        "type": "response.function_call_arguments.delta",
                        "response_id": &response_id,
                        "item_id": tc_id,
                        "output_index": output_index,
                        "delta": tc_args
                    })));
                    frames.push(sse_line(&json!({
                        "type": "response.function_call_arguments.done",
                        "response_id": &response_id,
                        "item_id": tc_id,
                        "output_index": output_index,
                        "arguments": tc_args
                    })));
                    frames.push(sse_line(&json!({
                        "type": "response.output_item.done",
                        "response_id": &response_id,
                        "output_index": output_index,
                        "item": {
                            "id": tc_id,
                            "type": "function_call",
                            "call_id": tc_id,
                            "name": tc_name,
                            "arguments": tc_args,
                            "status": "completed"
                        }
                    })));
                    output_items.push(json!({
                        "id": tc_id,
                        "type": "function_call",
                        "call_id": tc_id,
                        "name": tc_name,
                        "arguments": tc_args,
                        "status": "completed"
                    }));
                }
            }

            // Extract finish_reason for status/incomplete_details
            let finish_reason = obj
                .get("choices")
                .and_then(|c| c.as_array())
                .and_then(|a| a.first())
                .and_then(|c| c.get("finish_reason"))
                .and_then(|f| f.as_str());

            let incomplete_details = match finish_reason {
                Some("length") | Some("content_filter") => json!({ "reason": finish_reason }),
                _ => json!(null),
            };

            let final_status = match finish_reason {
                Some("length") | Some("content_filter") => "incomplete",
                _ => "completed",
            };

            // Get upstream model
            let upstream_model = obj.get("model").and_then(|m| m.as_str()).unwrap_or(model);

            // Usage
            let usage = obj.get("usage").cloned().unwrap_or_else(|| json!({}));
            let input_tokens = usage
                .get("prompt_tokens")
                .and_then(|v| v.as_i64())
                .unwrap_or(0);
            let output_tokens = usage
                .get("completion_tokens")
                .and_then(|v| v.as_i64())
                .unwrap_or(0);
            let total_tokens = usage
                .get("total_tokens")
                .and_then(|v| v.as_i64())
                .unwrap_or(input_tokens + output_tokens);
            let cached_tokens = usage
                .get("prompt_tokens_details")
                .and_then(|d| d.get("cached_tokens"))
                .and_then(|v| v.as_i64())
                .unwrap_or(0);
            let reasoning_tokens = usage
                .get("completion_tokens_details")
                .and_then(|d| d.get("reasoning_tokens"))
                .and_then(|v| v.as_i64())
                .unwrap_or(0);

            let completed_response = json!({
                "id": &response_id,
                "object": "response",
                "created_at": created_at,
                "status": final_status,
                "error": null,
                "incomplete_details": incomplete_details,
                "instructions": req_body.get("instructions"),
                "max_output_tokens": req_body.get("max_output_tokens"),
                "model": upstream_model,
                "output": output_items,
                "output_text": content,
                "parallel_tool_calls": req_body.get("parallel_tool_calls").unwrap_or(&json!(true)),
                "reasoning": req_body.get("reasoning").cloned().unwrap_or(json!({"effort": null, "summary": null})),
                "temperature": req_body.get("temperature").unwrap_or(&json!(1.0)),
                "text": req_body.get("text").cloned().unwrap_or(json!({"format": {"type": "text"}})),
                "tool_choice": req_body.get("tool_choice").unwrap_or(&json!("auto")),
                "tools": req_body.get("tools").unwrap_or(&json!([])),
                "top_p": req_body.get("top_p").unwrap_or(&json!(1.0)),
                "truncation": req_body.get("truncation").unwrap_or(&json!("disabled")),
                "previous_response_id": null,
                "store": req_body.get("store").unwrap_or(&json!(true)),
                "usage": {
                    "input_tokens": input_tokens,
                    "input_tokens_details": { "cached_tokens": cached_tokens },
                    "output_tokens": output_tokens,
                    "output_tokens_details": { "reasoning_tokens": reasoning_tokens },
                    "total_tokens": total_tokens
                },
                "user": req_body.get("user"),
                "metadata": req_body.get("metadata").unwrap_or(&json!({}))
            });

            if is_stream {
                frames.push(sse_line(&json!({
                    "type": "response.completed",
                    "response": &completed_response
                })));
            } else {
                // Store response for later retrieval via GET
                let mut store = state.response_store.write().await;
                store.insert(response_id.clone(), completed_response.clone());
                // Evict oldest if store exceeds 100 entries
                if store.len() > 100 {
                    if let Some(oldest_key) = store.keys().next().cloned() {
                        store.remove(&oldest_key);
                    }
                }
                return Ok(axum::Json(completed_response).into_response());
            }
        }
        Err(e) => {
            let error_response = json!({
                "id": &response_id,
                "object": "response",
                "created_at": created_at,
                "status": "failed",
                "error": { "message": format!("{e}"), "type": "proxy_error" }
            });

            if is_stream {
                frames.push(sse_line(&json!({
                    "type": "response.failed",
                    "response": &error_response
                })));
            } else {
                return Ok(axum::Json(error_response).into_response());
            }
        }
    }

    frames.push(sse_done());
    build_sse_response(frames)
}

/// Build an SSE response from pre-collected frames using a streaming channel.
fn build_sse_response(frames: Vec<Bytes>) -> Result<axum::response::Response, ProxyError> {
    let (tx, rx) = tokio::sync::mpsc::channel::<Result<Bytes, std::io::Error>>(frames.len());

    // Send all frames in background, then drop the sender
    tokio::spawn(async move {
        for frame in frames {
            if tx.send(Ok(frame)).await.is_err() {
                break;
            }
        }
        // Sender dropped → stream ends
    });

    let stream = futures::stream::unfold(rx, |mut rx| async move {
        let item = rx.recv().await?;
        Some((item, rx))
    });

    let body = Body::from_stream(stream);

    let response = axum::http::Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/event-stream; charset=utf-8")
        .header(header::CACHE_CONTROL, "no-cache")
        .header(header::CONNECTION, "close")
        .body(body)
        .map_err(|e| ProxyError::Internal(format!("Failed to build response: {e}")))?;

    Ok(response)
}

// ─── Response Store Helpers ───────────────────────────────────────────

/// GET /v1/responses/:id — Retrieve a stored response.
pub async fn get_response(
    State(state): State<ProxyState>,
    axum::extract::Path(response_id): axum::extract::Path<String>,
) -> axum::response::Response {
    let store = state.response_store.read().await;
    if let Some(resp) = store.get(&response_id).cloned() {
        return axum::Json(resp).into_response();
    }

    (StatusCode::NOT_FOUND, axum::Json(json!({
        "error": {
            "message": format!("Response '{}' not found. This proxy does not persist responses.", response_id),
            "type": "not_found_error",
            "code": "response_not_found"
        }
    })))
    .into_response()
}

/// DELETE /v1/responses/:id — Delete a stored response.
pub async fn delete_response(
    State(state): State<ProxyState>,
    axum::extract::Path(response_id): axum::extract::Path<String>,
) -> axum::response::Response {
    let mut store = state.response_store.write().await;
    store.remove(&response_id);

    axum::Json(json!({
        "id": response_id,
        "object": "response",
        "deleted": true
    }))
    .into_response()
}

/// POST /v1/responses/:id/cancel — Cancel a response.
///
/// If the response exists in the store (future: background responses),
/// marks it as cancelled and returns the updated response.
/// Otherwise returns 404 since the proxy doesn't persist responses.
pub async fn cancel_response(
    State(state): State<ProxyState>,
    axum::extract::Path(response_id): axum::extract::Path<String>,
) -> impl IntoResponse {
    let store = state.response_store.read().await;
    if let Some(mut resp) = store.get(&response_id).cloned() {
        resp["status"] = json!("cancelled");
        return axum::Json(resp).into_response();
    }

    (
        StatusCode::NOT_FOUND,
        axum::Json(json!({
            "error": {
                "message": format!("Response '{}' not found. This proxy does not persist responses.", response_id),
                "type": "not_found_error",
                "code": "response_not_found"
            }
        })),
    )
        .into_response()
}

// ─── Unit Tests ───────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Tool Type Tests ──

    #[test]
    fn test_tool_type_function_allowed() {
        let tools = vec![json!({ "type": "function", "name": "get_weather" })];
        // function should not trigger any rejection
        for tool in &tools {
            assert_eq!(tool.get("type").and_then(|v| v.as_str()), Some("function"));
        }
    }

    #[test]
    fn test_tool_type_host_tool_passthrough() {
        // host_tool is passed through as-is (pure relay)
        let tool = json!({ "type": "host_tool" });
        assert_eq!(tool.get("type").and_then(|v| v.as_str()), Some("host_tool"));
    }

    // ── Input Message Conversion Tests ──

    #[test]
    fn test_input_to_messages_string() {
        let input = json!("Hello");
        let msgs = input_to_messages(&input, None);
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0]["role"], "user");
        assert_eq!(msgs[0]["content"], "Hello");
    }

    #[test]
    fn test_input_to_messages_with_instructions() {
        let input = json!("Hello");
        let msgs = input_to_messages(&input, Some("Be helpful"));
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0]["role"], "system");
        assert_eq!(msgs[0]["content"], "Be helpful");
        assert_eq!(msgs[1]["role"], "user");
    }

    #[test]
    fn test_input_to_messages_null_input() {
        let input = Value::Null;
        let msgs = input_to_messages(&input, Some("Instructions"));
        // Null input + instructions → only system message, no default user "Hello"
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0]["role"], "system");
        assert_eq!(msgs[0]["content"], "Instructions");
    }

    #[test]
    fn test_input_to_messages_function_call() {
        let input = json!([
            { "type": "function_call", "call_id": "call_1", "name": "get_weather", "arguments": "{\"city\":\"NYC\"}" },
            { "type": "function_call_output", "call_id": "call_1", "output": "Sunny" }
        ]);
        let msgs = input_to_messages(&input, None);
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0]["role"], "assistant");
        assert_eq!(msgs[1]["role"], "tool");
        assert_eq!(msgs[1]["tool_call_id"], "call_1");
    }

    // ── Tool Conversion Tests ──

    #[test]
    fn test_convert_tools_function() {
        let tools = vec![
            json!({ "type": "function", "name": "my_fn", "parameters": { "type": "object" } }),
        ];
        let result = convert_tools(&tools);
        assert!(result.is_some());
        let arr = result.unwrap();
        let arr = arr.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["function"]["name"], "my_fn");
    }

    #[test]
    fn test_convert_tools_function_preserves_extra_fields() {
        let tools = vec![json!({
            "type": "function",
            "name": "my_fn",
            "parameters": { "type": "object" },
            "x-provider": { "mode": "raw" }
        })];
        let result = convert_tools(&tools).unwrap();
        let arr = result.as_array().unwrap();
        assert_eq!(arr[0]["type"], "function");
        assert_eq!(arr[0]["function"]["name"], "my_fn");
        assert_eq!(arr[0]["x-provider"]["mode"], "raw");
        assert!(arr[0].get("name").is_none());
    }

    #[test]
    fn test_passthrough_output_item_removes_index_and_adds_type() {
        let item = passthrough_output_item(
            &json!({ "id": "call_1", "index": 2, "custom": true }),
            Some("completed"),
        );
        assert_eq!(item["id"], "call_1");
        assert_eq!(item["type"], "tool_call");
        assert_eq!(item["custom"], true);
        assert_eq!(item["status"], "completed");
        assert!(item.get("index").is_none());
    }

    #[test]
    fn test_convert_tools_empty() {
        let tools: Vec<Value> = vec![];
        assert!(convert_tools(&tools).is_none());
    }

    #[test]
    fn test_convert_tools_non_function_passthrough() {
        // Non-function types are passed through as-is (pure translation layer).
        let tools = vec![json!({ "type": "web_search", "search_context_size": "medium" })];
        let result = convert_tools(&tools);
        assert!(result.is_some());
        let arr = result.unwrap().as_array().unwrap().clone();
        assert_eq!(arr.len(), 1);
        // Passed through unchanged
        assert_eq!(arr[0]["type"], "web_search");
        assert_eq!(arr[0]["search_context_size"], "medium");
    }

    #[test]
    fn test_convert_tools_mixed_passthrough() {
        // Function tools get converted, non-function tools pass through
        let tools = vec![
            json!({ "type": "function", "name": "my_fn", "parameters": { "type": "object" } }),
            json!({ "type": "web_search" }),
            json!({ "type": "local_shell" }),
        ];
        let result = convert_tools(&tools);
        let arr = result.unwrap().as_array().unwrap().clone();
        assert_eq!(arr.len(), 3);
        assert_eq!(arr[0]["function"]["name"], "my_fn"); // converted
        assert_eq!(arr[1]["type"], "web_search"); // passthrough
        assert_eq!(arr[2]["type"], "local_shell"); // passthrough
    }

    // ── Image URL Content Tests ──

    #[test]
    fn test_input_to_messages_mixed_text_and_image() {
        let input = json!([{
            "type": "message", "role": "user",
            "content": [
                { "type": "text", "text": "describe" },
                { "type": "input_image", "image_url": "https://example.com/img.jpg" }
            ]
        }]);
        let msgs = input_to_messages(&input, None);
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0]["role"], "user");

        // Content should be an array with text + image_url
        let content = msgs[0]["content"]
            .as_array()
            .expect("content should be array");
        assert_eq!(content.len(), 2);
        assert_eq!(content[0]["type"], "text");
        assert_eq!(content[0]["text"], "describe");
        assert_eq!(content[1]["type"], "image_url");
        assert_eq!(
            content[1]["image_url"]["url"],
            "https://example.com/img.jpg"
        );
        assert_eq!(content[1]["image_url"]["detail"], "auto");
    }

    #[test]
    fn test_input_to_messages_pure_image() {
        let input = json!([{
            "type": "message", "role": "user",
            "content": [
                { "type": "input_image", "image_url": "https://example.com/photo.png" }
            ]
        }]);
        let msgs = input_to_messages(&input, None);
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0]["role"], "user");

        // Content should be an array with only image_url
        let content = msgs[0]["content"]
            .as_array()
            .expect("content should be array");
        assert_eq!(content.len(), 1);
        assert_eq!(content[0]["type"], "image_url");
        assert_eq!(
            content[0]["image_url"]["url"],
            "https://example.com/photo.png"
        );
        assert_eq!(content[0]["image_url"]["detail"], "auto");
    }

    #[test]
    fn test_input_to_messages_image_with_custom_detail() {
        let input = json!([{
            "type": "message", "role": "user",
            "content": [
                { "type": "input_image", "image_url": "https://example.com/img.jpg", "detail": "high" }
            ]
        }]);
        let msgs = input_to_messages(&input, None);
        assert_eq!(msgs.len(), 1);

        let content = msgs[0]["content"]
            .as_array()
            .expect("content should be array");
        assert_eq!(content[0]["image_url"]["detail"], "high");
    }

    #[test]
    fn test_input_to_messages_image_no_regression_text_only() {
        // Text-only content array should still produce a plain string
        let input = json!([{
            "type": "message", "role": "user",
            "content": [
                { "type": "text", "text": "hello" }
            ]
        }]);
        let msgs = input_to_messages(&input, None);
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0]["role"], "user");
        // Without images or unknown parts, content remains a plain string
        assert_eq!(msgs[0]["content"], "hello");
    }

    #[test]
    fn test_input_to_messages_preserves_unknown_structured_item() {
        let input =
            json!([{ "type": "custom_tool_result", "id": "item_1", "payload": { "ok": true } }]);
        let msgs = input_to_messages(&input, None);
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0]["role"], "user");
        assert_eq!(msgs[0]["content"]["type"], "custom_tool_result");
        assert_eq!(msgs[0]["content"]["id"], "item_1");
        assert_eq!(msgs[0]["content"]["payload"]["ok"], true);
    }

    #[test]
    fn test_append_utf8_safe_preserves_split_multibyte_content() {
        let mut buffer = String::new();
        let mut remainder = Vec::new();
        let text = "data: {\"delta\":\"你好世界\"}\n\n";
        let bytes = text.as_bytes();
        let split_at = bytes.iter().position(|byte| *byte >= 0x80).unwrap() + 1;

        append_utf8_safe(&mut buffer, &mut remainder, &bytes[..split_at]);
        assert!(!buffer.contains('�'));
        assert!(!remainder.is_empty());

        append_utf8_safe(&mut buffer, &mut remainder, &bytes[split_at..]);
        assert_eq!(buffer, text);
        assert!(remainder.is_empty());
        assert!(!buffer.contains('�'));
    }

    #[test]
    fn test_sse_data_payload_accepts_optional_space() {
        assert_eq!(
            sse_data_payload("data: {\"ok\":true}"),
            Some("{\"ok\":true}")
        );
        assert_eq!(
            sse_data_payload("data:{\"ok\":true}"),
            Some("{\"ok\":true}")
        );
        assert_eq!(sse_data_payload("event: message"), None);
    }
}
