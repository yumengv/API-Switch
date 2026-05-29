use super::{join_url, ProtocolAdapter};
use axum::body::Body;
use bytes::Bytes;
use futures::StreamExt;
/// Anthropic (Claude) protocol adapter.
///
/// Converts between OpenAI format (external) and Anthropic native format (upstream).
/// - Endpoint: `v1/messages`
/// - Auth: `x-api-key` header
/// - Request/response body translation
/// - SSE streaming format conversion (Anthropic events → OpenAI chunks)
use serde_json::{json, Value};
use std::time::Duration;

/// 穿透开关：true = 未知字段保留穿透，false = 只保留已知白名单字段
///
/// 默认 true，贯彻「中转翻译器不丢信息」的公理。
/// 如果发现某个上游/客户端对未知字段返回 400，可临时改为 false 发布紧急版本。
const ENABLE_UNKNOWN_FIELD_PASSTHROUGH: bool = true;
const STREAM_IDLE_TIMEOUT: Duration = Duration::from_secs(300);

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

    // ─── 黑名单穿透：保留未知字段，仅丢弃已知 OpenAI 专有字段 ───────────
    // 决策（见 docs/protocol-passthrough-fix-plan.md §3.4/§5.2）：跨协议出口由
    // 硬白名单改为黑名单默认。白名单会误删目标协议新增的原生字段（如 Opus 4.8
    // 的新字段），而上游通常忽略未知字段——"漏删"比"漏放"更易致故障。故保留
    // 未知/未来字段穿透，仅剔除已知 OpenAI 专有字段（这些 Anthropic 不认）。
    // 注意：model/messages/max_tokens/system/tools/stream/temperature/top_p/top_k/
    // stop/tool_choice/thinking/reasoning_effort/parallel_tool_calls/user/
    // max_completion_tokens 已在前面 remove 并映射，不在此处。
    const OPENAI_SPECIFIC_DROP: &[&str] = &[
        // OpenAI Chat Completions 专有
        "frequency_penalty",
        "presence_penalty",
        "logit_bias",
        "logprobs",
        "top_logprobs",
        "n",
        "seed",
        "response_format",
        "stream_options",
        "function_call",
        "functions",
        "store",
        "modalities",
        "prediction",
        "audio",
        "service_tier",
        "reasoning",
        // OpenAI Responses API 专有
        "input",
        "instructions",
        "include",
        "prompt",
        "max_output_tokens",
        "text",
        "truncation",
        "previous_response_id",
        "max_tool_calls",
        "prompt_cache_key",
        "prompt_cache_retention",
        "safety_identifier",
        "client_metadata",
        // 内部字段
        "provider_specific",
        "__as_raw_claude_req",
        "__as_raw_responses_req",
    ];
    if let Value::Object(ref mut anthropic_obj) = anthropic {
        for (key, value) in obj.iter() {
            // 已显式映射的字段不覆盖；已知 OpenAI 专有字段丢弃；其余穿透。
            if anthropic_obj.contains_key(key) || OPENAI_SPECIFIC_DROP.contains(&key.as_str()) {
                continue;
            }
            anthropic_obj.insert(key.clone(), value.clone());
        }
    }

    *body = anthropic;
}

fn extract_chat_reasoning(value: &Value) -> Option<&str> {
    value
        .get("reasoning_text")
        .or_else(|| value.get("reasoning_content"))
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .or_else(|| {
            value
                .get("provider_specific")
                .and_then(|p| p.get("thinking"))
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
        })
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
            } else if anthropic_parts.len() == 1
                && anthropic_parts[0].get("type").and_then(|t| t.as_str()) == Some("text")
            {
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
    let Some(obj) = body.as_object() else {
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

    // 公理二：clone body 作为基底然后 edit-in-place，避免 json!({...}) 白名单构造
    // 丢掉上游 Claude 响应里的其他字段（container、stop_sequence、官方新增字段等）。
    let mut response_body = body.clone();
    if let Some(out) = response_body.as_object_mut() {
        // 改写/设置 OpenAI 特有字段
        out.insert(
            "id".to_string(),
            obj.get("id")
                .cloned()
                .unwrap_or_else(|| json!("chatcmpl-anthropic")),
        );
        out.insert("object".to_string(), json!("chat.completion"));
        out.insert("created".to_string(), json!(chrono::Utc::now().timestamp()));
        out.insert("model".to_string(), json!(model));
        out.insert("choices".to_string(), json!([choice]));
        out.insert("usage".to_string(), usage_json);

        // 移除 Claude 特有但 OpenAI chat.completion 不应出现的顶层字段
        out.remove("type"); // Claude 顶层 "type": "message"，OpenAI 用 "object"
        out.remove("role"); // OpenAI 把 role 塞在 choices[0].message.role
        out.remove("stop_reason"); // OpenAI 用 choices[0].finish_reason
        out.remove("stop_sequence"); // 同上
        out.remove("content"); // OpenAI 没有顶层 content 数组，全在 choices 里

        // 如果关了穿透，只保留 OpenAI 官方 chat.completion 已知字段
        if !ENABLE_UNKNOWN_FIELD_PASSTHROUGH {
            let openai_known: std::collections::HashSet<&str> = [
                "id",
                "object",
                "created",
                "model",
                "choices",
                "usage",
                "system_fingerprint",
                "service_tier",
                "provider_specific",
            ]
            .into_iter()
            .collect();
            out.retain(|k, _| openai_known.contains(k.as_str()));
        }
        // 否则（默认）其他字段（container、x_anthropic_future_field 等）自然保留
    }

    // Include thinking content if present
    if !thinking_parts.is_empty() {
        let thinking = thinking_parts.join("");
        response_body["provider_specific"] = json!({
            "thinking": thinking
        });
        if let Some(message) = response_body
            .get_mut("choices")
            .and_then(Value::as_array_mut)
            .and_then(|choices| choices.first_mut())
            .and_then(|choice| choice.get_mut("message"))
            .and_then(Value::as_object_mut)
        {
            message.insert("reasoning_text".to_string(), json!(thinking));
            message.insert("reasoning_content".to_string(), json!(thinking));
        }
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
                                        "delta": {
                                            "provider_specific": {"thinking": thinking},
                                            "reasoning_text": thinking,
                                            "reasoning_content": thinking
                                        },
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
                    let thinking = delta.get("thinking").and_then(|t| t.as_str()).unwrap_or("");
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
                                "delta": {
                                            "provider_specific": {"thinking": thinking},
                                            "reasoning_text": thinking,
                                            "reasoning_content": thinking
                                        },
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

// ═══════════════════════════════════════════════════════════════════
//  Public API: Claude <-> OpenAI format conversion (下游方向)
// ═══════════════════════════════════════════════════════════════════

/// Convert Claude request format to OpenAI format.
///
/// - system (top-level) -> first message with role: "system"
/// - max_tokens kept as-is (default 4096)
/// - Claude tools input_schema -> OpenAI parameters
/// - Pass through: model, stream, temperature, top_p
/// - tool_choice: mapped from Claude format (auto/any/none/tool) to OpenAI format
pub fn claude_to_openai_request(claude: &Value) -> Value {
    let mut messages = Vec::new();

    // Extract top-level system message
    if let Some(system) = claude.get("system") {
        let text = extract_text_from_content(system);
        if !text.is_empty() {
            messages.push(json!({"role": "system", "content": text}));
        }
    }

    // Convert Claude messages -> OpenAI messages
    if let Some(msgs) = claude.get("messages").and_then(|m| m.as_array()) {
        for msg in msgs {
            messages.extend(convert_claude_message_to_openai(msg));
        }
    }

    let mut openai = json!({
        "model": claude.get("model").and_then(|m| m.as_str()).unwrap_or(""),
        "messages": messages,
        "max_tokens": claude.get("max_tokens").cloned().unwrap_or(json!(4096)),
    });

    // Pass through common fields
    for field in &["stream", "temperature", "top_p", "top_k"] {
        if let Some(val) = claude.get(*field) {
            openai[*field] = val.clone();
        }
    }

    // metadata.user_id -> user (reverse mapping)
    if let Some(user) = claude
        .get("metadata")
        .and_then(|m| m.get("user_id"))
        .cloned()
    {
        openai["user"] = user;
    }

    // stop_sequences -> stop (Claude uses stop_sequences, OpenAI uses stop)
    if let Some(stop) = claude.get("stop_sequences") {
        openai["stop"] = stop.clone();
    } else if let Some(stop) = claude.get("stop") {
        openai["stop"] = stop.clone();
    }

    // tool_choice: Claude format -> OpenAI format
    if let Some(tc) = claude.get("tool_choice") {
        match tc {
            Value::Object(o) => {
                let disable_parallel = o
                    .get("disable_parallel_tool_use")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);

                let tc_type = o.get("type").and_then(|t| t.as_str()).unwrap_or("auto");
                let mapped = match tc_type {
                    "auto" => json!("auto"),
                    "any" => json!("required"),
                    "none" => json!("none"),
                    "tool" => {
                        if let Some(name) = o.get("name").and_then(|n| n.as_str()) {
                            json!({"type": "function", "function": {"name": name}})
                        } else {
                            json!("auto")
                        }
                    }
                    _ => json!("auto"),
                };
                openai["tool_choice"] = mapped;

                if disable_parallel {
                    openai["parallel_tool_calls"] = json!(false);
                }
            }
            _ => {
                openai["tool_choice"] = tc.clone();
            }
        }
    }

    // Convert Claude tools (input_schema -> parameters)
    if let Some(tools) = claude.get("tools").and_then(|t| t.as_array()) {
        let openai_tools: Vec<Value> = tools
            .iter()
            .filter_map(|tool| {
                let name = tool.get("name")?.as_str()?;
                let description = tool
                    .get("description")
                    .and_then(|d| d.as_str())
                    .unwrap_or("");
                let parameters = tool.get("input_schema").cloned().unwrap_or(json!({}));
                Some(json!({
                    "type": "function",
                    "function": {
                        "name": name,
                        "description": description,
                        "parameters": parameters,
                    }
                }))
            })
            .collect();

        if !openai_tools.is_empty() {
            openai["tools"] = json!(openai_tools);
        }
    }

    // 公理二：未知字段穿透。上面手动处理了所有已知字段，
    // 这里把 claude 顶层剩余的（未知/未来的）字段也带过去，避免"中转翻译器"丢信息。
    //
    // 规则：入口（A→OpenAI 中间协议）不做过滤，不能转换的一律穿透；
    // 非本协议字段的过滤抛弃在【出口】（中间→目标协议 B）进行，
    // 见各协议 transform_request 的 *_FOREIGN_DROP 黑名单。
    if ENABLE_UNKNOWN_FIELD_PASSTHROUGH {
        if let (Some(src), Some(dst)) = (claude.as_object(), openai.as_object_mut()) {
            for (key, value) in src {
                if !dst.contains_key(key) {
                    dst.insert(key.clone(), value.clone());
                }
            }
        }
    }

    openai
}

/// Convert OpenAI response format to Claude format.
///
/// - id -> add "msg_" prefix if missing
/// - choices[0].message.content (string) -> content: [{type: "text", text: ...}]
/// - choices[0].finish_reason -> stop_reason mapping: stop->end_turn, length->max_tokens, tool_calls->tool_use
/// - choices[0].message.tool_calls -> content tool_use blocks
/// - usage.prompt_tokens -> usage.input_tokens
/// - usage.completion_tokens -> usage.output_tokens
pub fn openai_to_claude_response(openai: &Value) -> Value {
    let id = openai
        .get("id")
        .and_then(|id| id.as_str())
        .unwrap_or("chatcmpl-anthropic");

    let claude_id = if id.starts_with("msg_") {
        id.to_string()
    } else {
        format!("msg_{id}")
    };

    let choice = openai.get("choices").and_then(|c| c.get(0));

    let message = choice.and_then(|c| c.get("message"));
    let content_str = message
        .and_then(|m| m.get("content"))
        .and_then(|c| c.as_str())
        .unwrap_or("");

    let tool_calls = message
        .and_then(|m| m.get("tool_calls"))
        .and_then(|tc| tc.as_array());
    let reasoning = message.and_then(extract_chat_reasoning);

    // Build content array
    let mut content = Vec::new();

    if let Some(reasoning) = reasoning {
        content.push(json!({
            "type": "thinking",
            "thinking": reasoning
        }));
    }

    if !content_str.is_empty() {
        content.push(json!({
            "type": "text",
            "text": content_str
        }));
    }

    if let Some(tcs) = tool_calls {
        for tc in tcs {
            let tc_id = tc.get("id").and_then(|i| i.as_str()).unwrap_or("");
            let name = tc
                .get("function")
                .and_then(|f| f.get("name"))
                .and_then(|n| n.as_str())
                .unwrap_or("");
            let arguments = tc
                .get("function")
                .and_then(|f| f.get("arguments"))
                .and_then(|a| a.as_str())
                .unwrap_or("{}");

            let input: Value = serde_json::from_str(arguments).unwrap_or(json!({}));

            content.push(json!({
                "type": "tool_use",
                "id": tc_id,
                "name": name,
                "input": input
            }));
        }
    }

    // Map finish_reason
    let finish_reason = choice
        .and_then(|c| c.get("finish_reason"))
        .and_then(|fr| fr.as_str())
        .unwrap_or("stop");

    let stop_reason = match finish_reason {
        "stop" => "end_turn",
        "length" => "max_tokens",
        "tool_calls" => "tool_use",
        other => other,
    };

    // Model
    let model = openai
        .get("model")
        .and_then(|m| m.as_str())
        .unwrap_or("claude");

    // Usage mapping (including cache fields per Anthropic spec)
    let usage = openai.get("usage").cloned().unwrap_or(json!({}));
    let input_tokens = usage
        .get("prompt_tokens")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let output_tokens = usage
        .get("completion_tokens")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let cache_read = usage
        .get("prompt_tokens_details")
        .and_then(|d| d.get("cached_tokens"))
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let cache_creation = usage
        .get("prompt_tokens_details")
        .and_then(|d| d.get("cache_creation_tokens"))
        .and_then(Value::as_i64)
        .unwrap_or(0);

    let mut usage_json = json!({
        "input_tokens": input_tokens,
        "output_tokens": output_tokens,
    });
    if cache_read > 0 || cache_creation > 0 {
        usage_json["cache_creation_input_tokens"] = json!(cache_creation);
        usage_json["cache_read_input_tokens"] = json!(cache_read);
    }

    // 公理二：clone openai 作为基底，然后 edit-in-place 改写 Claude 特有字段。
    // 避免白名单 json!({...}) 构造新对象把上游其他字段（已知但非白名单/未来新增）丢掉。
    let mut out = openai.clone();
    if let Some(obj) = out.as_object_mut() {
        // 改写/设置 Claude 特有字段
        obj.insert("id".to_string(), json!(claude_id));
        obj.insert("type".to_string(), json!("message"));
        obj.insert("role".to_string(), json!("assistant"));
        // model 字段 OpenAI 和 Claude 共用，用 openai 里的即可；但仍显式写一份保证值正确
        obj.insert("model".to_string(), json!(model));
        obj.insert("content".to_string(), json!(content));
        obj.insert("stop_reason".to_string(), json!(stop_reason));
        obj.insert("usage".to_string(), usage_json);

        // 移除 OpenAI / Gemini / Responses 特有但 Claude 不应出现的字段
        obj.remove("object"); // "chat.completion" 不是 Claude 语义
        obj.remove("choices"); // Claude 没有 choices 结构
        obj.remove("created"); // Claude 用 id 而不是时间戳
        obj.remove("system_fingerprint"); // OpenAI 特有
        obj.remove("candidates"); // Gemini 特有
        obj.remove("usageMetadata"); // Gemini 特有
        obj.remove("output"); // Responses 特有
        obj.remove("output_text"); // Responses 特有
        obj.remove("incomplete_details"); // Responses 特有
        obj.remove("instructions"); // Responses 特有
        obj.remove("parallel_tool_calls"); // OpenAI request/response 混入字段
        obj.remove("previous_response_id"); // Responses 特有
        obj.remove("text"); // Responses 特有
        obj.remove("truncation"); // Responses 特有

        // 如果关了穿透，只保留 Claude 官方文档已知字段
        if !ENABLE_UNKNOWN_FIELD_PASSTHROUGH {
            let claude_known: std::collections::HashSet<&str> = [
                "id",
                "type",
                "role",
                "model",
                "content",
                "stop_reason",
                "stop_sequence",
                "usage",
                "container",
            ]
            .into_iter()
            .collect();
            obj.retain(|k, _| claude_known.contains(k.as_str()));
        }
        // 否则（默认）保留所有其他字段作为 passthrough
    }
    out
}

/// Transform an error into Claude error format.
#[allow(dead_code)]
pub fn transform_claude_error(status: u16, message: &str) -> Value {
    let error_type = match status {
        400 => "invalid_request_error",
        401 => "authentication_error",
        403 => "permission_error",
        404 => "not_found_error",
        429 => "rate_limit_error",
        500..=599 => "api_error",
        _ => "api_error",
    };

    json!({
        "type": "error",
        "error": {
            "type": error_type,
            "message": message
        }
    })
}

// ═══════════════════════════════════════════════════════════════════
//  ClaudeSSETransformer: OpenAI SSE -> Claude SSE (下游方向)
// ═══════════════════════════════════════════════════════════════════

/// Transforms OpenAI streaming SSE chunks into Claude SSE format.
///
/// BUG FIX: Tracks `text_block_open` state to reuse the same content block
/// instead of creating a new one for every chunk.
pub struct ClaudeSSETransformer {
    message_id: String,
    model: String,
    started: bool,
    text_block_open: bool,
    thinking_block_open: bool,
    content_block_index: i64,
    in_tool_use: bool,
    tool_use_count: i64,
    usage_input_tokens: i64,
    usage_output_tokens: i64,
    /// 是否已经 emit 过 message_delta（用于 P1 修复：
    /// 当 OpenAI 先发 finish_reason 帧、再发 usage-only 帧时，
    /// 需要在 usage 真正到达后补发一次 message_delta，
    /// 否则 Claude 客户端看到的 output_tokens 永远是 0。
    /// Claude 协议允许 message_delta 多次出现。）
    message_delta_emitted: bool,
}

impl ClaudeSSETransformer {
    pub fn new(message_id: String, model: String) -> Self {
        Self {
            message_id,
            model,
            started: false,
            text_block_open: false,
            thinking_block_open: false,
            content_block_index: 0,
            in_tool_use: false,
            tool_use_count: 0,
            usage_input_tokens: 0,
            usage_output_tokens: 0,
            message_delta_emitted: false,
        }
    }

    /// Transform a single OpenAI SSE chunk into Claude SSE events.
    ///
    /// Returns a vector of JSON strings, each wrapped by the caller as `data: {event}\n\n`.
    pub fn transform_chunk(&mut self, openai_chunk: &str) -> Vec<String> {
        let mut events = Vec::new();

        let Ok(chunk) = serde_json::from_str::<Value>(openai_chunk) else {
            return events;
        };

        // Capture usage from the chunk if present (OpenAI stream_options.include_usage).
        //
        // NOTE: this block must run BEFORE the `choices=[]` early return below,
        // otherwise the standalone usage-only frame (帧 4 in the typical
        // OpenAI sequence: role / content / finish / usage-only / [DONE])
        // would be dropped and `self.usage_output_tokens` would stay at 0,
        // making every Claude `message_delta` report `output_tokens: 0` (P1 bug).
        if let Some(u) = chunk.get("usage") {
            let input = u.get("prompt_tokens").and_then(Value::as_i64).unwrap_or(0);
            let output = u
                .get("completion_tokens")
                .and_then(Value::as_i64)
                .unwrap_or(0);
            if input > 0 {
                self.usage_input_tokens = input;
            }
            if output > 0 {
                self.usage_output_tokens = output;
            }
        }

        // Emit message_start if this is the first chunk with role
        if let Some(delta) = chunk
            .get("choices")
            .and_then(|c| c.get(0))
            .and_then(|c| c.get("delta"))
        {
            if delta.get("role").is_some() && !self.started {
                self.started = true;
                events.push(
                    serde_json::to_string(&json!({
                        "type": "message_start",
                        "message": {
                            "id": self.message_id,
                            "type": "message",
                            "role": "assistant",
                            "content": [],
                            "model": self.model,
                            "stop_reason": Value::Null,
                            "usage": {
                                "input_tokens": self.usage_input_tokens,
                                "output_tokens": 0
                            }
                        }
                    }))
                    .unwrap_or_default(),
                );
            }
        }

        let Some(choice) = chunk.get("choices").and_then(|c| c.get(0)).cloned() else {
            // usage-only 帧（choices=[]）。如果 message_delta 已经 emit 过
            // 且 usage 已经更新，则补发一次 message_delta 让 Claude 客户端
            // 拿到真实的 output_tokens（Claude 协议允许 message_delta 多次）。
            if self.message_delta_emitted && self.usage_output_tokens > 0 {
                events.push(
                    serde_json::to_string(&json!({
                        "type": "message_delta",
                        "delta": {},
                        "usage": {
                            "output_tokens": self.usage_output_tokens
                        }
                    }))
                    .unwrap_or_default(),
                );
            }
            return events;
        };

        let delta = choice.get("delta").cloned().unwrap_or(json!({}));
        let finish_reason = choice.get("finish_reason").and_then(|fr| fr.as_str());

        if let Some(reasoning) = extract_chat_reasoning(&delta) {
            if !self.thinking_block_open {
                if self.text_block_open {
                    self.text_block_open = false;
                    events.push(
                        serde_json::to_string(&json!({
                            "type": "content_block_stop",
                            "index": self.content_block_index
                        }))
                        .unwrap_or_default(),
                    );
                    self.content_block_index += 1;
                }
                self.thinking_block_open = true;
                events.push(
                    serde_json::to_string(&json!({
                        "type": "content_block_start",
                        "index": self.content_block_index,
                        "content_block": {
                            "type": "thinking",
                            "thinking": ""
                        }
                    }))
                    .unwrap_or_default(),
                );
            }
            events.push(
                serde_json::to_string(&json!({
                    "type": "content_block_delta",
                    "index": self.content_block_index,
                    "delta": {
                        "type": "thinking_delta",
                        "thinking": reasoning
                    }
                }))
                .unwrap_or_default(),
            );
        }

        // Handle text content delta (THE FIX: reuse same text block)
        if let Some(content_val) = delta.get("content") {
            if let Value::String(text) = content_val {
                if !text.is_empty() {
                    if !self.text_block_open {
                        // Open text content block ONCE
                        self.text_block_open = true;
                        events.push(
                            serde_json::to_string(&json!({
                                "type": "content_block_start",
                                "index": self.content_block_index,
                                "content_block": {
                                    "type": "text",
                                    "text": ""
                                }
                            }))
                            .unwrap_or_default(),
                        );
                    }
                    // Emit text delta (multiple times in same block)
                    events.push(
                        serde_json::to_string(&json!({
                            "type": "content_block_delta",
                            "index": self.content_block_index,
                            "delta": {
                                "type": "text_delta",
                                "text": text
                            }
                        }))
                        .unwrap_or_default(),
                    );
                }
            }
        }

        // Handle tool call deltas
        if let Some(tool_calls) = delta.get("tool_calls").and_then(|tc| tc.as_array()) {
            // Close any open text/thinking block before tool_use blocks
            if self.text_block_open {
                self.text_block_open = false;
                events.push(
                    serde_json::to_string(&json!({
                        "type": "content_block_stop",
                        "index": self.content_block_index
                    }))
                    .unwrap_or_default(),
                );
                self.content_block_index += 1;
            } else if self.thinking_block_open {
                self.thinking_block_open = false;
                events.push(
                    serde_json::to_string(&json!({
                        "type": "content_block_stop",
                        "index": self.content_block_index
                    }))
                    .unwrap_or_default(),
                );
                self.content_block_index += 1;
            }

            for tc in tool_calls {
                let has_id = tc.get("id").is_some();

                if has_id {
                    // Close previous tool block if still open from a prior tool call
                    if self.in_tool_use {
                        events.push(
                            serde_json::to_string(&json!({
                                "type": "content_block_stop",
                                "index": self.content_block_index
                            }))
                            .unwrap_or_default(),
                        );
                        self.content_block_index += 1;
                    }

                    // Start new tool_use content block
                    self.in_tool_use = true;
                    self.tool_use_count += 1;

                    let tc_id = tc.get("id").and_then(|i| i.as_str()).unwrap_or("");
                    let name = tc
                        .get("function")
                        .and_then(|f| f.get("name"))
                        .and_then(|n| n.as_str())
                        .unwrap_or("");

                    events.push(
                        serde_json::to_string(&json!({
                            "type": "content_block_start",
                            "index": self.content_block_index,
                            "content_block": {
                                "type": "tool_use",
                                "id": tc_id,
                                "name": name,
                                "input": {}
                            }
                        }))
                        .unwrap_or_default(),
                    );
                }

                // Emit partial JSON arguments delta for the active tool block
                if let Some(arguments) = tc
                    .get("function")
                    .and_then(|f| f.get("arguments"))
                    .and_then(|a| a.as_str())
                {
                    if !arguments.is_empty() {
                        events.push(
                            serde_json::to_string(&json!({
                                "type": "content_block_delta",
                                "index": self.content_block_index,
                                "delta": {
                                    "type": "input_json_delta",
                                    "partial_json": arguments
                                }
                            }))
                            .unwrap_or_default(),
                        );
                    }
                }
            }
        }

        // Handle finish reason
        if let Some(fr) = finish_reason {
            // Close any open content blocks
            if self.text_block_open {
                self.text_block_open = false;
                events.push(
                    serde_json::to_string(&json!({
                        "type": "content_block_stop",
                        "index": self.content_block_index
                    }))
                    .unwrap_or_default(),
                );
                self.content_block_index += 1;
            } else if self.thinking_block_open {
                self.thinking_block_open = false;
                events.push(
                    serde_json::to_string(&json!({
                        "type": "content_block_stop",
                        "index": self.content_block_index
                    }))
                    .unwrap_or_default(),
                );
                self.content_block_index += 1;
            } else if self.in_tool_use {
                self.in_tool_use = false;
                events.push(
                    serde_json::to_string(&json!({
                        "type": "content_block_stop",
                        "index": self.content_block_index
                    }))
                    .unwrap_or_default(),
                );
                self.content_block_index += 1;
            }

            // Map finish_reason -> stop_reason
            let stop_reason = match fr {
                "stop" => "end_turn",
                "length" => "max_tokens",
                "tool_calls" => "tool_use",
                other => other,
            };

            // Build usage section for message_delta (per Claude protocol spec)
            let usage_json = if self.usage_output_tokens > 0 {
                json!({
                    "output_tokens": self.usage_output_tokens
                })
            } else {
                json!({
                    "output_tokens": 0
                })
            };

            events.push(
                serde_json::to_string(&json!({
                    "type": "message_delta",
                    "delta": {
                        "stop_reason": stop_reason,
                        "stop_sequence": Value::Null
                    },
                    "usage": usage_json
                }))
                .unwrap_or_default(),
            );
            self.message_delta_emitted = true;

            events.push(
                serde_json::to_string(&json!({
                    "type": "message_stop"
                }))
                .unwrap_or_default(),
            );
        }

        events
    }
}

// ═══════════════════════════════════════════════════════════════════
//  Private helpers for downstream conversion
// ═══════════════════════════════════════════════════════════════════

/// 把 OpenAI SSE 流转换成 Claude SSE 流。
///
/// 这属于 Claude 协议机的下游流式行为，不应留在 handler 中维护。
pub fn transform_openai_sse_to_claude_stream(
    response: axum::response::Response,
    requested_model: String,
) -> Result<axum::response::Response, crate::error::AppError> {
    let upstream_stream = response.into_body().into_data_stream();

    let message_id = format!("msg_{}", chrono::Utc::now().timestamp());
    let transformer = ClaudeSSETransformer::new(message_id, requested_model);
    let sse_buffer = String::new();
    let sse_utf8_remainder: Vec<u8> = Vec::new();

    let transformed_stream = futures::stream::unfold(
        (
            upstream_stream,
            transformer,
            sse_buffer,
            sse_utf8_remainder,
            0usize,
            Box::pin(tokio::time::sleep(STREAM_IDLE_TIMEOUT)),
        ),
        |(
            mut stream,
            mut transformer,
            mut sse_buffer,
            mut sse_utf8_remainder,
            mut streamed_bytes,
            mut idle_timeout,
        )| async move {
            loop {
                if crate::proxy::sse::stream_buffer_exceeded(
                    &sse_buffer,
                    &sse_utf8_remainder,
                    streamed_bytes,
                ) {
                    return Some((
                        Err(std::io::Error::new(
                            std::io::ErrorKind::Other,
                            "stream buffer exceeds 10MB limit",
                        )),
                        (
                            stream,
                            transformer,
                            sse_buffer,
                            sse_utf8_remainder,
                            streamed_bytes,
                            idle_timeout,
                        ),
                    ));
                }

                tokio::select! {
                    _ = &mut idle_timeout => {
                        return Some((
                            Err(std::io::Error::new(
                                std::io::ErrorKind::TimedOut,
                                "stream idle timeout",
                            )),
                            (
                                stream,
                                transformer,
                                sse_buffer,
                                sse_utf8_remainder,
                                streamed_bytes,
                                idle_timeout,
                            ),
                        ));
                    }
                    chunk_result = stream.next() => {
                        match chunk_result {
                            Some(Ok(chunk)) => {
                                idle_timeout.as_mut().reset(tokio::time::Instant::now() + STREAM_IDLE_TIMEOUT);
                                streamed_bytes += chunk.len();
                                crate::proxy::sse::append_utf8_safe(
                                    &mut sse_buffer,
                                    &mut sse_utf8_remainder,
                                    &chunk,
                                );
                            }
                            Some(Err(e)) => {
                                return Some((
                                    Err(std::io::Error::new(
                                        std::io::ErrorKind::Other,
                                        format!("Stream read error: {e}"),
                                    )),
                                    (
                                        stream,
                                        transformer,
                                        sse_buffer,
                                        sse_utf8_remainder,
                                        streamed_bytes,
                                        idle_timeout,
                                    ),
                                ));
                            }
                            None => {
                                return None;
                            }
                        }
                    }
                }

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
                                (
                                    stream,
                                    transformer,
                                    sse_buffer,
                                    sse_utf8_remainder,
                                    streamed_bytes,
                                    idle_timeout,
                                ),
                            ));
                        }

                        let events = transformer.transform_chunk(payload);
                        if !events.is_empty() {
                            let mut output = Vec::new();
                            for event in &events {
                                output.extend_from_slice(format!("data: {event}\n\n").as_bytes());
                            }
                            return Some((
                                Ok(Bytes::from(output)),
                                (
                                    stream,
                                    transformer,
                                    sse_buffer,
                                    sse_utf8_remainder,
                                    streamed_bytes,
                                    idle_timeout,
                                ),
                            ));
                        }
                    }
                    continue;
                }
            }
        },
    );

    axum::http::Response::builder()
        .status(axum::http::StatusCode::OK)
        .header("content-type", "text/event-stream")
        .header("cache-control", "no-cache")
        .header("connection", "keep-alive")
        .header("x-accel-buffering", "no")
        .body(Body::from_stream(transformed_stream))
        .map_err(|e| {
            crate::error::AppError::Internal(format!("Failed to build Claude SSE response: {e}"))
        })
}

/// Extract text from Claude content (string or array of text blocks).
fn extract_text_from_content(content: &Value) -> String {
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

/// Convert a single Claude message to one or more OpenAI messages.
///
/// Returns a Vec because Claude user messages with tool_result blocks
/// expand to multiple OpenAI messages (one `tool` role per result).
fn convert_claude_message_to_openai(msg: &Value) -> Vec<Value> {
    let role = msg.get("role").and_then(|r| r.as_str()).unwrap_or("user");

    match role {
        "assistant" => {
            let content = msg.get("content");

            // If content is just a string, return simple assistant message
            if let Some(Value::String(s)) = content {
                return vec![json!({
                    "role": "assistant",
                    "content": s
                })];
            }

            // If content is an array, extract text, tool_use, and image blocks
            if let Some(Value::Array(blocks)) = content {
                let mut text_parts = Vec::new();
                let mut tool_calls = Vec::new();
                let mut image_parts = Vec::new();

                for block in blocks {
                    match block.get("type").and_then(|t| t.as_str()) {
                        Some("text") => {
                            if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                                text_parts.push(text.to_string());
                            }
                        }
                        Some("tool_use") => {
                            let tc_id = block.get("id").and_then(|v| v.as_str()).unwrap_or("");
                            let name = block.get("name").and_then(|v| v.as_str()).unwrap_or("");
                            let input = block.get("input").cloned().unwrap_or(json!({}));
                            tool_calls.push(json!({
                                "id": tc_id,
                                "type": "function",
                                "function": {
                                    "name": name,
                                    "arguments": serde_json::to_string(&input).unwrap_or_default()
                                }
                            }));
                        }
                        Some("image") => {
                            let source = match block.get("source") {
                                Some(s) => s,
                                None => continue,
                            };
                            let source_type =
                                source.get("type").and_then(|t| t.as_str()).unwrap_or("");
                            let image_url = match source_type {
                                "base64" => {
                                    let media_type = source
                                        .get("media_type")
                                        .and_then(|t| t.as_str())
                                        .unwrap_or("application/octet-stream");
                                    let data =
                                        source.get("data").and_then(|d| d.as_str()).unwrap_or("");
                                    format!("data:{};base64,{}", media_type, data)
                                }
                                "url" => source
                                    .get("url")
                                    .and_then(|u| u.as_str())
                                    .unwrap_or("")
                                    .to_string(),
                                _ => continue,
                            };
                            if !image_url.is_empty() {
                                image_parts.push(json!({
                                    "type": "image_url",
                                    "image_url": { "url": image_url }
                                }));
                            }
                        }
                        _ => {}
                    }
                }

                let mut result = json!({"role": "assistant"});

                if image_parts.is_empty() {
                    // No images — simple string content (backward compatible)
                    if !text_parts.is_empty() {
                        result["content"] = json!(text_parts.join(""));
                    }
                } else {
                    // Has images — build structured content array
                    let mut content_parts: Vec<Value> = text_parts
                        .iter()
                        .map(|t| json!({"type": "text", "text": t}))
                        .collect();
                    content_parts.extend(image_parts);
                    result["content"] = json!(content_parts);
                }

                if !tool_calls.is_empty() {
                    result["tool_calls"] = json!(tool_calls);
                }

                return vec![result];
            }

            vec![json!({"role": "assistant", "content": Value::Null})]
        }
        "user" | "human" => {
            let content = msg.get("content");

            // Check for array content with tool_result blocks
            if let Some(Value::Array(blocks)) = content {
                let mut tool_results = Vec::new();
                let mut text_parts = Vec::new();
                let mut image_parts = Vec::new();

                for block in blocks {
                    match block.get("type").and_then(|t| t.as_str()) {
                        Some("tool_result") => {
                            let tool_use_id = block
                                .get("tool_use_id")
                                .and_then(|v| v.as_str())
                                .unwrap_or("");
                            let result_content = block
                                .get("content")
                                .map(extract_text_from_content)
                                .unwrap_or_default();
                            tool_results.push(json!({
                                "role": "tool",
                                "tool_call_id": tool_use_id,
                                "content": result_content
                            }));
                        }
                        Some("text") => {
                            if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                                text_parts.push(text.to_string());
                            }
                        }
                        Some("image") => {
                            let source = match block.get("source") {
                                Some(s) => s,
                                None => continue,
                            };
                            let source_type =
                                source.get("type").and_then(|t| t.as_str()).unwrap_or("");
                            let image_url = match source_type {
                                "base64" => {
                                    let media_type = source
                                        .get("media_type")
                                        .and_then(|t| t.as_str())
                                        .unwrap_or("application/octet-stream");
                                    let data =
                                        source.get("data").and_then(|d| d.as_str()).unwrap_or("");
                                    format!("data:{};base64,{}", media_type, data)
                                }
                                "url" => source
                                    .get("url")
                                    .and_then(|u| u.as_str())
                                    .unwrap_or("")
                                    .to_string(),
                                _ => continue,
                            };
                            if !image_url.is_empty() {
                                image_parts.push(json!({
                                    "type": "image_url",
                                    "image_url": { "url": image_url }
                                }));
                            }
                        }
                        _ => {}
                    }
                }

                let mut messages = Vec::new();

                if image_parts.is_empty() {
                    // No images — simple string content
                    if !text_parts.is_empty() {
                        messages.push(json!({"role": "user", "content": text_parts.join("")}));
                    }
                } else {
                    // Has images — build structured content array
                    let mut content_parts: Vec<Value> = text_parts
                        .iter()
                        .map(|t| json!({"type": "text", "text": t}))
                        .collect();
                    content_parts.extend(image_parts);
                    if !content_parts.is_empty() {
                        messages.push(json!({"role": "user", "content": content_parts}));
                    }
                }

                messages.extend(tool_results);

                if messages.is_empty() {
                    messages.push(json!({"role": "user", "content": ""}));
                }

                return messages;
            }

            // Simple string content
            vec![json!({
                "role": "user",
                "content": extract_text_from_content(content.unwrap_or(&json!("")))
            })]
        }
        _ => {
            vec![json!({
                "role": role,
                "content": extract_text_from_content(msg.get("content").unwrap_or(&json!("")))
            })]
        }
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
        assert_eq!(
            body["tool_choice"],
            json!({"type": "tool", "name": "get_weather"})
        );
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
        // 输出边界：不属于 Claude 协议的字段必须被剔除
        assert!(body.get("frequency_penalty").is_none());
        assert!(body.get("presence_penalty").is_none());
        assert!(body.get("seed").is_none());
        assert!(body.get("logit_bias").is_none());
        assert!(body.get("logprobs").is_none());
        assert!(body.get("n").is_none());
        assert!(body.get("top_logprobs").is_none());
        assert!(body.get("service_tier").is_none());
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

    // ─── claude_to_openai tests (downstream) ─────────────────────────────

    #[test]
    fn basic_claude_to_openai() {
        let claude = json!({
            "model": "claude-3-sonnet-20240229",
            "max_tokens": 1024,
            "messages": [
                {"role": "user", "content": "Hello"}
            ]
        });

        let openai = claude_to_openai_request(&claude);

        assert_eq!(openai["model"], "claude-3-sonnet-20240229");
        assert_eq!(openai["max_tokens"], 1024);
        assert_eq!(openai["messages"][0]["role"], "user");
        assert_eq!(openai["messages"][0]["content"], "Hello");
    }

    #[test]
    fn claude_to_openai_with_system() {
        let claude = json!({
            "model": "claude-3-sonnet-20240229",
            "system": "You are a helpful assistant.",
            "messages": [
                {"role": "user", "content": "Hello"}
            ]
        });

        let openai = claude_to_openai_request(&claude);

        assert_eq!(openai["messages"][0]["role"], "system");
        assert_eq!(
            openai["messages"][0]["content"],
            "You are a helpful assistant."
        );
        assert_eq!(openai["messages"][1]["role"], "user");
        assert_eq!(openai["messages"][1]["content"], "Hello");
    }

    #[test]
    fn claude_to_openai_with_tools() {
        let claude = json!({
            "model": "claude-3-sonnet-20240229",
            "messages": [
                {"role": "user", "content": "What is the weather in Tokyo?"}
            ],
            "tools": [
                {
                    "name": "get_weather",
                    "description": "Get weather for a city",
                    "input_schema": {
                        "type": "object",
                        "properties": {
                            "city": {"type": "string"}
                        },
                        "required": ["city"]
                    }
                }
            ]
        });

        let openai = claude_to_openai_request(&claude);

        assert_eq!(openai["tools"][0]["type"], "function");
        assert_eq!(openai["tools"][0]["function"]["name"], "get_weather");
        assert_eq!(
            openai["tools"][0]["function"]["description"],
            "Get weather for a city"
        );
        assert_eq!(
            openai["tools"][0]["function"]["parameters"]["type"],
            "object"
        );
        assert_eq!(
            openai["tools"][0]["function"]["parameters"]["properties"]["city"]["type"],
            "string"
        );
    }

    // ─── openai_to_claude tests (downstream) ─────────────────────────────

    #[test]
    fn basic_openai_to_claude() {
        let openai = json!({
            "id": "chatcmpl-abc123",
            "object": "chat.completion",
            "model": "claude-3-sonnet-20240229",
            "choices": [
                {
                    "index": 0,
                    "message": {
                        "role": "assistant",
                        "content": "Hello! How can I help?"
                    },
                    "finish_reason": "stop"
                }
            ],
            "usage": {
                "prompt_tokens": 10,
                "completion_tokens": 8,
                "total_tokens": 18
            }
        });

        let claude = openai_to_claude_response(&openai);

        assert!(claude["id"].as_str().unwrap().starts_with("msg_"));
        assert_eq!(claude["type"], "message");
        assert_eq!(claude["role"], "assistant");
        assert_eq!(claude["model"], "claude-3-sonnet-20240229");
        assert_eq!(claude["content"][0]["type"], "text");
        assert_eq!(claude["content"][0]["text"], "Hello! How can I help?");
        assert_eq!(claude["stop_reason"], "end_turn");
        assert_eq!(claude["usage"]["input_tokens"], 10);
        assert_eq!(claude["usage"]["output_tokens"], 8);
    }

    #[test]
    fn openai_to_claude_max_tokens() {
        let openai = json!({
            "id": "chatcmpl-def456",
            "model": "claude-3-opus-20240229",
            "choices": [
                {
                    "message": {
                        "role": "assistant",
                        "content": "This is a long response..."
                    },
                    "finish_reason": "length"
                }
            ],
            "usage": {"prompt_tokens": 100, "completion_tokens": 2048}
        });

        let claude = openai_to_claude_response(&openai);

        assert_eq!(claude["stop_reason"], "max_tokens");
        assert_eq!(claude["usage"]["input_tokens"], 100);
        assert_eq!(claude["usage"]["output_tokens"], 2048);
    }

    #[test]
    fn openai_to_claude_with_tool_calls() {
        let openai = json!({
            "id": "chatcmpl-ghi789",
            "model": "claude-3-opus-20240229",
            "choices": [
                {
                    "message": {
                        "role": "assistant",
                        "content": "Let me check the weather.",
                        "tool_calls": [
                            {
                                "id": "call_abc123",
                                "type": "function",
                                "function": {
                                    "name": "get_weather",
                                    "arguments": "{\"city\": \"Tokyo\"}"
                                }
                            }
                        ]
                    },
                    "finish_reason": "tool_calls"
                }
            ],
            "usage": {"prompt_tokens": 50, "completion_tokens": 25}
        });

        let claude = openai_to_claude_response(&openai);

        assert_eq!(claude["stop_reason"], "tool_use");

        let content = claude["content"].as_array().unwrap();
        let text_block = content.iter().find(|b| b["type"] == "text").unwrap();
        assert_eq!(text_block["text"], "Let me check the weather.");

        let tool_block = content.iter().find(|b| b["type"] == "tool_use").unwrap();
        assert_eq!(tool_block["id"], "call_abc123");
        assert_eq!(tool_block["name"], "get_weather");
        assert_eq!(tool_block["input"]["city"], "Tokyo");
    }

    // ─── transform_claude_error tests ───────────────────────────────

    #[test]
    fn claude_error_400() {
        let error = transform_claude_error(400, "Invalid request format");
        assert_eq!(error["type"], "error");
        assert_eq!(error["error"]["type"], "invalid_request_error");
        assert_eq!(error["error"]["message"], "Invalid request format");
    }

    #[test]
    fn claude_error_401() {
        let error = transform_claude_error(401, "API key is invalid");
        assert_eq!(error["type"], "error");
        assert_eq!(error["error"]["type"], "authentication_error");
        assert_eq!(error["error"]["message"], "API key is invalid");
    }

    #[test]
    fn claude_error_429() {
        let error = transform_claude_error(429, "Rate limit exceeded");
        assert_eq!(error["type"], "error");
        assert_eq!(error["error"]["type"], "rate_limit_error");
        assert_eq!(error["error"]["message"], "Rate limit exceeded");
    }

    // ─── SSE transformer tests (downstream) ──────────────────────────────

    #[test]
    fn sse_first_chunk_message_start() {
        let mut transformer =
            ClaudeSSETransformer::new("msg_test".to_string(), "claude-3-opus".to_string());

        let chunk = r#"{"id":"chatcmpl-abc","choices":[{"delta":{"role":"assistant"},"finish_reason":null}]}"#;
        let events = transformer.transform_chunk(chunk);
        assert!(!events.is_empty());

        let first: Value = serde_json::from_str(&events[0]).unwrap();
        assert_eq!(first["type"], "message_start");
        assert_eq!(first["message"]["id"], "msg_test");
        assert_eq!(first["message"]["role"], "assistant");
        assert_eq!(first["message"]["model"], "claude-3-opus");
    }

    #[test]
    fn sse_text_content_delta() {
        let mut transformer =
            ClaudeSSETransformer::new("msg_test".to_string(), "claude-3-opus".to_string());

        // First text chunk: emits content_block_start + content_block_delta
        let chunk1 = r#"{"id":"chatcmpl-abc","choices":[{"delta":{"content":"Hello"},"finish_reason":null}]}"#;
        let events1 = transformer.transform_chunk(chunk1);
        assert_eq!(events1.len(), 2);

        let start: Value = serde_json::from_str(&events1[0]).unwrap();
        assert_eq!(start["type"], "content_block_start");
        assert_eq!(start["index"], 0);
        assert_eq!(start["content_block"]["type"], "text");

        let delta1: Value = serde_json::from_str(&events1[1]).unwrap();
        assert_eq!(delta1["type"], "content_block_delta");
        assert_eq!(delta1["index"], 0);
        assert_eq!(delta1["delta"]["type"], "text_delta");
        assert_eq!(delta1["delta"]["text"], "Hello");

        // Second text chunk: ONLY content_block_delta (no new block start!)
        let chunk2 = r#"{"id":"chatcmpl-abc","choices":[{"delta":{"content":" world"},"finish_reason":null}]}"#;
        let events2 = transformer.transform_chunk(chunk2);
        assert_eq!(events2.len(), 1);

        let delta2: Value = serde_json::from_str(&events2[0]).unwrap();
        assert_eq!(delta2["type"], "content_block_delta");
        assert_eq!(delta2["index"], 0);
        assert_eq!(delta2["delta"]["type"], "text_delta");
        assert_eq!(delta2["delta"]["text"], " world");

        // Third text chunk: still only content_block_delta
        let chunk3 =
            r#"{"id":"chatcmpl-abc","choices":[{"delta":{"content":"!"},"finish_reason":null}]}"#;
        let events3 = transformer.transform_chunk(chunk3);
        assert_eq!(events3.len(), 1);

        let delta3: Value = serde_json::from_str(&events3[0]).unwrap();
        assert_eq!(delta3["type"], "content_block_delta");
        assert_eq!(delta3["delta"]["text"], "!");
    }

    #[test]
    fn sse_finish_reason_end_turn() {
        let mut transformer =
            ClaudeSSETransformer::new("msg_test".to_string(), "claude-3-opus".to_string());

        // Text content
        let chunk1 =
            r#"{"id":"chatcmpl-abc","choices":[{"delta":{"content":"Hi"},"finish_reason":null}]}"#;
        transformer.transform_chunk(chunk1);

        // Finish
        let chunk2 = r#"{"id":"chatcmpl-abc","choices":[{"delta":{},"finish_reason":"stop"}]}"#;
        let events = transformer.transform_chunk(chunk2);

        // Should have: content_block_stop, message_delta, message_stop
        assert_eq!(events.len(), 3);

        let block_stop: Value = serde_json::from_str(&events[0]).unwrap();
        assert_eq!(block_stop["type"], "content_block_stop");
        assert_eq!(block_stop["index"], 0);

        let msg_delta: Value = serde_json::from_str(&events[1]).unwrap();
        assert_eq!(msg_delta["type"], "message_delta");
        assert_eq!(msg_delta["delta"]["stop_reason"], "end_turn");

        let msg_stop: Value = serde_json::from_str(&events[2]).unwrap();
        assert_eq!(msg_stop["type"], "message_stop");

        // text_block_open should be reset
        assert!(!transformer.text_block_open);
    }

    #[test]
    fn sse_finish_reason_tool_use() {
        let mut transformer =
            ClaudeSSETransformer::new("msg_test".to_string(), "claude-3-opus".to_string());

        // Text content
        let chunk1 = r#"{"id":"chatcmpl-abc","choices":[{"delta":{"content":"Let me check"},"finish_reason":null}]}"#;
        transformer.transform_chunk(chunk1);

        // Tool call start (closes text block, opens tool_use block)
        let chunk2 = r#"{"id":"chatcmpl-abc","choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_123","type":"function","function":{"name":"get_weather","arguments":""}}]},"finish_reason":null}]}"#;
        let events2 = transformer.transform_chunk(chunk2);

        let has_stop = events2.iter().any(|e| {
            let v: Value = serde_json::from_str(e).unwrap();
            v["type"] == "content_block_stop"
        });
        let has_tool_start = events2.iter().any(|e| {
            let v: Value = serde_json::from_str(e).unwrap();
            v["type"] == "content_block_start" && v["content_block"]["type"] == "tool_use"
        });
        assert!(has_stop, "Should have content_block_stop for text block");
        assert!(
            has_tool_start,
            "Should have content_block_start for tool_use"
        );

        // Tool call arguments
        let chunk3 = r#"{"id":"chatcmpl-abc","choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"{\"city\":"}}]},"finish_reason":null}]}"#;
        let events3 = transformer.transform_chunk(chunk3);
        assert!(!events3.is_empty());

        let arg_delta: Value = serde_json::from_str(&events3[0]).unwrap();
        assert_eq!(arg_delta["type"], "content_block_delta");
        assert_eq!(arg_delta["delta"]["type"], "input_json_delta");

        // Finish
        let chunk4 =
            r#"{"id":"chatcmpl-abc","choices":[{"delta":{},"finish_reason":"tool_calls"}]}"#;
        let events4 = transformer.transform_chunk(chunk4);

        let has_tool_stop = events4.iter().any(|e| {
            let v: Value = serde_json::from_str(e).unwrap();
            v["type"] == "content_block_stop"
        });
        let has_msg_delta = events4.iter().any(|e| {
            let v: Value = serde_json::from_str(e).unwrap();
            v["type"] == "message_delta" && v["delta"]["stop_reason"] == "tool_use"
        });
        let has_msg_stop = events4.iter().any(|e| {
            let v: Value = serde_json::from_str(e).unwrap();
            v["type"] == "message_stop"
        });

        assert!(has_tool_stop, "Should have content_block_stop for tool_use");
        assert!(
            has_msg_delta,
            "Should have message_delta with stop_reason=tool_use"
        );
        assert!(has_msg_stop, "Should have message_stop");
    }

    // ─── tool_choice reverse mapping tests (downstream) ──────────────────

    #[test]
    fn test_tool_choice_auto_reverse() {
        let claude = json!({
            "model": "claude-3-sonnet-20240229",
            "messages": [{"role": "user", "content": "Hi"}],
            "tool_choice": {"type": "auto"}
        });
        let openai = claude_to_openai_request(&claude);
        assert_eq!(openai["tool_choice"], "auto");
        assert!(openai.get("parallel_tool_calls").is_none());
    }

    #[test]
    fn test_tool_choice_any_reverse() {
        let claude = json!({
            "model": "claude-3-sonnet-20240229",
            "messages": [{"role": "user", "content": "Hi"}],
            "tool_choice": {"type": "any"}
        });
        let openai = claude_to_openai_request(&claude);
        assert_eq!(openai["tool_choice"], "required");
        assert!(openai.get("parallel_tool_calls").is_none());
    }

    #[test]
    fn test_tool_choice_none_reverse() {
        let claude = json!({
            "model": "claude-3-sonnet-20240229",
            "messages": [{"role": "user", "content": "Hi"}],
            "tool_choice": {"type": "none"}
        });
        let openai = claude_to_openai_request(&claude);
        assert_eq!(openai["tool_choice"], "none");
        assert!(openai.get("parallel_tool_calls").is_none());
    }

    #[test]
    fn test_tool_choice_tool_reverse() {
        let claude = json!({
            "model": "claude-3-sonnet-20240229",
            "messages": [{"role": "user", "content": "Hi"}],
            "tool_choice": {"type": "tool", "name": "get_weather"}
        });
        let openai = claude_to_openai_request(&claude);
        assert_eq!(openai["tool_choice"]["type"], "function");
        assert_eq!(openai["tool_choice"]["function"]["name"], "get_weather");
        assert!(openai.get("parallel_tool_calls").is_none());
    }

    #[test]
    fn test_tool_choice_disable_parallel() {
        let claude = json!({
            "model": "claude-3-sonnet-20240229",
            "messages": [{"role": "user", "content": "Hi"}],
            "tool_choice": {"type": "auto", "disable_parallel_tool_use": true}
        });
        let openai = claude_to_openai_request(&claude);
        assert_eq!(openai["tool_choice"], "auto");
        assert_eq!(openai["parallel_tool_calls"], false);
    }

    #[test]
    fn test_tool_choice_passthrough_string() {
        let claude = json!({
            "model": "claude-3-sonnet-20240229",
            "messages": [{"role": "user", "content": "Hi"}],
            "tool_choice": "auto"
        });
        let openai = claude_to_openai_request(&claude);
        assert_eq!(openai["tool_choice"], "auto");
    }

    #[test]
    fn openai_to_claude_request_drops_unsupported_fields() {
        let mut body = json!({
            "model": "auto",
            "messages": [{"role": "user", "content": "hi"}],
            "max_tokens": 100,
            "response_format": {"type": "json_object"},
            "logit_bias": {"123": 1},
            "logprobs": true,
            "top_logprobs": 2,
            "n": 2,
            "seed": 123,
            "stream_options": {"include_usage": true},
            "parallel_tool_calls": true,
            "service_tier": "auto",
            "prompt_cache_key": "cache",
            "safety_identifier": "safe",
            "input": "wrong",
            "instructions": "wrong",
            "include": ["wrong"],
            "prompt": {"id": "pmpt_1"},
            "max_output_tokens": 10,
            "text": {"format": {"type": "text"}},
            "truncation": "auto",
            "previous_response_id": "resp_1",
            "max_tool_calls": 2
        });

        transform_request_to_anthropic(&mut body, "claude-3-5-sonnet");

        assert!(body.get("messages").is_some());
        assert!(body.get("max_tokens").is_some());
        assert!(body.get("response_format").is_none());
        assert!(body.get("logit_bias").is_none());
        assert!(body.get("logprobs").is_none());
        assert!(body.get("top_logprobs").is_none());
        assert!(body.get("n").is_none());
        assert!(body.get("seed").is_none());
        assert!(body.get("stream_options").is_none());
        assert!(body.get("parallel_tool_calls").is_none());
        assert!(body.get("service_tier").is_none());
        assert!(body.get("prompt_cache_key").is_none());
        assert!(body.get("safety_identifier").is_none());
        assert!(body.get("input").is_none());
        assert!(body.get("instructions").is_none());
        assert!(body.get("include").is_none());
        assert!(body.get("prompt").is_none());
        assert!(body.get("max_output_tokens").is_none());
        assert!(body.get("text").is_none());
        assert!(body.get("truncation").is_none());
        assert!(body.get("previous_response_id").is_none());
        assert!(body.get("max_tool_calls").is_none());
    }

    /// 规则验证：Claude→OpenAI【入口】不过滤，不能转换的字段一律穿透到中间态；
    /// 非本协议字段的剔除在【出口】各协议 transform_request 完成。
    #[test]
    fn claude_to_openai_passes_through_unconvertible_fields() {
        let claude = json!({
            "model": "claude-opus-4-8",
            "max_tokens": 100,
            "thinking": {"type": "enabled", "budget_tokens": 4096},
            "context_management": {"edits": []},
            "mcp_servers": [{"name": "x"}],
            "messages": [{"role": "user", "content": "hi"}],
            "x_future_field": "keep_me"
        });

        let openai = claude_to_openai_request(&claude);

        // 入口穿透：不能转换的 Claude 字段保留在中间态（由出口黑名单负责剔除）
        assert!(openai.get("thinking").is_some(), "入口应穿透 thinking");
        assert!(openai.get("context_management").is_some());
        assert!(openai.get("mcp_servers").is_some());
        assert_eq!(openai["x_future_field"], "keep_me");
    }

    #[test]
    fn openai_to_claude_response_drops_foreign_fields() {
        let openai = json!({
            "id": "chatcmpl_1",
            "object": "chat.completion",
            "created": 123,
            "model": "claude-3-5-sonnet",
            "choices": [{"index": 0, "message": {"role": "assistant", "content": "hi"}, "finish_reason": "stop"}],
            "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2},
            "system_fingerprint": "fp_1",
            "candidates": [],
            "output": [],
            "x_anthropic_future_field": "keep"
        });

        let claude = openai_to_claude_response(&openai);

        assert_eq!(claude["type"], "message");
        assert!(claude.get("content").is_some());
        assert!(claude.get("object").is_none());
        assert!(claude.get("choices").is_none());
        assert!(claude.get("created").is_none());
        assert!(claude.get("system_fingerprint").is_none());
        assert!(claude.get("candidates").is_none());
        assert!(claude.get("output").is_none());
        assert_eq!(claude["x_anthropic_future_field"], "keep");
    }

    // ─── image reverse conversion tests (downstream) ─────────────────────

    #[test]
    fn test_claude_base64_image_to_openai() {
        let claude = json!({
            "model": "claude-3-sonnet-20240229",
            "messages": [{
                "role": "user",
                "content": [
                    {"type": "text", "text": "describe"},
                    {"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": "abc123"}}
                ]
            }]
        });
        let openai = claude_to_openai_request(&claude);
        let msg = &openai["messages"][0];
        let content = msg["content"].as_array().expect("content should be array");
        assert_eq!(content.len(), 2);
        assert_eq!(content[0]["type"], "text");
        assert_eq!(content[0]["text"], "describe");
        assert_eq!(content[1]["type"], "image_url");
        assert_eq!(
            content[1]["image_url"]["url"],
            "data:image/png;base64,abc123"
        );
    }

    #[test]
    fn test_claude_url_image_to_openai() {
        let claude = json!({
            "model": "claude-3-sonnet-20240229",
            "messages": [{
                "role": "user",
                "content": [
                    {"type": "image", "source": {"type": "url", "url": "https://example.com/img.png"}}
                ]
            }]
        });
        let openai = claude_to_openai_request(&claude);
        let msg = &openai["messages"][0];
        let content = msg["content"].as_array().expect("content should be array");
        assert_eq!(content[0]["type"], "image_url");
        assert_eq!(
            content[0]["image_url"]["url"],
            "https://example.com/img.png"
        );
    }
}
