use serde_json::{json, Value};

/// 穿透开关：true = 未知字段保留穿透，false = 只保留已知白名单字段
///
/// 默认 true，贯彻「中转翻译器不丢信息」的公理。
/// 如果发现某个上游/客户端对未知字段返回 400，可临时改为 false 发布紧急版本。
const ENABLE_UNKNOWN_FIELD_PASSTHROUGH: bool = true;

// ═══════════════════════════════════════════════════════════════════
//  Public API: Gemini <-> OpenAI format conversion
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

/// Transform an error into Gemini error format.
pub fn transform_gemini_error(status: u16, message: &str) -> Value {
    let error_code = match status {
        400 | 422 => "INVALID_ARGUMENT",
        401 => "UNAUTHENTICATED",
        403 => "PERMISSION_DENIED",
        404 => "NOT_FOUND",
        429 => "RESOURCE_EXHAUSTED",
        500..=599 => "INTERNAL",
        _ => "UNKNOWN",
    };

    json!({
        "error": {
            "code": error_code,
            "message": message,
            "status": error_code
        }
    })
}

// ═══════════════════════════════════════════════════════════════════
//  GeminiSSETransformer: OpenAI SSE -> Gemini SSE
// ═══════════════════════════════════════════════════════════════════

/// Transforms OpenAI streaming SSE chunks into Gemini SSE format.
///
/// Gemini SSE format:
/// ```
/// data: {"candidates":[{"content":{"parts":[{"text":"chunk1"}],"role":"model"}}]}
/// data: {"candidates":[{"content":{"parts":[{"text":"chunk2"}],"role":"model"},"finishReason":"STOP"}],"usageMetadata":{...}}
/// ```
pub struct GeminiSSETransformer {
    model: String,
    started: bool,
}

impl GeminiSSETransformer {
    pub fn new(_message_id: String, model: String) -> Self {
        Self {
            model,
            started: false,
        }
    }

    /// Transform a single OpenAI SSE chunk into Gemini SSE events.
    ///
    /// Returns a vector of JSON strings, each wrapped by the caller as `data: {event}\n\n`.
    pub fn transform_chunk(&mut self, openai_chunk: &str) -> Vec<String> {
        let mut events = Vec::new();

        let Ok(chunk) = serde_json::from_str::<Value>(openai_chunk) else {
            return events;
        };

        let Some(choice) = chunk.get("choices").and_then(|c| c.get(0)).cloned() else {
            return events;
        };

        let delta = choice.get("delta").cloned().unwrap_or(json!({}));
        let finish_reason = choice.get("finish_reason").and_then(|fr| fr.as_str());

        // Mark as started on first chunk
        if !self.started && (delta.get("content").is_some() || delta.get("role").is_some()) {
            self.started = true;
        }

        // Handle text content delta
        if let Some(content_val) = delta.get("content") {
            if let Value::String(text) = content_val {
                if !text.is_empty() {
                    let event = json!({
                        "candidates": [
                            {
                                "content": {
                                    "parts": [
                                        {"text": text}
                                    ],
                                    "role": "model"
                                }
                            }
                        ]
                    });

                    events.push(serde_json::to_string(&event).unwrap_or_default());
                }
            }
        }

        // Handle finish reason
        if let Some(fr) = finish_reason {
            let gemini_finish_reason = match fr {
                "stop" => "STOP",
                "length" => "MAX_TOKENS",
                _ => fr,
            };

            let usage = chunk.get("usage").cloned().unwrap_or(json!({}));
            let prompt_tokens = usage
                .get("prompt_tokens")
                .and_then(Value::as_i64)
                .unwrap_or(0);
            let completion_tokens = usage
                .get("completion_tokens")
                .and_then(Value::as_i64)
                .unwrap_or(0);

            let event = json!({
                "candidates": [
                    {
                        "content": Value::Null,
                        "finishReason": gemini_finish_reason
                    }
                ],
                "usageMetadata": {
                    "promptTokenCount": prompt_tokens,
                    "candidatesTokenCount": completion_tokens,
                    "totalTokenCount": prompt_tokens + completion_tokens
                }
            });

            events.push(serde_json::to_string(&event).unwrap_or_default());
        }

        events
    }
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

    // ─── transform_gemini_error tests ────────────────────────────────

    #[test]
    fn gemini_error_400() {
        let error = transform_gemini_error(400, "Invalid request format");
        assert_eq!(error["error"]["code"], "INVALID_ARGUMENT");
        assert_eq!(error["error"]["message"], "Invalid request format");
        assert_eq!(error["error"]["status"], "INVALID_ARGUMENT");
    }

    #[test]
    fn gemini_error_401() {
        let error = transform_gemini_error(401, "API key is invalid");
        assert_eq!(error["error"]["code"], "UNAUTHENTICATED");
        assert_eq!(error["error"]["message"], "API key is invalid");
    }

    #[test]
    fn gemini_error_403() {
        let error = transform_gemini_error(403, "Permission denied");
        assert_eq!(error["error"]["code"], "PERMISSION_DENIED");
    }

    #[test]
    fn gemini_error_429() {
        let error = transform_gemini_error(429, "Rate limit exceeded");
        assert_eq!(error["error"]["code"], "RESOURCE_EXHAUSTED");
        assert_eq!(error["error"]["message"], "Rate limit exceeded");
    }

    #[test]
    fn gemini_error_500() {
        let error = transform_gemini_error(500, "Internal server error");
        assert_eq!(error["error"]["code"], "INTERNAL");
    }

    // ─── SSE transformer tests ───────────────────────────────────────

    #[test]
    fn sse_basic_text_chunk() {
        let mut transformer =
            GeminiSSETransformer::new("msg_test".to_string(), "gemini-pro".to_string());

        let chunk = r#"{"id":"chatcmpl-abc","choices":[{"delta":{"content":"Hello"},"finish_reason":null}]}"#;
        let events = transformer.transform_chunk(chunk);

        assert_eq!(events.len(), 1);

        let event: Value = serde_json::from_str(&events[0]).unwrap();
        assert_eq!(
            event["candidates"][0]["content"]["parts"][0]["text"],
            "Hello"
        );
        assert_eq!(event["candidates"][0]["content"]["role"], "model");
    }

    #[test]
    fn sse_multiple_chunks() {
        let mut transformer =
            GeminiSSETransformer::new("msg_test".to_string(), "gemini-pro".to_string());

        let chunk1 =
            r#"{"id":"chatcmpl-abc","choices":[{"delta":{"content":"Hi"},"finish_reason":null}]}"#;
        let events1 = transformer.transform_chunk(chunk1);
        assert_eq!(events1.len(), 1);

        let chunk2 = r#"{"id":"chatcmpl-abc","choices":[{"delta":{"content":" there"},"finish_reason":null}]}"#;
        let events2 = transformer.transform_chunk(chunk2);
        assert_eq!(events2.len(), 1);

        let event1: Value = serde_json::from_str(&events1[0]).unwrap();
        assert_eq!(event1["candidates"][0]["content"]["parts"][0]["text"], "Hi");

        let event2: Value = serde_json::from_str(&events2[0]).unwrap();
        assert_eq!(
            event2["candidates"][0]["content"]["parts"][0]["text"],
            " there"
        );
    }

    #[test]
    fn sse_finish_with_usage() {
        let mut transformer =
            GeminiSSETransformer::new("msg_test".to_string(), "gemini-pro".to_string());

        // Text chunk
        let chunk1 = r#"{"id":"chatcmpl-abc","choices":[{"delta":{"content":"Hello"},"finish_reason":null}]}"#;
        transformer.transform_chunk(chunk1);

        // Finish chunk with usage
        let chunk2 = r#"{"id":"chatcmpl-abc","choices":[{"delta":{},"finish_reason":"stop"}],"usage":{"prompt_tokens":10,"completion_tokens":5,"total_tokens":15}}"#;
        let events = transformer.transform_chunk(chunk2);

        assert_eq!(events.len(), 1);

        let event: Value = serde_json::from_str(&events[0]).unwrap();
        assert_eq!(event["candidates"][0]["finishReason"], "STOP");
        assert_eq!(event["usageMetadata"]["promptTokenCount"], 10);
        assert_eq!(event["usageMetadata"]["candidatesTokenCount"], 5);
        assert_eq!(event["usageMetadata"]["totalTokenCount"], 15);
    }

    #[test]
    fn sse_finish_max_tokens() {
        let mut transformer =
            GeminiSSETransformer::new("msg_test".to_string(), "gemini-pro".to_string());

        let chunk = r#"{"id":"chatcmpl-abc","choices":[{"delta":{},"finish_reason":"length"}],"usage":{"prompt_tokens":100,"completion_tokens":2000}}"#;
        let events = transformer.transform_chunk(chunk);

        assert_eq!(events.len(), 1);

        let event: Value = serde_json::from_str(&events[0]).unwrap();
        assert_eq!(event["candidates"][0]["finishReason"], "MAX_TOKENS");
        assert_eq!(event["usageMetadata"]["candidatesTokenCount"], 2000);
    }

    #[test]
    fn sse_empty_chunk() {
        let mut transformer =
            GeminiSSETransformer::new("msg_test".to_string(), "gemini-pro".to_string());

        let chunk = r#"{"id":"chatcmpl-abc","choices":[]}"#;
        let events = transformer.transform_chunk(chunk);

        assert!(events.is_empty());
    }
}
