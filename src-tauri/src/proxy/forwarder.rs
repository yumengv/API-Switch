use super::circuit_breaker::CircuitBreaker;
use super::handlers::ProxyError;
use super::middleware::{CallerKind, RequestContext};
use super::protocol::get_adapter;
use super::server::ProxyState;
use crate::database::{AccessKey, ApiEntry, AppSettings, Database};
use crate::refresh_tray_if_enabled;
use crate::services::api_key_utils::primary_api_key;
use axum::body::Body;
use axum::http::{HeaderMap, HeaderValue};
use axum::response::IntoResponse;
use bytes::Bytes;
use futures::Stream;
use serde_json::Value;
use std::error::Error;
use std::future::Future;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::Arc;
use std::task::Poll;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tauri::Emitter;
use tokio::time::sleep;

const STREAMING_IDLE_TIMEOUT: Duration = Duration::from_secs(300);
const STREAMING_PING_INTERVAL: Duration = Duration::from_secs(10);
const RAW_RESPONSES_REQUEST_FIELD: &str = "__as_raw_responses_req";
const RESPONSES_PASSTHROUGH_HEADER: &str = "x-api-switch-responses-passthrough";


#[derive(Debug, Default)]
struct SseDoneState {
    seen_done: bool,
    appended_model_info: bool,
    upstream_model_info_seen: bool,
}

#[derive(Debug, Clone, Copy)]
enum StreamEndReason {
    Done,
    UpstreamError,
    Timeout,
    DecodeTimeout,
    Dropped,
}

impl StreamEndReason {
    fn as_str(self) -> &'static str {
        match self {
            StreamEndReason::Done => "done",
            StreamEndReason::UpstreamError => "upstream_error",
            StreamEndReason::Timeout => "timeout",
            StreamEndReason::DecodeTimeout => "decode_timeout",
            StreamEndReason::Dropped => "dropped",
        }
    }
}

fn is_completed_stream_success(
    status_code: i32,
    has_sse_error: bool,
    has_text_delta: bool,
    has_tool_calls: bool,
    completion_tokens: i64,
) -> bool {
    // 仅在 HTTP 200、无 SSE 错误且存在有效输出时判定成功。
    // 有效输出定义为：出现文本 delta、出现工具调用、或返回的 completion_tokens>0。
    status_code == 200
        && !has_sse_error
        && (has_text_delta || has_tool_calls || completion_tokens > 0)
}

fn is_recoverable_decode_timeout_after_stream_started(
    status_code: i32,
    first_token_ms: i64,
    chunk_count: i64,
    streamed_bytes: i64,
    has_text_delta: bool,
    has_tool_calls: bool,
    completion_tokens: i64,
    has_sse_error: bool,
    is_timeout: bool,
    is_decode: bool,
    is_body: bool,
) -> bool {
    let stream_started =
        status_code == 200 && (first_token_ms > 0 || chunk_count > 0 || streamed_bytes > 0);
    let has_valid_output = has_text_delta || has_tool_calls || completion_tokens > 0;

    // 临时止血：HTTP 200 且已经开始推流后出现 body/decode timeout，
    // 更像传输中途不完整，而不是模型、Token 或上游入口不可用。
    // 本次请求仍记录失败，但不触发普通冷却/长期禁用阈值。
    stream_started && is_timeout && is_decode && !is_body && !has_valid_output && !has_sse_error
}

fn is_decode_timeout_after_stream_started(
    err: &reqwest::Error,
    status_code: i32,
    first_token_ms: i64,
    chunk_count: i64,
    streamed_bytes: i64,
    has_text_delta: bool,
    has_tool_calls: bool,
    completion_tokens: i64,
    has_sse_error: bool,
) -> bool {
    is_recoverable_decode_timeout_after_stream_started(
        status_code,
        first_token_ms,
        chunk_count,
        streamed_bytes,
        has_text_delta,
        has_tool_calls,
        completion_tokens,
        has_sse_error,
        err.is_timeout(),
        err.is_decode(),
        err.is_body(),
    )
}

/// 判断客户端断开的流是否算成功。
/// 修复: 原逻辑要求 prompt_tokens > 0 || completion_tokens > 0，但很多上游 API 在流式响应中
/// 不返回 usage 信息，导致客户端断开时即使模型正常工作也被误判为失败。
/// 只要 status_code == 200 就算成功（客户端断开不代表模型故障）。
fn is_dropped_stream_success(
    status_code: i32,
    _prompt_tokens: i64,
    _completion_tokens: i64,
) -> bool {
    status_code == 200
}

#[derive(Debug, Clone)]
struct AttemptInfo {
    entry_id: String,
    channel_name: String,
    model: String,
    status_code: i32,
    success: bool,
    error: Option<String>,
}

fn attempt_path_json(attempts: &[AttemptInfo]) -> String {
    serde_json::to_string(
        &attempts
            .iter()
            .map(|a| {
                serde_json::json!({
                    "entry_id": a.entry_id,
                    "channel": a.channel_name,
                    "model": a.model,
                    "status_code": a.status_code,
                    "success": a.success,
                    "error": a.error,
                })
            })
            .collect::<Vec<_>>(),
    )
    .unwrap_or_else(|_| "[]".to_string())
}

fn push_attempt(
    attempts: &mut Vec<AttemptInfo>,
    entry: &ApiEntry,
    status_code: i32,
    success: bool,
    error: Option<String>,
) {
    attempts.push(AttemptInfo {
        entry_id: entry.id.clone(),
        channel_name: entry
            .channel_name
            .clone()
            .unwrap_or_else(|| "unknown".to_string()),
        model: entry.model.clone(),
        status_code,
        success,
        error,
    });
}

fn attempt_path_with_current(
    prior_attempts: &[AttemptInfo],
    entry: &ApiEntry,
    status_code: i32,
    success: bool,
    error: Option<String>,
) -> String {
    let mut attempts = prior_attempts.to_vec();
    push_attempt(&mut attempts, entry, status_code, success, error);
    attempt_path_json(&attempts)
}

fn sanitize_url_for_log(url: &str) -> String {
    let mut sanitized = url.to_string();
    for key in ["key", "api_key", "access_token", "token"] {
        let marker = format!("{key}=");
        let mut search_from = 0;
        while let Some(pos) = sanitized[search_from..].find(&marker) {
            let value_start = search_from + pos + marker.len();
            let value_end = sanitized[value_start..]
                .find('&')
                .map(|offset| value_start + offset)
                .unwrap_or(sanitized.len());
            sanitized.replace_range(value_start..value_end, "***");
            search_from = value_start + 3;
        }
    }
    sanitized
}

fn response_header_value(headers: &reqwest::header::HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(|value| value.to_string())
}

fn build_stream_diagnostic(
    stage: &str,
    err: Option<&reqwest::Error>,
    entry: &ApiEntry,
    url: &str,
    status_code: i32,
    headers: &reqwest::header::HeaderMap,
    chunk_count: i64,
    streamed_bytes: i64,
    buffered_sse_bytes: usize,
    first_token_ms: i64,
    elapsed_ms: i64,
    prompt_tokens: i64,
    completion_tokens: i64,
    has_text_delta: bool,
    has_tool_calls: bool,
    has_sse_error: bool,
    stream_success: Option<bool>,
) -> String {
    let has_valid_output = has_text_delta || has_tool_calls || completion_tokens > 0;
    let detail = serde_json::json!({
        "stage": stage,
        "channel": entry.channel_name.clone().unwrap_or_else(|| "unknown".to_string()),
        "entry_id": entry.id,
        "resolved_model": entry.model,
        "url": sanitize_url_for_log(url),
        "status_code": status_code,
        "elapsed_ms": elapsed_ms,
        "first_token_ms": first_token_ms,
        "chunk_count": chunk_count,
        "streamed_bytes": streamed_bytes,
        "buffered_sse_bytes": buffered_sse_bytes,
        "prompt_tokens": prompt_tokens,
        "completion_tokens": completion_tokens,
        "has_text_delta": has_text_delta,
        "has_tool_calls": has_tool_calls,
        "has_sse_error": has_sse_error,
        "has_valid_output": has_valid_output,
        "stream_success": stream_success,
        "content_type": response_header_value(headers, "content-type"),
        "content_encoding": response_header_value(headers, "content-encoding"),
        "transfer_encoding": response_header_value(headers, "transfer-encoding"),
        "server": response_header_value(headers, "server"),
        "via": response_header_value(headers, "via"),
        "cf_ray": response_header_value(headers, "cf-ray"),
        "x_request_id": response_header_value(headers, "x-request-id"),
        "x_accel_buffering": response_header_value(headers, "x-accel-buffering"),
        "error": err.map(|e| serde_json::json!({
            "kind": reqwest_error_kind(e),
            "is_timeout": e.is_timeout(),
            "is_connect": e.is_connect(),
            "is_request": e.is_request(),
            "is_body": e.is_body(),
            "is_decode": e.is_decode(),
            "is_redirect": e.is_redirect(),
            "source_chain": reqwest_source_chain(e),
            "message": e.to_string(),
        })),
    });
    format!("Stream diagnostic: {detail}")
}

fn reqwest_error_kind(err: &reqwest::Error) -> &'static str {
    if err.is_timeout() {
        "timeout"
    } else if err.is_connect() {
        "connect"
    } else if err.is_request() {
        "request"
    } else if err.is_body() {
        "body"
    } else if err.is_decode() {
        "decode"
    } else if err.is_redirect() {
        "redirect"
    } else {
        "unknown"
    }
}

fn reqwest_source_chain(err: &reqwest::Error) -> Vec<String> {
    let mut chain = Vec::new();
    let mut source = err.source();
    while let Some(current) = source {
        chain.push(current.to_string());
        source = current.source();
    }
    chain
}

fn format_reqwest_error(stage: &str, url: &str, err: reqwest::Error) -> String {
    let detail = serde_json::json!({
        "stage": stage,
        "url": sanitize_url_for_log(url),
        "kind": reqwest_error_kind(&err),
        "is_timeout": err.is_timeout(),
        "is_connect": err.is_connect(),
        "is_request": err.is_request(),
        "is_body": err.is_body(),
        "is_decode": err.is_decode(),
        "is_redirect": err.is_redirect(),
        "source_chain": reqwest_source_chain(&err),
        "message": err.to_string(),
    });
    format!("Request failed: {detail}")
}

/// Forward error with upstream status code (0 = connection failure).
type ForwardError = (String, u16);

struct ForwardResult {
    response: axum::response::Response,
    prompt_tokens: i64,
    completion_tokens: i64,
    reasoning_tokens: i64,
    first_token_ms: i64,
    status_code: i32,
}

/// StreamLogGuard: safety net for writing usage log when stream is dropped
/// without reaching Poll::Ready(None) (e.g. client disconnect).
/// Primary log writing happens in Poll::Ready(None) — this guard is fallback only.
struct StreamLogGuard {
    logged: Arc<AtomicBool>,
    db: Arc<Database>,
    app_handle: Option<tauri::AppHandle>,
    access_key: Option<AccessKey>,
    entry: ApiEntry,
    requested_model: String,
    prompt_tokens: Arc<AtomicI64>,
    completion_tokens: Arc<AtomicI64>,
    first_token_ms: Arc<AtomicI64>,
    chunk_count: Arc<AtomicI64>,
    streamed_bytes: Arc<AtomicI64>,
    status_code: i32,
    start: Instant,
    prior_attempts: Vec<AttemptInfo>,
    upstream_url: String,
    response_headers: reqwest::header::HeaderMap,
    /// 空流检测冷却时长（秒），取自 settings.circuit_recovery_secs
    empty_stream_cooldown_secs: i64,
}

impl Drop for StreamLogGuard {
    fn drop(&mut self) {
        if !self.logged.swap(true, Ordering::SeqCst) {
            let prompt_tokens = self.prompt_tokens.load(Ordering::SeqCst);
            let completion_tokens = self.completion_tokens.load(Ordering::SeqCst);
            let chunk_total = self.chunk_count.load(Ordering::SeqCst);
            let byte_total = self.streamed_bytes.load(Ordering::SeqCst);
            let first_token_ms = self.first_token_ms.load(Ordering::SeqCst);
            let latency_ms = self.start.elapsed().as_millis() as i64;
            let success =
                is_dropped_stream_success(self.status_code, prompt_tokens, completion_tokens);
            let attempt_path = attempt_path_with_current(
                &self.prior_attempts,
                &self.entry,
                self.status_code,
                success,
                None,
            );
            let stream_summary = build_stream_diagnostic(
                "stream_dropped",
                None,
                &self.entry,
                &self.upstream_url,
                self.status_code,
                &self.response_headers,
                chunk_total,
                byte_total,
                0, // sse_buffer not available in drop context
                first_token_ms,
                latency_ms,
                prompt_tokens,
                completion_tokens,
                false,
                false,
                false,
                Some(success),
            );
            let db = self.db.clone();
            let app_handle = self.app_handle.clone();
            let access_key = self.access_key.clone();
            let entry = self.entry.clone();
            let requested_model = self.requested_model.clone();
            let status_code = self.status_code;
            let empty_stream_cooldown_secs = self.empty_stream_cooldown_secs;
            tokio::spawn(async move {
                log_usage(
                    &db,
                    &app_handle,
                    access_key.as_ref(),
                    &entry,
                    &requested_model,
                    true,
                    prompt_tokens,
                    completion_tokens,
                    0,
                    first_token_ms,
                    latency_ms,
                    status_code,
                    success,
                    Some(stream_summary.as_str()),
                    Some(attempt_path.as_str()),
                    Some(StreamEndReason::Dropped),
                );

                // 空流检测：上游返回 200 但 SSE 流中无实际输出数据。
                // 此类流被丢弃后，如果仅标记 success=true 不触发冷却，
                // 坏通道会持续留在路由池中反复被选中，导致下游反复中断。
                // 此处设置 DB cooldown 让路由跳过该 entry，但不递增
                // failure_counts，防止触发 6h 自动禁用。
                if status_code == 200
                    && byte_total < 512
                    && chunk_total < 5
                    && prompt_tokens == 0
                    && completion_tokens == 0
                {
                    let cooldown_until =
                        chrono::Utc::now().timestamp() + empty_stream_cooldown_secs;
                    let _ = db.set_entry_cooldown(&entry.id, Some(cooldown_until));
                    if let Some(h) = &app_handle {
                        let _ = h.emit("entries-changed", ());
                    }
                    crate::state_version::bump("pool");
                    log::warn!(
                        "Entry {} cooled down for {}s after empty stream drop.",
                        entry.id,
                        empty_stream_cooldown_secs
                    );
                }
            });
        }
    }
}

/// Forward a request to the resolved entries with retry/failover.
///
/// Personal-version cooldown strategy:
/// 1. Any upstream failure is considered abnormal for this model entry.
/// 2. Failed entries are cooled down for `circuit_recovery_secs` seconds and skipped by routing.
/// 3. Unrecoverable status codes can disable an entry automatically.
pub async fn forward_with_retry(
    state: &ProxyState,
    entries: &[ApiEntry],
    body: &Value,
    _original_headers: &HeaderMap,
    requested_model: &str,
    access_key: Option<&AccessKey>,
    is_stream: bool,
    middleware: &[Arc<dyn super::middleware::ForwarderMiddleware>],
    caller_kind: CallerKind,
) -> Result<axum::response::Response, ProxyError> {
    let mut last_error: Option<(String, u16)> = None;
    let mut attempts: Vec<AttemptInfo> = Vec::new();

    for entry in entries {
        let start = Instant::now();

        // Check circuit breaker
        {
            let breakers = state.circuit_breakers.read().await;
            if let Some(cb) = breakers.get(&entry.id) {
                if !cb.is_available() {
                    continue;
                }
            }
        }

        match forward_single(
            state,
            entry,
            body,
            requested_model,
            access_key,
            is_stream,
            attempts.clone(),
            middleware,
            &caller_kind,
        )
        .await
        {
            Ok(result) => {
                let elapsed = start.elapsed();

                if !is_stream {
                    record_circuit_success(state, &entry.id).await;
                    push_attempt(&mut attempts, entry, result.status_code, true, None);
                    let attempt_path = attempt_path_json(&attempts);
                    let latency_ms = elapsed.as_millis() as i64;
                    log_usage(
                        &state.db,
                        &state.app_handle,
                        access_key,
                        entry,
                        requested_model,
                        is_stream,
                        result.prompt_tokens,
                        result.completion_tokens,
                        result.reasoning_tokens,
                        result.first_token_ms,
                        latency_ms,
                        result.status_code,
                        true,
                        None,
                        Some(attempt_path.as_str()),
                        None,
                    );
                }
                return Ok(result.response);
            }
            Err((e, status)) => {
                let elapsed = start.elapsed();
                let latency_ms = elapsed.as_millis() as i64;
                let log_status = if status > 0 { status as i32 } else { 502 };
                let settings = state.settings.read().await.clone();
                push_attempt(&mut attempts, entry, log_status, false, Some(e.clone()));
                let attempt_path = attempt_path_json(&attempts);

                // Step 1: Always write usage log for every failed attempt
log_usage(
                        &state.db,
                        &state.app_handle,
                        access_key,
                        entry,
                        requested_model,
                        is_stream,
                        0,
                        0,
                        0,
                        0,
                        latency_ms,
                        log_status,
                        false,
                        Some(&e),
                        Some(attempt_path.as_str()),
                        None,
                    );

                // Step 2: disable unrecoverable status codes or error messages
                // matching disable keywords; otherwise cool down briefly.
                // Connection failures report status=0 and must remain recoverable.
                let disable_by_status = status > 0
                    && should_disable_entry_for_status(&settings.circuit_disable_codes, status);
                let effective_keywords = if settings.disable_keywords.trim().is_empty() {
                    // 默认关键词，用于没有自定义关键词的情况
                    "Your credit balance is too low\nThis organization has been disabled.\nYou exceeded your current quota\nPermission denied\nThe security token included in the request is invalid\nOperation not allowed\nYour account is not authorized\ninsufficient_quota\nquota_exceeded_error\ntoken plan limit exhausted\nUpstream rate limit exceeded\ninvalid api key\nUnauthorized - Invalid token"
                } else {
                    &settings.disable_keywords
                };
                let disable_by_keyword =
                    status > 0 && should_disable_entry_for_message(effective_keywords, &e);

                if disable_by_keyword {
                    log::info!(
                        "Cooldown entry {} (keyword match), status={}: {}",
                        entry.id,
                        status,
                        e
                    );
                    if settings.keyword_freeze_scope == "channel" {
                        freeze_channel_entries(state, entry).await;
                    } else {
                        cool_down_entry(state, entry).await;
                    }
                } else if disable_by_status {
                    disable_entry(state, entry).await;
                } else {
                    cool_down_entry(state, entry).await;
                }

                last_error = Some((e, status));
                continue;
            }
        }
    }

    Err(last_error
        .map(|(msg, status)| {
            if status > 0 {
                ProxyError::Upstream {
                    status,
                    message: msg,
                }
            } else {
                ProxyError::Internal(msg)
            }
        })
        .unwrap_or(ProxyError::AllProvidersFailed))
}

fn remove_reasoning_trigger_fields(value: &mut Value) {
    let Some(obj) = value.as_object_mut() else {
        return;
    };

    obj.remove("thinking");
    obj.remove("reasoning");
    obj.remove("reasoning_content");
    obj.remove("reasoning_text");
    obj.remove("reasoning_details");
    obj.remove("reasoning_effort");
}

fn apply_disable_reasoning(body: &mut Value) {
    remove_reasoning_trigger_fields(body);

    if let Some(messages) = body.get_mut("messages").and_then(Value::as_array_mut) {
        for message in messages {
            remove_reasoning_trigger_fields(message);
        }
    }
}

async fn forward_single(
    state: &ProxyState,
    entry: &ApiEntry,
    body: &Value,
    requested_model: &str,
    access_key: Option<&AccessKey>,
    is_stream: bool,
    prior_attempts: Vec<AttemptInfo>,
    middleware: &[Arc<dyn super::middleware::ForwarderMiddleware>],
    caller_kind: &CallerKind,
) -> Result<ForwardResult, ForwardError> {
    let channel = state
        .db
        .get_channel(&entry.channel_id)
        .map_err(|e| (format!("DB error: {e}"), 502))?;

    let adapter = get_adapter(&channel.api_type);
    let url = adapter.build_chat_url(&channel.base_url, &entry.model);
    let is_responses_passthrough = matches!(caller_kind, CallerKind::Responses)
        && channel.api_type == "responses"
        && body.get(RAW_RESPONSES_REQUEST_FIELD).is_some();

    let mut upstream_body = if is_responses_passthrough {
        let mut raw = body
            .get(RAW_RESPONSES_REQUEST_FIELD)
            .cloned()
            .unwrap_or_else(|| body.clone());
        if let Some(obj) = raw.as_object_mut() {
            obj.insert("model".to_string(), Value::String(entry.model.clone()));
            obj.remove(RAW_RESPONSES_REQUEST_FIELD);
        }
        raw
    } else {
        let mut converted = body.clone();
        adapter.transform_request(&mut converted, &entry.model);
        converted
    };

    if let Some(obj) = upstream_body.as_object_mut() {
        obj.remove(RAW_RESPONSES_REQUEST_FIELD);
    }

    if state.settings.read().await.disable_reasoning {
        apply_disable_reasoning(&mut upstream_body);
    }

    // Call middleware on_request
    let ctx = RequestContext {
        caller_kind: caller_kind.clone(),
        requested_model: Arc::<str>::from(requested_model.to_string()),
    };
    for mw in middleware.iter() {
        mw.on_request(&mut upstream_body, &ctx);
    }

    let mut request = adapter
        .apply_auth(
            state.http_client.post(&url),
            primary_api_key(&channel.api_key),
        )
        .json(&upstream_body);

    if is_stream {
        request = request.header("Accept", "text/event-stream");
        request = request.header("Accept-Encoding", "identity");
    }

    // Start timer BEFORE sending request — this measures true TTFB
    let request_start = std::time::Instant::now();

    let response = request
        .send()
        .await
        .map_err(|e| (format_reqwest_error("send", &url, e), 0))?;

    let status = response.status().as_u16();

    if !response.status().is_success() {
        // Read full error body for debugging
        let raw_body = response.bytes().await.unwrap_or_default();
        let error_body = String::from_utf8_lossy(&raw_body);

        // If error body is empty, just use status code
        let error_msg = if error_body.is_empty() {
            format!("Upstream error {status}")
        } else {
            format!("Upstream error {status}: {error_body}")
        };
        return Err((error_msg, status));
    }

    let status_code = status as i32;

    if is_stream {
        let needs_transform = !is_responses_passthrough && adapter.needs_sse_transform();
        let append_model_info = should_append_model_info(state, body, caller_kind);
        let mut response = build_streaming_response(
            state,
            entry,
            access_key,
            requested_model,
            &url,
            response,
            status_code,
            needs_transform,
            adapter,
            request_start,
            prior_attempts,
            append_model_info,
            middleware,
            &ctx,
        );
        if is_responses_passthrough {
            response.headers_mut().insert(
                RESPONSES_PASSTHROUGH_HEADER,
                HeaderValue::from_static("true"),
            );
        }
        Ok(ForwardResult {
            response,
            prompt_tokens: 0,
            completion_tokens: 0,
            reasoning_tokens: 0,
            first_token_ms: 0,
            status_code,
        })
    } else {
        let mut response_body: Value = response
            .json()
            .await
            .map_err(|e| (format!("Failed to parse response: {e}"), 502))?;

        if !is_responses_passthrough {
            adapter.transform_response(&mut response_body);

            // Normalize reasoning fields in response messages (reasoning_content ↔ reasoning_text)
            if let Some(choices) = response_body
                .get_mut("choices")
                .and_then(|c| c.as_array_mut())
            {
                for choice in choices.iter_mut() {
                    if let Some(message) = choice.get_mut("message") {
                        normalize_reasoning_fields(message);
                    }
                }
            }
        }

        for mw in middleware.iter() {
            mw.on_response_complete(&mut response_body, &ctx);
        }
        let (prompt_tokens, completion_tokens, reasoning_tokens) = extract_usage_tokens(&response_body);

        let has_valid_output = if is_responses_passthrough {
            responses_response_has_valid_output(&response_body)
        } else {
            nonstream_response_has_valid_output(&response_body)
        };
        if !has_valid_output {
            return Err((
                "upstream HTTP 200 completed without valid output".to_string(),
                502,
            ));
        }

        let mut response = axum::Json(response_body).into_response();
        if is_responses_passthrough {
            response.headers_mut().insert(
                RESPONSES_PASSTHROUGH_HEADER,
                HeaderValue::from_static("true"),
            );
        }

        Ok(ForwardResult {
            response,
            prompt_tokens,
            completion_tokens,
            reasoning_tokens,
            first_token_ms: 0,
            status_code,
        })
    }
}

fn extract_usage_tokens(body: &Value) -> (i64, i64, i64) {
    let usage = body
        .get("usage")
        .or_else(|| body.pointer("/response/usage"));
    let prompt_tokens = usage
        .and_then(|v| v.get("prompt_tokens"))
        .or_else(|| usage.and_then(|v| v.get("input_tokens")))
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let completion_tokens = usage
        .and_then(|v| v.get("completion_tokens"))
        .or_else(|| usage.and_then(|v| v.get("output_tokens")))
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let reasoning_tokens = usage
        .and_then(|v| v.pointer("/completion_tokens_details/reasoning_tokens"))
        .or_else(|| usage.and_then(|v| v.pointer("/output_tokens_details/reasoning_tokens")))
        .and_then(Value::as_i64)
        .unwrap_or(0);
    (prompt_tokens, completion_tokens, reasoning_tokens)
}

fn responses_response_has_valid_output(body: &Value) -> bool {
    body.get("object").and_then(Value::as_str) == Some("response")
        && body
            .get("output")
            .and_then(Value::as_array)
            .is_some_and(|output| !output.is_empty())
}

fn nonstream_response_has_valid_output(body: &Value) -> bool {
    body.get("choices")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .any(|choice| {
            let message = choice.get("message");
            let has_content = message
                .and_then(|msg| msg.get("content"))
                .and_then(Value::as_str)
                .is_some_and(|content| !content.is_empty());
            let has_tool_calls = message
                .and_then(|msg| msg.get("tool_calls"))
                .and_then(Value::as_array)
                .is_some_and(|tool_calls| !tool_calls.is_empty());
            let has_function_call = message
                .and_then(|msg| msg.get("function_call"))
                .is_some_and(|function_call| !function_call.is_null());
            // 推理模型可能返回 reasoning_content 而无 content
            let has_reasoning = message.is_some_and(|msg| {
                msg.get("reasoning_content").and_then(Value::as_str).is_some_and(|v| !v.is_empty())
                    || msg.get("reasoning_text").and_then(Value::as_str).is_some_and(|v| !v.is_empty())
                    || msg.get("reasoning_details").and_then(Value::as_str).is_some_and(|v| !v.is_empty())
            });

            has_content || has_tool_calls || has_function_call || has_reasoning
        })
}

fn request_uses_structured_output(body: &Value) -> bool {
    body.get("response_format").is_some()
}

/// 递归扫描 body 的任一位置是否存在 tool calling 相关字段。
///
/// **保留但不在生产路径使用**——历史上 `should_append_model_info` 曾用此函数
/// 在请求侧屏蔽 model 注入（commit 3f5825d），但这会误屏蔽所有 agent 客户端
/// （Cursor / Claude Code 等几乎每次请求都带 `tools` 字段，即使本轮不实际调用）。
/// 现在请求侧检查已撤销，真正的 tool-calling 屏蔽在响应侧 `has_tool_calls`。
///
/// 函数保留供未来可能的请求侧精细化判断重用，不是死代码。
#[allow(dead_code)]
fn contains_tool_calling_field(value: &Value) -> bool {
    match value {
        Value::Object(map) => {
            if map.get("tools").is_some()
                || map.get("tool_choice").is_some()
                || map.get("functions").is_some()
                || map.get("function_call").is_some()
                || map.get("parallel_tool_calls").is_some()
                || map.get("max_tool_calls").is_some()
                || map.get("tool_calls").is_some()
            {
                return true;
            }

            map.values().any(contains_tool_calling_field)
        }
        Value::Array(items) => items.iter().any(contains_tool_calling_field),
        _ => false,
    }
}

#[allow(dead_code)]
fn request_uses_tool_calling(body: &Value) -> bool {
    contains_tool_calling_field(body)
}

fn should_append_model_info(
    state: &ProxyState,
    body: &Value,
    _caller_kind: &super::middleware::CallerKind,
) -> bool {
    // Responses 协议自带原生 `response.model` 字段，绝不能向 output_text 正文
    // 追加 `model: xxx`，否则会污染客户端的 output_text。P5 修复。

    let setting_enabled = state
        .settings
        .try_read()
        .map(|settings| settings.show_conversation_model)
        .unwrap_or(true);

    // 结构化输出（response_format=json_object/json_schema）必须屏蔽注入：
    // 正文会被客户端当成 JSON 解析，追加 "model: xxx" 会破坏 JSON 合法性。
    //
    // 工具调用上下文不在请求侧屏蔽：agent 客户端（Cursor、Claude Code 等）
    // 几乎每次请求都带 `tools` 字段声明可用工具，即使本轮不实际调用。
    // 如果在请求侧一律屏蔽，大部分 agent 场景永远看不到模型名。
    // 真正的屏蔽发生在响应侧 `has_tool_calls`（参见 build_streaming_response
    // 和 append_and_parse_sse / transform_sse_chunk 的 gate 条件）：
    // 只有上游实际返回 tool_calls / function_call 时才跳过注入，避免 CALL
    // 循环里累积多个 `model: xxx`。
    setting_enabled && !request_uses_structured_output(body)
}

fn build_streaming_response(
    state: &ProxyState,
    entry: &ApiEntry,
    access_key: Option<&AccessKey>,
    requested_model: &str,
    upstream_url: &str,
    response: reqwest::Response,
    status_code: i32,
    needs_transform: bool,
    adapter: Box<dyn super::protocol::ProtocolAdapter + Send + Sync>,
    request_start: std::time::Instant,
    prior_attempts: Vec<AttemptInfo>,
    append_model_info: bool,
    middleware: &[Arc<dyn super::middleware::ForwarderMiddleware>],
    ctx: &RequestContext,
) -> axum::response::Response {
    let response_headers = response.headers().clone();
    let upstream_url = upstream_url.to_string();
    let start = request_start;
    let middleware = middleware.to_vec();
    let ctx = Arc::new(ctx.clone());
    let db = state.db.clone();
    let app_handle = state.app_handle.clone();
    let entry = entry.clone();
    let access_key = access_key.cloned();
    let requested_model = requested_model.to_string();
    let first_token_ms = Arc::new(AtomicI64::new(0));
    let prompt_tokens = Arc::new(AtomicI64::new(0));
    let completion_tokens = Arc::new(AtomicI64::new(0));
    let has_sse_error = Arc::new(AtomicBool::new(false));
    let chunk_count = Arc::new(AtomicI64::new(0));
    let streamed_bytes = Arc::new(AtomicI64::new(0));
    let has_text_delta = Arc::new(AtomicBool::new(false));
    let has_tool_calls = Arc::new(AtomicBool::new(false));
    let seen_first_chunk = Arc::new(AtomicBool::new(false));
    let logged = Arc::new(AtomicBool::new(false));
    let mut sse_buffer = String::new();
    let mut sse_utf8_remainder: Vec<u8> = Vec::new();
    let mut done_state = SseDoneState::default();
    let mut upstream_stream = Box::pin(response.bytes_stream());
    let mut idle_timeout = Box::pin(sleep(STREAMING_IDLE_TIMEOUT));
    let _ping_interval = Box::pin(sleep(STREAMING_PING_INTERVAL));
    let entry_id = entry.id.clone();
    let circuit_breakers = state.circuit_breakers.clone();
    let failure_counts = state.failure_counts.clone();
    let settings_cache = state.settings.clone();
    let entries_app_handle = state.app_handle.clone();
    let success_circuit_breakers = state.circuit_breakers.clone();
    let success_failure_counts = state.failure_counts.clone();

    // 读取 settings 中的冷却时长，用于空流检测冷却
    let empty_stream_cooldown_secs: i64 = state
        .settings
        .try_read()
        .map(|s| s.circuit_recovery_secs.max(1))
        .unwrap_or(300);

    // Guard captured by the move closure → lives as long as the stream body
    let guard = StreamLogGuard {
        logged: logged.clone(),
        db: db.clone(),
        app_handle: app_handle.clone(),
        access_key: access_key.clone(),
        entry: entry.clone(),
        requested_model: requested_model.clone(),
        prompt_tokens: prompt_tokens.clone(),
        completion_tokens: completion_tokens.clone(),
        first_token_ms: first_token_ms.clone(),
        chunk_count: chunk_count.clone(),
        streamed_bytes: streamed_bytes.clone(),
        status_code,
        start,
        prior_attempts: prior_attempts.clone(),
        upstream_url: upstream_url.clone(),
        response_headers: response_headers.clone(),
        empty_stream_cooldown_secs,
    };

    let body_stream =
        futures::stream::poll_fn(move |cx| -> Poll<Option<Result<Bytes, std::io::Error>>> {
            let _ = &guard; // keep guard alive in the closure's capture list

            // Temporarily disable downstream SSE ping injection.
            // Some clients do not correctly ignore SSE comment frames like `: PING\n\n`
            // and may concatenate them into JSON payloads, causing parse errors.
            // if ping_interval.as_mut().poll(cx).is_ready() {
            //     ping_interval.as_mut().reset(tokio::time::Instant::now() + STREAMING_PING_INTERVAL);
            //     return Poll::Ready(Some(Ok(Bytes::from_static(b": PING\n\n"))));
            // }

            if idle_timeout.as_mut().poll(cx).is_ready() {
                if !logged.swap(true, Ordering::SeqCst) {
                    let attempt_path = attempt_path_with_current(
                        &prior_attempts,
                        &entry,
                        504,
                        false,
                        Some("stream idle timeout".to_string()),
                    );
                    let db2 = db.clone();
                    let ah2 = app_handle.clone();
                    let ak2 = access_key.clone();
                    let e2 = entry.clone();
                    let rm2 = requested_model.clone();
                    let pt = prompt_tokens.load(Ordering::SeqCst);
                    let ct = completion_tokens.load(Ordering::SeqCst);
                    let ft = first_token_ms.load(Ordering::SeqCst);
                    let lat = start.elapsed().as_millis() as i64;
                    tokio::spawn(async move {
                        log_usage(
                            &db2,
                            &ah2,
                            ak2.as_ref(),
                            &e2,
                            &rm2,
                            true,
                            pt,
                            ct,
                            0,
                            ft,
                            lat,
                            504,
                            false,
                            Some("stream idle timeout"),
                            Some(attempt_path.as_str()),
                            Some(StreamEndReason::Timeout),
                        );
                    });
                    spawn_cool_down_entry(
                        circuit_breakers.clone(),
                        failure_counts.clone(),
                        settings_cache.clone(),
                        db.clone(),
                        entries_app_handle.clone(),
                        entry_id.clone(),
                    );
                }
                return Poll::Ready(Some(Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "stream idle timeout",
                ))));
            }

            match upstream_stream.as_mut().poll_next(cx) {
                Poll::Ready(Some(Ok(chunk))) => {
                    idle_timeout
                        .as_mut()
                        .reset(tokio::time::Instant::now() + STREAMING_IDLE_TIMEOUT);
                    if !seen_first_chunk.swap(true, Ordering::SeqCst) {
                        first_token_ms.store(start.elapsed().as_millis() as i64, Ordering::SeqCst);
                    }
                    chunk_count.fetch_add(1, Ordering::SeqCst);
                    streamed_bytes.fetch_add(chunk.len() as i64, Ordering::SeqCst);

                    if super::sse::stream_buffer_exceeded(
                        &sse_buffer,
                        &sse_utf8_remainder,
                        streamed_bytes.load(Ordering::SeqCst) as usize,
                    ) {
                        if !logged.swap(true, Ordering::SeqCst) {
                            let attempt_path = attempt_path_with_current(
                                &prior_attempts,
                                &entry,
                                413,
                                false,
                                Some("stream buffer exceeds 10MB limit".to_string()),
                            );
                            let db2 = db.clone();
                            let ah2 = app_handle.clone();
                            let ak2 = access_key.clone();
                            let e2 = entry.clone();
                            let rm2 = requested_model.clone();
                            let pt = prompt_tokens.load(Ordering::SeqCst);
                            let ct = completion_tokens.load(Ordering::SeqCst);
                            let ft = first_token_ms.load(Ordering::SeqCst);
                            let lat = start.elapsed().as_millis() as i64;
                            tokio::spawn(async move {
log_usage(
                                &db2,
                                &ah2,
                                ak2.as_ref(),
                                &e2,
                                &rm2,
                                true,
                                pt,
                                ct,
                                0,
                                ft,
                                lat,
                                413,
                                false,
                                Some("stream buffer exceeds 10MB limit"),
                                Some(attempt_path.as_str()),
                                Some(StreamEndReason::Dropped),
                            );
                            });
                            spawn_cool_down_entry(
                                circuit_breakers.clone(),
                                failure_counts.clone(),
                                settings_cache.clone(),
                                db.clone(),
                                entries_app_handle.clone(),
                                entry_id.clone(),
                            );
                        }
                        return Poll::Ready(Some(Err(std::io::Error::new(
                            std::io::ErrorKind::Other,
                            "stream buffer exceeds 10MB limit",
                        ))));
                    }

                    if needs_transform {
                        if let Some(transformed) = transform_sse_chunk(
                            &chunk,
                            &mut sse_buffer,
                            &mut sse_utf8_remainder,
                            &adapter,
                            &prompt_tokens,
                            &completion_tokens,
                            &has_text_delta,
                            &has_tool_calls,
                            append_model_info.then_some(entry.model.as_str()),
                            &mut done_state,
                        ) {
                            if let Ok(mut chunk_text) = String::from_utf8(transformed.to_vec()) {
                                for mw in &middleware {
                                    mw.on_sse_chunk(&mut chunk_text, ctx.as_ref());
                                }
                                return Poll::Ready(Some(Ok(Bytes::from(chunk_text))));
                            }
                            return Poll::Ready(Some(Ok(transformed)));
                        } else {
                            cx.waker().wake_by_ref();
                            return Poll::Pending;
                        }
                    } else {
                        if let Some(with_model_info) = append_and_parse_sse(
                            &mut sse_buffer,
                            &mut sse_utf8_remainder,
                            &chunk,
                            &prompt_tokens,
                            &completion_tokens,
                            &has_sse_error,
                            &has_text_delta,
                            &has_tool_calls,
                            append_model_info.then_some(entry.model.as_str()),
                            &mut done_state,
                        ) {
                            // Normalize reasoning fields in model-info-injected chunk
                            let with_model_info =
                                normalize_reasoning_in_sse_chunk(&with_model_info)
                                    .unwrap_or(with_model_info);
                            if let Ok(mut chunk_text) = String::from_utf8(with_model_info.to_vec())
                            {
                                for mw in &middleware {
                                    mw.on_sse_chunk(&mut chunk_text, ctx.as_ref());
                                }
                                return Poll::Ready(Some(Ok(Bytes::from(chunk_text))));
                            }
                            return Poll::Ready(Some(Ok(with_model_info)));
                        }
                        // Normalize reasoning fields in raw SSE chunk (passthrough)
                        let chunk = normalize_reasoning_in_sse_chunk(&chunk).unwrap_or(chunk);
                        return Poll::Ready(Some(Ok(chunk)));
                    }
                }
                Poll::Ready(Some(Err(err))) => {
                    if !logged.swap(true, Ordering::SeqCst) {
                        let chunk_total = chunk_count.load(Ordering::SeqCst);
                        let byte_total = streamed_bytes.load(Ordering::SeqCst);
                        let ft = first_token_ms.load(Ordering::SeqCst);
                        let lat = start.elapsed().as_millis() as i64;
                        let pt = prompt_tokens.load(Ordering::SeqCst);
                        let ct = completion_tokens.load(Ordering::SeqCst);
                        let has_text_output = has_text_delta.load(Ordering::SeqCst);
                        let has_tool_output = has_tool_calls.load(Ordering::SeqCst);
                        let has_error = has_sse_error.load(Ordering::SeqCst);
                        let suppress_cooldown = is_decode_timeout_after_stream_started(
                            &err,
                            status_code,
                            ft,
                            chunk_total,
                            byte_total,
                            has_text_output,
                            has_tool_output,
                            ct,
                            has_error,
                        );
                        let stream_end_reason = if suppress_cooldown {
                            StreamEndReason::DecodeTimeout
                        } else {
                            StreamEndReason::UpstreamError
                        };
                        let diagnostic = build_stream_diagnostic(
                            "stream_read",
                            Some(&err),
                            &entry,
                            &upstream_url,
                            status_code,
                            &response_headers,
                            chunk_total,
                            byte_total,
                            sse_buffer.len(),
                            ft,
                            lat,
                            pt,
                            ct,
                            has_text_output,
                            has_tool_output,
                            has_error,
                            Some(false),
                        );
                        let error_message = format!("Stream error: {err}\n{diagnostic}");
                        let attempt_path = attempt_path_with_current(
                            &prior_attempts,
                            &entry,
                            502,
                            false,
                            Some(error_message.clone()),
                        );
                        let db2 = db.clone();
                        let ah2 = app_handle.clone();
                        let ak2 = access_key.clone();
                        let e2 = entry.clone();
                        let rm2 = requested_model.clone();
                        tokio::spawn(async move {
                            log_usage(
                                &db2,
                                &ah2,
                                ak2.as_ref(),
                                &e2,
                                &rm2,
                                true,
                                pt,
                                ct,
                                0,
                                ft,
                                lat,
                                502,
                                false,
                                Some(error_message.as_str()),
                                Some(attempt_path.as_str()),
                                Some(stream_end_reason),
                            );
                        });
                        if !suppress_cooldown {
                            spawn_cool_down_entry(
                                circuit_breakers.clone(),
                                failure_counts.clone(),
                                settings_cache.clone(),
                                db.clone(),
                                entries_app_handle.clone(),
                                entry_id.clone(),
                            );
                        }
                    }
                    Poll::Ready(Some(Err(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        err,
                    ))))
                }
                Poll::Ready(None) => {
                    if !logged.swap(true, Ordering::SeqCst) {
                        let pt = prompt_tokens.load(Ordering::SeqCst);
                        let ct = completion_tokens.load(Ordering::SeqCst);
                        let has_error = has_sse_error.load(Ordering::SeqCst);
                        let chunk_total = chunk_count.load(Ordering::SeqCst);
                        let byte_total = streamed_bytes.load(Ordering::SeqCst);
                        let ft = first_token_ms.load(Ordering::SeqCst);
                        let sc = status_code;
                        let has_text_output = has_text_delta.load(Ordering::SeqCst);
                        let has_tool_output = has_tool_calls.load(Ordering::SeqCst);
                        let success = is_completed_stream_success(
                            sc,
                            has_error,
                            has_text_output,
                            has_tool_output,
                            ct,
                        );
                        let failure_reason = if success {
                            None
                        } else if has_error {
                            Some("upstream stream completed with SSE error".to_string())
                        } else {
                            Some("upstream stream completed without valid output".to_string())
                        };
                        let attempt_path = attempt_path_with_current(
                            &prior_attempts,
                            &entry,
                            status_code,
                            success,
                            failure_reason.clone(),
                        );
                        let stream_summary = build_stream_diagnostic(
                            "stream_complete",
                            None,
                            &entry,
                            &upstream_url,
                            status_code,
                            &response_headers,
                            chunk_total,
                            byte_total,
                            sse_buffer.len(),
                            ft,
                            start.elapsed().as_millis() as i64,
                            pt,
                            ct,
                            has_text_output,
                            has_tool_output,
                            has_error,
                            Some(success),
                        );
                        let log_message = failure_reason
                            .as_ref()
                            .map(|reason| format!("{reason}\n{stream_summary}"))
                            .unwrap_or(stream_summary);
                        let db2 = db.clone();
                        let ah2 = app_handle.clone();
                        let ak2 = access_key.clone();
                        let e2 = entry.clone();
                        let rm2 = requested_model.clone();
                        let ft = first_token_ms.load(Ordering::SeqCst);
                        let lat = start.elapsed().as_millis() as i64;
                        let sc = status_code;
                        let scb = success_circuit_breakers.clone();
                        let sfc = success_failure_counts.clone();
                        let eid = entry_id.clone();
                        let sdb = settings_cache.clone();
                        let eah = entries_app_handle.clone();
                        tokio::spawn(async move {
                            log_usage(
                                &db2,
                                &ah2,
                                ak2.as_ref(),
                                &e2,
                                &rm2,
                                true,
                                pt,
                                ct,
                                0,
                                ft,
                                lat,
                                sc,
                                success,
                                Some(log_message.as_str()),
                                Some(attempt_path.as_str()),
                                Some(StreamEndReason::Done),
                            );
                            if success {
                                spawn_record_circuit_success(scb, sfc, sdb, db2.clone(), eah, eid);
                            } else {
                                spawn_cool_down_entry(scb, sfc, sdb, db2.clone(), eah, eid);
                            }
                        });
                        if let Some(reason) = failure_reason {
                            return Poll::Ready(Some(Err(std::io::Error::new(
                                std::io::ErrorKind::Other,
                                reason,
                            ))));
                        }
                    }
                    Poll::Ready(None)
                }
                Poll::Pending => Poll::Pending,
            }
        });

    axum::http::Response::builder()
        .status(
            axum::http::StatusCode::from_u16(status_code as u16)
                .unwrap_or(axum::http::StatusCode::OK),
        )
        .header("content-type", "text/event-stream")
        .header("cache-control", "no-cache")
        .header("connection", "keep-alive")
        .header("x-accel-buffering", "no")
        .body(Body::from_stream(body_stream))
        .unwrap()
}

fn stream_chunk_has_text_delta(value: &Value) -> bool {
    let has_chat_text = value
        .get("choices")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .any(|choice| {
            choice
                .get("delta")
                .and_then(|delta| delta.get("content"))
                .and_then(Value::as_str)
                .is_some_and(|content| !content.is_empty())
        });
    if has_chat_text {
        return true;
    }

    matches!(
        value.get("type").and_then(Value::as_str),
        Some(
            "response.output_text.delta"
                | "response.reasoning_text.delta"
                | "response.reasoning_summary_text.delta"
        )
    ) && value
        .get("delta")
        .and_then(Value::as_str)
        .is_some_and(|delta| !delta.is_empty())
}

/// 在 message / delta 级别归一已有的 reasoning 等价字段。
/// - 有 `reasoning_content` 无 `reasoning_text` → 补 `reasoning_text`
/// - 有 `reasoning_text` 无 `reasoning_content` → 补 `reasoning_content`
/// - 有 `reasoning_details` 无 `reasoning_content` → 补 `reasoning_content`
///
/// 只翻译已存在的信息，不缓存、不回放、不凭空生成 reasoning 历史。
fn normalize_reasoning_fields(value: &mut Value) {
    let obj = match value.as_object_mut() {
        Some(obj) => obj,
        None => return,
    };
    let canonical = obj
        .get("reasoning_content")
        .cloned()
        .or_else(|| obj.get("reasoning_text").cloned())
        .or_else(|| {
            obj.get("reasoning_details")
                .and_then(Value::as_str)
                .map(|reasoning| Value::String(reasoning.to_string()))
        });

    if let Some(reasoning) = canonical {
        if !obj.contains_key("reasoning_content") {
            obj.insert("reasoning_content".into(), reasoning.clone());
        }
        if !obj.contains_key("reasoning_text") {
            obj.insert("reasoning_text".into(), reasoning);
        }
    }
}

/// 扫描 SSE chunk 字节中的 reasoning delta 等价字段，
/// 补全缺少的对应字段。返回 `Some(modified_bytes)` 如果有任何修改。
fn normalize_reasoning_in_sse_chunk(chunk: &Bytes) -> Option<Bytes> {
    let text = std::str::from_utf8(chunk).ok()?;
    if !text.contains("reasoning_content")
        && !text.contains("reasoning_text")
        && !text.contains("reasoning_details")
    {
        return None;
    }

    let mut output = Vec::with_capacity(chunk.len() + 256);
    let mut changed = false;

    for line in text.split_inclusive('\n') {
        let check = line.trim_end();
        if let Some(payload) = sse_data_payload_for_reasoning(check) {
            if let Ok(mut val) = serde_json::from_str::<Value>(payload) {
                let before = val.clone();
                if let Some(choices) = val.get_mut("choices").and_then(|c| c.as_array_mut()) {
                    for choice in choices {
                        if let Some(delta) = choice.get_mut("delta") {
                            normalize_reasoning_fields(delta);
                        }
                    }
                }
                if val != before {
                    if let Ok(modified) = serde_json::to_string(&val) {
                        output.extend_from_slice(b"data: ");
                        output.extend_from_slice(modified.as_bytes());
                        if line.ends_with("\r\n") {
                            output.extend_from_slice(b"\r\n");
                        } else {
                            output.push(b'\n');
                        }
                        changed = true;
                        continue;
                    }
                }
            }
        }
        output.extend_from_slice(line.as_bytes());
    }

    changed.then_some(Bytes::from(output))
}

fn sse_data_payload_for_reasoning(line: &str) -> Option<&str> {
    line.strip_prefix("data:").map(str::trim_start)
}

fn stream_chunk_has_model_info_delta(value: &Value, model: &str) -> bool {
    value
        .get("choices")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .any(|choice| {
            choice
                .get("delta")
                .and_then(|delta| delta.get("content"))
                .and_then(Value::as_str)
                .is_some_and(|content| {
                    let trimmed = content.trim();
                    trimmed == format!("model: {model}") || trimmed == format!("模型: {model}")
                })
        })
}

/// 检测 OpenAI 兼容 chunk 是否包含函数/工具调用信号。
/// 一旦响应中存在工具调用，本轮就是 CALL 中间态，不应附加模型信息，
/// 否则下游循环复用时会把 `model: ...` 当成对话内容重复显示。
fn stream_chunk_has_tool_calls(value: &Value) -> bool {
    let has_responses_tool = matches!(
        value.get("type").and_then(Value::as_str),
        Some("response.function_call_arguments.delta")
    ) || (value.get("type").and_then(Value::as_str) == Some("response.output_item.added")
        && value
            .get("item")
            .and_then(|item| item.get("type"))
            .and_then(Value::as_str)
            == Some("function_call"));
    if has_responses_tool {
        return true;
    }

    value
        .get("choices")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .any(|choice| {
            let delta = choice.get("delta");
            let message = choice.get("message");

            let has_tool_calls = |container: Option<&Value>| {
                container
                    .and_then(|item| item.get("tool_calls"))
                    .and_then(Value::as_array)
                    .is_some_and(|tool_calls| !tool_calls.is_empty())
            };

            let has_function_call = |container: Option<&Value>| {
                container
                    .and_then(|item| item.get("function_call"))
                    .is_some_and(|function_call| !function_call.is_null())
            };

            let finish_reason = choice
                .get("finish_reason")
                .and_then(Value::as_str)
                .unwrap_or("");
            let is_tool_finish = matches!(finish_reason, "tool_calls" | "function_call");

            has_tool_calls(delta)
                || has_tool_calls(message)
                || has_function_call(delta)
                || has_function_call(message)
                || is_tool_finish
        })
}

fn model_info_delta(model: &str) -> Vec<u8> {
    let created = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default();
    let payload = serde_json::json!({
        "id": format!("chatcmpl-api-switch-model-info-{created}"),
        "object": "chat.completion.chunk",
        "created": created,
        "model": model,
        "choices": [{
            "index": 0,
            "delta": {
                "content": format!("\n\nmodel: {model}")
            },
            "finish_reason": null
        }]
    });
    format!("data: {payload}\n\n").into_bytes()
}

fn transform_sse_chunk(
    chunk: &Bytes,
    buffer: &mut String,
    remainder: &mut Vec<u8>,
    adapter: &Box<dyn super::protocol::ProtocolAdapter + Send + Sync>,
    prompt_tokens: &Arc<AtomicI64>,
    completion_tokens: &Arc<AtomicI64>,
    has_text_delta: &Arc<AtomicBool>,
    has_tool_calls: &Arc<AtomicBool>,
    model_info: Option<&str>,
    done_state: &mut SseDoneState,
) -> Option<Bytes> {
    super::sse::append_utf8_safe(buffer, remainder, chunk);
    let mut output = Vec::new();

    while let Some(line_end) = buffer.find('\n') {
        let mut line = buffer.drain(..=line_end).collect::<String>();
        if line.ends_with('\n') {
            line.pop();
        }
        if line.ends_with('\r') {
            line.pop();
        }

        if let Some(payload) = line.strip_prefix("data: ") {
            if payload == "[DONE]" {
                if !done_state.seen_done {
                    done_state.seen_done = true;
                    if let Some(model) = model_info.filter(|_| {
                        has_text_delta.load(Ordering::Relaxed)
                            && !has_tool_calls.load(Ordering::Relaxed)
                            && !done_state.appended_model_info
                            && !done_state.upstream_model_info_seen
                    }) {
                        output.push(model_info_delta(model));
                        done_state.appended_model_info = true;
                    }
                    output.push(b"data: [DONE]\n\n".to_vec());
                }
                continue;
            }

            let (prompt, completion) = adapter.extract_sse_usage(payload);
            if prompt > 0 {
                prompt_tokens.store(prompt, Ordering::Relaxed);
            }
            if completion > 0 {
                completion_tokens.store(completion, Ordering::Relaxed);
            }

            if let Some(transformed) = adapter.transform_sse_line(payload) {
                if let Ok(mut value) = serde_json::from_str::<Value>(&transformed) {
                    if let Some(choices) = value.get_mut("choices").and_then(Value::as_array_mut) {
                        for choice in choices {
                            if let Some(delta) = choice.get_mut("delta") {
                                normalize_reasoning_fields(delta);
                            }
                        }
                    }
                    if stream_chunk_has_text_delta(&value) {
                        has_text_delta.store(true, Ordering::Relaxed);
                    }
                    if stream_chunk_has_tool_calls(&value) {
                        has_tool_calls.store(true, Ordering::Relaxed);
                    }
                    if let Some(model) = model_info {
                        if stream_chunk_has_model_info_delta(&value, model) {
                            done_state.upstream_model_info_seen = true;
                        }
                    }
                    if let Ok(normalized) = serde_json::to_string(&value) {
                        output.push(format!("data: {normalized}\n\n").into_bytes());
                    }
                } else {
                    output.push(format!("data: {transformed}\n\n").into_bytes());
                }
            }
        }
    }

    if output.is_empty() {
        None
    } else {
        Some(Bytes::from(output.concat()))
    }
}

fn append_and_parse_sse(
    buffer: &mut String,
    remainder: &mut Vec<u8>,
    chunk: &Bytes,
    prompt_tokens: &Arc<AtomicI64>,
    completion_tokens: &Arc<AtomicI64>,
    has_sse_error: &Arc<AtomicBool>,
    has_text_delta: &Arc<AtomicBool>,
    has_tool_calls: &Arc<AtomicBool>,
    model_info: Option<&str>,
    done_state: &mut SseDoneState,
) -> Option<Bytes> {
    super::sse::append_utf8_safe(buffer, remainder, chunk);
    let mut saw_done = false;

    while let Some(line_end) = buffer.find('\n') {
        let mut line = buffer.drain(..=line_end).collect::<String>();
        if line.ends_with('\n') {
            line.pop();
        }
        if line.ends_with('\r') {
            line.pop();
        }

        if let Some(payload) = line.strip_prefix("data: ") {
            if payload == "[DONE]" {
                if !done_state.seen_done {
                    done_state.seen_done = true;
                    saw_done = true;
                }
                continue;
            }

            let Ok(value) = serde_json::from_str::<Value>(payload) else {
                continue;
            };
            // 只有 error 值是非 null 对象时才算真正的错误
            // 修复: value.get("error").is_some() 对 "error": null 也会返回 true，导致误判
            if let Some(err) = value.get("error") {
                if !err.is_null() {
                    has_sse_error.store(true, Ordering::Relaxed);
                }
            }
            if stream_chunk_has_text_delta(&value) {
                has_text_delta.store(true, Ordering::Relaxed);
            }
            if stream_chunk_has_tool_calls(&value) {
                has_tool_calls.store(true, Ordering::Relaxed);
            }
            if let Some(model) = model_info {
                if stream_chunk_has_model_info_delta(&value, model) {
                    done_state.upstream_model_info_seen = true;
                }
            }
            let (prompt, completion, _reasoning) = extract_usage_tokens(&value);
            if prompt > 0 {
                prompt_tokens.store(prompt, Ordering::Relaxed);
            }
            if completion > 0 {
                completion_tokens.store(completion, Ordering::Relaxed);
            }
        }
    }

    let Some(model) = model_info.filter(|_| {
        saw_done
            && has_text_delta.load(Ordering::Relaxed)
            && !has_tool_calls.load(Ordering::Relaxed)
            && !done_state.appended_model_info
            && !done_state.upstream_model_info_seen
    }) else {
        return None;
    };

    let marker = b"data: [DONE]";
    let Some(pos) = chunk
        .windows(marker.len())
        .position(|window| window == marker)
    else {
        return None;
    };

    done_state.appended_model_info = true;

    let mut output = Vec::with_capacity(chunk.len() + 66 + model.len());
    let prefix = &chunk[..pos];
    output.extend_from_slice(prefix);
    if !prefix.is_empty() && !prefix.ends_with(b"\n\n") {
        if prefix.ends_with(b"\n") {
            output.extend_from_slice(b"\n");
        } else {
            output.extend_from_slice(b"\n\n");
        }
    }
    output.extend_from_slice(&model_info_delta(model));
    output.extend_from_slice(b"data: [DONE]\n\n");
    Some(Bytes::from(output))
}

fn refresh_tray(app_handle: &Option<tauri::AppHandle>) {
    if let Some(h) = app_handle {
        refresh_tray_if_enabled(h);
    }
}

fn status_matches_rule(rule: &str, status: u16) -> bool {
    let rule = rule.trim();
    if rule.is_empty() {
        return false;
    }

    if let Some((start, end)) = rule.split_once('-') {
        let Ok(start) = start.trim().parse::<u16>() else {
            return false;
        };
        let Ok(end) = end.trim().parse::<u16>() else {
            return false;
        };
        return status >= start && status <= end;
    }

    rule.parse::<u16>() == Ok(status)
}

fn should_disable_entry_for_status(disable_codes: &str, status: u16) -> bool {
    disable_codes
        .split(',')
        .any(|rule| status_matches_rule(rule, status))
}

/// Check if the upstream error message contains any disable keyword.
/// Keywords are separated by newlines and matched case-insensitively.
fn should_disable_entry_for_message(disable_keywords: &str, message: &str) -> bool {
    if disable_keywords.is_empty() || message.is_empty() {
        return false;
    }
    let lower_msg = message.to_lowercase();
    disable_keywords
        .split('\n')
        .map(|k| k.trim())
        .filter(|k| !k.is_empty())
        .any(|keyword| lower_msg.contains(&keyword.to_lowercase()))
}

async fn disable_entry(state: &ProxyState, entry: &ApiEntry) {
    let recovery_secs = state.settings.read().await.circuit_recovery_secs.max(1);
    let cooldown_until = chrono::Utc::now().timestamp() + recovery_secs;

    let _ = state.db.toggle_entry(&entry.id, false);
    let _ = state.db.set_entry_cooldown(&entry.id, Some(cooldown_until));
    if let Some(h) = &state.app_handle {
        let _ = h.emit("entries-changed", ());
    }
    crate::state_version::bump("pool");
    refresh_tray(&state.app_handle);

    let mut breakers = state.circuit_breakers.write().await;
    breakers.remove(&entry.id);
}

async fn freeze_channel_entries(state: &ProxyState, entry: &ApiEntry) {
    // 限定词通常代表账号、额度、密钥或上游通道级故障；只冷冻同渠道 6 小时，不永久禁用用户开关。
    let cooldown_until = chrono::Utc::now().timestamp() + 21600;
    match state
        .db
        .freeze_entries_for_channel(&entry.channel_id, cooldown_until)
    {
        Ok(entry_ids) => {
            if let Some(h) = &state.app_handle {
                let _ = h.emit("entries-changed", ());
            }
            crate::state_version::bump("pool");
            refresh_tray(&state.app_handle);

            let mut counts = state.failure_counts.write().await;
            let mut breakers = state.circuit_breakers.write().await;
            for entry_id in &entry_ids {
                counts.remove(entry_id);
                breakers.remove(entry_id);
            }

            log::warn!(
                "Channel {} frozen for 6h after keyword match. Frozen entries: {}.",
                entry.channel_id,
                entry_ids.len()
            );
        }
        Err(err) => {
            log::error!(
                "Failed to freeze channel {} after keyword match: {}",
                entry.channel_id,
                err
            );
        }
    }
}

async fn record_circuit_success(state: &ProxyState, entry_id: &str) {
    let _ = state.db.set_entry_cooldown(entry_id, None);
    if let Some(h) = &state.app_handle {
        let _ = h.emit("entries-changed", ());
    }
    crate::state_version::bump("pool");
    refresh_tray(&state.app_handle);

    // Clear failure count
    state.failure_counts.write().await.remove(entry_id);

    let mut breakers = state.circuit_breakers.write().await;
    let recovery_secs = state.settings.read().await.circuit_recovery_secs as u64;

    let cb = breakers
        .entry(entry_id.to_string())
        .or_insert_with(|| CircuitBreaker::new(recovery_secs));
    cb.record_success();
}

async fn cool_down_entry(state: &ProxyState, entry: &ApiEntry) {
    let settings = state.settings.read().await.clone();
    let threshold = (settings.circuit_failure_threshold as u32).max(1);
    let recovery_secs = settings.circuit_recovery_secs.max(1);

    // Increment failure count in memory
    let mut counts = state.failure_counts.write().await;
    let count = counts.entry(entry.id.clone()).or_insert(0);
    *count += 1;
    let current_count = *count;
    drop(counts);

    // Any failure is counted. Before threshold: temporary cooldown.
    // At/above threshold: remove from AUTO and set a 6h long cooldown.
    if current_count >= threshold {
        let six_hours_later = chrono::Utc::now().timestamp() + 21600;
        let _ = state
            .db
            .set_entry_cooldown(&entry.id, Some(six_hours_later));
        let _ = state.db.toggle_entry(&entry.id, false);
        if let Some(h) = &state.app_handle {
            let _ = h.emit("entries-changed", ());
        }
        crate::state_version::bump("pool");
        refresh_tray(&state.app_handle);

        let mut breakers = state.circuit_breakers.write().await;
        breakers.remove(&entry.id);

        log::warn!(
            "Entry {} disabled after {} consecutive failures. Long cooldown: 6h.",
            entry.id,
            current_count
        );
        return;
    }

    let cooldown_until = chrono::Utc::now().timestamp() + recovery_secs as i64;
    let _ = state.db.set_entry_cooldown(&entry.id, Some(cooldown_until));
    if let Some(h) = &state.app_handle {
        let _ = h.emit("entries-changed", ());
    }
    crate::state_version::bump("pool");
    refresh_tray(&state.app_handle);

    let mut breakers = state.circuit_breakers.write().await;
    let recovery_secs_u64 = settings.circuit_recovery_secs as u64;

    let cb = breakers
        .entry(entry.id.clone())
        .or_insert_with(|| CircuitBreaker::new(recovery_secs_u64));
    cb.set_recovery_secs(recovery_secs_u64);
    cb.record_failure(threshold);

    log::warn!(
        "Entry {} cooled down for {}s after recoverable failure count {}/{}.",
        entry.id,
        recovery_secs,
        current_count,
        threshold
    );
}

fn spawn_record_circuit_success(
    circuit_breakers: Arc<tokio::sync::RwLock<std::collections::HashMap<String, CircuitBreaker>>>,
    failure_counts: Arc<tokio::sync::RwLock<std::collections::HashMap<String, u32>>>,
    settings: Arc<tokio::sync::RwLock<AppSettings>>,
    db: Arc<Database>,
    app_handle: Option<tauri::AppHandle>,
    entry_id: String,
) {
    tokio::spawn(async move {
        let recovery_secs = settings.read().await.circuit_recovery_secs as u64;

        let _ = db.set_entry_cooldown(&entry_id, None);
        crate::state_version::bump("pool");
        if let Some(h) = &app_handle {
            let _ = h.emit("entries-changed", ());
        }
        crate::state_version::bump("pool");
        refresh_tray(&app_handle);

        // Clear failure count
        failure_counts.write().await.remove(&entry_id);

        let mut breakers = circuit_breakers.write().await;
        let cb = breakers
            .entry(entry_id)
            .or_insert_with(|| CircuitBreaker::new(recovery_secs));
        cb.set_recovery_secs(recovery_secs);
        cb.record_success();
    });
}

fn spawn_cool_down_entry(
    circuit_breakers: Arc<tokio::sync::RwLock<std::collections::HashMap<String, CircuitBreaker>>>,
    failure_counts: Arc<tokio::sync::RwLock<std::collections::HashMap<String, u32>>>,
    settings: Arc<tokio::sync::RwLock<AppSettings>>,
    db: Arc<Database>,
    app_handle: Option<tauri::AppHandle>,
    entry_id: String,
) {
    tokio::spawn(async move {
        let settings = settings.read().await.clone();
        let threshold = (settings.circuit_failure_threshold as u32).max(1);
        let recovery_secs = settings.circuit_recovery_secs as u64;

        // Increment failure count
        let mut counts = failure_counts.write().await;
        let count = counts.entry(entry_id.clone()).or_insert(0);
        *count += 1;
        let current_count = *count;
        drop(counts);

        // Any failure is counted. Before threshold: temporary cooldown.
        // At/above threshold: remove from AUTO and set a 6h long cooldown.
        if current_count >= threshold {
            let six_hours_later = chrono::Utc::now().timestamp() + 21600;
            let _ = db.set_entry_cooldown(&entry_id, Some(six_hours_later));
            let _ = db.toggle_entry(&entry_id, false);
            if let Some(h) = &app_handle {
                let _ = h.emit("entries-changed", ());
            }
            crate::state_version::bump("pool");
            refresh_tray(&app_handle);

            log::warn!(
                "Entry {} disabled after {} consecutive failures. Long cooldown: 6h.",
                entry_id,
                current_count
            );
            return;
        }

        let cooldown_until = chrono::Utc::now().timestamp() + recovery_secs as i64;
        let _ = db.set_entry_cooldown(&entry_id, Some(cooldown_until));
        if let Some(h) = &app_handle {
            let _ = h.emit("entries-changed", ());
        }
        crate::state_version::bump("pool");
        refresh_tray(&app_handle);

        let mut breakers = circuit_breakers.write().await;
        let cb = breakers
            .entry(entry_id.clone())
            .or_insert_with(|| CircuitBreaker::new(recovery_secs));
        cb.set_recovery_secs(recovery_secs);
        cb.record_failure(threshold);

        log::warn!(
            "Entry {} cooled down for {}s after recoverable failure count {}/{}.",
            entry_id,
            recovery_secs,
            current_count,
            threshold
        );
    });
}

fn log_usage(
    db: &Database,
    app_handle: &Option<tauri::AppHandle>,
    access_key: Option<&AccessKey>,
    entry: &ApiEntry,
    requested_model: &str,
    is_stream: bool,
    prompt_tokens: i64,
    completion_tokens: i64,
    reasoning_tokens: i64,
    first_token_ms: i64,
    latency_ms: i64,
    status_code: i32,
    success: bool,
    error_message: Option<&str>,
    attempt_path: Option<&str>,
    stream_end_reason: Option<StreamEndReason>,
) {
    let log_type = if success { 2 } else { 5 };
    let content = error_message.unwrap_or("");
    let token_name = access_key.map(|ak| ak.name.as_str()).unwrap_or("NONE");
    let use_time = ((latency_ms as f64) / 1000.0).ceil() as i64;
    let other = serde_json::json!({
        "requested_model": requested_model,
        "resolved_model": entry.model,
        "first_token_ms": first_token_ms,
        "status_code": status_code,
        "success": success,
        "reasoning_tokens": reasoning_tokens,
        "attempt_path": attempt_path.and_then(|path| serde_json::from_str::<Value>(path).ok()),
        "stream_end_reason": stream_end_reason.map(StreamEndReason::as_str),
    })
    .to_string();

    if db
        .insert_usage_log(
            log_type,
            content,
            access_key.map(|ak| ak.id.as_str()),
            access_key.map(|ak| ak.name.as_str()).unwrap_or("NONE"),
            token_name,
            &entry.id,
            &entry.channel_id,
            entry.channel_name.as_deref().unwrap_or("unknown"),
            &entry.model,
            requested_model,
            0,
            is_stream,
            prompt_tokens,
            completion_tokens,
            latency_ms,
            first_token_ms,
            use_time,
            status_code,
            success,
            "",
            "default",
            &other,
            error_message,
            None,
        )
        .is_ok()
    {
        crate::state_version::bump("log");
    }

    if let Some(h) = app_handle {
        let _ = h.emit("new-usage-log", ());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proxy::protocol::get_adapter;

    #[test]
    fn transformed_sse_chunks_are_standard_sse_frames() {
        let adapter = get_adapter("claude");
        let chunk = Bytes::from_static(
            b"data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_1\",\"model\":\"claude-3\"}}\n\
data: [DONE]\n"
        );
        let mut buffer = String::new();
        let prompt_tokens = Arc::new(AtomicI64::new(0));
        let completion_tokens = Arc::new(AtomicI64::new(0));

        let mut remainder = Vec::new();
        let output = transform_sse_chunk(
            &chunk,
            &mut buffer,
            &mut remainder,
            &adapter,
            &prompt_tokens,
            &completion_tokens,
            &Arc::new(AtomicBool::new(false)),
            &Arc::new(AtomicBool::new(false)),
            None,
            &mut SseDoneState::default(),
        )
        .expect("transformed output");
        let output = String::from_utf8(output.to_vec()).expect("valid utf8");

        assert!(output.contains("\n\n"));
        assert!(output.ends_with("data: [DONE]\n\n"));
        assert!(!output.contains("data: [DONE]\n\ndata: [DONE]"));
    }

    #[test]
    fn model_info_delta_uses_openai_chat_completion_chunk_shape() {
        let frame = String::from_utf8(model_info_delta("gpt-test")).expect("valid utf8");
        let payload = frame
            .strip_prefix("data: ")
            .and_then(|s| s.strip_suffix("\n\n"))
            .expect("standard SSE data frame");
        let value: Value = serde_json::from_str(payload).expect("valid json");

        assert!(value["id"].as_str().unwrap_or_default().starts_with("chatcmpl-"));
        assert_eq!(value["object"], "chat.completion.chunk");
        assert!(value["created"].as_u64().is_some());
        assert_eq!(value["model"], "gpt-test");
        assert_eq!(value["choices"][0]["index"], 0);
        assert_eq!(value["choices"][0]["delta"]["content"], "\n\nmodel: gpt-test");
        assert!(value["choices"][0]["finish_reason"].is_null());
    }

    #[test]
    fn appended_model_info_frame_is_openai_compatible_before_done() {
        let mut buffer = String::new();
        let chunk = Bytes::from_static(
            b"data: {\"choices\":[{\"delta\":{\"content\":\"hello\"}}]}\n\
data: [DONE]\n",
        );
        let prompt_tokens = Arc::new(AtomicI64::new(0));
        let completion_tokens = Arc::new(AtomicI64::new(0));
        let has_sse_error = Arc::new(AtomicBool::new(false));
        let has_text_delta = Arc::new(AtomicBool::new(false));
        let has_tool_calls = Arc::new(AtomicBool::new(false));
        let mut done_state = SseDoneState::default();
        let mut remainder = Vec::new();

        let output = append_and_parse_sse(
            &mut buffer,
            &mut remainder,
            &chunk,
            &prompt_tokens,
            &completion_tokens,
            &has_sse_error,
            &has_text_delta,
            &has_tool_calls,
            Some("gpt-test"),
            &mut done_state,
        )
        .expect("model info should be appended");
        let output = String::from_utf8(output.to_vec()).expect("valid utf8");
        let model_frame = output
            .split("\n\n")
            .find(|frame| frame.contains("model: gpt-test"))
            .expect("model info frame exists");
        let payload = model_frame.strip_prefix("data: ").expect("data frame");
        let value: Value = serde_json::from_str(payload).expect("valid json");

        assert_eq!(value["object"], "chat.completion.chunk");
        assert_eq!(value["model"], "gpt-test");
        assert_eq!(value["choices"][0]["index"], 0);
        assert!(value["choices"][0]["finish_reason"].is_null());
        assert!(output.ends_with("data: [DONE]\n\n"));
    }

    #[test]
    fn decode_timeout_after_stream_started_suppresses_cooldown_classification() {
        assert!(is_recoverable_decode_timeout_after_stream_started(
            200, 3705, 1, 218, false, false, 0, false, true, true, false
        ));
    }

    #[test]
    fn decode_timeout_before_stream_started_does_not_suppress_cooldown() {
        assert!(!is_recoverable_decode_timeout_after_stream_started(
            200, 0, 0, 0, false, false, 0, false, true, true, false
        ));
    }

    #[test]
    fn decode_timeout_with_sse_error_does_not_suppress_cooldown() {
        assert!(!is_recoverable_decode_timeout_after_stream_started(
            200, 1000, 1, 128, false, false, 0, true, true, true, false
        ));
    }

    #[test]
    fn dropped_stream_success_only_depends_on_status_code() {
        assert!(is_dropped_stream_success(200, 0, 0));
        assert!(!is_dropped_stream_success(500, 999, 999));
    }

    #[test]
    fn append_and_parse_sse_error_null_not_marked_as_error() {
        let mut buffer = String::new();
        let chunk = Bytes::from_static(b"data: {\"error\":null}\n");
        let prompt_tokens = Arc::new(AtomicI64::new(0));
        let completion_tokens = Arc::new(AtomicI64::new(0));
        let has_sse_error = Arc::new(AtomicBool::new(false));
        let has_text_delta = Arc::new(AtomicBool::new(false));
        let mut done_state = SseDoneState::default();

        let mut remainder = Vec::new();
        let output = append_and_parse_sse(
            &mut buffer,
            &mut remainder,
            &chunk,
            &prompt_tokens,
            &completion_tokens,
            &has_sse_error,
            &has_text_delta,
            &Arc::new(AtomicBool::new(false)),
            Some("gpt-test"),
            &mut done_state,
        );

        assert!(output.is_none());
        assert!(!has_sse_error.load(Ordering::Relaxed));
    }

    #[test]
    fn append_and_parse_sse_error_object_marked_as_error() {
        let mut buffer = String::new();
        let chunk = Bytes::from_static(b"data: {\"error\":{\"message\":\"boom\"}}\n");
        let prompt_tokens = Arc::new(AtomicI64::new(0));
        let completion_tokens = Arc::new(AtomicI64::new(0));
        let has_sse_error = Arc::new(AtomicBool::new(false));
        let has_text_delta = Arc::new(AtomicBool::new(false));
        let mut done_state = SseDoneState::default();

        let mut remainder = Vec::new();
        let output = append_and_parse_sse(
            &mut buffer,
            &mut remainder,
            &chunk,
            &prompt_tokens,
            &completion_tokens,
            &has_sse_error,
            &has_text_delta,
            &Arc::new(AtomicBool::new(false)),
            Some("gpt-test"),
            &mut done_state,
        );

        assert!(output.is_none());
        assert!(has_sse_error.load(Ordering::Relaxed));
    }

    #[test]
    fn append_and_parse_sse_duplicate_done_only_appends_model_once() {
        let mut buffer = String::new();
        let first_chunk = Bytes::from_static(
            b"data: {\"choices\":[{\"delta\":{\"content\":\"hello\"}}]}\n\
data: [DONE]\n",
        );
        let second_chunk = Bytes::from_static(b"data: [DONE]\n");
        let prompt_tokens = Arc::new(AtomicI64::new(0));
        let completion_tokens = Arc::new(AtomicI64::new(0));
        let has_sse_error = Arc::new(AtomicBool::new(false));
        let has_text_delta = Arc::new(AtomicBool::new(false));
        let mut done_state = SseDoneState::default();

        let mut remainder = Vec::new();
        let first_output = append_and_parse_sse(
            &mut buffer,
            &mut remainder,
            &first_chunk,
            &prompt_tokens,
            &completion_tokens,
            &has_sse_error,
            &has_text_delta,
            &Arc::new(AtomicBool::new(false)),
            Some("gpt-test"),
            &mut done_state,
        )
        .expect("first done should append model once");
        let second_output = append_and_parse_sse(
            &mut buffer,
            &mut remainder,
            &second_chunk,
            &prompt_tokens,
            &completion_tokens,
            &has_sse_error,
            &has_text_delta,
            &Arc::new(AtomicBool::new(false)),
            Some("gpt-test"),
            &mut done_state,
        );

        let first_text = String::from_utf8(first_output.to_vec()).expect("valid utf8");
        assert_eq!(first_text.matches("model: gpt-test").count(), 1);
        assert!(second_output.is_none());
    }

    #[test]
    fn append_and_parse_sse_does_not_duplicate_existing_model_info() {
        let mut buffer = String::new();
        let chunk = Bytes::from_static(
            b"data: {\"choices\":[{\"delta\":{\"content\":\"hello\"}}]}\n\
data: {\"choices\":[{\"delta\":{\"content\":\"\\n\\nmodel: gpt-test\"}}]}\n\
data: [DONE]\n",
        );
        let prompt_tokens = Arc::new(AtomicI64::new(0));
        let completion_tokens = Arc::new(AtomicI64::new(0));
        let has_sse_error = Arc::new(AtomicBool::new(false));
        let has_text_delta = Arc::new(AtomicBool::new(false));
        let mut done_state = SseDoneState::default();

        let mut remainder = Vec::new();
        let output = append_and_parse_sse(
            &mut buffer,
            &mut remainder,
            &chunk,
            &prompt_tokens,
            &completion_tokens,
            &has_sse_error,
            &has_text_delta,
            &Arc::new(AtomicBool::new(false)),
            Some("gpt-test"),
            &mut done_state,
        );

        let text = String::from_utf8(output.unwrap_or(chunk).to_vec()).expect("valid utf8");
        assert_eq!(text.matches("model: gpt-test").count(), 1);
    }

    #[test]
    fn transform_sse_chunk_duplicate_done_only_outputs_single_done_and_model_once() {
        let adapter = get_adapter("claude");
        let chunk = Bytes::from_static(
            b"data: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"hi\"}}\n\
data: [DONE]\n\
data: [DONE]\n"
        );
        let mut buffer = String::new();
        let prompt_tokens = Arc::new(AtomicI64::new(0));
        let completion_tokens = Arc::new(AtomicI64::new(0));
        let has_text_delta = Arc::new(AtomicBool::new(false));
        let mut done_state = SseDoneState::default();

        let mut remainder = Vec::new();
        let output = transform_sse_chunk(
            &chunk,
            &mut buffer,
            &mut remainder,
            &adapter,
            &prompt_tokens,
            &completion_tokens,
            &has_text_delta,
            &Arc::new(AtomicBool::new(false)),
            Some("claude-3"),
            &mut done_state,
        )
        .expect("transformed output");

        let output = String::from_utf8(output.to_vec()).expect("valid utf8");
        assert_eq!(output.matches("data: [DONE]").count(), 1);
        assert_eq!(output.matches("model: claude-3").count(), 1);
    }

    #[test]
    fn transform_sse_chunk_does_not_duplicate_existing_model_info() {
        let adapter = get_adapter("openai");
        let chunk = Bytes::from_static(
            b"data: {\"choices\":[{\"delta\":{\"content\":\"hi\"}}]}\n\
data: {\"choices\":[{\"delta\":{\"content\":\"\\n\\nmodel: gpt-test\"}}]}\n\
data: [DONE]\n",
        );
        let mut buffer = String::new();
        let prompt_tokens = Arc::new(AtomicI64::new(0));
        let completion_tokens = Arc::new(AtomicI64::new(0));
        let has_text_delta = Arc::new(AtomicBool::new(false));
        let mut done_state = SseDoneState::default();

        let mut remainder = Vec::new();
        let output = transform_sse_chunk(
            &chunk,
            &mut buffer,
            &mut remainder,
            &adapter,
            &prompt_tokens,
            &completion_tokens,
            &has_text_delta,
            &Arc::new(AtomicBool::new(false)),
            Some("gpt-test"),
            &mut done_state,
        )
        .expect("transformed output");

        let output = String::from_utf8(output.to_vec()).expect("valid utf8");
        assert_eq!(output.matches("data: [DONE]").count(), 1);
        assert_eq!(output.matches("model: gpt-test").count(), 1);
    }

    #[test]
    fn passthrough_responses_usage_detection_supports_input_output_tokens() {
        let body = serde_json::json!({
            "object": "response",
            "output": [],
            "usage": {"input_tokens": 12, "output_tokens": 7}
        });
        let (prompt_tokens, completion_tokens, _reasoning) = extract_usage_tokens(&body);

        assert_eq!(prompt_tokens, 12);
        assert_eq!(completion_tokens, 7);
        assert!(!responses_response_has_valid_output(&body));
    }

    #[test]
    fn passthrough_responses_valid_output_requires_nonempty_output_array() {
        let valid = serde_json::json!({
            "object": "response",
            "output": [{"type": "message"}]
        });
        let invalid = serde_json::json!({
            "object": "response",
            "output": []
        });

        assert!(responses_response_has_valid_output(&valid));
        assert!(!responses_response_has_valid_output(&invalid));
    }

    #[test]
    fn passthrough_responses_text_delta_is_valid_stream_output() {
        let text = serde_json::json!({
            "type": "response.output_text.delta",
            "delta": "hi"
        });
        let reasoning = serde_json::json!({
            "type": "response.reasoning_text.delta",
            "delta": "think"
        });
        let empty = serde_json::json!({
            "type": "response.output_text.delta",
            "delta": ""
        });

        assert!(stream_chunk_has_text_delta(&text));
        assert!(stream_chunk_has_text_delta(&reasoning));
        assert!(!stream_chunk_has_text_delta(&empty));
    }

    #[test]
    fn passthrough_responses_tool_call_chunks_are_valid_stream_output() {
        let tool_delta = serde_json::json!({
            "type": "response.function_call_arguments.delta",
            "delta": "{\"city\":"
        });
        let item_added = serde_json::json!({
            "type": "response.output_item.added",
            "item": {"type": "function_call", "id": "call_1"}
        });

        assert!(stream_chunk_has_tool_calls(&tool_delta));
        assert!(stream_chunk_has_tool_calls(&item_added));
    }

    #[test]
    fn normalize_reasoning_fields_maps_details_without_creating_history() {
        let mut details = serde_json::json!({
            "role": "assistant",
            "content": "",
            "reasoning_details": "kept reasoning"
        });
        normalize_reasoning_fields(&mut details);
        assert_eq!(details["reasoning_content"], "kept reasoning");
        assert_eq!(details["reasoning_text"], "kept reasoning");
        assert_eq!(details["reasoning_details"], "kept reasoning");

        let mut no_reasoning = serde_json::json!({
            "role": "assistant",
            "content": "answer"
        });
        normalize_reasoning_fields(&mut no_reasoning);
        assert!(no_reasoning.get("reasoning_content").is_none());
        assert!(no_reasoning.get("reasoning_text").is_none());
    }

    #[test]
    fn normalize_reasoning_in_sse_chunk_maps_details_delta() {
        let chunk = Bytes::from_static(
            b"data: {\"choices\":[{\"delta\":{\"reasoning_details\":\"hidden\"}}]}\n\n",
        );

        let output = normalize_reasoning_in_sse_chunk(&chunk).expect("modified chunk");
        let output = String::from_utf8(output.to_vec()).expect("valid utf8");

        assert!(output.contains("\"reasoning_details\":\"hidden\""));
        assert!(output.contains("\"reasoning_content\":\"hidden\""));
        assert!(output.contains("\"reasoning_text\":\"hidden\""));
    }

    #[test]
    fn normalize_reasoning_fields_keeps_structured_details_separate() {
        let mut details = serde_json::json!({
            "role": "assistant",
            "reasoning_details": [{"type": "summary", "text": "hidden"}]
        });

        normalize_reasoning_fields(&mut details);

        assert!(details.get("reasoning_content").is_none());
        assert!(details.get("reasoning_text").is_none());
        assert!(details.get("reasoning_details").is_some());
    }

    #[test]
    fn normalize_reasoning_in_sse_chunk_supports_data_without_space_and_crlf() {
        let chunk = Bytes::from_static(
            b"data:{\"choices\":[{\"delta\":{\"reasoning_content\":\"hidden\"}}]}\r\n\r\n",
        );

        let output = normalize_reasoning_in_sse_chunk(&chunk).expect("modified chunk");
        let output = String::from_utf8(output.to_vec()).expect("valid utf8");

        assert!(output.contains("data: {"));
        assert!(output.contains("\"reasoning_text\":\"hidden\""));
        assert!(output.contains("\r\n"));
    }

    #[test]
    fn normalize_reasoning_in_sse_chunk_passes_non_utf8_through() {
        let chunk = Bytes::from_static(b"data: \xff\xfe\n");
        assert!(normalize_reasoning_in_sse_chunk(&chunk).is_none());
    }

    #[test]
    fn transform_sse_chunk_normalizes_reasoning_after_adapter_transform() {
        let adapter = get_adapter("responses");
        let chunk = Bytes::from_static(
            b"data: {\"choices\":[{\"delta\":{\"reasoning_details\":\"hidden\"}}]}\n",
        );
        let mut buffer = String::new();
        let mut remainder = Vec::new();
        let prompt_tokens = Arc::new(AtomicI64::new(0));
        let completion_tokens = Arc::new(AtomicI64::new(0));
        let has_text_delta = Arc::new(AtomicBool::new(false));
        let has_tool_calls = Arc::new(AtomicBool::new(false));
        let mut done_state = SseDoneState::default();

        let output = transform_sse_chunk(
            &chunk,
            &mut buffer,
            &mut remainder,
            &adapter,
            &prompt_tokens,
            &completion_tokens,
            &has_text_delta,
            &has_tool_calls,
            None,
            &mut done_state,
        )
        .expect("transformed output");
        let output = String::from_utf8(output.to_vec()).expect("valid utf8");

        assert!(output.contains("\"reasoning_details\":\"hidden\""));
        assert!(output.contains("\"reasoning_content\":\"hidden\""));
        assert!(output.contains("\"reasoning_text\":\"hidden\""));
    }

    #[test]
    fn completed_stream_success_logic() {
        assert!(!is_completed_stream_success(200, false, false, false, 0));
        assert!(is_completed_stream_success(200, false, true, false, 0));
        assert!(is_completed_stream_success(200, false, false, true, 0));
        assert!(is_completed_stream_success(200, false, false, false, 5));
        assert!(!is_completed_stream_success(200, true, true, false, 5));
        assert!(!is_completed_stream_success(502, false, true, false, 5));
    }

    #[test]
    fn nonstream_response_requires_visible_output() {
        let empty_content_with_usage = serde_json::json!({
            "choices": [{"message": {"role": "assistant", "content": ""}}],
            "usage": {"prompt_tokens": 100, "completion_tokens": 20}
        });
        assert!(!nonstream_response_has_valid_output(
            &empty_content_with_usage
        ));

        let text = serde_json::json!({
            "choices": [{"message": {"role": "assistant", "content": "ok"}}]
        });
        assert!(nonstream_response_has_valid_output(&text));

        let tool_call = serde_json::json!({
            "choices": [{"message": {"role": "assistant", "content": null, "tool_calls": [{"id": "call_1"}]}}]
        });
        assert!(nonstream_response_has_valid_output(&tool_call));
    }

    #[test]
    fn nonstream_response_reasoning_only_is_valid_output() {
        let reasoning_only = serde_json::json!({
            "choices": [{"message": {"role": "assistant", "content": "", "reasoning_content": "hidden"}}],
            "usage": {"prompt_tokens": 100, "completion_tokens": 20}
        });
        assert!(nonstream_response_has_valid_output(&reasoning_only));
    }

    #[test]
    fn append_and_parse_sse_empty_done_has_no_valid_output() {
        let mut buffer = String::new();
        let chunk = Bytes::from_static(b"data: [DONE]\n");
        let prompt_tokens = Arc::new(AtomicI64::new(0));
        let completion_tokens = Arc::new(AtomicI64::new(0));
        let has_sse_error = Arc::new(AtomicBool::new(false));
        let has_text_delta = Arc::new(AtomicBool::new(false));
        let has_tool_calls = Arc::new(AtomicBool::new(false));
        let mut done_state = SseDoneState::default();
        let mut remainder = Vec::new();

        let output = append_and_parse_sse(
            &mut buffer,
            &mut remainder,
            &chunk,
            &prompt_tokens,
            &completion_tokens,
            &has_sse_error,
            &has_text_delta,
            &has_tool_calls,
            Some("gpt-test"),
            &mut done_state,
        );

        assert!(output.is_none());
        assert!(!has_text_delta.load(Ordering::Relaxed));
        assert!(!has_tool_calls.load(Ordering::Relaxed));
        assert_eq!(completion_tokens.load(Ordering::Relaxed), 0);
        assert!(!is_completed_stream_success(
            200,
            has_sse_error.load(Ordering::Relaxed),
            has_text_delta.load(Ordering::Relaxed),
            has_tool_calls.load(Ordering::Relaxed),
            completion_tokens.load(Ordering::Relaxed)
        ));
    }

    #[test]
    fn request_structured_output_and_tool_calling_detection_are_separate() {
        let tools_body = serde_json::json!({
            "model": "auto",
            "messages": [{"role": "user", "content": "hi"}],
            "tools": [{"type": "function", "function": {"name": "run"}}]
        });
        assert!(!request_uses_structured_output(&tools_body));
        assert!(request_uses_tool_calling(&tools_body));

        let tool_choice_body = serde_json::json!({
            "model": "auto",
            "messages": [{"role": "user", "content": "hi"}],
            "tool_choice": "auto"
        });
        assert!(!request_uses_structured_output(&tool_choice_body));
        assert!(request_uses_tool_calling(&tool_choice_body));

        let structured_body = serde_json::json!({
            "model": "auto",
            "messages": [{"role": "user", "content": "hi"}],
            "response_format": {"type": "json_object"}
        });
        assert!(request_uses_structured_output(&structured_body));
        assert!(!request_uses_tool_calling(&structured_body));

        let plain_body = serde_json::json!({
            "model": "auto",
            "messages": [{"role": "user", "content": "hi"}]
        });
        assert!(!request_uses_structured_output(&plain_body));
        assert!(!request_uses_tool_calling(&plain_body));
    }

    // ── tool_calls 场景测试 ─────────────────────────────────────────

    #[test]
    fn append_and_parse_sse_no_model_info_when_tool_calls_present() {
        // 场景：响应同时包含文本内容和工具调用（LLM 常见行为）
        // 预期：不附加模型信息，因为 tool_call 是中间步骤
        let mut buffer = String::new();
        let chunk = Bytes::from_static(
            b"data: {\"choices\":[{\"delta\":{\"content\":\"Let me check.\"}}]}\n\
data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_1\",\"type\":\"function\",\"function\":{\"name\":\"get_weather\",\"arguments\":\"\"}}]}}]}\n\
data: {\"choices\":[],\"finish_reason\":\"tool_calls\"}\n\
data: [DONE]\n"
        );
        let prompt_tokens = Arc::new(AtomicI64::new(0));
        let completion_tokens = Arc::new(AtomicI64::new(0));
        let has_sse_error = Arc::new(AtomicBool::new(false));
        let has_text_delta = Arc::new(AtomicBool::new(false));
        let has_tool_calls = Arc::new(AtomicBool::new(false));
        let mut done_state = SseDoneState::default();

        let mut remainder = Vec::new();
        let output = append_and_parse_sse(
            &mut buffer,
            &mut remainder,
            &chunk,
            &prompt_tokens,
            &completion_tokens,
            &has_sse_error,
            &has_text_delta,
            &has_tool_calls,
            Some("gpt-test"),
            &mut done_state,
        );

        // 有文本内容，所以 has_text_delta 应为 true
        assert!(has_text_delta.load(Ordering::Relaxed));
        // 有工具调用，所以 has_tool_calls 应为 true
        assert!(has_tool_calls.load(Ordering::Relaxed));
        // 不应附加模型信息
        assert!(output.is_none());
        assert!(!done_state.appended_model_info);
    }

    #[test]
    fn append_and_parse_sse_no_model_info_when_pure_tool_calls() {
        // 场景：纯工具调用响应（无文本内容）
        // 预期：不附加模型信息
        let mut buffer = String::new();
        let chunk = Bytes::from_static(
            b"data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_1\",\"type\":\"function\",\"function\":{\"name\":\"get_weather\",\"arguments\":\"\"}}]}}]}\n\
data: {\"choices\":[],\"finish_reason\":\"tool_calls\"}\n\
data: [DONE]\n"
        );
        let prompt_tokens = Arc::new(AtomicI64::new(0));
        let completion_tokens = Arc::new(AtomicI64::new(0));
        let has_sse_error = Arc::new(AtomicBool::new(false));
        let has_text_delta = Arc::new(AtomicBool::new(false));
        let has_tool_calls = Arc::new(AtomicBool::new(false));
        let mut done_state = SseDoneState::default();

        let mut remainder = Vec::new();
        let output = append_and_parse_sse(
            &mut buffer,
            &mut remainder,
            &chunk,
            &prompt_tokens,
            &completion_tokens,
            &has_sse_error,
            &has_text_delta,
            &has_tool_calls,
            Some("gpt-test"),
            &mut done_state,
        );

        // 无文本内容，has_text_delta 应为 false
        assert!(!has_text_delta.load(Ordering::Relaxed));
        // 有工具调用，has_tool_calls 应为 true
        assert!(has_tool_calls.load(Ordering::Relaxed));
        // 不应附加模型信息
        assert!(output.is_none());
        assert!(!done_state.appended_model_info);
    }

    #[test]
    fn append_and_parse_sse_appends_model_info_for_pure_text() {
        // 场景：纯文本响应（无工具调用）
        // 预期：附加模型信息
        let mut buffer = String::new();
        let chunk = Bytes::from_static(
            b"data: {\"choices\":[{\"delta\":{\"content\":\"Hello!\"}}]}\n\
data: {\"choices\":[],\"finish_reason\":\"stop\"}\n\
data: [DONE]\n",
        );
        let prompt_tokens = Arc::new(AtomicI64::new(0));
        let completion_tokens = Arc::new(AtomicI64::new(0));
        let has_sse_error = Arc::new(AtomicBool::new(false));
        let has_text_delta = Arc::new(AtomicBool::new(false));
        let has_tool_calls = Arc::new(AtomicBool::new(false));
        let mut done_state = SseDoneState::default();

        let mut remainder = Vec::new();
        let output = append_and_parse_sse(
            &mut buffer,
            &mut remainder,
            &chunk,
            &prompt_tokens,
            &completion_tokens,
            &has_sse_error,
            &has_text_delta,
            &has_tool_calls,
            Some("gpt-test"),
            &mut done_state,
        );

        // 有文本内容
        assert!(has_text_delta.load(Ordering::Relaxed));
        // 无工具调用
        assert!(!has_tool_calls.load(Ordering::Relaxed));
        // 应附加模型信息
        let text =
            String::from_utf8(output.expect("should have output").to_vec()).expect("valid utf8");
        assert_eq!(text.matches("model: gpt-test").count(), 1);
        assert!(done_state.appended_model_info);
    }

    /// 回归测试（历史 bug）：带 tools 定义但本轮响应**没有**实际 tool_calls
    /// 应该正常注入 model: xxx。
    ///
    /// 历史：commit 3f5825d 曾在请求侧检查 body 里是否含任何 tool 字段来屏蔽注入，
    /// 结果所有 agent 客户端（Cursor / Claude Code 等）都看不到模型名。
    /// 本测试锁定修复：请求侧检查已撤销，只看响应侧 `has_tool_calls`。
    #[test]
    fn should_append_model_info_ignores_request_tools_definition() {
        // 仅测纯函数行为：request_uses_structured_output 与 request body 是否含 tools 无关
        let body_with_tools = serde_json::json!({
            "model": "auto",
            "messages": [{"role": "user", "content": "你好"}],
            "tools": [{
                "type": "function",
                "function": {"name": "read_file", "parameters": {}}
            }],
            "tool_choice": "auto"
        });
        // 核心断言：带 tools 的请求不应触发 structured_output 分支
        assert!(!request_uses_structured_output(&body_with_tools));
        // 次要断言：contains_tool_calling_field 仍识别 tools 定义（以备未来精细化判断）
        assert!(contains_tool_calling_field(&body_with_tools));
    }

    #[test]
    fn append_and_parse_sse_appends_model_info_when_text_only_response_despite_tools_available() {
        // Scenario: agent clients (Cursor / Claude Code) always send `tools` definition,
        // but the actual response contains text only - no tool_calls.
        // Historic 3f5825d bug: tools-in-request blocked injection entirely -> users
        //   on agent clients never saw `model: xxx`.
        // Fix: request-side tool-definition check removed; only response-side
        //   has_tool_calls blocks injection. A text-only response must inject once.
        let mut buffer = String::new();
        let chunk = Bytes::from_static(
            b"data: {\"choices\":[{\"delta\":{\"content\":\"Hello!\"}}]}\n\
data: {\"choices\":[{\"delta\":{\"content\":\" How can I help?\"}}]}\n\
data: {\"choices\":[],\"finish_reason\":\"stop\"}\n\
data: [DONE]\n",
        );
        let prompt_tokens = Arc::new(AtomicI64::new(0));
        let completion_tokens = Arc::new(AtomicI64::new(0));
        let has_sse_error = Arc::new(AtomicBool::new(false));
        let has_text_delta = Arc::new(AtomicBool::new(false));
        let has_tool_calls = Arc::new(AtomicBool::new(false));
        let mut done_state = SseDoneState::default();

        let mut remainder = Vec::new();
        let output = append_and_parse_sse(
            &mut buffer,
            &mut remainder,
            &chunk,
            &prompt_tokens,
            &completion_tokens,
            &has_sse_error,
            &has_text_delta,
            &has_tool_calls,
            Some("gpt-4o"),
            &mut done_state,
        );

        assert!(has_text_delta.load(Ordering::Relaxed));
        assert!(
            !has_tool_calls.load(Ordering::Relaxed),
            "response has no tool_calls; has_tool_calls must remain false"
        );
        let text = String::from_utf8(
            output
                .expect("model: gpt-4o should be injected (regression: tools-in-request no longer blocks)")
                .to_vec())
        .expect("valid utf8");
        assert_eq!(text.matches("model: gpt-4o").count(), 1);
        assert!(done_state.appended_model_info);
    }

    #[test]
    fn transform_sse_chunk_no_model_info_when_tool_calls_present() {
        // 场景：Claude 适配器路径，响应包含工具调用
        // 预期：不附加模型信息
        let adapter = get_adapter("claude");
        let chunk = Bytes::from_static(
            b"data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"tool_use\",\"id\":\"toolu_1\",\"name\":\"get_weather\"}}\n\
data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{}\"}}\n\
data: {\"type\":\"message_stop\"}\n\
data: [DONE]\n"
        );
        let mut buffer = String::new();
        let prompt_tokens = Arc::new(AtomicI64::new(0));
        let completion_tokens = Arc::new(AtomicI64::new(0));
        let has_text_delta = Arc::new(AtomicBool::new(false));
        let has_tool_calls = Arc::new(AtomicBool::new(false));
        let mut done_state = SseDoneState::default();

        let mut remainder = Vec::new();
        let output = transform_sse_chunk(
            &chunk,
            &mut buffer,
            &mut remainder,
            &adapter,
            &prompt_tokens,
            &completion_tokens,
            &has_text_delta,
            &has_tool_calls,
            Some("claude-3"),
            &mut done_state,
        )
        .expect("transformed output");

        // 工具调用被转换为 OpenAI 格式的 tool_calls
        assert!(has_tool_calls.load(Ordering::Relaxed));
        // 不应附加模型信息
        let output = String::from_utf8(output.to_vec()).expect("valid utf8");
        assert_eq!(output.matches("model: claude-3").count(), 0);
        assert!(!done_state.appended_model_info);
    }

    #[test]
    fn append_and_parse_sse_no_model_info_when_finish_reason_tool_calls() {
        // 场景：finish_reason 为 "tool_calls" 但 delta 中没有 tool_calls 数组
        // 这是 Claude 适配器在 message_delta 事件中的输出格式
        // 预期：不附加模型信息
        let mut buffer = String::new();
        let chunk = Bytes::from_static(
            b"data: {\"choices\":[{\"delta\":{\"content\":\"Let me check.\"}}]}\n\
data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\
data: [DONE]\n",
        );
        let prompt_tokens = Arc::new(AtomicI64::new(0));
        let completion_tokens = Arc::new(AtomicI64::new(0));
        let has_sse_error = Arc::new(AtomicBool::new(false));
        let has_text_delta = Arc::new(AtomicBool::new(false));
        let has_tool_calls = Arc::new(AtomicBool::new(false));
        let mut done_state = SseDoneState::default();

        let mut remainder = Vec::new();
        let output = append_and_parse_sse(
            &mut buffer,
            &mut remainder,
            &chunk,
            &prompt_tokens,
            &completion_tokens,
            &has_sse_error,
            &has_text_delta,
            &has_tool_calls,
            Some("gpt-test"),
            &mut done_state,
        );

        // 有文本内容
        assert!(has_text_delta.load(Ordering::Relaxed));
        // finish_reason: "tool_calls" 应被检测为工具调用
        assert!(has_tool_calls.load(Ordering::Relaxed));
        // 不应附加模型信息
        assert!(output.is_none());
        assert!(!done_state.appended_model_info);
    }

    #[test]
    fn append_and_parse_sse_no_model_info_when_finish_reason_function_call() {
        // 场景：finish_reason 为 "function_call"（旧格式）
        // 预期：不附加模型信息
        let mut buffer = String::new();
        let chunk = Bytes::from_static(
            b"data: {\"choices\":[{\"delta\":{\"content\":\"Let me check.\"}}]}\n\
data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"function_call\"}]}\n\
data: [DONE]\n",
        );
        let prompt_tokens = Arc::new(AtomicI64::new(0));
        let completion_tokens = Arc::new(AtomicI64::new(0));
        let has_sse_error = Arc::new(AtomicBool::new(false));
        let has_text_delta = Arc::new(AtomicBool::new(false));
        let has_tool_calls = Arc::new(AtomicBool::new(false));
        let mut done_state = SseDoneState::default();

        let mut remainder = Vec::new();
        let output = append_and_parse_sse(
            &mut buffer,
            &mut remainder,
            &chunk,
            &prompt_tokens,
            &completion_tokens,
            &has_sse_error,
            &has_text_delta,
            &has_tool_calls,
            Some("gpt-test"),
            &mut done_state,
        );

        // 有文本内容
        assert!(has_text_delta.load(Ordering::Relaxed));
        // finish_reason: "function_call" 应被检测为工具调用
        assert!(has_tool_calls.load(Ordering::Relaxed));
        // 不应附加模型信息
        assert!(output.is_none());
        assert!(!done_state.appended_model_info);
    }

    #[test]
    fn append_and_parse_sse_no_model_info_when_delta_function_call() {
        let mut buffer = String::new();
        let chunk = Bytes::from_static(
            b"data: {\"choices\":[{\"delta\":{\"content\":\"Let me check.\"}}]}\n\
data: {\"choices\":[{\"delta\":{\"function_call\":{\"name\":\"get_weather\",\"arguments\":\"{}\"}}}]}\n\
data: [DONE]\n"
        );
        let prompt_tokens = Arc::new(AtomicI64::new(0));
        let completion_tokens = Arc::new(AtomicI64::new(0));
        let has_sse_error = Arc::new(AtomicBool::new(false));
        let has_text_delta = Arc::new(AtomicBool::new(false));
        let has_tool_calls = Arc::new(AtomicBool::new(false));
        let mut done_state = SseDoneState::default();

        let mut remainder = Vec::new();
        let output = append_and_parse_sse(
            &mut buffer,
            &mut remainder,
            &chunk,
            &prompt_tokens,
            &completion_tokens,
            &has_sse_error,
            &has_text_delta,
            &has_tool_calls,
            Some("gpt-test"),
            &mut done_state,
        );

        assert!(has_text_delta.load(Ordering::Relaxed));
        assert!(has_tool_calls.load(Ordering::Relaxed));
        assert!(output.is_none());
        assert!(!done_state.appended_model_info);
    }

    #[test]
    fn stream_chunk_detects_message_level_tool_and_function_calls() {
        let message_tool_calls = serde_json::json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "tool_calls": [{
                        "id": "call_1",
                        "type": "function",
                        "function": {"name": "get_weather", "arguments": "{}"}
                    }]
                }
            }]
        });
        assert!(stream_chunk_has_tool_calls(&message_tool_calls));

        let message_function_call = serde_json::json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "function_call": {"name": "get_weather", "arguments": "{}"}
                }
            }]
        });
        assert!(stream_chunk_has_tool_calls(&message_function_call));
    }

    #[test]
    fn disable_reasoning_rewrites_four_model_requests() {
        let models = [
            "deepseek-v4-falsh",
            "mimo-v2.5-pro",
            "qwen/qwen3.5-122b-a10b",
            "openai/gpt-oss-120b",
        ];

        for model in models {
            let mut body = serde_json::json!({
                "model": model,
                "thinking": true,
                "reasoning": { "effort": "high" },
                "reasoning_content": "顶层思维链",
                "reasoning_text": "顶层兼容思维链",
                "reasoning_details": "顶层详情思维链",
                "reasoning_effort": "high",
                "messages": [
                    {
                        "role": "user",
                        "content": "你好"
                    },
                    {
                        "role": "assistant",
                        "content": "历史回答",
                        "thinking": true,
                        "reasoning": { "effort": "medium" },
                        "reasoning_content": "历史思维链",
                        "reasoning_text": "历史兼容思维链",
                        "reasoning_details": "历史详情思维链",
                        "reasoning_effort": "medium"
                    }
                ]
            });

            apply_disable_reasoning(&mut body);

            let obj = body.as_object().expect("请求体必须是对象");
            assert!(!obj.contains_key("thinking"));
            assert!(!obj.contains_key("reasoning"));
            assert!(!obj.contains_key("reasoning_content"));
            assert!(!obj.contains_key("reasoning_text"));
            assert!(!obj.contains_key("reasoning_details"));
            assert!(!obj.contains_key("reasoning_effort"));

            let messages = body.get("messages").and_then(Value::as_array).expect("必须保留消息数组");
            let assistant = messages.get(1).and_then(Value::as_object).expect("必须保留助手消息");
            assert!(!assistant.contains_key("thinking"));
            assert!(!assistant.contains_key("reasoning"));
            assert!(!assistant.contains_key("reasoning_content"));
            assert!(!assistant.contains_key("reasoning_text"));
            assert!(!assistant.contains_key("reasoning_details"));
            assert!(!assistant.contains_key("reasoning_effort"));
            assert_eq!(assistant.get("content"), Some(&Value::String("历史回答".to_string())));
        }
    }
}

