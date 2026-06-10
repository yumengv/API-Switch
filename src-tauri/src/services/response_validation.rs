use crate::proxy::protocol::ProtocolAdapter;
use serde_json::Value;

pub fn validate_chat_response_body(
    adapter: &(dyn ProtocolAdapter + Send + Sync),
    body: &str,
) -> Result<Value, String> {
    let trimmed = body.trim();
    if trimmed.is_empty() {
        return Err("empty_response".to_string());
    }

    let raw = serde_json::from_str::<Value>(trimmed)
        .map_err(|e| format!("invalid_json_response: {e}"))?;

    if let Some(detail) = detect_error_payload(&raw) {
        return Err(detail);
    }

    let mut normalized = raw.clone();
    adapter.transform_response(&mut normalized);

    if let Some(detail) = detect_error_payload(&normalized) {
        return Err(detail);
    }

    if has_chat_output(&normalized) || has_chat_output(&raw) {
        Ok(normalized)
    } else {
        Err("invalid_response: missing assistant output".to_string())
    }
}

fn detect_error_payload(value: &Value) -> Option<String> {
    let obj = value.as_object()?;

    if let Some(error) = obj.get("error") {
        if !is_empty_error_value(error) {
            return Some(format_error_value(error));
        }
    }

    if let Some(error) = value.pointer("/response/error") {
        if !is_empty_error_value(error) {
            return Some(format_error_value(error));
        }
    }

    if obj.get("success").and_then(Value::as_bool) == Some(false) {
        return Some(top_level_message(value).unwrap_or_else(|| "success_false".to_string()));
    }

    for key in ["status_code", "statusCode", "http_status", "httpStatus"] {
        if let Some(code) = obj.get(key).and_then(value_as_status_code) {
            if code >= 400 {
                return Some(top_level_message(value).unwrap_or_else(|| format!("http_{code}")));
            }
        }
    }

    if let Some(code) = obj.get("code") {
        if let Some(status_code) = value_as_status_code(code) {
            if status_code >= 400 {
                return Some(top_level_message(value).unwrap_or_else(|| format!("http_{status_code}")));
            }
        }
        if let Some(code_text) = code.as_str() {
            if error_text_like(code_text) {
                return Some(top_level_message(value).unwrap_or_else(|| code_text.to_string()));
            }
        }
    }

    if let Some(status) = obj.get("status") {
        if let Some(status_code) = value_as_status_code(status) {
            if status_code >= 400 {
                return Some(top_level_message(value).unwrap_or_else(|| format!("http_{status_code}")));
            }
        }
        if let Some(status_text) = status.as_str() {
            let lower = status_text.to_ascii_lowercase();
            if matches!(
                lower.as_str(),
                "error" | "failed" | "failure" | "denied" | "forbidden" | "unauthorized"
            ) {
                return Some(top_level_message(value).unwrap_or_else(|| status_text.to_string()));
            }
        }
    }

    if obj.get("type").and_then(Value::as_str) == Some("error") {
        return Some(top_level_message(value).unwrap_or_else(|| "type_error".to_string()));
    }

    if let Some(message) = top_level_message(value) {
        if error_text_like(&message) && !has_chat_output(value) {
            return Some(message);
        }
    }

    None
}

fn is_empty_error_value(value: &Value) -> bool {
    match value {
        Value::Null => true,
        Value::Bool(false) => true,
        Value::String(text) => text.trim().is_empty(),
        Value::Object(map) => map.is_empty(),
        _ => false,
    }
}

fn format_error_value(value: &Value) -> String {
    if let Some(text) = value.as_str() {
        return text.to_string();
    }
    if let Some(message) = value.get("message").and_then(Value::as_str) {
        return message.to_string();
    }
    if let Some(detail) = value.get("detail").and_then(Value::as_str) {
        return detail.to_string();
    }
    serde_json::to_string(value).unwrap_or_else(|_| "api_error".to_string())
}

fn top_level_message(value: &Value) -> Option<String> {
    let obj = value.as_object()?;
    for key in ["message", "detail", "error_message", "errorMessage", "title"] {
        if let Some(text) = obj.get(key).and_then(Value::as_str) {
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }
    None
}

fn value_as_status_code(value: &Value) -> Option<u16> {
    if let Some(code) = value.as_u64() {
        return u16::try_from(code).ok();
    }
    value
        .as_str()
        .and_then(|text| text.trim().parse::<u16>().ok())
}

fn error_text_like(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    [
        "http 401",
        "http 403",
        "401",
        "403",
        "forbidden",
        "unauthorized",
        "permission denied",
        "access denied",
        "not authorized",
        "invalid api key",
        "invalid_api_key",
        "invalid token",
        "insufficient_quota",
        "quota_exceeded",
        "credit balance",
        "account has been disabled",
        "organization has been disabled",
        "rate limit",
        "too many requests",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

fn has_chat_output(value: &Value) -> bool {
    has_openai_choices(value)
        || non_empty_string(value.get("output_text"))
        || non_empty_array(value.get("output"))
        || non_empty_array(value.get("content"))
        || non_empty_string(value.get("content"))
        || non_empty_array(value.get("candidates"))
}

fn has_openai_choices(value: &Value) -> bool {
    let Some(choices) = value.get("choices").and_then(Value::as_array) else {
        return false;
    };
    choices.iter().any(|choice| {
        non_empty_string(choice.pointer("/message/content"))
            || non_empty_string(choice.get("text"))
            || non_empty_array(choice.pointer("/message/tool_calls"))
            || choice.get("finish_reason").is_some()
    })
}

fn non_empty_string(value: Option<&Value>) -> bool {
    value
        .and_then(Value::as_str)
        .map(|text| !text.trim().is_empty())
        .unwrap_or(false)
}

fn non_empty_array(value: Option<&Value>) -> bool {
    value
        .and_then(Value::as_array)
        .map(|items| !items.is_empty())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proxy::protocol::get_adapter;
    use serde_json::json;

    #[test]
    fn rejects_openai_error_even_with_http_200() {
        let adapter = get_adapter("openai");
        let body = json!({
            "error": {
                "message": "HTTP 403 Forbidden",
                "type": "permission_error"
            }
        });

        let err = validate_chat_response_body(adapter.as_ref(), &body.to_string()).unwrap_err();
        assert!(err.contains("403"));
    }

    #[test]
    fn rejects_non_json_error_page() {
        let adapter = get_adapter("openai");
        let err = validate_chat_response_body(adapter.as_ref(), "403 Forbidden").unwrap_err();
        assert!(err.starts_with("invalid_json_response"));
    }

    #[test]
    fn accepts_chat_completion() {
        let adapter = get_adapter("openai");
        let body = json!({
            "choices": [{
                "message": { "role": "assistant", "content": "OK" },
                "finish_reason": "stop"
            }],
            "usage": { "prompt_tokens": 1, "completion_tokens": 1 }
        });

        assert!(validate_chat_response_body(adapter.as_ref(), &body.to_string()).is_ok());
    }
}
