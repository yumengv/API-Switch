use super::{join_url, ProtocolAdapter};
/// Google Gemini protocol adapter.
///
/// Google provides an **OpenAI-compatible endpoint** at `/v1beta/openai/`:
///   - Chat: `{base_url}/v1beta/openai/chat/completions`
///   - Models: `{base_url}/v1beta/openai/models`
///   - Auth: `?key=<API_KEY>` query parameter
///
/// Through this endpoint, the request/response body is standard OpenAI format.
/// No body transformation is needed.
///
/// **Important**: The user should set `base_url` to
/// `https://generativelanguage.googleapis.com` (without any version path).
/// The adapter appends the full `/v1beta/openai/...` path.
///
/// References:
/// - https://ai.google.dev/gemini-api/docs/openai
/// - https://ai.google.dev/api/models
use serde_json::{json, Value};

/// 穿透开关：true = 未知字段保留穿透，false = 只保留已知白名单字段
///
/// 默认 true，贯彻「中转翻译器不丢信息」的公理。
/// 如果发现某个上游/客户端对未知字段返回 400，可临时改为 false 发布紧急版本。
///
/// 注：GeminiAdapter 通过 Google OpenAI-compatible endpoint 直通，body 不翻译，
/// 此常量为阶段 3 预留，阶段 2 暂未使用。
#[allow(dead_code)]
const ENABLE_UNKNOWN_FIELD_PASSTHROUGH: bool = true;

pub struct GeminiAdapter;

impl ProtocolAdapter for GeminiAdapter {
    fn build_chat_url(&self, base_url: &str, _model: &str) -> String {
        // Google's OpenAI-compatible endpoint
        join_url(base_url, "v1beta/openai/chat/completions")
    }

    fn build_models_url(&self, base_url: &str, api_key: &str) -> String {
        // Gemini uses query-param auth for all requests
        format!(
            "{}?key={}",
            join_url(base_url, "v1beta/openai/models"),
            api_key
        )
    }

    fn uses_query_auth(&self) -> bool {
        true
    }

    fn build_auth_headers(&self, _api_key: &str) -> Vec<(String, String)> {
        vec![] // Gemini uses query-param auth exclusively
    }

    fn apply_auth(
        &self,
        builder: reqwest::RequestBuilder,
        api_key: &str,
    ) -> reqwest::RequestBuilder {
        builder.query(&[("key", api_key)])
    }

    fn transform_request(&self, body: &mut Value, actual_model: &str) {
        if let Some(obj) = body.as_object_mut() {
            obj.insert("model".to_string(), Value::String(actual_model.to_string()));
        }
    }

    fn transform_response(&self, _body: &mut Value) {
        // Google's OpenAI-compatible endpoint returns standard OpenAI format.
        // No transformation needed.
    }

    fn needs_sse_transform(&self) -> bool {
        false
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
            .and_then(|u| u.get("prompt_tokens"))
            .and_then(Value::as_i64)
            .unwrap_or(0);
        let completion = value
            .get("usage")
            .and_then(|u| u.get("completion_tokens"))
            .and_then(Value::as_i64)
            .unwrap_or(0);
        (prompt, completion)
    }

    fn transform_sse_line(&self, data_line: &str) -> Option<String> {
        // Should never be called (needs_sse_transform = false).
        Some(data_line.to_string())
    }

    fn parse_models_response(&self, body: &Value) -> Vec<(String, Option<String>)> {
        // Google's OpenAI-compatible endpoint returns standard OpenAI format:
        // { data: [{ id: "gemini-1.5-pro", owned_by: "google", ... }] }
        let openai_models: Vec<(String, Option<String>)> = body
            .get("data")
            .and_then(|d| d.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|m| {
                        let id = m.get("id")?.as_str()?.to_string();
                        let owned_by = m.get("owned_by").and_then(|v| v.as_str()).map(String::from);
                        Some((id, owned_by))
                    })
                    .collect()
            })
            .unwrap_or_default();

        if !openai_models.is_empty() {
            return openai_models;
        }

        parse_gemini_native_models(body)
    }
}

// ==================== Native Gemini format conversion ====================
//
// The code below handles Gemini's *native* API format, used when the user
// sets base_url to the native endpoint rather than the OpenAI-compatible one.
// This is kept as a fallback / future option — it is NOT used in the current
// trait implementation which targets the OpenAI-compatible endpoint.

/// Build URL for Gemini native `generateContent` endpoint.
/// Format: `{base_url}/v1beta/models/{model}:generateContent`
#[allow(dead_code)]
pub fn build_gemini_native_chat_url(base_url: &str, model: &str) -> String {
    join_url(
        base_url,
        &format!("v1beta/models/{}:generateContent", model),
    )
}

/// Build URL for Gemini native `streamGenerateContent` endpoint.
/// Format: `{base_url}/v1beta/models/{model}:streamGenerateContent?alt=sse`
#[allow(dead_code)]
pub fn build_gemini_native_stream_url(base_url: &str, model: &str) -> String {
    join_url(
        base_url,
        &format!("v1beta/models/{}:streamGenerateContent?alt=sse", model),
    )
}

/// Build URL for Gemini native model listing.
/// Format: `{base_url}/v1beta/models?key=<api_key>`
#[allow(dead_code)]
pub fn build_gemini_native_models_url(base_url: &str, api_key: &str) -> String {
    format!("{}?key={}", join_url(base_url, "v1beta/models"), api_key)
}

/// Parse Gemini native `listModels` response.
/// Format: { models: [{ name: "models/gemini-pro", displayName: "Gemini Pro", ... }] }
#[allow(dead_code)]
pub fn parse_gemini_native_models(body: &Value) -> Vec<(String, Option<String>)> {
    body.get("models")
        .and_then(|d| d.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|m| {
                    let name = m.get("name")?.as_str()?.to_string();
                    // Strip "models/" prefix → "gemini-pro"
                    let id = name.strip_prefix("models/").unwrap_or(&name).to_string();
                    let display_name = m
                        .get("displayName")
                        .and_then(|v| v.as_str())
                        .map(String::from);
                    Some((id, display_name))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Transform an OpenAI-format request into Gemini native format.
///
/// OpenAI format:
/// ```json
/// { "model": "...", "messages": [...], "stream": true, "temperature": 0.7, "max_tokens": 1024 }
/// ```
///
/// Gemini format:
/// ```json
/// { "contents": [{"role": "user", "parts": [{"text": "..."}]}], "generationConfig": {"temperature": 0.7, "maxOutputTokens": 1024} }
/// ```
#[allow(dead_code)]
pub fn transform_request_to_gemini(body: &mut Value, actual_model: &str) {
    let Some(obj) = body.as_object_mut() else {
        return;
    };

    // Extract system instruction
    let mut system_instruction = None;
    let mut contents = Vec::new();

    if let Some(msgs) = obj.remove("messages").and_then(|v| v.as_array().cloned()) {
        for msg in msgs {
            let role = msg.get("role").and_then(|r| r.as_str()).unwrap_or("user");
            match role {
                "system" => {
                    // Gemini puts system in top-level "systemInstruction"
                    if let Some(content) = msg.get("content") {
                        let text = extract_gemini_text(content);
                        if !text.is_empty() {
                            system_instruction = Some(json!({"parts": [{"text": text}]}));
                        }
                    }
                }
                _ => {
                    contents.push(convert_message_to_gemini(&msg));
                }
            }
        }
    }

    // Build Gemini request
    let mut gemini = json!({
        "contents": contents,
        "model": actual_model,
    });

    if let Some(sys) = system_instruction {
        gemini["systemInstruction"] = sys;
    }

    // generationConfig mapping
    let mut gen_config = json!({});
    if let Some(temp) = obj.remove("temperature") {
        gen_config["temperature"] = temp;
    }
    if let Some(top_p) = obj.remove("top_p") {
        gen_config["topP"] = top_p;
    }
    if let Some(max_tokens) = obj.remove("max_tokens") {
        gen_config["maxOutputTokens"] = max_tokens;
    }
    if let Some(stop) = obj.remove("stop") {
        gen_config["stopSequences"] = stop;
    }
    if !gen_config.as_object().map(|o| o.is_empty()).unwrap_or(true) {
        gemini["generationConfig"] = gen_config;
    }

    // Stream flag
    if let Some(stream) = obj.remove("stream") {
        gemini["stream"] = stream;
    }

    // Tools (function calling) — Gemini uses a different format
    if let Some(tools) = obj.remove("tools") {
        let converted = convert_tools_to_gemini(&tools);
        if !converted.as_array().map(|a| a.is_empty()).unwrap_or(true) {
            gemini["tools"] = converted;
        }
    }

    *body = gemini;
}

/// Transform Gemini native response into OpenAI format.
#[allow(dead_code)]
pub fn transform_response_from_gemini(body: &mut Value) {
    let Some(obj) = body.as_object_mut() else {
        return;
    };

    let model = obj
        .get("model")
        .and_then(|m| m.as_str())
        .unwrap_or("gemini")
        .to_string();

    // Extract text from Gemini's "candidates[].content.parts[].text"
    let mut text_parts = Vec::new();
    let mut finish_reason = String::from("stop");

    if let Some(candidates) = obj.get("candidates").and_then(|c| c.as_array()) {
        if let Some(first) = candidates.first() {
            // finishReason: "STOP" → "stop", "MAX_TOKENS" → "length", "SAFETY" → "content_filter"
            finish_reason = match first
                .get("finishReason")
                .and_then(|r| r.as_str())
                .unwrap_or("STOP")
            {
                "STOP" => "stop".to_string(),
                "MAX_TOKENS" => "length".to_string(),
                "SAFETY" => "content_filter".to_string(),
                "RECITATION" => "content_filter".to_string(),
                other => other.to_lowercase(),
            };

            if let Some(content) = first.get("content") {
                if let Some(parts) = content.get("parts").and_then(|p| p.as_array()) {
                    for part in parts {
                        if let Some(text) = part.get("text").and_then(|t| t.as_str()) {
                            text_parts.push(text.to_string());
                        }
                    }
                }
            }
        }
    }

    // Token usage — Gemini uses "usageMetadata.promptTokenCount" / "candidatesTokenCount"
    let prompt_tokens = obj
        .get("usageMetadata")
        .and_then(|u| u.get("promptTokenCount"))
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let completion_tokens = obj
        .get("usageMetadata")
        .and_then(|u| u.get("candidatesTokenCount"))
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let total_tokens = obj
        .get("usageMetadata")
        .and_then(|u| u.get("totalTokenCount"))
        .and_then(Value::as_i64)
        .unwrap_or(prompt_tokens + completion_tokens);

    *body = json!({
        "id": format!("chatcmpl-gemini-{}", chrono::Utc::now().timestamp_millis()),
        "object": "chat.completion",
        "created": chrono::Utc::now().timestamp(),
        "model": model,
        "choices": [{
            "index": 0,
            "message": {
                "role": "assistant",
                "content": text_parts.join("")
            },
            "finish_reason": finish_reason,
        }],
        "usage": {
            "prompt_tokens": prompt_tokens,
            "completion_tokens": completion_tokens,
            "total_tokens": total_tokens,
        }
    });
}

/// Transform a Gemini native SSE line into OpenAI SSE format.
/// Gemini streaming returns JSON objects (not `data: ` prefixed).
/// Each object: `{ "candidates": [...], "usageMetadata": {...} }`
#[allow(dead_code)]
pub fn transform_gemini_sse_line(data_line: &str) -> Option<String> {
    let Ok(value) = serde_json::from_str::<Value>(data_line) else {
        return None;
    };

    let model = value
        .get("model")
        .and_then(|m| m.as_str())
        .unwrap_or("gemini")
        .to_string();

    // Extract text from candidates[0].content.parts[].text
    let mut text = String::new();
    let mut finish_reason = None;

    if let Some(candidates) = value.get("candidates").and_then(|c| c.as_array()) {
        if let Some(first) = candidates.first() {
            let fr = first
                .get("finishReason")
                .and_then(|r| r.as_str())
                .unwrap_or("");
            if !fr.is_empty() {
                finish_reason = Some(match fr {
                    "STOP" => "stop".to_string(),
                    "MAX_TOKENS" => "length".to_string(),
                    "SAFETY" => "content_filter".to_string(),
                    other => other.to_lowercase(),
                });
            }
            if let Some(content) = first.get("content") {
                if let Some(parts) = content.get("parts").and_then(|p| p.as_array()) {
                    for part in parts {
                        if let Some(t) = part.get("text").and_then(|v| v.as_str()) {
                            text.push_str(t);
                        }
                    }
                }
            }
        }
    }

    let mut chunk = json!({
        "id": format!("chatcmpl-gemini-{}", chrono::Utc::now().timestamp_millis()),
        "object": "chat.completion.chunk",
        "created": chrono::Utc::now().timestamp(),
        "model": model,
        "choices": [{
            "index": 0,
            "delta": {},
            "finish_reason": null
        }]
    });

    if !text.is_empty() {
        chunk["choices"][0]["delta"]["content"] = json!(text);
    }

    if let Some(fr) = finish_reason {
        chunk["choices"][0]["finish_reason"] = json!(fr);
    }

    // Usage if present in the last chunk
    if let Some(usage) = value.get("usageMetadata") {
        chunk["usage"] = json!({
            "prompt_tokens": usage.get("promptTokenCount").and_then(Value::as_i64).unwrap_or(0),
            "completion_tokens": usage.get("candidatesTokenCount").and_then(Value::as_i64).unwrap_or(0),
            "total_tokens": usage.get("totalTokenCount").and_then(Value::as_i64).unwrap_or(0),
        });
    }

    Some(serde_json::to_string(&chunk).unwrap_or_default())
}

// ── Gemini native conversion helpers ──────────────────────────────────

#[allow(dead_code)]
fn extract_gemini_text(content: &Value) -> String {
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

#[allow(dead_code)]
fn convert_message_to_gemini(msg: &Value) -> Value {
    let role = msg.get("role").and_then(|r| r.as_str()).unwrap_or("user");
    let content = msg.get("content");

    // Gemini uses "model" role for assistant
    let gemini_role = match role {
        "assistant" => "model",
        _ => "user",
    };

    match content {
        None => json!({"role": gemini_role, "parts": []}),
        Some(Value::String(s)) => json!({"role": gemini_role, "parts": [{"text": s}]}),
        Some(Value::Array(arr)) => {
            let mut parts = Vec::new();

            for part in arr {
                let part_type = part.get("type").and_then(|t| t.as_str()).unwrap_or("");
                match part_type {
                    "text" => {
                        if let Some(text) = part.get("text").and_then(|t| t.as_str()) {
                            parts.push(json!({"text": text}));
                        }
                    }
                    "image_url" => {
                        // Convert base64 data URLs to Gemini inline data
                        if let Some(url) = part
                            .get("image_url")
                            .and_then(|u| u.get("url"))
                            .and_then(|v| v.as_str())
                        {
                            if let Some(data) = url.strip_prefix("data:") {
                                let items: Vec<&str> = data.splitn(2, ";base64,").collect();
                                if items.len() == 2 {
                                    parts.push(json!({
                                        "inlineData": {
                                            "mimeType": items[0],
                                            "data": items[1]
                                        }
                                    }));
                                }
                            }
                        }
                    }
                    "tool_call_id" => {
                        // OpenAI tool result → Gemini functionResponse
                        if role == "tool" {
                            let func_resp = json!({
                                "functionResponse": {
                                    "name": msg.get("tool_call_id").and_then(|v| v.as_str()).unwrap_or(""),
                                    "response": extract_gemini_text(content.expect("tool result content"))
                                }
                            });
                            return json!({"role": "user", "parts": [func_resp]});
                        }
                    }
                    _ => {}
                }
            }

            // Handle tool_calls in assistant messages
            if role == "assistant" {
                if let Some(tcs) = msg.get("tool_calls").and_then(|v| v.as_array()) {
                    let mut fc_parts = Vec::new();
                    for tc in tcs {
                        let fn_body = tc.get("function");
                        if let (Some(name), Some(args_str)) = (
                            fn_body.and_then(|f| f.get("name")).and_then(|n| n.as_str()),
                            fn_body
                                .and_then(|f| f.get("arguments"))
                                .and_then(|a| a.as_str()),
                        ) {
                            let args: Value = serde_json::from_str(args_str).unwrap_or(json!({}));
                            fc_parts.push(json!({
                                "functionCall": {
                                    "name": name,
                                    "args": args
                                }
                            }));
                        }
                    }
                    // Text parts first, then function calls
                    let mut all_parts = parts;
                    all_parts.extend(fc_parts);
                    return json!({"role": "model", "parts": all_parts});
                }
            }

            json!({"role": gemini_role, "parts": parts})
        }
        _ => json!({"role": gemini_role, "parts": []}),
    }
}

#[allow(dead_code)]
fn convert_tools_to_gemini(openai_tools: &Value) -> Value {
    let Some(tools_arr) = openai_tools.as_array() else {
        return json!([]);
    };

    let declarations: Vec<Value> = tools_arr
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
                "parameters": parameters
            }))
        })
        .collect();

    json!([{"functionDeclarations": declarations}])
}

// ═══════════════════════════════════════════════════════════════════
//  Public API: Gemini <-> OpenAI format conversion (from gemini_output.rs)
// ═══════════════════════════════════════════════════════════════════

/// Convert Gemini request format to OpenAI format.
///
/// - contents[].parts[].text -> messages[].content
/// - contents[].role: "user" -> "user", "model" -> "assistant"
/// - generationConfig.maxOutputTokens -> max_tokens
/// - generationConfig.temperature -> temperature
pub fn gemini_to_openai_request(gemini: &Value) -> Value {
    let mut messages = Vec::new();

    // Convert Gemini contents -> OpenAI messages
    if let Some(contents) = gemini.get("contents").and_then(|c| c.as_array()) {
        for content in contents {
            let role = content
                .get("role")
                .and_then(|r| r.as_str())
                .unwrap_or("user");
            let openai_role = match role {
                "model" => "assistant",
                "user" => "user",
                other => other,
            };

            // Extract text from parts
            let empty_vec = vec![];
            let parts = content
                .get("parts")
                .and_then(|p| p.as_array())
                .unwrap_or(&empty_vec);
            let text: String = parts
                .iter()
                .filter_map(|part| part.get("text").and_then(|t| t.as_str()))
                .collect::<Vec<_>>()
                .join("");

            if !text.is_empty() {
                messages.push(json!({
                    "role": openai_role,
                    "content": text
                }));
            }
        }
    }

    let mut openai = json!({
        "model": gemini.get("model").and_then(|m| m.as_str()).unwrap_or(""),
        "messages": messages,
    });

    // Extract generationConfig parameters
    if let Some(gen_config) = gemini.get("generationConfig") {
        if let Some(max_output_tokens) = gen_config.get("maxOutputTokens").and_then(|v| v.as_i64())
        {
            openai["max_tokens"] = json!(max_output_tokens);
        }
        if let Some(temperature) = gen_config.get("temperature").and_then(|v| v.as_f64()) {
            openai["temperature"] = json!(temperature);
        }
        if let Some(top_p) = gen_config.get("topP").and_then(|v| v.as_f64()) {
            openai["top_p"] = json!(top_p);
        }
    }

    // Pass through stream flag
    if let Some(stream) = gemini.get("stream").and_then(|s| s.as_bool()) {
        openai["stream"] = json!(stream);
    }

    // Convert Gemini tools -> OpenAI tools
    if let Some(tools) = gemini.get("tools").and_then(|t| t.as_array()) {
        let openai_tools: Vec<Value> = tools
            .iter()
            .filter_map(|tool| {
                // Gemini tools format has functionDeclarations array
                let declarations = tool
                    .get("functionDeclarations")
                    .and_then(|d| d.as_array())?;
                let mut functions = Vec::new();

                for decl in declarations {
                    let name = decl.get("name")?.as_str()?;
                    let description = decl
                        .get("description")
                        .and_then(|d| d.as_str())
                        .unwrap_or("");
                    let parameters = decl.get("parameters").cloned().unwrap_or(json!({}));

                    functions.push(json!({
                        "type": "function",
                        "function": {
                            "name": name,
                            "description": description,
                            "parameters": parameters,
                        }
                    }));
                }

                Some(functions)
            })
            .flatten()
            .collect();

        if !openai_tools.is_empty() {
            openai["tools"] = json!(openai_tools);
        }
    }

    // 公理二：未知字段穿透。保留 Gemini 原始请求里的 safetySettings、cachedContent、
    // x_future_gemini_field 等官方/未来/自定义字段，避免"中转翻译器"丢信息。
    if ENABLE_UNKNOWN_FIELD_PASSTHROUGH {
        if let (Some(src), Some(dst)) = (gemini.as_object(), openai.as_object_mut()) {
            for (key, value) in src {
                if !dst.contains_key(key) {
                    dst.insert(key.clone(), value.clone());
                }
            }
        }
    }

    openai
}

/// Convert OpenAI response format to Gemini format.
///
/// - choices[0].message.content -> candidates[0].content.parts[0].text
/// - choices[0].finish_reason -> candidates[0].finishReason: stop->STOP, length->MAX_TOKENS
/// - usage.prompt_tokens -> usageMetadata.promptTokenCount
/// - usage.completion_tokens -> usageMetadata.candidatesTokenCount
pub fn openai_to_gemini_response(openai: &Value) -> Value {
    let choice = openai
        .get("choices")
        .and_then(|c| c.get(0))
        .expect("OpenAI response must have at least one choice");

    let message = choice.get("message").expect("Choice must have message");
    let content_str = message
        .get("content")
        .and_then(|c| c.as_str())
        .unwrap_or("");

    // Map finish_reason
    let finish_reason = choice
        .get("finish_reason")
        .and_then(|fr| fr.as_str())
        .unwrap_or("stop");

    let gemini_finish_reason = match finish_reason {
        "stop" => "STOP",
        "length" => "MAX_TOKENS",
        other => other,
    };

    // Build content parts
    let mut parts = Vec::new();
    if !content_str.is_empty() {
        parts.push(json!({
            "text": content_str
        }));
    }

    // Usage mapping
    let usage = openai.get("usage").cloned().unwrap_or(json!({}));
    let prompt_tokens = usage
        .get("prompt_tokens")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let candidates_tokens = usage
        .get("completion_tokens")
        .and_then(Value::as_i64)
        .unwrap_or(0);

    // 公理二：clone openai 作为基底，edit-in-place 改写 Gemini 特有字段，
    // 避免白名单 json!({...}) 构造新对象把上游其他字段丢掉。
    let mut out = openai.clone();
    if let Some(obj) = out.as_object_mut() {
        // 新增 Gemini 特有字段
        obj.insert(
            "candidates".to_string(),
            json!([
                {
                    "content": {
                        "parts": parts,
                        "role": "model"
                    },
                    "finishReason": gemini_finish_reason
                }
            ]),
        );
        obj.insert(
            "usageMetadata".to_string(),
            json!({
                "promptTokenCount": prompt_tokens,
                "candidatesTokenCount": candidates_tokens,
                "totalTokenCount": prompt_tokens + candidates_tokens
            }),
        );

        // 移除 OpenAI 特有但 Gemini 不用的字段（已翻译成 candidates/usageMetadata）
        obj.remove("object"); // "chat.completion" 不是 Gemini 语义
        obj.remove("choices"); // 已翻译成 candidates
        obj.remove("created"); // Gemini 不用时间戳
        obj.remove("usage"); // 已翻译成 usageMetadata

        // 如果关了穿透，只保留 Gemini 官方文档已知字段
        if !ENABLE_UNKNOWN_FIELD_PASSTHROUGH {
            let gemini_known: std::collections::HashSet<&str> = [
                "candidates",
                "usageMetadata",
                "modelVersion",
                "promptFeedback",
            ]
            .into_iter()
            .collect();
            obj.retain(|k, _| gemini_known.contains(k.as_str()));
        }
        // 否则（默认）保留 system_fingerprint、x_openai_future_field 等其他字段
    }
    out
}

// ═══════════════════════════════════════════════════════════════════
//  OpenAI SSE → Gemini SSE 流式转换
// ═══════════════════════════════════════════════════════════════════
//
// 当 /v1beta/models/{model}:streamGenerateContent 接收到 Gemini 原生流式请求后，
// 内部转换为 OpenAI 格式转发，上游返回 OpenAI SSE (chat.completion.chunk)，
// 需要在此处逐行转换为 Gemini 原生 SSE 格式再返回给客户端。
//
// OpenAI 格式:
//   data: {"id":"...","object":"chat.completion.chunk","choices":[{"index":0,"delta":{"content":"Hello"},"finish_reason":null}]}
//
// Gemini 格式:
//   data: {"candidates":[{"content":{"parts":[{"text":"Hello"}],"role":"model"},"finishReason":null}]}

use axum::body::Body;
use bytes::Bytes;
use futures::StreamExt;
use std::time::Duration;

/// 将单个 OpenAI SSE data line 转换为 Gemini 原生 SSE 格式。
///
/// OpenAI chunk → Gemini chunk 字段映射：
/// - choices[0].delta.content                → candidates[0].content.parts[0].text
/// - choices[0].finish_reason (stop/length)   → candidates[0].finishReason (STOP/MAX_TOKENS)
/// - usage.prompt_tokens                      → usageMetadata.promptTokenCount
/// - usage.completion_tokens                  → usageMetadata.candidatesTokenCount
fn openai_sse_chunk_to_gemini(data_line: &str) -> Option<String> {
    if data_line == "[DONE]" {
        return None;
    }

    let Ok(value) = serde_json::from_str::<Value>(data_line) else {
        return None;
    };

    let mut text = String::new();
    let mut finish_reason: Option<String> = None;

    if let Some(choices) = value.get("choices").and_then(|c| c.as_array()) {
        if let Some(choice) = choices.first() {
            if let Some(delta) = choice.get("delta") {
                if let Some(content) = delta.get("content").and_then(|c| c.as_str()) {
                    text.push_str(content);
                }
            }
            if let Some(fr) = choice.get("finish_reason").and_then(|f| f.as_str()) {
                if !fr.is_empty() && fr != "null" {
                    finish_reason = Some(match fr {
                        "stop" => "STOP",
                        "length" => "MAX_TOKENS",
                        "content_filter" => "SAFETY",
                        "tool_calls" => "TOOL_CALLS",
                        other => other,
                    }.to_string());
                }
            }
        }
    }

    let mut candidate = json!({
        "content": {
            "parts": [{"text": text}],
            "role": "model"
        }
    });

    if let Some(fr) = finish_reason {
        candidate["finishReason"] = json!(fr);
    }

    let mut gemini_chunk = json!({
        "candidates": [candidate]
    });

    // 最后一个 chunk 可能携带 usage 信息
    if let Some(usage) = value.get("usage") {
        let prompt = usage.get("prompt_tokens").and_then(Value::as_i64).unwrap_or(0);
        let completion = usage.get("completion_tokens").and_then(Value::as_i64).unwrap_or(0);
        gemini_chunk["usageMetadata"] = json!({
            "promptTokenCount": prompt,
            "candidatesTokenCount": completion,
            "totalTokenCount": prompt + completion,
        });
    }

    // 透传 model 字段
    if let Some(model) = value.get("model").and_then(|m| m.as_str()) {
        gemini_chunk["model"] = json!(model);
    }

    Some(serde_json::to_string(&gemini_chunk).unwrap_or_default())
}

/// 将上游返回的 OpenAI SSE 流转换为 Gemini 原生 SSE 流。
///
/// 输入: OpenAI chat.completion.chunk SSE 流（来自 forwarder）
/// 输出: Gemini candidates SSE 流（返回给 Gemini 原生客户端）
pub fn transform_openai_sse_to_gemini_stream(
    response: axum::response::Response,
) -> Result<axum::response::Response, crate::error::AppError> {
    let upstream_stream = response.into_body().into_data_stream();

    let sse_buffer = String::new();
    let sse_utf8_remainder: Vec<u8> = Vec::new();

    let transformed_stream = futures::stream::unfold(
        (upstream_stream, sse_buffer, sse_utf8_remainder, 0usize),
        |(mut stream, mut sse_buffer, mut sse_utf8_remainder, mut streamed_bytes)| async move {
            loop {
                if crate::proxy::sse::stream_buffer_exceeded(
                    &sse_buffer, &sse_utf8_remainder, streamed_bytes,
                ) {
                    return Some((
                        Err::<Bytes, std::io::Error>(std::io::Error::new(
                            std::io::ErrorKind::Other,
                            "stream buffer exceeds 10MB limit",
                        )),
                        (stream, sse_buffer, sse_utf8_remainder, streamed_bytes),
                    ));
                }

                tokio::select! {
                    _ = tokio::time::sleep(Duration::from_secs(300)) => {
                        return Some((
                            Err::<Bytes, std::io::Error>(std::io::Error::new(
                                std::io::ErrorKind::TimedOut,
                                "stream idle timeout",
                            )),
                            (stream, sse_buffer, sse_utf8_remainder, streamed_bytes),
                        ));
                    }
                    chunk_result = stream.next() => {
                        match chunk_result {
                            Some(Ok(chunk)) => {
                                streamed_bytes += chunk.len();
                                crate::proxy::sse::append_utf8_safe(
                                    &mut sse_buffer, &mut sse_utf8_remainder, &chunk,
                                );
                            }
                            Some(Err(e)) => {
                                return Some((
                                    Err::<Bytes, std::io::Error>(std::io::Error::new(
                                        std::io::ErrorKind::Other,
                                        format!("Stream read error: {e}"),
                                    )),
                                    (stream, sse_buffer, sse_utf8_remainder, streamed_bytes),
                                ));
                            }
                            None => return None,
                        }
                    }
                }

                if let Some(line_end) = sse_buffer.find('\n') {
                    let mut line = sse_buffer.drain(..=line_end).collect::<String>();
                    if line.ends_with('\n') { line.pop(); }
                    if line.ends_with('\r') { line.pop(); }

                    if let Some(payload) = line.strip_prefix("data: ") {
                        if payload == "[DONE]" {
                            return Some((
                                Ok::<_, std::io::Error>(Bytes::from("data: [DONE]\n\n")),
                                (stream, sse_buffer, sse_utf8_remainder, streamed_bytes),
                            ));
                        }

                        if let Some(gemini_line) = openai_sse_chunk_to_gemini(payload) {
                            let output = Bytes::from(format!("data: {gemini_line}\n\n"));
                            return Some((
                                Ok::<_, std::io::Error>(output),
                                (stream, sse_buffer, sse_utf8_remainder, streamed_bytes),
                            ));
                        }
                    }
                }
            }
        },
    );

    Ok(axum::http::Response::builder()
        .header("content-type", "text/event-stream")
        .header("cache-control", "no-cache")
        .header("connection", "keep-alive")
        .header("x-accel-buffering", "no")
        .body(Body::from_stream(transformed_stream))
        .unwrap())
}

// ═══════════════════════════════════════════════════════════════════
//  Tests
// ═══════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ─── gemini_to_openai tests ─────────────────────────────────────

    #[test]
    fn basic_gemini_to_openai() {
        let gemini = json!({
            "contents": [
                {
                    "role": "user",
                    "parts": [{"text": "Hello"}]
                }
            ],
            "generationConfig": {
                "maxOutputTokens": 100,
                "temperature": 0.7
            }
        });

        let openai = gemini_to_openai_request(&gemini);

        assert_eq!(openai["messages"][0]["role"], "user");
        assert_eq!(openai["messages"][0]["content"], "Hello");
        assert_eq!(openai["max_tokens"], 100);
        assert_eq!(openai["temperature"], 0.7);
    }

    #[test]
    fn gemini_to_openai_with_assistant() {
        let gemini = json!({
            "contents": [
                {
                    "role": "user",
                    "parts": [{"text": "What is 2+2?"}]
                },
                {
                    "role": "model",
                    "parts": [{"text": "4"}]
                }
            ]
        });

        let openai = gemini_to_openai_request(&gemini);

        assert_eq!(openai["messages"].as_array().unwrap().len(), 2);
        assert_eq!(openai["messages"][0]["role"], "user");
        assert_eq!(openai["messages"][0]["content"], "What is 2+2?");
        assert_eq!(openai["messages"][1]["role"], "assistant");
        assert_eq!(openai["messages"][1]["content"], "4");
    }

    #[test]
    fn gemini_to_openai_multiple_parts() {
        let gemini = json!({
            "contents": [
                {
                    "role": "user",
                    "parts": [
                        {"text": "Part 1"},
                        {"text": " Part 2"}
                    ]
                }
            ]
        });

        let openai = gemini_to_openai_request(&gemini);

        assert_eq!(openai["messages"][0]["content"], "Part 1 Part 2");
    }

    #[test]
    fn gemini_to_openai_with_model() {
        let gemini = json!({
            "model": "gemini-pro",
            "contents": [{"role": "user", "parts": [{"text": "Hi"}]}]
        });

        let openai = gemini_to_openai_request(&gemini);

        assert_eq!(openai["model"], "gemini-pro");
    }

    // ─── openai_to_gemini tests ─────────────────────────────────────

    #[test]
    fn basic_openai_to_gemini() {
        let openai = json!({
            "id": "chatcmpl-abc123",
            "object": "chat.completion",
            "model": "gpt-4",
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

        let gemini = openai_to_gemini_response(&openai);

        assert_eq!(
            gemini["candidates"][0]["content"]["parts"][0]["text"],
            "Hello! How can I help?"
        );
        assert_eq!(gemini["candidates"][0]["content"]["role"], "model");
        assert_eq!(gemini["candidates"][0]["finishReason"], "STOP");
        assert_eq!(gemini["usageMetadata"]["promptTokenCount"], 10);
        assert_eq!(gemini["usageMetadata"]["candidatesTokenCount"], 8);
        assert_eq!(gemini["usageMetadata"]["totalTokenCount"], 18);
    }

    #[test]
    fn openai_to_gemini_max_tokens() {
        let openai = json!({
            "id": "chatcmpl-def456",
            "model": "gpt-4",
            "choices": [
                {
                    "message": {
                        "role": "assistant",
                        "content": "This is a long response..."
                    },
                    "finish_reason": "length"
                }
            ],
            "usage": {
                "prompt_tokens": 100,
                "completion_tokens": 2048
            }
        });

        let gemini = openai_to_gemini_response(&openai);

        assert_eq!(gemini["candidates"][0]["finishReason"], "MAX_TOKENS");
        assert_eq!(gemini["usageMetadata"]["promptTokenCount"], 100);
        assert_eq!(gemini["usageMetadata"]["candidatesTokenCount"], 2048);
        assert_eq!(gemini["usageMetadata"]["totalTokenCount"], 2148);
    }

    // ─── openai_sse_chunk_to_gemini 测试 ──────────────────────────

    #[test]
    fn openai_sse_chunk_basic_text() {
        let line = r#"{"id":"1","object":"chat.completion.chunk","choices":[{"index":0,"delta":{"content":"Hello"},"finish_reason":null}]}"#;
        let result = openai_sse_chunk_to_gemini(line).unwrap();
        let v: Value = serde_json::from_str(&result).unwrap();
        assert_eq!(v["candidates"][0]["content"]["parts"][0]["text"], "Hello");
        assert_eq!(v["candidates"][0]["finishReason"], Value::Null);
    }

    #[test]
    fn openai_sse_chunk_delta_accumulation() {
        let line = r#"{"id":"2","choices":[{"index":0,"delta":{"content":" world"},"finish_reason":null}]}"#;
        let result = openai_sse_chunk_to_gemini(line).unwrap();
        let v: Value = serde_json::from_str(&result).unwrap();
        assert_eq!(v["candidates"][0]["content"]["parts"][0]["text"], " world");
    }

    #[test]
    fn openai_sse_chunk_finish_reason_stop() {
        let line = r#"{"id":"3","choices":[{"index":0,"delta":{},"finish_reason":"stop"}],"usage":{"prompt_tokens":10,"completion_tokens":5,"total_tokens":15}}"#;
        let result = openai_sse_chunk_to_gemini(line).unwrap();
        let v: Value = serde_json::from_str(&result).unwrap();
        assert_eq!(v["candidates"][0]["finishReason"], "STOP");
        assert_eq!(v["usageMetadata"]["promptTokenCount"], 10);
        assert_eq!(v["usageMetadata"]["candidatesTokenCount"], 5);
        assert_eq!(v["usageMetadata"]["totalTokenCount"], 15);
    }

    #[test]
    fn openai_sse_chunk_finish_reason_length() {
        let line = r#"{"id":"4","choices":[{"index":0,"delta":{},"finish_reason":"length"}]}"#;
        let result = openai_sse_chunk_to_gemini(line).unwrap();
        let v: Value = serde_json::from_str(&result).unwrap();
        assert_eq!(v["candidates"][0]["finishReason"], "MAX_TOKENS");
    }

    #[test]
    fn openai_sse_chunk_finish_reason_content_filter() {
        let line = r#"{"id":"5","choices":[{"index":0,"delta":{},"finish_reason":"content_filter"}]}"#;
        let result = openai_sse_chunk_to_gemini(line).unwrap();
        let v: Value = serde_json::from_str(&result).unwrap();
        assert_eq!(v["candidates"][0]["finishReason"], "SAFETY");
    }

    #[test]
    fn openai_sse_chunk_done_returns_none() {
        assert!(openai_sse_chunk_to_gemini("[DONE]").is_none());
    }

    #[test]
    fn openai_sse_chunk_invalid_json_returns_none() {
        assert!(openai_sse_chunk_to_gemini("not json").is_none());
    }

    #[test]
    fn openai_sse_chunk_model_passthrough() {
        let line = r#"{"id":"6","model":"gpt-4o","choices":[{"index":0,"delta":{"content":"hi"},"finish_reason":null}]}"#;
        let result = openai_sse_chunk_to_gemini(line).unwrap();
        let v: Value = serde_json::from_str(&result).unwrap();
        assert_eq!(v["model"], "gpt-4o");
    }

    #[test]
    fn openai_sse_chunk_no_choices() {
        let line = r#"{"id":"7"}"#;
        let result = openai_sse_chunk_to_gemini(line).unwrap();
        let v: Value = serde_json::from_str(&result).unwrap();
        // 无 choices → content_text = ""，candidates 仍存在
        assert_eq!(v["candidates"][0]["content"]["parts"][0]["text"], "");
        assert!(v.get("finishReason").is_none());
    }

    #[test]
    fn openai_sse_chunk_usage_maps_correctly() {
        let line = r#"{"id":"8","choices":[{"index":0,"delta":{},"finish_reason":"stop"}],"usage":{"prompt_tokens":50,"completion_tokens":30,"total_tokens":80}}"#;
        let result = openai_sse_chunk_to_gemini(line).unwrap();
        let v: Value = serde_json::from_str(&result).unwrap();
        assert_eq!(v["usageMetadata"]["promptTokenCount"], 50);
        assert_eq!(v["usageMetadata"]["candidatesTokenCount"], 30);
        assert_eq!(v["usageMetadata"]["totalTokenCount"], 80);
    }
}
