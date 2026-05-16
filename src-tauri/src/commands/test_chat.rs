use crate::database::Database;
use crate::error::AppError;
use crate::proxy::protocol::get_adapter;
use crate::refresh_tray_if_enabled;
use crate::services::log_service::{insert_test_usage_log, TestUsageLogInput};
use crate::AppState;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::time::Instant;
use tauri::{Emitter, State};

#[derive(Debug, Serialize)]
pub struct TestChatResponse {
    pub content: String,
    pub latency_ms: u64,
    pub usage: Option<TestChatUsage>,
}

#[derive(Debug, Serialize)]
pub struct TestChatUsage {
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    pub total_tokens: i64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TestChatMessage {
    pub role: String,
    pub content: String,
}

fn refresh_entries(app: &tauri::AppHandle) {
    let _ = app.emit("entries-changed", ());
    crate::state_version::bump();
    refresh_tray_if_enabled(app);
}

fn mark_entry_available(
    db: &Database,
    app: &tauri::AppHandle,
    entry_id: &str,
    response_ms: &str,
) -> Result<(), AppError> {
    db.update_entry_response_ms(entry_id, response_ms)?;
    db.toggle_entry(entry_id, true)?;
    db.set_entry_cooldown(entry_id, None)?;
    refresh_entries(app);
    Ok(())
}

fn mark_entry_unavailable(
    db: &Database,
    app: &tauri::AppHandle,
    entry_id: &str,
) -> Result<(), AppError> {
    db.update_entry_response_ms(entry_id, "X")?;
    db.toggle_entry(entry_id, false)?;
    refresh_entries(app);
    Ok(())
}

#[tauri::command]
pub async fn test_chat(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    entry_id: String,
    messages: Vec<TestChatMessage>,
) -> Result<TestChatResponse, AppError> {
    let db = state.db.clone();

    // Get the entry directly (all entries, not just enabled ones)
    let entries = db.get_entries_for_routing_all()?;
    let entry = entries
        .iter()
        .find(|e| e.id == entry_id)
        .ok_or_else(|| AppError::NotFound(format!("Entry {entry_id} not found")))?
        .clone();

    // Get channel info
    let channel = db.get_channel(&entry.channel_id)?;

    // Get protocol adapter
    let adapter = get_adapter(&channel.api_type);

    // Build URL and transform request
    let url = adapter.build_chat_url(&channel.base_url, &entry.model);
    let mut upstream_body = json!({
        "model": entry.model,
        "messages": messages,
        "stream": false,
    });
    adapter.transform_request(&mut upstream_body, &entry.model);

    let start = Instant::now();

    // Send request directly to upstream
    let client = reqwest::Client::new();
    let request = adapter
        .apply_auth(client.post(&url), &channel.api_key)
        .json(&upstream_body);

    let response = match request.send().await {
        Ok(response) => response,
        Err(e) => {
            let latency_ms = start.elapsed().as_millis() as i64;
            let message = format!("Request failed: {e}");
            insert_test_usage_log(
                &db,
                None,
                TestUsageLogInput {
                    entry: &entry,
                    channel: &channel,
                    operation: "test_chat",
                    log_group: "test_chat",
                    prompt_tokens: 0,
                    completion_tokens: 0,
                    latency_ms,
                    status_code: 502,
                    success: false,
                    error_message: Some(&message),
                    error_kind: Some("network_error"),
                    response_ms: Some("X"),
                    error_preview: None,
                },
            );
            mark_entry_unavailable(&db, &app, &entry.id)?;
            return Err(AppError::Network(message));
        }
    };

    if !response.status().is_success() {
        let latency_ms = start.elapsed().as_millis() as i64;
        let status = response.status();
        let status_code = status.as_u16() as i32;
        let body = response.text().await.unwrap_or_default();
        let error_message = format!("Upstream error {status}: {body}");
        let log_message = format!("upstream_http_{}", status.as_u16());
        insert_test_usage_log(
            &db,
            None,
            TestUsageLogInput {
                entry: &entry,
                channel: &channel,
                operation: "test_chat",
                log_group: "test_chat",
                prompt_tokens: 0,
                completion_tokens: 0,
                latency_ms,
                status_code,
                success: false,
                error_message: Some(&log_message),
                error_kind: Some("http_error"),
                response_ms: Some("X"),
                error_preview: Some(&body),
            },
        );
        mark_entry_unavailable(&db, &app, &entry.id)?;
        return Err(AppError::Proxy(error_message));
    }

    let latency_ms = start.elapsed().as_millis() as u64;

    let json_body: serde_json::Value = match response.json().await {
        Ok(body) => body,
        Err(e) => {
            let message = format!("Failed to parse response: {e}");
            insert_test_usage_log(
                &db,
                None,
                TestUsageLogInput {
                    entry: &entry,
                    channel: &channel,
                    operation: "test_chat",
                    log_group: "test_chat",
                    prompt_tokens: 0,
                    completion_tokens: 0,
                    latency_ms: latency_ms as i64,
                    status_code: 502,
                    success: false,
                    error_message: Some(&message),
                    error_kind: Some("parse_error"),
                    response_ms: Some("X"),
                    error_preview: None,
                },
            );
            mark_entry_unavailable(&db, &app, &entry.id)?;
            return Err(AppError::Internal(message));
        }
    };

    // Transform response if needed (e.g. Claude → OpenAI format)
    let mut json_body = json_body;
    adapter.transform_response(&mut json_body);

    // Extract content
    let content = json_body
        .get("choices")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("message"))
        .and_then(|m| m.get("content"))
        .and_then(|c| c.as_str())
        .unwrap_or("")
        .to_string();

    if content.trim().is_empty() {
        let message = "empty_response_content";
        insert_test_usage_log(
            &db,
            None,
            TestUsageLogInput {
                entry: &entry,
                channel: &channel,
                operation: "test_chat",
                log_group: "test_chat",
                prompt_tokens: 0,
                completion_tokens: 0,
                latency_ms: latency_ms as i64,
                status_code: 200,
                success: false,
                error_message: Some(message),
                error_kind: Some("empty_content"),
                response_ms: Some("X"),
                error_preview: None,
            },
        );
        mark_entry_unavailable(&db, &app, &entry.id)?;
        return Err(AppError::Internal(message.to_string()));
    }

    // Extract usage
    let usage = json_body.get("usage").map(|u| TestChatUsage {
        prompt_tokens: u.get("prompt_tokens").and_then(|v| v.as_i64()).unwrap_or(0),
        completion_tokens: u
            .get("completion_tokens")
            .and_then(|v| v.as_i64())
            .unwrap_or(0),
        total_tokens: u.get("total_tokens").and_then(|v| v.as_i64()).unwrap_or(0),
    });

    let response_ms = latency_ms.to_string();
    insert_test_usage_log(
        &db,
        None,
        TestUsageLogInput {
            entry: &entry,
            channel: &channel,
            operation: "test_chat",
            log_group: "test_chat",
            prompt_tokens: usage.as_ref().map(|u| u.prompt_tokens).unwrap_or(0),
            completion_tokens: usage.as_ref().map(|u| u.completion_tokens).unwrap_or(0),
            latency_ms: latency_ms as i64,
            status_code: 200,
            success: true,
            error_message: None,
            error_kind: None,
            response_ms: Some(&response_ms),
            error_preview: None,
        },
    );

    mark_entry_available(&db, &app, &entry.id, &response_ms)?;

    Ok(TestChatResponse {
        content,
        latency_ms,
        usage,
    })
}
