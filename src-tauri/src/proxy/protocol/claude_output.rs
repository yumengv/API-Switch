use serde_json::{json, Value};

/// 穿透开关：true = 未知字段保留穿透，false = 只保留已知白名单字段
///
/// 默认 true，贯彻「中转翻译器不丢信息」的公理。
/// 如果发现某个上游/客户端对未知字段返回 400，可临时改为 false 发布紧急版本。
const ENABLE_UNKNOWN_FIELD_PASSTHROUGH: bool = true;

// ═══════════════════════════════════════════════════════════════════
//  Public API: Claude <-> OpenAI format conversion
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
    // 这里把 claude 顶层剩余的（未知/未来的）字段也带过去，
    // 避免"中转翻译器"丢信息。
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

    // Build content array
    let mut content = Vec::new();

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

        // 移除 OpenAI 特有但 Claude 不应出现的字段
        obj.remove("object"); // "chat.completion" 不是 Claude 语义
        obj.remove("choices"); // Claude 没有 choices 结构
        obj.remove("created"); // Claude 用 id 而不是时间戳
        obj.remove("system_fingerprint"); // OpenAI 特有

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
//  ClaudeSSETransformer: OpenAI SSE -> Claude SSE
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
            // Close any open text block before tool_use blocks
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
//  Private helpers
// ═══════════════════════════════════════════════════════════════════

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

// ═══════════════════════════════════════════════════════════════════
//  Tests
// ═══════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ─── claude_to_openai tests ─────────────────────────────────────

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

    // ─── openai_to_claude tests ─────────────────────────────────────

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

    // ─── SSE transformer tests ──────────────────────────────────────

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

    // ─── tool_choice reverse mapping tests ───────────────────────────

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

    // ─── image reverse conversion tests ────────────────────────────────

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
