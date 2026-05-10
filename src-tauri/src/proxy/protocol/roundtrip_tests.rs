//! 协议翻译器 round-trip 测试套件
//!
//! 根据 REFACTOR_PLAN.md 的两条公理：
//! 1. 我们是中转和翻译器，不是内容修改者。
//! 2. 协议以官方文档为准，这边进来什么，那边出去必须是一样的。
//!
//! 这里的测试不测"当前代码能跑通什么"，测"按照公理目标代码应该做到什么"。
//! 测试会有红有绿：
//!   - 绿的 = 当前代码已符合公理，后续重构不能破坏
//!   - 红的 = 当前代码偏离公理，后续阶段要让它变绿
//!
//! 随着阶段 1 → 阶段 4 的推进，红色测试逐个变绿，架构自然自洽。

use serde_json::{json, Value};

/// JSON 相等断言，失败时输出结构化 diff 便于定位
fn assert_json_eq(actual: &Value, expected: &Value, context: &str) {
    if actual != expected {
        panic!(
            "{context}\n─── expected ───\n{}\n─── actual ───\n{}\n",
            serde_json::to_string_pretty(expected).unwrap_or_default(),
            serde_json::to_string_pretty(actual).unwrap_or_default(),
        );
    }
}

/// 从 JSON 对象中移除指定顶层字段，便于对比时忽略期望会变化的字段
fn without_fields(mut value: Value, fields: &[&str]) -> Value {
    if let Some(obj) = value.as_object_mut() {
        for field in fields {
            obj.remove(*field);
        }
    }
    value
}

// ═══════════════════════════════════════════════════════════════════
//  Claude 协议 round-trip 测试
//
//  验证链路：
//    下游方向：Claude 请求 A → OpenAI 中间格式 B → Claude 请求 A'
//             应满足 A ≡ A'
//    上游方向：Claude 响应 X → OpenAI 中间格式 Y → Claude 响应 X'
//             应满足 X ≡ X'
// ═══════════════════════════════════════════════════════════════════

mod claude_roundtrip {
    use super::*;
    use crate::proxy::protocol::{
        claude::ClaudeAdapter, claude_to_openai_request, openai_to_claude_response,
        ClaudeSSETransformer, ProtocolAdapter,
    };

    const TEST_MODEL: &str = "claude-3-sonnet-20240229";

    // ─── 请求方向：下游入口 → 上游 adapter 的 round-trip ──────────────

    /// 最基础场景：单轮对话，文本 messages
    #[test]
    fn request_basic_text_message() {
        let claude_original = json!({
            "model": TEST_MODEL,
            "messages": [
                {"role": "user", "content": "Hello"}
            ],
            "max_tokens": 1024,
            "stream": false
        });

        // 下游入口翻译：Claude → OpenAI
        let openai_intermediate = claude_to_openai_request(&claude_original);

        // 上游 adapter 翻译：OpenAI → Claude
        let adapter = ClaudeAdapter;
        let mut back_to_claude = openai_intermediate.clone();
        adapter.transform_request(&mut back_to_claude, TEST_MODEL);

        assert_eq!(back_to_claude["model"], json!(TEST_MODEL));
        assert_eq!(back_to_claude["messages"][0]["role"], "user");
        assert_eq!(back_to_claude["messages"][0]["content"], "Hello");
        assert_eq!(back_to_claude["max_tokens"], 1024);
    }

    /// system 消息：Claude 顶层 system → OpenAI messages[0] role=system → Claude 顶层 system
    #[test]
    fn request_system_message_roundtrip() {
        let claude_original = json!({
            "model": TEST_MODEL,
            "system": "You are helpful.",
            "messages": [
                {"role": "user", "content": "Hi"}
            ],
            "max_tokens": 1024
        });

        let openai_intermediate = claude_to_openai_request(&claude_original);

        // 验证中间格式：system 变成第一条 message
        assert_eq!(openai_intermediate["messages"][0]["role"], "system");
        assert_eq!(openai_intermediate["messages"][0]["content"], "You are helpful.");

        let adapter = ClaudeAdapter;
        let mut back_to_claude = openai_intermediate.clone();
        adapter.transform_request(&mut back_to_claude, TEST_MODEL);

        // 验证还原：system 回到顶层
        // 注意：当前 adapter 实现将 system 存为 array 形式（Claude 4.5+ 兼容），
        // 这是官方文档允许的形态，不视为违规
        let system = &back_to_claude["system"];
        let system_text = match system {
            Value::String(s) => s.clone(),
            Value::Array(arr) => arr
                .iter()
                .filter_map(|b| b.get("text").and_then(|t| t.as_str()))
                .collect::<Vec<_>>()
                .join(""),
            _ => String::new(),
        };
        assert_eq!(system_text, "You are helpful.");
        assert_eq!(back_to_claude["messages"][0]["role"], "user");
    }

    /// tools 定义：Claude input_schema ↔ OpenAI function.parameters
    #[test]
    fn request_tools_roundtrip() {
        let claude_original = json!({
            "model": TEST_MODEL,
            "messages": [{"role": "user", "content": "What's the weather?"}],
            "max_tokens": 1024,
            "tools": [{
                "name": "get_weather",
                "description": "Get weather for a city",
                "input_schema": {
                    "type": "object",
                    "properties": {"city": {"type": "string"}},
                    "required": ["city"]
                }
            }]
        });

        let openai_intermediate = claude_to_openai_request(&claude_original);

        // 验证中间格式：tools 变成 function 形态
        assert_eq!(openai_intermediate["tools"][0]["type"], "function");
        assert_eq!(openai_intermediate["tools"][0]["function"]["name"], "get_weather");

        let adapter = ClaudeAdapter;
        let mut back_to_claude = openai_intermediate.clone();
        adapter.transform_request(&mut back_to_claude, TEST_MODEL);

        // 验证还原：tools 回到 Claude 形态
        let tool = &back_to_claude["tools"][0];
        assert_eq!(tool["name"], "get_weather");
        assert_eq!(tool["description"], "Get weather for a city");
        assert_eq!(tool["input_schema"]["type"], "object");
        assert_eq!(tool["input_schema"]["properties"]["city"]["type"], "string");
    }

    /// tool_choice：Claude {type:"auto"} ↔ OpenAI "auto"
    #[test]
    fn request_tool_choice_auto_roundtrip() {
        let claude_original = json!({
            "model": TEST_MODEL,
            "messages": [{"role": "user", "content": "Hi"}],
            "max_tokens": 1024,
            "tool_choice": {"type": "auto"}
        });

        let openai_intermediate = claude_to_openai_request(&claude_original);
        assert_eq!(openai_intermediate["tool_choice"], "auto");

        let adapter = ClaudeAdapter;
        let mut back_to_claude = openai_intermediate.clone();
        adapter.transform_request(&mut back_to_claude, TEST_MODEL);

        assert_eq!(back_to_claude["tool_choice"]["type"], "auto");
    }

    /// **公理二的核心验证**：未知字段穿透
    /// Claude 请求带一个官方文档没有的自定义字段，
    /// 经过 claude → openai → claude 往返后必须还在。
    #[test]
    fn request_unknown_field_passthrough_top_level() {
        let claude_original = json!({
            "model": TEST_MODEL,
            "messages": [{"role": "user", "content": "Hi"}],
            "max_tokens": 1024,
            "x_api_switch_tracking_id": "abc-123",
            "x_future_anthropic_field": {"nested": "value"}
        });

        let openai_intermediate = claude_to_openai_request(&claude_original);

        // 中间格式里自定义字段应该还在
        assert_eq!(
            openai_intermediate["x_api_switch_tracking_id"], "abc-123",
            "claude_to_openai 丢失了 top-level 自定义字段 x_api_switch_tracking_id"
        );
        assert_eq!(
            openai_intermediate["x_future_anthropic_field"]["nested"], "value",
            "claude_to_openai 丢失了 top-level 嵌套自定义字段"
        );

        let adapter = ClaudeAdapter;
        let mut back_to_claude = openai_intermediate.clone();
        adapter.transform_request(&mut back_to_claude, TEST_MODEL);

        // 还原后仍然在
        assert_eq!(
            back_to_claude["x_api_switch_tracking_id"], "abc-123",
            "ClaudeAdapter.transform_request 丢失了自定义字段"
        );
        assert_eq!(
            back_to_claude["x_future_anthropic_field"]["nested"], "value",
            "ClaudeAdapter.transform_request 丢失了嵌套自定义字段"
        );
    }

    // ─── 响应方向：上游 adapter → 下游入口的 round-trip ──────────────

    /// 基础响应：上游 Claude 响应 → OpenAI 中间 → 下游 Claude 响应
    #[test]
    fn response_basic_text() {
        let claude_original = json!({
            "id": "msg_abc",
            "type": "message",
            "role": "assistant",
            "model": "claude-3-sonnet-20240229",
            "content": [
                {"type": "text", "text": "Hello!"}
            ],
            "stop_reason": "end_turn",
            "usage": {
                "input_tokens": 10,
                "output_tokens": 5
            }
        });

        // 上游 adapter 翻译：Claude → OpenAI
        let mut openai_intermediate = claude_original.clone();
        let adapter = ClaudeAdapter;
        adapter.transform_response(&mut openai_intermediate);

        // 下游入口翻译：OpenAI → Claude
        let back_to_claude = openai_to_claude_response(&openai_intermediate);

        assert_eq!(back_to_claude["id"], "msg_abc");
        assert_eq!(back_to_claude["type"], "message");
        assert_eq!(back_to_claude["role"], "assistant");
        // content 必须是 array 形式，包含 text block
        let content = back_to_claude["content"].as_array().expect("content should be array");
        assert_eq!(content.len(), 1);
        assert_eq!(content[0]["type"], "text");
        assert_eq!(content[0]["text"], "Hello!");
        assert_eq!(back_to_claude["stop_reason"], "end_turn");
        assert_eq!(back_to_claude["usage"]["input_tokens"], 10);
        assert_eq!(back_to_claude["usage"]["output_tokens"], 5);
    }

    /// tool_use 响应
    #[test]
    fn response_tool_use() {
        let claude_original = json!({
            "id": "msg_tool",
            "type": "message",
            "role": "assistant",
            "model": "claude-3-sonnet-20240229",
            "content": [
                {"type": "tool_use", "id": "toolu_1", "name": "get_weather", "input": {"city": "Tokyo"}}
            ],
            "stop_reason": "tool_use",
            "usage": {"input_tokens": 20, "output_tokens": 10}
        });

        let mut openai_intermediate = claude_original.clone();
        let adapter = ClaudeAdapter;
        adapter.transform_response(&mut openai_intermediate);

        let back_to_claude = openai_to_claude_response(&openai_intermediate);

        assert_eq!(back_to_claude["stop_reason"], "tool_use");
        let content = back_to_claude["content"].as_array().expect("content should be array");
        let tool_use = content.iter().find(|b| b["type"] == "tool_use").expect("tool_use block");
        assert_eq!(tool_use["id"], "toolu_1");
        assert_eq!(tool_use["name"], "get_weather");
        assert_eq!(tool_use["input"]["city"], "Tokyo");
    }

    /// **公理二的核心验证 #2**：响应方向的未知字段穿透
    /// 这是当前代码**违反公理最严重**的地方：
    /// openai_to_claude_response 用 json!({...}) 白名单构造新对象，
    /// 上游返回的所有非白名单字段都丢失。
    /// 本测试预期 FAIL，作为阶段 2 的修复目标。
    #[test]
    fn response_unknown_field_passthrough() {
        let claude_original = json!({
            "id": "msg_xyz",
            "type": "message",
            "role": "assistant",
            "model": "claude-3-sonnet-20240229",
            "content": [{"type": "text", "text": "Hi"}],
            "stop_reason": "end_turn",
            "usage": {"input_tokens": 5, "output_tokens": 3},
            "stop_sequence": null,
            "x_anthropic_future_field": "preserve_me",
            "container": {"id": "container_abc"}
        });

        let mut openai_intermediate = claude_original.clone();
        let adapter = ClaudeAdapter;
        adapter.transform_response(&mut openai_intermediate);

        let back_to_claude = openai_to_claude_response(&openai_intermediate);

        // 公理二：未知字段必须穿透，这边进去那边出来
        assert!(
            back_to_claude.get("x_anthropic_future_field").is_some(),
            "响应方向未知字段 x_anthropic_future_field 丢失（违反公理二，阶段 2 修复）"
        );
        assert_eq!(
            back_to_claude["x_anthropic_future_field"], "preserve_me",
            "响应方向未知字段值被改动"
        );
        assert!(
            back_to_claude.get("container").is_some(),
            "响应方向官方但非白名单字段 container 丢失（违反公理二，阶段 2 修复）"
        );
    }

    // ─── SSE 流式方向 round-trip ───────────────────────────────────

    /// **P1 bug 的目标行为**：
    /// 上游 OpenAI 流式响应的 usage-only 帧（最后一帧，choices=[]）
    /// 应该让 ClaudeSSETransformer 能把 output_tokens 传递给 Claude 客户端。
    ///
    /// 当前代码：ClaudeSSETransformer::transform_chunk 在看到 choices=[] 时
    /// early return，导致 usage 从未写入 self.usage_output_tokens，
    /// 最终 message_delta 永远带 output_tokens=0。
    ///
    /// 本测试预期 FAIL，作为阶段 3 的修复目标。
    #[test]
    fn sse_claude_usage_tokens_not_dropped() {
        let mut transformer =
            ClaudeSSETransformer::new("msg_test".to_string(), TEST_MODEL.to_string());

        // 标准 OpenAI 流式帧序列（启用 stream_options.include_usage）：
        // 1. 首帧：role=assistant
        let _ = transformer.transform_chunk(
            r#"{"id":"c1","choices":[{"delta":{"role":"assistant"},"finish_reason":null}]}"#,
        );

        // 2. 文本帧
        let _ = transformer.transform_chunk(
            r#"{"id":"c1","choices":[{"delta":{"content":"Hi"},"finish_reason":null}]}"#,
        );

        // 3. finish_reason 帧（此刻 message_delta 会被 emit）
        let finish_events = transformer.transform_chunk(
            r#"{"id":"c1","choices":[{"delta":{},"finish_reason":"stop"}]}"#,
        );

        // 4. usage-only 帧（OpenAI 规范允许 choices=[] 只带 usage）
        let usage_events = transformer.transform_chunk(
            r#"{"id":"c1","choices":[],"usage":{"prompt_tokens":10,"completion_tokens":20}}"#,
        );

        // 断言：Claude 客户端能拿到 output_tokens=20 的信息
        // 要么在 finish_events 里（usage-only 帧之前就知道），
        // 要么在 usage_events 里（usage-only 帧补发一次 message_delta）
        let all_events: Vec<Value> = finish_events
            .iter()
            .chain(usage_events.iter())
            .filter_map(|e| serde_json::from_str::<Value>(e).ok())
            .collect();

        let has_correct_usage = all_events.iter().any(|v| {
            v["type"] == "message_delta"
                && v.get("usage")
                    .and_then(|u| u.get("output_tokens"))
                    .and_then(Value::as_i64)
                    == Some(20)
        });

        assert!(
            has_correct_usage,
            "Claude 客户端的 output_tokens 永远是 0，真实 usage 被丢弃（P1 bug，阶段 3 修复）\n\
             实际发出的事件：{:#?}",
            all_events
        );
    }

    /// message_start 事件的 usage 也要能被获取到
    #[test]
    fn sse_claude_input_tokens_preserved() {
        let mut transformer =
            ClaudeSSETransformer::new("msg_test".to_string(), TEST_MODEL.to_string());

        // 首帧带 prompt_tokens
        let start_events = transformer.transform_chunk(
            r#"{"id":"c1","choices":[{"delta":{"role":"assistant"},"finish_reason":null}],"usage":{"prompt_tokens":15}}"#,
        );

        let found = start_events.iter().any(|e| {
            let v: Value = serde_json::from_str(e).unwrap_or(Value::Null);
            v["type"] == "message_start"
                && v["message"]["usage"]["input_tokens"].as_i64() == Some(15)
        });

        assert!(found, "message_start 事件未携带 input_tokens=15");
    }
}

// ═══════════════════════════════════════════════════════════════════
//  辅助函数测试
// ═══════════════════════════════════════════════════════════════════

#[cfg(test)]
mod helpers {
    use super::*;

    #[test]
    fn without_fields_removes_specified_keys() {
        let original = json!({"a": 1, "b": 2, "c": 3});
        let filtered = without_fields(original, &["b"]);
        assert_eq!(filtered, json!({"a": 1, "c": 3}));
    }
}


// ═══════════════════════════════════════════════════════════════════
//  Gemini 协议 round-trip 测试
//
//  注意：Gemini 上游 adapter 使用 OpenAI 兼容端点（v1beta/openai/），
//  所以上游方向几乎是直通。这里的 round-trip 主要验证下游方向：
//  Gemini 客户端 → OpenAI 中间 → 还原 Gemini。
// ═══════════════════════════════════════════════════════════════════

mod gemini_roundtrip {
    use super::*;
    use crate::proxy::protocol::{
        gemini_to_openai_request, openai_to_gemini_response,
    };

    /// 基础文本请求 round-trip
    #[test]
    fn request_basic_text() {
        let gemini_original = json!({
            "model": "gemini-1.5-pro",
            "contents": [
                {"role": "user", "parts": [{"text": "Hello"}]}
            ],
            "generationConfig": {
                "temperature": 0.7,
                "maxOutputTokens": 512
            }
        });

        let openai_intermediate = gemini_to_openai_request(&gemini_original);

        // 验证降维到 OpenAI 格式
        assert_eq!(openai_intermediate["model"], "gemini-1.5-pro");
        assert_eq!(openai_intermediate["messages"][0]["role"], "user");
        assert_eq!(openai_intermediate["messages"][0]["content"], "Hello");
        assert_eq!(openai_intermediate["temperature"], 0.7);
        assert_eq!(openai_intermediate["max_tokens"], 512);
    }

    /// 响应方向基础 round-trip
    #[test]
    fn response_basic_text() {
        let openai = json!({
            "id": "chatcmpl-abc",
            "object": "chat.completion",
            "model": "gemini-1.5-pro",
            "choices": [{
                "index": 0,
                "message": {"role": "assistant", "content": "Hi there"},
                "finish_reason": "stop"
            }],
            "usage": {"prompt_tokens": 10, "completion_tokens": 5}
        });

        let gemini = openai_to_gemini_response(&openai);

        assert_eq!(gemini["candidates"][0]["content"]["role"], "model");
        assert_eq!(gemini["candidates"][0]["content"]["parts"][0]["text"], "Hi there");
        assert_eq!(gemini["candidates"][0]["finishReason"], "STOP");
        assert_eq!(gemini["usageMetadata"]["promptTokenCount"], 10);
        assert_eq!(gemini["usageMetadata"]["candidatesTokenCount"], 5);
    }

    /// **公理二**：请求方向未知字段穿透
    /// Gemini `safetySettings`、`cachedContent` 等字段在 OpenAI 里没有对应，
    /// 必须穿透保留。
    #[test]
    fn request_unknown_field_passthrough() {
        let gemini_original = json!({
            "model": "gemini-1.5-pro",
            "contents": [{"role": "user", "parts": [{"text": "Hi"}]}],
            "safetySettings": [
                {"category": "HARM_CATEGORY_HARASSMENT", "threshold": "BLOCK_ONLY_HIGH"}
            ],
            "x_future_gemini_field": "preserve_me"
        });

        let openai_intermediate = gemini_to_openai_request(&gemini_original);

        assert!(
            openai_intermediate.get("safetySettings").is_some(),
            "gemini_to_openai 丢失了官方字段 safetySettings（阶段2 修）"
        );
        assert!(
            openai_intermediate.get("x_future_gemini_field").is_some(),
            "gemini_to_openai 丢失了自定义字段 x_future_gemini_field（阶段2 修）"
        );
    }

    /// **公理二**：响应方向未知字段穿透
    #[test]
    fn response_unknown_field_passthrough() {
        let openai = json!({
            "id": "chatcmpl-xyz",
            "model": "gemini-1.5-pro",
            "choices": [{
                "index": 0,
                "message": {"role": "assistant", "content": "Reply"},
                "finish_reason": "stop"
            }],
            "usage": {"prompt_tokens": 5, "completion_tokens": 3},
            "x_openai_future_field": "preserve_me",
            "system_fingerprint": "fp_abc123"
        });

        let gemini = openai_to_gemini_response(&openai);

        assert!(
            gemini.get("x_openai_future_field").is_some(),
            "openai_to_gemini_response 丢失未知字段 x_openai_future_field（阶段2 修）"
        );
        assert!(
            gemini.get("system_fingerprint").is_some(),
            "openai_to_gemini_response 丢失 system_fingerprint（阶段2 修）"
        );
    }
}

// ═══════════════════════════════════════════════════════════════════
//  Azure 协议 round-trip 测试
//
//  Azure OpenAI 本质就是 OpenAI 协议 + 不同 URL + 不同鉴权，
//  body 几乎直通。这里主要验证 body 最小变换的正确性和字段穿透。
// ═══════════════════════════════════════════════════════════════════

mod azure_roundtrip {
    use super::*;
    use crate::proxy::protocol::{azure::AzureAdapter, azure_to_openai_request, ProtocolAdapter};

    const TEST_DEPLOYMENT: &str = "gpt-4o-deployment";

    /// 基础请求 round-trip
    #[test]
    fn request_basic_roundtrip() {
        let azure_original = json!({
            "messages": [{"role": "user", "content": "Hi"}],
            "max_tokens": 100,
            "temperature": 0.7
        });

        let openai_intermediate = azure_to_openai_request(&azure_original, TEST_DEPLOYMENT);
        assert_eq!(openai_intermediate["model"], TEST_DEPLOYMENT);
        assert_eq!(openai_intermediate["messages"][0]["content"], "Hi");

        // 上游方向：AzureAdapter 移除 model 字段
        let mut back_to_azure = openai_intermediate.clone();
        let adapter = AzureAdapter;
        adapter.transform_request(&mut back_to_azure, TEST_DEPLOYMENT);

        // Azure 上游不要 body 里的 model（放在 URL 里）
        assert!(back_to_azure.get("model").is_none());
        assert_eq!(back_to_azure["messages"][0]["content"], "Hi");
    }

    /// **公理二**：请求方向未知字段穿透
    #[test]
    fn request_unknown_field_passthrough() {
        let azure_original = json!({
            "messages": [{"role": "user", "content": "Hi"}],
            "max_tokens": 100,
            "x_azure_custom_header": "track-123",
            "dataSources": [{"type": "AzureCognitiveSearch", "parameters": {}}]
        });

        let openai_intermediate = azure_to_openai_request(&azure_original, TEST_DEPLOYMENT);

        assert!(
            openai_intermediate.get("x_azure_custom_header").is_some(),
            "azure_to_openai_request 应该保留自定义字段"
        );
        assert!(
            openai_intermediate.get("dataSources").is_some(),
            "azure_to_openai_request 应该保留 Azure 专有字段 dataSources"
        );
    }
}

// ═══════════════════════════════════════════════════════════════════
//  OpenAI / Custom：基准协议，无翻译需求
//
//  OpenAI 和 Custom 作为基准协议不做翻译，只验证 adapter 的核心行为
//  （transform_request 设置正确的 model、URL 拼接等）不会破坏 body。
// ═══════════════════════════════════════════════════════════════════

mod openai_roundtrip {
    use super::*;
    use crate::proxy::protocol::ProtocolAdapter;

    /// OpenAI adapter 的 transform_request 只应修改 model 字段，其他保持原样
    #[test]
    fn openai_adapter_preserves_body() {
        // 通过 factory 构造（保持测试独立于 openai 模块的 public API）
        let adapter = crate::proxy::protocol::get_adapter("openai");
        let mut body = json!({
            "model": "auto",
            "messages": [{"role": "user", "content": "Hello"}],
            "temperature": 0.5,
            "x_custom": "preserve_me"
        });
        let original_messages = body["messages"].clone();
        let original_custom = body["x_custom"].clone();

        adapter.transform_request(&mut body, "gpt-4o");

        assert_eq!(body["model"], "gpt-4o");
        assert_eq!(body["messages"], original_messages);
        assert_eq!(body["temperature"], 0.5);
        assert_eq!(
            body["x_custom"], original_custom,
            "OpenAI adapter 不应丢弃自定义字段"
        );
    }

    /// Custom adapter 同上：只改 model
    #[test]
    fn custom_adapter_preserves_body() {
        let adapter = crate::proxy::protocol::get_adapter("custom");
        let mut body = json!({
            "model": "auto",
            "messages": [{"role": "user", "content": "Hi"}],
            "x_deepseek_specific": "value"
        });

        adapter.transform_request(&mut body, "deepseek-chat");

        assert_eq!(body["model"], "deepseek-chat");
        assert_eq!(
            body["x_deepseek_specific"], "value",
            "Custom adapter 应保留所有非 model 字段"
        );
    }
}
