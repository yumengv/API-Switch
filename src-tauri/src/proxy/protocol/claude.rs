use super::{join_url, ProtocolAdapter};
/// Anthropic (Claude) protocol adapter.
///
/// Converts between OpenAI format (external) and Anthropic native format (upstream).
/// - Endpoint: `v1/messages`
/// - Auth: `x-api-key` header
/// - Request/response body translation
/// - SSE streaming format conversion (Anthropic events → OpenAI chunks)
use serde_json::{json, Value};

pub struct ClaudeAdapter;

impl ProtocolAdapter for ClaudeAdapter {
    fn build_chat_url(&self, base_url: &str, _model: &str) -> String {
        join_url(base_url, "v1/messages")
    }

    fn build_models_url(&self, base_url: &str, _api_key: &str) -> String {
        join_url(base_url, "v1/models")
    }

    fn uses_query_auth(&self) -> bool {
        false
    }

    fn build_auth_headers(&self, api_key: &str) -> Vec<(String, String)> {
        vec![
            ("x-api-key".to_string(), api_key.to_string()),
            ("anthropic-version".to_string(), "2023-06-01".to_string()),
            (
                "anthropic-dangerous-direct-browser-access".to_string(),
                "true".to_string(),
            ),
        ]
    }

    fn apply_auth(
        &self,
        builder: reqwest::RequestBuilder,
        api_key: &str,
    ) -> reqwest::RequestBuilder {
        builder
            .header("x-api-key", api_key)
            .header("anthropic-version", "2023-06-01")
            .header("anthropic-dangerous-direct-browser-access", "true")
    }

    fn transform_request(&self, body: &mut Value, actual_model: &str) {
        transform_request_to_anthropic(body, actual_model);
    }

    fn transform_response(&self, body: &mut Value) {
        transform_response_from_anthropic(body);
    }

    fn needs_sse_transform(&self) -> bool {
        true
    }

    fn extract_sse_usage(&self, data_line: &str) -> (i64, i64) {
        if data_line == "[DONE]" {
            return (0, 0);
        }
        let Ok(value) = serde_json::from_str::<Value>(data_line) else {
            return (0, 0);
        };
        let prompt = value
            .get("usage")
            .and_then(|u| u.get("input_tokens"))
            .and_then(Value::as_i64)
            .unwrap_or(0);
        let completion = value
            .get("usage")
            .and_then(|u| u.get("output_tokens"))
            .and_then(Value::as_i64)
            .unwrap_or(0);
        (prompt, completion)
    }

    fn transform_sse_line(&self, data_line: &str) -> Option<String> {
        transform_anthropic_sse_line(data_line)
    }

    fn parse_models_response(&self, body: &Value) -> Vec<(String, Option<String>)> {
        // Anthropic format: { data: [{ id, display_name }] }
        body.get("data")
            .and_then(|d| d.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|m| {
                        let id = m.get("id")?.as_str()?.to_string();
                        // Anthropic uses "display_name", not "owned_by"
                        let owned_by = m
                            .get("display_name")
                            .and_then(|v| v.as_str())
                            .map(String::from)
                            .or_else(|| {
                                m.get("owned_by").and_then(|v| v.as_str()).map(String::from)
                            });
                        Some((id, owned_by))
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
}

// ==================== Anthropic-specific implementation ====================

fn transform_request_to_anthropic(body: &mut Value, actual_model: &str) {
    let Some(obj) = body.as_object_mut() else {
        return;
    };

    // Extract system message (as content block array for Claude 4.5+ compatibility)
    let mut system_parts: Vec<Value> = Vec::new();
    let mut messages = Vec::new();

    if let Some(msgs) = obj
        .remove("messages")
        .and_then(|v| v.as_array().cloned())
        .map(|v| v.into_iter().collect::<Vec<_>>())
    {
        for msg in msgs {
            let role = msg.get("role").and_then(|r| r.as_str()).unwrap_or("user");
            match role {
                "system" => {
                    if let Some(content) = msg.get("content") {
                        let text = extract_text_content(content);
                        if !text.is_empty() {
                            system_parts.push(json!({
                                "type": "text",
                                "text": text
                            }));
                        }
                    }
                }
                _ => {
                    messages.push(convert_message_to_anthropic(&msg));
                }
            }
        }
    }

    // Build Anthropic request
    // max_completion_tokens takes precedence over max_tokens (OpenAI new param name)
    let max_tokens = obj
        .remove("max_completion_tokens")
        .or_else(|| obj.remove("max_tokens"))
        .unwrap_or(json!(4096));

    let mut anthropic = json!({
        "model": actual_model,
        "messages": messages,
        "max_tokens": max_tokens,
    });

    if !system_parts.is_empty() {
        anthropic["system"] = json!(system_parts);
    }

    // Handle tools / function calling
    if let Some(tools) = obj.remove("tools") {
        anthropic["tools"] = convert_tools_to_anthropic(&tools);
    }

    // Pass through common fields
    for field in ["stream", "temperature", "top_p", "top_k"] {
        if let Some(val) = obj.remove(field) {
            anthropic[field] = val;
        }
    }

    // stop → stop_sequences
    if let Some(stop) = obj.remove("stop") {
        anthropic["stop_sequences"] = stop;
    }

    // tool_choice → Anthropic format
    if let Some(tc) = obj.remove("tool_choice") {
        let mapped = match &tc {
            Value::String(s) => match s.as_str() {
                "auto" => json!({"type": "auto"}),
                "required" => json!({"type": "any"}),
                "none" => json!({"type": "none"}),
                _ => json!({"type": "auto"}),
            },
            Value::Object(o) => {
                if let Some(func_name) = o
                    .get("function")
                    .and_then(|f| f.get("name"))
                    .and_then(|n| n.as_str())
                {
                    json!({"type": "tool", "name": func_name})
                } else if o.get("type").and_then(|t| t.as_str()) == Some("required") {
                    json!({"type": "any"})
                } else {
                    json!({"type": o.get("type").and_then(|t| t.as_str()).unwrap_or("auto")})
                }
            }
            _ => json!({"type": "auto"}),
        };
        anthropic["tool_choice"] = mapped;
    }

    // parallel_tool_calls → disable_parallel_tool_use (logic is inverted)
    if let Some(parallel) = obj.remove("parallel_tool_calls") {
        if parallel == json!(false) {
            if let Some(tc_obj) = anthropic.get_mut("tool_choice") {
                if let Value::Object(ref mut tc_map) = tc_obj {
                    tc_map.insert("disable_parallel_tool_use".to_string(), json!(true));
                }
            }
        }
    }

    // reasoning_effort → thinking config
    if let Some(effort) = obj.remove("reasoning_effort") {
        let budget = match effort.as_str().unwrap_or("medium") {
            "minimal" => 1024,
            "low" => 2048,
            "medium" => 10000,
            "high" => 32768,
            _ => 10000,
        };
        anthropic["thinking"] = json!({"type": "enabled", "budget_tokens": budget});
    }
    // Direct thinking passthrough (some clients set it directly)
    if let Some(thinking) = obj.remove("thinking") {
        anthropic["thinking"] = thinking;
    }

    // response_format handling (check BEFORE removing unsupported fields)
    let json_format = obj
        .get("response_format")
        .and_then(|f| f.get("type"))
        .and_then(|t| t.as_str());

    match json_format {
        Some("json_schema") => {
            // json_schema → add JSON instruction to system prompt
            if !system_parts.is_empty() {
                system_parts.push(json!({"type": "text", "text": ""}));
            }
            system_parts.push(json!({
                "type": "text",
                "text": "You must respond with valid JSON only. No markdown fences, no explanation — pure JSON."
            }));
            anthropic["system"] = json!(system_parts);
        }
        Some("json_object") => {
            // json_object → add JSON instruction to system prompt
            if !system_parts.is_empty() {
                system_parts.push(json!({"type": "text", "text": ""}));
            }
            system_parts.push(json!({
                "type": "text",
                "text": "You must respond with valid JSON only. No markdown fences, no explanation — pure JSON."
            }));
            anthropic["system"] = json!(system_parts);
        }
        _ => {}
    }

    // user → metadata.user_id
    if let Some(user) = obj.remove("user") {
        if let Value::Object(ref mut meta) = anthropic["metadata"] {
            meta.insert("user_id".to_string(), user);
        } else {
            anthropic["metadata"] = json!({ "user_id": user });
        }
    }

    // Pass through all remaining fields (pure relay)
    if let Value::Object(ref mut anthropic_obj) = anthropic {
        for (key, value) in obj.iter() {
            if !anthropic_obj.contains_key(key) {
                anthropic_obj.insert(key.clone(), value.clone());
            }
        }
    }

    *body = anthropic;
}

fn extract_text_content(content: &Value) -> String {
    match content {
        Value::String(s) => s.clone(),
        Value::Array(arr) => arr
            .iter()
            .filter_map(|part| {
                if part.get("type")?.as_str()? == "text" {
                    part.get("text")?.as_str().map(String::from)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join(""),
        _ => String::new(),
    }
}

fn convert_message_to_anthropic(msg: &Value) -> Value {
    let role = msg.get("role").and_then(|r| r.as_str()).unwrap_or("user");
    let content = msg.get("content");

    let anthropic_role = if role == "assistant" {
        "assistant"
    } else {
        "user"
    };

    match content {
        None => {
            // No content — but may still have tool_calls (assistant)
            if role == "assistant" {
                if let Some(tool_calls) = msg.get("tool_calls").and_then(|v| v.as_array()) {
                    let tool_use_parts: Vec<Value> = tool_calls
                        .iter()
                        .filter_map(|tc| {
                            let fn_body = tc.get("function")?;
                            Some(json!({
                                "type": "tool_use",
                                "id": tc.get("id")?.as_str()?,
                                "name": fn_body.get("name")?.as_str()?,
                                "input": serde_json::from_str::<Value>(
                                    fn_body.get("arguments")?.as_str()?
                                ).ok()?
                            }))
                        })
                        .collect();
                    if tool_use_parts.is_empty() {
                        json!({"role": "assistant", "content": ""})
                    } else {
                        json!({"role": "assistant", "content": tool_use_parts})
                    }
                } else {
                    json!({"role": anthropic_role, "content": ""})
                }
            } else {
                json!({"role": anthropic_role, "content": ""})
            }
        }
        Some(Value::String(s)) => {
            // String content — assistant may also have tool_calls
            if role == "assistant" {
                if let Some(tool_calls) = msg.get("tool_calls").and_then(|v| v.as_array()) {
                    let tool_use_parts: Vec<Value> = tool_calls
                        .iter()
                        .filter_map(|tc| {
                            let fn_body = tc.get("function")?;
                            Some(json!({
                                "type": "tool_use",
                                "id": tc.get("id")?.as_str()?,
                                "name": fn_body.get("name")?.as_str()?,
                                "input": serde_json::from_str::<Value>(
                                    fn_body.get("arguments")?.as_str()?
                                ).ok()?
                            }))
                        })
                        .collect();
                    let mut all_parts = vec![json!({"type": "text", "text": s})];
                    all_parts.extend(tool_use_parts);
                    json!({"role": "assistant", "content": all_parts})
                } else {
                    json!({"role": anthropic_role, "content": s.clone()})
                }
            } else if role == "tool" {
                // tool result with string content → Anthropic tool_result format
                let tool_use_id = msg
                    .get("tool_call_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                json!({
                    "role": "user",
                    "content": json!([{
                        "type": "tool_result",
                        "tool_use_id": tool_use_id,
                        "content": s
                    }])
                })
            } else {
                json!({"role": anthropic_role, "content": s.clone()})
            }
        }
        Some(Value::Array(arr)) => {
            let anthropic_parts: Vec<Value> = arr
                .iter()
                .filter_map(|part| {
                    let part_type = part.get("type")?.as_str()?;
                    match part_type {
                        "text" => Some(json!({
                            "type": "text",
                            "text": part.get("text")?.as_str()?
                        })),
                        "image_url" => {
                            let url = part.get("image_url")?.get("url")?.as_str()?;
                            if let Some(data) = url.strip_prefix("data:") {
                                let parts: Vec<&str> = data.splitn(2, ";base64,").collect();
                                if parts.len() == 2 {
                                    return Some(json!({
                                        "type": "image",
                                        "source": {
                                            "type": "base64",
                                            "media_type": parts[0],
                                            "data": parts[1]
                                        }
                                    }));
                                }
                            } else if url.starts_with("http://") || url.starts_with("https://") {
                                // Claude 4+ supports URL source directly
                                return Some(json!({
                                    "type": "image",
                                    "source": {
                                        "type": "url",
                                        "url": url
                                    }
                                }));
                            }
                            None
                        }
                        "tool_calls" | "tool_call_id" => None, // handled separately below
                        _ => None,
                    }
                })
                .collect();

            // Handle tool_calls in assistant messages
            if role == "assistant" {
                if let Some(tool_calls) = msg.get("tool_calls").and_then(|v| v.as_array()) {
                    let tool_use_parts: Vec<Value> = tool_calls
                        .iter()
                        .filter_map(|tc| {
                            let fn_body = tc.get("function")?;
                            Some(json!({
                                "type": "tool_use",
                                "id": tc.get("id")?.as_str()?,
                                "name": fn_body.get("name")?.as_str()?,
                                "input": serde_json::from_str::<Value>(
                                    fn_body.get("arguments")?.as_str()?
                                ).ok()?
                            }))
                        })
                        .collect();

                    let mut all_parts = anthropic_parts;
                    all_parts.extend(tool_use_parts);
                    return json!({"role": "assistant", "content": all_parts});
                }
            }

            // Handle tool result messages
            if role == "tool" {
                let tool_use_id = msg
                    .get("tool_call_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let text = content.map(|c| extract_text_content(c)).unwrap_or_default();
                return json!({
                    "role": "user",
                    "content": json!([{
                        "type": "tool_result",
                        "tool_use_id": tool_use_id,
                        "content": text
                    }])
                });
            }

            if anthropic_parts.is_empty() {
                json!({"role": anthropic_role, "content": ""})
            } else if anthropic_parts.len() == 1 && anthropic_parts[0].get("type").and_then(|t| t.as_str()) == Some("text") {
                // Single text block → collapse to string for backward compat
                json!({"role": anthropic_role, "content": anthropic_parts[0]["text"].clone()})
            } else {
                json!({"role": anthropic_role, "content": anthropic_parts})
            }
        }
        _ => json!({"role": anthropic_role, "content": ""}),
    }
}

fn convert_tools_to_anthropic(openai_tools: &Value) -> Value {
    let Some(tools_arr) = openai_tools.as_array() else {
        return json!([]);
    };

    let anthropic_tools: Vec<Value> = tools_arr
        .iter()
        .filter_map(|tool| {
            let func = tool.get("function")?;
            let name = func.get("name")?.as_str()?;
            let description = func
                .get("description")
                .and_then(|d| d.as_str())
                .unwrap_or("");
            let parameters = func.get("parameters").cloned().unwrap_or(json!({}));

            Some(json!({
                "name": name,
                "description": description,
                "input_schema": parameters
            }))
        })
        .collect();

    json!(anthropic_tools)
}

fn transform_response_from_anthropic(body: &mut Value) {
    let Some(obj) = body.as_object_mut() else {
        return;
    };

    let role = obj
        .get("role")
        .and_then(|r| r.as_str())
        .unwrap_or("assistant");
    let stop_reason = obj
        .get("stop_reason")
        .and_then(|r| r.as_str())
        .unwrap_or("end_turn");
    let model = obj
        .get("model")
        .and_then(|m| m.as_str())
        .unwrap_or("claude");

    // Build message content
    let content = obj.get("content").and_then(|c| c.as_array());
    let mut tool_calls = Vec::new();
    let mut text_parts = Vec::new();
    let mut thinking_parts = Vec::new();

    if let Some(content_arr) = content {
        for block in content_arr {
            match block.get("type").and_then(|t| t.as_str()) {
                Some("text") => {
                    if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                        text_parts.push(text.to_string());
                    }
                }
                Some("tool_use") => {
                    tool_calls.push(json!({
                        "id": block.get("id"),
                        "type": "function",
                        "function": {
                            "name": block.get("name"),
                            "arguments": serde_json::to_string(
                                block.get("input").unwrap_or(&json!({}))
                            ).unwrap_or_default()
                        }
                    }));
                }
                Some("thinking") => {
                    if let Some(thinking) = block.get("thinking").and_then(|t| t.as_str()) {
                        thinking_parts.push(thinking.to_string());
                    }
                }
                _ => {}
            }
        }
    }

    let finish_reason = match stop_reason {
        "end_turn" => "stop",
        "max_tokens" => "length",
        "tool_use" => "tool_calls",
        "stop_sequence" => "stop",
        _ => stop_reason,
    };

    let message = json!({
        "role": role,
        "content": text_parts.join("")
    });

    let mut choice = json!({
        "index": 0,
        "message": message,
        "finish_reason": finish_reason,
    });

    if !tool_calls.is_empty() {
        choice["message"]["tool_calls"] = json!(tool_calls);
    }

    // Usage: Anthropic uses input_tokens/output_tokens → OpenAI prompt_tokens/completion_tokens
    let usage = obj.get("usage");
    let input_tokens = usage
        .and_then(|u| u.get("input_tokens"))
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let output_tokens = usage
        .and_then(|u| u.get("output_tokens"))
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let cache_read = usage
        .and_then(|u| u.get("cache_read_input_tokens"))
        .and_then(Value::as_i64)
        .unwrap_or(0);

    let mut usage_json = json!({
        "prompt_tokens": input_tokens,
        "completion_tokens": output_tokens,
        "total_tokens": input_tokens + output_tokens,
    });
    if cache_read > 0 {
        usage_json["prompt_tokens_details"] = json!({
            "cached_tokens": cache_read
        });
    }

    let mut response_body = json!({
        "id": obj.get("id").cloned().unwrap_or_else(|| json!("chatcmpl-anthropic")),
        "object": "chat.completion",
        "created": chrono::Utc::now().timestamp(),
        "model": model,
        "choices": [choice],
        "usage": usage_json,
    });

    // Include thinking content if present
    if !thinking_parts.is_empty() {
        response_body["provider_specific"] = json!({
            "thinking": thinking_parts.join("")
        });
    }

    *body = response_body;
}

/// Transform a single Anthropic SSE event data line into an OpenAI chunk.
/// Returns None to drop the line.
fn transform_anthropic_sse_line(data_line: &str) -> Option<String> {
    if data_line == "[DONE]" {
        return Some("[DONE]".to_string());
    }

    let Ok(value) = serde_json::from_str::<Value>(data_line) else {
        return None;
    };

    let event_type = value.get("type").and_then(|t| t.as_str()).unwrap_or("");

    match event_type {
        "message_start" => {
            if let Some(message) = value.get("message") {
                let model = message
                    .get("model")
                    .and_then(|m| m.as_str())
                    .unwrap_or("claude");
                let id = message
                    .get("id")
                    .and_then(|i| i.as_str())
                    .unwrap_or("chatcmpl-anthropic");
            return Some(
                serde_json::to_string(&json!({
                    "id": id,
                    "object": "chat.completion.chunk",
                    "created": chrono::Utc::now().timestamp(),
                    "model": model,
                    "choices": [{
                        "index": 0,
                        "delta": {"role": "assistant", "content": ""},
                        "finish_reason": null
                    }]
                }))
                .unwrap_or_default(),
            );
            }
            None
        }
        "content_block_start" => {
            let index = value.get("index").and_then(|i| i.as_i64()).unwrap_or(0);
            if let Some(content_block) = value.get("content_block") {
                let block_type = content_block
                    .get("type")
                    .and_then(|t| t.as_str())
                    .unwrap_or("");
                match block_type {
                    "text" => {
                        let text = content_block
                            .get("text")
                            .and_then(|t| t.as_str())
                            .unwrap_or("");
                if !text.is_empty() {
                    return Some(
                        serde_json::to_string(&json!({
                            "id": "chatcmpl-anthropic",
                            "object": "chat.completion.chunk",
                            "created": chrono::Utc::now().timestamp(),
                            "model": "claude",
                            "choices": [{
                                "index": index,
                                "delta": {"role": "assistant", "content": text},
                                "finish_reason": null
                            }]
                        }))
                        .unwrap_or_default(),
                    );
                }
                // Empty first text chunk — still emit the role
                Some(
                    serde_json::to_string(&json!({
                        "id": "chatcmpl-anthropic",
                        "object": "chat.completion.chunk",
                        "created": chrono::Utc::now().timestamp(),
                        "model": "claude",
                        "choices": [{
                            "index": index,
                            "delta": {},
                            "finish_reason": null
                        }]
                    }))
                    .unwrap_or_default(),
                )
                    }
            "tool_use" => {
                let id = content_block
                    .get("id")
                    .and_then(|i| i.as_str())
                    .unwrap_or("");
                let name = content_block
                    .get("name")
                    .and_then(|n| n.as_str())
                    .unwrap_or("");
                Some(
                    serde_json::to_string(&json!({
                        "id": "chatcmpl-anthropic",
                        "object": "chat.completion.chunk",
                        "created": chrono::Utc::now().timestamp(),
                        "model": "claude",
                        "choices": [{
                            "index": 0,
                            "delta": {
                                "role": "assistant",
                                "tool_calls": [{
                                    "index": index,
                                    "id": id,
                                    "type": "function",
                                    "function": {"name": name, "arguments": ""}
                                }]
                            },
                            "finish_reason": null
                        }]
                    }))
                    .unwrap_or_default(),
                )
                    }
            "thinking" => {
                // Thinking block started — emit as provider_specific delta
                let thinking = content_block
                    .get("thinking")
                    .and_then(|t| t.as_str())
                    .unwrap_or("");
                if !thinking.is_empty() {
                    return Some(
                        serde_json::to_string(&json!({
                            "id": "chatcmpl-anthropic",
                            "object": "chat.completion.chunk",
                            "created": chrono::Utc::now().timestamp(),
                            "model": "claude",
                            "choices": [{
                                "index": index,
                                "delta": {"provider_specific": {"thinking": thinking}},
                                "finish_reason": null
                            }]
                        }))
                        .unwrap_or_default(),
                    );
                }
                None
            }
                    _ => None,
                }
            } else {
                None
            }
        }
        "content_block_delta" => {
            let index = value.get("index").and_then(|i| i.as_i64()).unwrap_or(0);
            let delta = value.get("delta").cloned().unwrap_or_else(|| json!({}));
            let delta_type = delta.get("type").and_then(|t| t.as_str()).unwrap_or("");

            match delta_type {
            "text_delta" => {
                let text = delta.get("text").and_then(|t| t.as_str()).unwrap_or("");
                if text.is_empty() {
                    return None;
                }
                Some(
                    serde_json::to_string(&json!({
                        "id": "chatcmpl-anthropic",
                        "object": "chat.completion.chunk",
                        "created": chrono::Utc::now().timestamp(),
                        "model": "claude",
                        "choices": [{
                            "index": index,
                            "delta": {"content": text},
                            "finish_reason": null
                        }]
                    }))
                    .unwrap_or_default(),
                )
                }
            "input_json_delta" => {
                let partial_json = delta
                    .get("partial_json")
                    .and_then(|t| t.as_str())
                    .unwrap_or("");
                if partial_json.is_empty() {
                    return None;
                }
                Some(
                    serde_json::to_string(&json!({
                        "id": "chatcmpl-anthropic",
                        "object": "chat.completion.chunk",
                        "created": chrono::Utc::now().timestamp(),
                        "model": "claude",
                        "choices": [{
                            "index": 0,
                            "delta": {
                                "tool_calls": [{
                                    "index": index,
                                    "function": {"arguments": partial_json}
                                }]
                            },
                            "finish_reason": null
                        }]
                    }))
                    .unwrap_or_default(),
                )
                }
            "thinking_delta" => {
                let thinking = delta
                    .get("thinking")
                    .and_then(|t| t.as_str())
                    .unwrap_or("");
                if thinking.is_empty() {
                    return None;
                }
                Some(
                    serde_json::to_string(&json!({
                        "id": "chatcmpl-anthropic",
                        "object": "chat.completion.chunk",
                        "created": chrono::Utc::now().timestamp(),
                        "model": "claude",
                        "choices": [{
                            "index": index,
                            "delta": {"provider_specific": {"thinking": thinking}},
                            "finish_reason": null
                        }]
                    }))
                    .unwrap_or_default(),
                )
                }
                _ => None,
            }
        }
        "content_block_stop" => None,
        "message_delta" => {
            let stop_reason = value
                .get("delta")
                .and_then(|d| d.get("stop_reason"))
                .and_then(|r| r.as_str())
                .unwrap_or("");

            let finish_reason: Value = match stop_reason {
                "end_turn" => json!("stop"),
                "max_tokens" => json!("length"),
                "tool_use" => json!("tool_calls"),
                "stop_sequence" => json!("stop"),
                s if s.is_empty() => Value::Null,
                _ => json!(stop_reason),
            };

            // Check for usage
            let usage = value.get("usage");
            let mut chunk = json!({
                "id": "chatcmpl-anthropic",
                "object": "chat.completion.chunk",
                "created": chrono::Utc::now().timestamp(),
                "model": "claude",
                "choices": [{
                    "index": 0,
                    "delta": {},
                    "finish_reason": finish_reason
                }]
            });

            if let Some(u) = usage {
                chunk["usage"] = json!({
                    "prompt_tokens": u.get("input_tokens").and_then(Value::as_i64).unwrap_or(0),
                    "completion_tokens": u.get("output_tokens").and_then(Value::as_i64).unwrap_or(0),
                    "total_tokens": u.get("input_tokens").and_then(Value::as_i64).unwrap_or(0)
                        + u.get("output_tokens").and_then(Value::as_i64).unwrap_or(0),
                });
            }

            Some(serde_json::to_string(&chunk).unwrap_or_default())
        }
        "message_stop" => Some("[DONE]".to_string()),
        "ping" => None,
        "error" => {
            let error_info = value
                .get("error")
                .cloned()
                .unwrap_or_else(|| json!({"message": "unknown error"}));
            Some(
                serde_json::to_string(&json!({
                    "id": "chatcmpl-anthropic",
                    "object": "chat.completion.chunk",
                    "created": chrono::Utc::now().timestamp(),
                    "model": "claude",
                    "choices": [{
                        "index": 0,
                        "delta": {},
                        "finish_reason": "stop"
                    }],
                    "error": error_info
                }))
                .unwrap_or_default(),
            )
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ─── tool_choice mapping tests (Phase 1.1) ────────────────────────

    #[test]
    fn test_tool_choice_auto() {
        let mut body = json!({
            "model": "claude-3-sonnet-20240229",
            "messages": [{"role": "user", "content": "Hi"}],
            "tool_choice": "auto"
        });
        transform_request_to_anthropic(&mut body, "claude-3-sonnet-20240229");
        assert_eq!(body["tool_choice"], json!({"type": "auto"}));
    }

    #[test]
    fn test_tool_choice_required() {
        let mut body = json!({
            "model": "claude-3-sonnet-20240229",
            "messages": [{"role": "user", "content": "Hi"}],
            "tool_choice": "required"
        });
        transform_request_to_anthropic(&mut body, "claude-3-sonnet-20240229");
        assert_eq!(body["tool_choice"], json!({"type": "any"}));
    }

    #[test]
    fn test_tool_choice_none() {
        let mut body = json!({
            "model": "claude-3-sonnet-20240229",
            "messages": [{"role": "user", "content": "Hi"}],
            "tool_choice": "none"
        });
        transform_request_to_anthropic(&mut body, "claude-3-sonnet-20240229");
        assert_eq!(body["tool_choice"], json!({"type": "none"}));
    }

    #[test]
    fn test_tool_choice_named_function() {
        let mut body = json!({
            "model": "claude-3-sonnet-20240229",
            "messages": [{"role": "user", "content": "Hi"}],
            "tool_choice": {"type": "function", "function": {"name": "get_weather"}}
        });
        transform_request_to_anthropic(&mut body, "claude-3-sonnet-20240229");
        assert_eq!(body["tool_choice"], json!({"type": "tool", "name": "get_weather"}));
    }

    #[test]
    fn test_tool_choice_with_parallel_false() {
        let mut body = json!({
            "model": "claude-3-sonnet-20240229",
            "messages": [{"role": "user", "content": "Hi"}],
            "tool_choice": "auto",
            "parallel_tool_calls": false
        });
        transform_request_to_anthropic(&mut body, "claude-3-sonnet-20240229");
        assert_eq!(body["tool_choice"]["type"], "auto");
        assert_eq!(body["tool_choice"]["disable_parallel_tool_use"], true);
    }

    // ─── passthrough param tests ─────────────────────────────────────

    #[test]
    fn test_params_passthrough() {
        let mut body = json!({
            "model": "claude-3-sonnet-20240229",
            "messages": [{"role": "user", "content": "Hi"}],
            "frequency_penalty": 0.5,
            "presence_penalty": 0.3,
            "seed": 42,
            "logit_bias": {"1": 100},
            "logprobs": true,
            "n": 2,
            "response_format": {"type": "json_object"},
            "top_logprobs": 5,
            "service_tier": "auto"
        });
        transform_request_to_anthropic(&mut body, "claude-3-sonnet-20240229");
        // All fields should be passed through (pure relay)
        assert!(body.get("frequency_penalty").is_some());
        assert!(body.get("presence_penalty").is_some());
        assert!(body.get("seed").is_some());
        assert!(body.get("logit_bias").is_some());
        assert!(body.get("logprobs").is_some());
        assert!(body.get("n").is_some());
        assert!(body.get("top_logprobs").is_some());
        assert!(body.get("service_tier").is_some());
        // core fields must survive
        assert!(body.get("model").is_some());
        assert!(body.get("messages").is_some());
    }

    // ─── stop_sequences mapping ───────────────────────────────────────

    #[test]
    fn test_stop_to_stop_sequences() {
        let mut body = json!({
            "model": "claude-3-sonnet-20240229",
            "messages": [{"role": "user", "content": "Hi"}],
            "stop": ["END", "STOP"]
        });
        transform_request_to_anthropic(&mut body, "claude-3-sonnet-20240229");
        assert_eq!(body["stop_sequences"], json!(["END", "STOP"]));
        assert!(body.get("stop").is_none());
    }

    // ─── image URL support ─────────────────────────────────────────────

    #[test]
    fn test_http_image_url_to_anthropic() {
        let mut body = json!({
            "model": "claude-3-sonnet-20240229",
            "messages": [{
                "role": "user",
                "content": [
                    {"type": "text", "text": "describe this"},
                    {"type": "image_url", "image_url": {"url": "https://example.com/photo.png"}}
                ]
            }]
        });
        transform_request_to_anthropic(&mut body, "claude-3-sonnet-20240229");
        let msgs = body["messages"].as_array().unwrap();
        let content = msgs[0]["content"].as_array().unwrap();
        let img = content.iter().find(|b| b["type"] == "image").unwrap();
        assert_eq!(img["source"]["type"], "url");
        assert_eq!(img["source"]["url"], "https://example.com/photo.png");
    }

    #[test]
    fn test_base64_image_still_works() {
        let mut body = json!({
            "model": "claude-3-sonnet-20240229",
            "messages": [{
                "role": "user",
                "content": [
                    {"type": "image_url", "image_url": {"url": "data:image/png;base64,abc123"}}
                ]
            }]
        });
        transform_request_to_anthropic(&mut body, "claude-3-sonnet-20240229");
        let msgs = body["messages"].as_array().unwrap();
        let content = msgs[0]["content"].as_array().unwrap();
        let img = content.iter().find(|b| b["type"] == "image").unwrap();
        assert_eq!(img["source"]["type"], "base64");
        assert_eq!(img["source"]["data"], "abc123");
    }

    // ─── user → metadata.user_id (2.3) ────────────────────────────────

    #[test]
    fn test_user_to_metadata_user_id() {
        let mut body = json!({
            "model": "claude-3-sonnet-20240229",
            "messages": [{"role": "user", "content": "Hi"}],
            "user": "user-123"
        });
        transform_request_to_anthropic(&mut body, "claude-3-sonnet-20240229");
        assert_eq!(body["metadata"]["user_id"], "user-123");
        assert!(body.get("user").is_none());
    }

    // ─── reasoning_effort → thinking (2.4) ─────────────────────────────

    #[test]
    fn test_reasoning_effort_high() {
        let mut body = json!({
            "model": "claude-3-sonnet-20240229",
            "messages": [{"role": "user", "content": "Hi"}],
            "reasoning_effort": "high"
        });
        transform_request_to_anthropic(&mut body, "claude-3-sonnet-20240229");
        assert_eq!(body["thinking"]["type"], "enabled");
        assert_eq!(body["thinking"]["budget_tokens"], 32768);
    }

    #[test]
    fn test_reasoning_effort_low() {
        let mut body = json!({
            "model": "claude-3-sonnet-20240229",
            "messages": [{"role": "user", "content": "Hi"}],
            "reasoning_effort": "low"
        });
        transform_request_to_anthropic(&mut body, "claude-3-sonnet-20240229");
        assert_eq!(body["thinking"]["type"], "enabled");
        assert_eq!(body["thinking"]["budget_tokens"], 2048);
    }

    #[test]
    fn test_thinking_passthrough() {
        let mut body = json!({
            "model": "claude-3-sonnet-20240229",
            "messages": [{"role": "user", "content": "Hi"}],
            "thinking": {"type": "enabled", "budget_tokens": 5000}
        });
        transform_request_to_anthropic(&mut body, "claude-3-sonnet-20240229");
        assert_eq!(body["thinking"]["budget_tokens"], 5000);
    }

    // ─── max_completion_tokens alias (3.1) ─────────────────────────────

    #[test]
    fn test_max_completion_tokens_alias() {
        let mut body = json!({
            "model": "claude-3-sonnet-20240229",
            "messages": [{"role": "user", "content": "Hi"}],
            "max_completion_tokens": 2048
        });
        transform_request_to_anthropic(&mut body, "claude-3-sonnet-20240229");
        assert_eq!(body["max_tokens"], 2048);
    }

    #[test]
    fn test_max_completion_tokens_precedence() {
        let mut body = json!({
            "model": "claude-3-sonnet-20240229",
            "messages": [{"role": "user", "content": "Hi"}],
            "max_tokens": 1024,
            "max_completion_tokens": 4096
        });
        transform_request_to_anthropic(&mut body, "claude-3-sonnet-20240229");
        assert_eq!(body["max_tokens"], 4096);
    }

    // ─── response_format fallback (3.4) ────────────────────────────────

    #[test]
    fn test_response_format_json_object_adds_system_instruction() {
        let mut body = json!({
            "model": "claude-3-sonnet-20240229",
            "messages": [
                {"role": "system", "content": "You are a helper."},
                {"role": "user", "content": "Hi"}
            ],
            "response_format": {"type": "json_object"}
        });
        transform_request_to_anthropic(&mut body, "claude-3-sonnet-20240229");
        let system = body["system"].as_array().unwrap();
        let system_text: String = system
            .iter()
            .filter_map(|block| block.get("text").and_then(|t| t.as_str()))
            .collect::<Vec<_>>()
            .join("");
        assert!(system_text.contains("You are a helper."));
        assert!(system_text.contains("valid JSON only"));
    }
}
