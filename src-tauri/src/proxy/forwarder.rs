use super::circuit_breaker::CircuitBreaker;
use super::handlers::ProxyError;
use super::middleware::{CallerKind, ForwarderMiddleware, RequestContext};
use super::protocol::get_adapter;
use super::server::ProxyState;
use crate::database::{AccessKey, ApiEntry, AppSettings, Database};
use crate::refresh_tray_if_enabled;
use axum::body::Body;
use axum::http::HeaderMap;
use axum::response::IntoResponse;
use bytes::Bytes;
use futures::Stream;
use serde_json::Value;
use std::error::Error;
use std::future::Future;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::Arc;
use std::task::Poll;
use std::time::{Duration, Instant};
use tauri::Emitter;
use tokio::time::sleep;

const STREAMING_IDLE_TIMEOUT: Duration = Duration::from_secs(300);
const STREAMING_PING_INTERVAL: Duration = Duration::from_secs(10);

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
    Dropped,
}

impl StreamEndReason {
    fn as_str(self) -> &'static str {
        match self {
            StreamEndReason::Done => "done",
            StreamEndReason::UpstreamError => "upstream_error",
            StreamEndReason::Timeout => "timeout",
            StreamEndReason::Dropped => "dropped",
        }
    }
}

fn is_completed_stream_success(
    status_code: i32,
    has_sse_error: bool,
    chunk_count: i64,
    streamed_bytes: i64,
) -> bool {
    status_code == 200 && !has_sse_error && chunk_count > 0 && streamed_bytes > 0
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
) -> String {
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
    first_token_ms: i64,
    status_code: i32,
}

/// StreamLogGuard: safety net for writing usage log when stream is dropped
/// without reaching Poll::Ready(None) (e.g. client disconnect).
/// Primary log writing happens in Poll::Ready(None) — this guard is fallback only.
struct StreamLogGuard {
    logged: Arc<AtomicBool>,
    db: Arc<Database>,
    app_handle: tauri::AppHandle,
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
            );
            let db = self.db.clone();
            let app_handle = self.app_handle.clone();
            let access_key = self.access_key.clone();
            let entry = self.entry.clone();
            let requested_model = self.requested_model.clone();
            let status_code = self.status_code;
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
                    first_token_ms,
                    latency_ms,
                    status_code,
                    success,
                    Some(stream_summary.as_str()),
                    Some(attempt_path.as_str()),
                    Some(StreamEndReason::Dropped),
                );
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
    middleware: &[Box<dyn super::middleware::ForwarderMiddleware>],
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
                    latency_ms,
                    log_status,
                    false,
                    Some(&e),
                    Some(attempt_path.as_str()),
                    None,
                );

                // Step 2: disable unrecoverable status codes, otherwise cool down briefly.
                // Connection failures report status=0 and must remain recoverable.
                if status > 0
                    && should_disable_entry_for_status(&settings.circuit_disable_codes, status)
                {
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

async fn forward_single(
    state: &ProxyState,
    entry: &ApiEntry,
    body: &Value,
    requested_model: &str,
    access_key: Option<&AccessKey>,
    is_stream: bool,
    prior_attempts: Vec<AttemptInfo>,
    middleware: &[Box<dyn super::middleware::ForwarderMiddleware>],
    caller_kind: &CallerKind,
) -> Result<ForwardResult, ForwardError> {
    let channel = state
        .db
        .get_channel(&entry.channel_id)
        .map_err(|e| (format!("DB error: {e}"), 502))?;

    let adapter = get_adapter(&channel.api_type);
    let url = adapter.build_chat_url(&channel.base_url, &entry.model);

    let mut upstream_body = body.clone();
    adapter.transform_request(&mut upstream_body, &entry.model);

    // Call middleware on_request
    let ctx = RequestContext {
        caller_kind: caller_kind.clone(),
        requested_model,
    };
    for mw in middleware.iter() {
        mw.on_request(&mut upstream_body, &ctx);
    }

    let mut request = adapter
        .apply_auth(state.http_client.post(&url), &channel.api_key)
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
        let needs_transform = adapter.needs_sse_transform();
        let append_model_info = should_append_model_info(state, body);
        let response = build_streaming_response(
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
        Ok(ForwardResult {
            response,
            prompt_tokens: 0,
            completion_tokens: 0,
            first_token_ms: 0,
            status_code,
        })
    } else {
        let mut response_body: Value = response
            .json()
            .await
            .map_err(|e| (format!("Failed to parse response: {e}"), 502))?;

        adapter.transform_response(&mut response_body);
        let (prompt_tokens, completion_tokens) = extract_usage_tokens(&response_body);

        Ok(ForwardResult {
            response: axum::Json(response_body).into_response(),
            prompt_tokens,
            completion_tokens,
            first_token_ms: 0,
            status_code,
        })
    }
}

fn extract_usage_tokens(body: &Value) -> (i64, i64) {
    let usage = body.get("usage");
    let prompt_tokens = usage
        .and_then(|v| v.get("prompt_tokens"))
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let completion_tokens = usage
        .and_then(|v| v.get("completion_tokens"))
        .and_then(Value::as_i64)
        .unwrap_or(0);
    (prompt_tokens, completion_tokens)
}

fn request_uses_structured_output(body: &Value) -> bool {
    body.get("response_format").is_some()
}

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

fn request_uses_tool_calling(body: &Value) -> bool {
    contains_tool_calling_field(body)
}

fn should_append_model_info(state: &ProxyState, body: &Value) -> bool {
    let setting_enabled = state
        .settings
        .try_read()
        .map(|settings| settings.show_conversation_model)
        .unwrap_or(true);

    // 模型名是普通正文注入。结构化输出或工具调用上下文会把正文当协议内容复用，
    // 不能追加 `model: ...`，否则下游 CALL 循环会夹带模型信息并重复显示。
    setting_enabled && !request_uses_structured_output(body) && !request_uses_tool_calling(body)
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
    middleware: &[Box<dyn super::middleware::ForwarderMiddleware>],
    ctx: &RequestContext,
) -> axum::response::Response {
    let response_headers = response.headers().clone();
    let upstream_url = upstream_url.to_string();
    let start = request_start;
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
                            // TODO: wire middleware on_sse_chunk when implementations are ready
                            return Poll::Ready(Some(Ok(with_model_info)));
                        }
                    }

                    Poll::Ready(Some(Ok(chunk)))
                }
                Poll::Ready(Some(Err(err))) => {
                    if !logged.swap(true, Ordering::SeqCst) {
                        let chunk_total = chunk_count.load(Ordering::SeqCst);
                        let byte_total = streamed_bytes.load(Ordering::SeqCst);
                        let ft = first_token_ms.load(Ordering::SeqCst);
                        let lat = start.elapsed().as_millis() as i64;
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
                        let pt = prompt_tokens.load(Ordering::SeqCst);
                        let ct = completion_tokens.load(Ordering::SeqCst);
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
                                ft,
                                lat,
                                502,
                                false,
                                Some(error_message.as_str()),
                                Some(attempt_path.as_str()),
                                Some(StreamEndReason::UpstreamError),
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
                        let success =
                            is_completed_stream_success(sc, has_error, chunk_total, byte_total);
                        let attempt_path = attempt_path_with_current(
                            &prior_attempts,
                            &entry,
                            status_code,
                            success,
                            None,
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
                        );
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
                                ft,
                                lat,
                                sc,
                                success,
                                Some(stream_summary.as_str()),
                                Some(attempt_path.as_str()),
                                Some(StreamEndReason::Done),
                            );
                            if success {
                                spawn_record_circuit_success(scb, sfc, sdb, db2.clone(), eah, eid);
                            } else {
                                spawn_cool_down_entry(scb, sfc, sdb, db2.clone(), eah, eid);
                            }
                        });
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
                .is_some_and(|content| !content.is_empty())
        })
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
                .is_some_and(|content| content.trim() == format!("model: {model}"))
        })
}

/// 检测 OpenAI 兼容 chunk 是否包含函数/工具调用信号。
/// 一旦响应中存在工具调用，本轮就是 CALL 中间态，不应附加模型信息，
/// 否则下游循环复用时会把 `model: ...` 当成对话内容重复显示。
fn stream_chunk_has_tool_calls(value: &Value) -> bool {
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
    let payload = serde_json::json!({
        "choices": [{
            "delta": {
                "content": format!("\n\nmodel: {model}")
            }
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
                if let Ok(value) = serde_json::from_str::<Value>(&transformed) {
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
                }
                output.push(format!("data: {transformed}\n\n").into_bytes());
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
            let (prompt, completion) = extract_usage_tokens(&value);
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

    let mut output = Vec::with_capacity(chunk.len() + 64 + model.len());
    output.extend_from_slice(&chunk[..pos]);
    output.extend_from_slice(&model_info_delta(model));
    output.extend_from_slice(&chunk[pos..]);
    Some(Bytes::from(output))
}

fn refresh_tray(app_handle: &tauri::AppHandle) {
    refresh_tray_if_enabled(app_handle);
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

async fn disable_entry(state: &ProxyState, entry: &ApiEntry) {
    let recovery_secs = state.settings.read().await.circuit_recovery_secs.max(1);
    let cooldown_until = chrono::Utc::now().timestamp() + recovery_secs;

    let _ = state.db.toggle_entry(&entry.id, false);
    let _ = state.db.set_entry_cooldown(&entry.id, Some(cooldown_until));
    let _ = state.app_handle.emit("entries-changed", ());
    refresh_tray(&state.app_handle);

    let mut breakers = state.circuit_breakers.write().await;
    breakers.remove(&entry.id);
}

async fn record_circuit_success(state: &ProxyState, entry_id: &str) {
    let _ = state.db.set_entry_cooldown(entry_id, None);
    let _ = state.app_handle.emit("entries-changed", ());
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
    // At/above threshold: remove from AUTO and set a 24h long cooldown.
    if current_count >= threshold {
        let one_day_later = chrono::Utc::now().timestamp() + 86400;
        let _ = state.db.set_entry_cooldown(&entry.id, Some(one_day_later));
        let _ = state.db.toggle_entry(&entry.id, false);
        let _ = state.app_handle.emit("entries-changed", ());
        refresh_tray(&state.app_handle);

        let mut breakers = state.circuit_breakers.write().await;
        breakers.remove(&entry.id);

        log::warn!(
            "Entry {} disabled after {} consecutive failures. Long cooldown: 24h.",
            entry.id,
            current_count
        );
        return;
    }

    let cooldown_until = chrono::Utc::now().timestamp() + recovery_secs as i64;
    let _ = state.db.set_entry_cooldown(&entry.id, Some(cooldown_until));
    let _ = state.app_handle.emit("entries-changed", ());
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
    app_handle: tauri::AppHandle,
    entry_id: String,
) {
    tokio::spawn(async move {
        let recovery_secs = settings.read().await.circuit_recovery_secs as u64;

        let _ = db.set_entry_cooldown(&entry_id, None);
        let _ = app_handle.emit("entries-changed", ());
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
    app_handle: tauri::AppHandle,
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
        // At/above threshold: remove from AUTO and set a 24h long cooldown.
        if current_count >= threshold {
            let one_day_later = chrono::Utc::now().timestamp() + 86400;
            let _ = db.set_entry_cooldown(&entry_id, Some(one_day_later));
            let _ = db.toggle_entry(&entry_id, false);
            let _ = app_handle.emit("entries-changed", ());
            refresh_tray(&app_handle);

            let mut breakers = circuit_breakers.write().await;
            breakers.remove(&entry_id);

            log::warn!(
                "Entry {} disabled after {} consecutive failures. Long cooldown: 24h.",
                entry_id,
                current_count
            );
            return;
        }

        let cooldown_until = chrono::Utc::now().timestamp() + recovery_secs as i64;
        let _ = db.set_entry_cooldown(&entry_id, Some(cooldown_until));
        let _ = app_handle.emit("entries-changed", ());
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
    app_handle: &tauri::AppHandle,
    access_key: Option<&AccessKey>,
    entry: &ApiEntry,
    requested_model: &str,
    is_stream: bool,
    prompt_tokens: i64,
    completion_tokens: i64,
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
        "attempt_path": attempt_path.and_then(|path| serde_json::from_str::<Value>(path).ok()),
        "stream_end_reason": stream_end_reason.map(StreamEndReason::as_str),
    })
    .to_string();

    let _ = db.insert_usage_log(
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
    );

    let _ = app_handle.emit("new-usage-log", ());
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
    fn completed_stream_success_rejects_empty_or_error_streams() {
        assert!(!is_completed_stream_success(200, false, 0, 128));
        assert!(!is_completed_stream_success(200, false, 1, 0));
        assert!(!is_completed_stream_success(200, true, 1, 128));
        assert!(!is_completed_stream_success(502, false, 1, 128));
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
}
