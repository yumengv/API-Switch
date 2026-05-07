//! POST /v1/responses — OpenAI Responses API compatibility layer.
//!
//! Converts Responses API requests to Chat Completions format,
//! forwards non-streaming to the upstream, and wraps the result
//! as a Responses API SSE event stream.

use super::auth;
use super::forwarder;
use super::router;
use super::handlers::ProxyError;
use super::server::ProxyState;
use axum::body::Body;
use axum::extract::State;
use axum::http::{header, StatusCode};
use axum::response::IntoResponse;
use bytes::Bytes;
use futures::StreamExt;
use serde_json::{json, Value};
use uuid::Uuid;

// ─── input_to_messages ───────────────────────────────────────────────

/// Convert the Responses API `input` field into a Chat Completions `messages` array.
///
/// The `input` can be:
/// - A plain string → single user message
/// - A list of items: strings, message objects, function_call, or function_call_output
/// - An object (rare, stringified as user message)
///
/// multi-turn tool use: function_call → assistant tool_calls,
/// function_call_output → tool message.
fn input_to_messages(input: &Value, instructions: Option<&str>) -> Vec<Value> {
    let mut msgs: Vec<Value> = Vec::new();

    // Optional system message from `instructions`
    if let Some(inst) = instructions {
        if !inst.is_empty() {
            msgs.push(json!({ "role": "system", "content": inst }));
        }
    }

    match input {
        Value::String(s) => {
            msgs.push(json!({ "role": "user", "content": s }));
        }
        Value::Array(items) => {
            // Group consecutive function_call + function_call_output pairs
            // into a single assistant tool_calls message + individual tool messages
            let mut i = 0;
            while i < items.len() {
                let item = &items[i];

                if let Value::Object(obj) = item {
                    let typ = obj.get("type").and_then(|v| v.as_str()).unwrap_or("");

                    match typ {
                        // ── function_call → assistant message with tool_calls ──
                        "function_call" => {
                            let call_id = obj.get("call_id").and_then(|v| v.as_str()).unwrap_or("");
                            let name = obj.get("name").and_then(|v| v.as_str()).unwrap_or("");
                            let arguments = obj.get("arguments").and_then(|v| v.as_str()).unwrap_or("{}");

                            // Collect tool calls for this assistant turn
                            let mut tool_calls = vec![json!({
                                "id": call_id,
                                "type": "function",
                                "function": {
                                    "name": name,
                                    "arguments": arguments,
                                }
                            })];

                            // If next items are also function_calls (same turn), group them
                            let mut j = i + 1;
                            while j < items.len() {
                                if let Value::Object(next_obj) = &items[j] {
                                    let next_typ = next_obj.get("type").and_then(|v| v.as_str()).unwrap_or("");
                                    if next_typ == "function_call" {
                                        let next_call_id = next_obj.get("call_id").and_then(|v| v.as_str()).unwrap_or("");
                                        let next_name = next_obj.get("name").and_then(|v| v.as_str()).unwrap_or("");
                                        let next_args = next_obj.get("arguments").and_then(|v| v.as_str()).unwrap_or("{}");
                                        tool_calls.push(json!({
                                            "id": next_call_id,
                                            "type": "function",
                                            "function": {
                                                "name": next_name,
                                                "arguments": next_args,
                                            }
                                        }));
                                        j += 1;
                                    } else {
                                        break;
                                    }
                                } else {
                                    break;
                                }
                            }

                            msgs.push(json!({
                                "role": "assistant",
                                "content": null,
                                "tool_calls": tool_calls
                            }));
                            i = j;
                            continue;
                        }

                        // ── function_call_output → tool message ──
                        "function_call_output" => {
                            let call_id = obj.get("call_id").and_then(|v| v.as_str()).unwrap_or("");
                            let output = match obj.get("output") {
                                Some(Value::String(s)) => s.clone(),
                                Some(v) => serde_json::to_string(v).unwrap_or_else(|_| String::new()),
                                None => String::new(),
                            };

                            msgs.push(json!({
                                "role": "tool",
                                "tool_call_id": call_id,
                                "content": output,
                            }));
                            i += 1;
                            continue;
                        }

                        // ── regular message ──
                        _ => {
                            let role = match obj.get("role") {
                                Some(Value::String(r)) if r == "system" || r == "user" || r == "assistant" || r == "tool" => r.clone(),
                                _ => {
                                    if matches!(typ, "message") { "assistant".to_string() } else { "user".to_string() }
                                }
                            };

                            let text = match obj.get("content") {
                                Some(Value::String(s)) => s.clone(),
                                Some(Value::Array(parts)) => {
                                    let mut texts = Vec::new();
                                    for p in parts {
                                        match p {
                                            Value::String(s) => texts.push(s.clone()),
                                            Value::Object(o) => {
                                                let t = o.get("text")
                                                    .or_else(|| o.get("input_text"))
                                                    .or_else(|| o.get("output_text"))
                                                    .and_then(|v| v.as_str())
                                                    .unwrap_or("");
                                                if !t.is_empty() { texts.push(t.to_string()); }
                                            }
                                            _ => {}
                                        }
                                    }
                                    texts.join("\n")
                                }
                                _ => String::new(),
                            };

                            if !text.is_empty() {
                                msgs.push(json!({ "role": role, "content": text }));
                            } else if matches!(typ, "function_call" | "function_call_output") {
                                // Already handled above; skip empty message fallback
                            }

                            i += 1;
                        }
                    }
                } else if let Value::String(s) = item {
                    msgs.push(json!({ "role": "user", "content": s }));
                    i += 1;
                } else {
                    i += 1;
                }
            }
        }
        other => {
            let text = if other.is_null() {
                "Hello".to_string()
            } else {
                serde_json::to_string(other).unwrap_or_else(|_| "{}".to_string())
            };
            msgs.push(json!({ "role": "user", "content": text }));
        }
    }

    if msgs.is_empty() {
        msgs.push(json!({ "role": "user", "content": "Hello" }));
    }

    msgs
}

// ─── convert_tools ───────────────────────────────────────────────────

/// Convert Responses API tool definitions to Chat Completions format.
///
/// Responses API: `{ type: "function", name, description, parameters, strict }`
/// Chat API:      `{ type: "function", function: { name, description, parameters, strict } }`
fn convert_tools(tools: &[Value]) -> Option<Value> {
    let converted: Vec<Value> = tools
        .iter()
        .filter_map(|t| {
            let typ = t.get("type").and_then(|v| v.as_str())?;
            if typ != "function" {
                return None;
            }

            // If already in Chat format, pass through
            if t.get("function").is_some() {
                return Some(t.clone());
            }

            // Convert from Responses format
            let name = t.get("name").and_then(|v| v.as_str()).unwrap_or("tool");
            let description = t.get("description").and_then(|v| v.as_str()).unwrap_or("");
            let parameters = t.get("parameters").cloned().unwrap_or_else(|| {
                json!({ "type": "object", "properties": {} })
            });

            let mut function = json!({
                "name": name,
                "description": description,
                "parameters": parameters,
            });

            if let Some(strict) = t.get("strict") {
                function["strict"] = strict.clone();
            }

            Some(json!({ "type": "function", "function": function }))
        })
        .collect();

    if converted.is_empty() {
        None
    } else {
        Some(Value::Array(converted))
    }
}

// ─── SSE helpers ─────────────────────────────────────────────────────

fn sse_line(obj: &Value) -> Bytes {
    let line = format!("data: {}\n\n", serde_json::to_string(obj).unwrap_or_default());
    Bytes::from(line)
}

fn sse_done() -> Bytes {
    Bytes::from("data: [DONE]\n\n")
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

    let req_body: Value = serde_json::from_slice(&body_bytes)
        .map_err(|e| ProxyError::Internal(format!("Failed to parse JSON: {e}")))?;

    // Validate unsupported input types (file/image)
    if let Some(items) = req_body.get("input").and_then(|v| v.as_array()) {
        for item in items {
            if let Some(typ) = item.get("type").and_then(|v| v.as_str()) {
                if matches!(typ, "input_image" | "input_file") {
                    return Ok(axum::Json(json!({
                        "error": {
                            "message": format!("Unsupported input type '{}'. Only text, message, function_call, and function_call_output are supported.", typ),
                            "type": "invalid_request_error",
                            "code": "unsupported_input_type"
                        }
                    }))
                    .into_response());
                }
            }
        }
    }

    // Validate unsupported tool types (web_search, file_search, code_interpreter)
    if let Some(tools) = req_body.get("tools").and_then(|v| v.as_array()) {
        for tool in tools {
            if let Some(typ) = tool.get("type").and_then(|v| v.as_str()) {
                if !matches!(typ, "function") {
                    return Ok(axum::Json(json!({
                        "error": {
                            "message": format!("Unsupported tool type '{}'. Only 'function' type tools are supported.", typ),
                            "type": "invalid_request_error",
                            "code": "unsupported_tool_type"
                        }
                    }))
                    .into_response());
                }
            }
        }
    }

    // 3. Determine stream mode BEFORE building chat body
    let is_stream = req_body.get("stream").and_then(|v| v.as_bool()).unwrap_or(false);

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

    // Convert tools if present
    if let Some(tools) = req_body.get("tools").and_then(|v| v.as_array()) {
        if let Some(converted) = convert_tools(tools) {
            chat_body["tools"] = converted;
            chat_body["tool_choice"] = req_body.get("tool_choice")
                .cloned()
                .unwrap_or(json!("auto"));
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

    let upstream_response = forwarder::forward_with_retry(
        &state,
        &resolved,
        &chat_body,
        headers,
        &requested_model,
        access_key.as_ref(),
        is_stream,
    )
    .await;

    // 6. Build response based on stream mode
    let item_id = format!("msg_{}", Uuid::new_v4().to_string().replace('-', "")[..16].to_string());
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
        "previous_response_id": req_body.get("previous_response_id"),
        "reasoning": { "effort": null, "summary": null },
        "store": req_body.get("store").unwrap_or(&json!(true)),
        "temperature": req_body.get("temperature").unwrap_or(&json!(1.0)),
        "text": { "format": { "type": "text" } },
        "tool_choice": req_body.get("tool_choice").unwrap_or(&json!("auto")),
        "tools": req_body.get("tools").unwrap_or(&json!([])),
        "top_p": req_body.get("top_p").unwrap_or(&json!(1.0)),
        "truncation": req_body.get("truncation").unwrap_or(&json!("disabled")),
        "usage": null,
        "user": req_body.get("user"),
        "metadata": req_body.get("metadata").unwrap_or(&json!({}))
    });

    if is_stream {
        frames.push(sse_line(&json!({
            "type": "response.created",
            "response": &base_response
        })));

        frames.push(sse_line(&json!({
            "type": "response.in_progress",
            "response": {
                "id": &response_id,
                "object": "response",
                "created_at": created_at,
                "status": "in_progress",
                "output": []
            }
        })));
    }

    match upstream_response {
        Ok(resp) => {
            let status = resp.status().as_u16();

            if status != 200 {
                let body_bytes = axum::body::to_bytes(resp.into_body(), 32 * 1024 * 1024)
                    .await
                    .unwrap_or_default();
                let err_text = String::from_utf8_lossy(&body_bytes).chars().take(2000).collect::<String>();
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
                    return Ok(axum::Json(error_event).into_response());
                }
            }

            if is_stream {
                // ── TRUE STREAMING: convert upstream Chat SSE → Responses API SSE ──
                let upstream_stream = resp.into_body().into_data_stream();
                let mut buffer = String::new();
                let mut full_content = String::new();
                let mut usage = json!({});

                // Pre-build the initial events into frames
                let item_id_ref = item_id.clone();
                let response_id_ref = response_id.clone();

                // We'll collect all frames first, then return as streaming response
                // This is still "pseudo-streaming" for the conversion layer,
                // but upstream IS being streamed incrementally.
                let mut collected_frames: Vec<Bytes> = frames.drain(..).collect();

                use futures::StreamExt;
                let mut stream = upstream_stream;

                while let Some(chunk_result) = stream.next().await {
                    let bytes = match chunk_result {
                        Ok(b) => b,
                        Err(_) => break,
                    };
                    let chunk_str = String::from_utf8_lossy(&bytes);
                    buffer.push_str(&chunk_str);

                    // Process complete SSE lines in buffer
                    while let Some(newline_pos) = buffer.find('\n') {
                        let line = buffer[..newline_pos].to_string();
                        buffer = buffer[newline_pos + 1..].to_string();
                        let line = line.trim();

                        if line.is_empty() { continue; }

                        if let Some(data) = line.strip_prefix("data: ") {
                            if data == "[DONE]" {
                                // Emit output_item.done + response.completed
                                if !full_content.is_empty() {
                                    collected_frames.push(sse_line(&json!({
                                        "type": "response.output_item.done",
                                        "response_id": &response_id_ref,
                                        "output_index": 0,
                                        "item": {
                                            "type": "message",
                                            "role": "assistant",
                                            "id": &item_id_ref,
                                            "status": "completed",
                                            "content": [{ "type": "output_text", "text": &full_content, "annotations": [] }]
                                        }
                                    })));
                                }

                                let input_tokens = usage.get("prompt_tokens").and_then(|v| v.as_i64()).unwrap_or(0);
                                let output_tokens = usage.get("completion_tokens").and_then(|v| v.as_i64()).unwrap_or(0);
                                let total_tokens = usage.get("total_tokens").and_then(|v| v.as_i64()).unwrap_or(input_tokens + output_tokens);

                                collected_frames.push(sse_line(&json!({
                                    "type": "response.completed",
                                    "response": {
                                        "id": &response_id_ref,
                                        "object": "response",
                                        "created_at": created_at,
                                        "status": "completed",
                                        "error": null,
                                        "incomplete_details": null,
                                        "instructions": req_body.get("instructions"),
                                        "max_output_tokens": req_body.get("max_output_tokens"),
                                        "model": model,
                                        "output": [],
                                        "parallel_tool_calls": req_body.get("parallel_tool_calls").unwrap_or(&json!(true)),
                                        "previous_response_id": req_body.get("previous_response_id"),
                                        "reasoning": { "effort": null, "summary": null },
                                        "store": req_body.get("store").unwrap_or(&json!(true)),
                                        "temperature": req_body.get("temperature").unwrap_or(&json!(1.0)),
                                        "text": { "format": { "type": "text" } },
                                        "tool_choice": req_body.get("tool_choice").unwrap_or(&json!("auto")),
                                        "tools": req_body.get("tools").unwrap_or(&json!([])),
                                        "top_p": req_body.get("top_p").unwrap_or(&json!(1.0)),
                                        "truncation": req_body.get("truncation").unwrap_or(&json!("disabled")),
                                        "usage": {
                                            "input_tokens": input_tokens,
                                            "input_tokens_details": { "cached_tokens": 0 },
                                            "output_tokens": output_tokens,
                                            "output_tokens_details": { "reasoning_tokens": 0 },
                                            "total_tokens": total_tokens
                                        },
                                        "user": req_body.get("user"),
                                        "metadata": req_body.get("metadata").unwrap_or(&json!({}))
                                    }
                                })));
                                collected_frames.push(sse_done());
                                return build_sse_response(collected_frames);
                            }

                            // Parse upstream Chat Completions chunk
                            if let Ok(chunk_obj) = serde_json::from_str::<Value>(data) {
                                if let Some(u) = chunk_obj.get("usage") {
                                    usage = u.clone();
                                }
                                if let Some(content) = chunk_obj.get("choices")
                                    .and_then(|c| c.as_array())
                                    .and_then(|a| a.first())
                                    .and_then(|c| c.get("delta"))
                                    .and_then(|d| d.get("content"))
                                    .and_then(|c| c.as_str())
                                {
                                    if !content.is_empty() {
                                        full_content.push_str(content);
                                        collected_frames.push(sse_line(&json!({
                                            "type": "response.output_text.delta",
                                            "response_id": &response_id_ref,
                                            "item_id": &item_id_ref,
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

                // Stream ended without [DONE]
                collected_frames.push(sse_done());
                return build_sse_response(collected_frames);
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

            let msg = obj.get("choices")
                .and_then(|c| c.as_array())
                .and_then(|a| a.first())
                .and_then(|c| c.get("message"))
                .cloned()
                .unwrap_or_else(|| json!({}));

            let content = msg.get("content")
                .and_then(|v| v.as_str())
                .unwrap_or("");

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

            // Tool calls (function_call output)
            if let Some(tc_array) = tool_calls {
                for (idx, tc) in tc_array.iter().enumerate() {
                    let output_index = if content.is_empty() { idx as u32 } else { (idx + 1) as u32 };
                    let tc_id = tc.get("id").and_then(|v| v.as_str()).unwrap_or("");
                    let tc_fn = tc.get("function").cloned().unwrap_or_else(|| json!({}));
                    let tc_name = tc_fn.get("name").and_then(|v| v.as_str()).unwrap_or("");
                    let tc_args = tc_fn.get("arguments").and_then(|v| v.as_str()).unwrap_or("{}");

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

            // Usage
            let usage = obj.get("usage").cloned().unwrap_or_else(|| json!({}));
            let input_tokens = usage.get("prompt_tokens").and_then(|v| v.as_i64()).unwrap_or(0);
            let output_tokens = usage.get("completion_tokens").and_then(|v| v.as_i64()).unwrap_or(0);
            let total_tokens = usage.get("total_tokens").and_then(|v| v.as_i64()).unwrap_or(input_tokens + output_tokens);

            let completed_response = json!({
                "id": &response_id,
                "object": "response",
                "created_at": created_at,
                "status": "completed",
                "error": null,
                "incomplete_details": null,
                "instructions": req_body.get("instructions"),
                "max_output_tokens": req_body.get("max_output_tokens"),
                "model": model,
                "output": output_items,
                "parallel_tool_calls": req_body.get("parallel_tool_calls").unwrap_or(&json!(true)),
                "previous_response_id": req_body.get("previous_response_id"),
                "reasoning": { "effort": null, "summary": null },
                "store": req_body.get("store").unwrap_or(&json!(true)),
                "temperature": req_body.get("temperature").unwrap_or(&json!(1.0)),
                "text": { "format": { "type": "text" } },
                "tool_choice": req_body.get("tool_choice").unwrap_or(&json!("auto")),
                "tools": req_body.get("tools").unwrap_or(&json!([])),
                "top_p": req_body.get("top_p").unwrap_or(&json!(1.0)),
                "truncation": req_body.get("truncation").unwrap_or(&json!("disabled")),
                "usage": {
                    "input_tokens": input_tokens,
                    "input_tokens_details": { "cached_tokens": 0 },
                    "output_tokens": output_tokens,
                    "output_tokens_details": { "reasoning_tokens": 0 },
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
