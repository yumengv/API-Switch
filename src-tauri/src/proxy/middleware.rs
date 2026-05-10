use serde_json::Value;

/// 调用方类型
#[derive(Clone)]
#[allow(dead_code)]
pub enum CallerKind {
    OpenAiChat,
    ClaudeMessages,
    GeminiNative,
    AzureChat,
    Responses,
}

/// 请求上下文，传递给中间件
#[allow(dead_code)]
pub struct RequestContext<'a> {
    pub caller_kind: CallerKind,
    pub requested_model: &'a str,
}

/// 转发器中间件 trait
#[allow(dead_code)]
pub trait ForwarderMiddleware: Send + Sync {
    /// 请求体发出前
    fn on_request(&self, _body: &mut Value, _ctx: &RequestContext) {}
    /// 响应体接收后（非流式）
    fn on_response_complete(&self, _body: &mut Value, _ctx: &RequestContext) {}
    /// SSE 数据块（流式）
    fn on_sse_chunk(&self, _chunk: &mut String, _ctx: &RequestContext) {}
}

/// P2 修复：stream_options 不再无条件覆盖，改用 or_insert
#[allow(dead_code)]
pub struct StreamOptionsMiddleware;

impl ForwarderMiddleware for StreamOptionsMiddleware {
    fn on_request(&self, body: &mut Value, _ctx: &RequestContext) {
        if !body.get("stream").and_then(|s| s.as_bool()).unwrap_or(false) {
            return;
        }
        if let Some(obj) = body.as_object_mut() {
            let so = obj
                .entry("stream_options".to_string())
                .or_insert(serde_json::json!({}));
            if let Some(so_obj) = so.as_object_mut() {
                so_obj
                    .entry("include_usage".to_string())
                    .or_insert(serde_json::json!(true));
            }
        }
    }
}

/// P5 修复：ModelAnnotationMiddleware - Responses 入口不注入 model:xxx
#[allow(dead_code)]
pub struct ModelAnnotationMiddleware;

impl ForwarderMiddleware for ModelAnnotationMiddleware {
    fn on_sse_chunk(&self, chunk: &mut String, ctx: &RequestContext) {
        // Responses 入口不装这个中间件，本方法永不被调用
        // OpenAiChat / ClaudeMessages / Azure 装，所以对这些入口照常注入
        if matches!(ctx.caller_kind, CallerKind::Responses) {
            return;
        }

        // 注入 model:xxx 到 SSE 流中
        let model = ctx.requested_model;
        let payload = serde_json::json!({
            "choices": [{
                "delta": {
                    "content": format!("\n\nmodel: {model}")
                }
            }]
        });
        chunk.push_str(&format!("data: {payload}\n\n"));
    }
}

/// P6 修复：IdleTimeoutMiddleware - 每层 SSE 流都有 idle timeout
#[allow(dead_code)]
pub struct IdleTimeoutMiddleware {
    timeout_secs: u64,
}

impl IdleTimeoutMiddleware {
    pub fn new(timeout_secs: u64) -> Self {
        Self { timeout_secs }
    }
}

impl ForwarderMiddleware for IdleTimeoutMiddleware {
    fn on_sse_chunk(&self, _chunk: &mut String, _ctx: &RequestContext) {
        let _ = self.timeout_secs;
        // 每次收到 chunk 重置 timer（在 forwarder.rs 中实现）
        // 超时 → 中断流（在 forwarder.rs 中实现）
        // 这里只是标记中间件存在，实际逻辑在 forwarder.rs 中
    }
}

/// UsageLoggingMiddleware - 日志记录中间件
#[allow(dead_code)]
pub struct UsageLoggingMiddleware;

impl ForwarderMiddleware for UsageLoggingMiddleware {
    fn on_response_complete(&self, _body: &mut Value, _ctx: &RequestContext) {
        // 日志记录逻辑在 forwarder.rs 中实现
        // 这里只是标记中间件存在
    }
}

/// CircuitBreakerMiddleware - 熔断器中间件
#[allow(dead_code)]
pub struct CircuitBreakerMiddleware;

impl ForwarderMiddleware for CircuitBreakerMiddleware {
    fn on_response_complete(&self, _body: &mut Value, _ctx: &RequestContext) {
        // 熔断逻辑在 forwarder.rs 中实现
        // 这里只是标记中间件存在
    }
}
