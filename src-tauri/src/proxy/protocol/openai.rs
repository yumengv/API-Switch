use super::{join_url, ProtocolAdapter};
/// OpenAI protocol adapter.
///
/// Handles all openai-type channels — both standard OpenAI endpoints
/// (which expect /v1/chat/completions) and custom endpoints with a
/// user-provided API path. When the base URL already contains a path
/// after the host, that path is treated as the authoritative API root.
use serde_json::Value;
pub struct OpenAiAdapter;

/// Returns `true` when the base URL already carries a path after the
/// host:port. Such URLs are treated as authoritative API roots — the
/// caller should append the endpoint directly without forcing a /v1/
/// prefix.
fn has_custom_api_path(base_url: &str) -> bool {
    let base = base_url.trim_end_matches('/');
    if let Some(scheme_end) = base.find("://") {
        let after_scheme = &base[scheme_end + 3..];
        if let Some(slash_pos) = after_scheme.find('/') {
            return !after_scheme[slash_pos + 1..].is_empty();
        }
    }
    false
}

impl ProtocolAdapter for OpenAiAdapter {
    fn build_chat_url(&self, base_url: &str, _model: &str) -> String {
        if has_custom_api_path(base_url) {
            let base = base_url.trim_end_matches('/');
            format!("{}/chat/completions", base)
        } else {
            join_url(base_url, "v1/chat/completions")
        }
    }

    fn build_models_url(&self, base_url: &str, _api_key: &str) -> String {
        if has_custom_api_path(base_url) {
            let base = base_url.trim_end_matches('/');
            format!("{}/models", base)
        } else {
            join_url(base_url, "v1/models")
        }
    }

    fn uses_query_auth(&self) -> bool {
        false
    }

    fn build_auth_headers(&self, api_key: &str) -> Vec<(String, String)> {
        vec![("Authorization".to_string(), format!("Bearer {api_key}"))]
    }

    fn apply_auth(
        &self,
        builder: reqwest::RequestBuilder,
        api_key: &str,
    ) -> reqwest::RequestBuilder {
        builder.bearer_auth(api_key)
    }

    fn transform_request(&self, body: &mut Value, actual_model: &str) {
        // OpenAI-compatible reasoning/THINK extensions are passthrough only: preserve, never synthesize.
        if let Some(obj) = body.as_object_mut() {
            obj.insert("model".to_string(), Value::String(actual_model.to_string()));
        }
    }

    fn transform_response(&self, _body: &mut Value) {
        // Passthrough preserves non-standard reasoning_content returned by OpenAI-compatible providers.
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
        body.get("data")
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
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn transform_request_preserves_openai_compatible_reasoning_extensions() {
        let adapter = OpenAiAdapter;
        let mut body = json!({
            "model": "auto",
            "reasoning_effort": "high",
            "thinking": {"type": "enabled", "budget_tokens": 4096},
            "messages": [{
                "role": "assistant",
                "content": "",
                "reasoning_content": "kept reasoning",
                "provider_specific": {"thinking": "kept provider thinking"}
            }]
        });

        adapter.transform_request(&mut body, "resolved-model");

        assert_eq!(body["model"], "resolved-model");
        assert_eq!(body["reasoning_effort"], "high");
        assert_eq!(body["thinking"]["type"], "enabled");
        assert_eq!(body["messages"][0]["reasoning_content"], "kept reasoning");
        assert_eq!(
            body["messages"][0]["provider_specific"]["thinking"],
            "kept provider thinking"
        );
    }

    #[test]
    fn transform_response_preserves_openai_compatible_reasoning_extensions() {
        let adapter = OpenAiAdapter;
        let mut body = json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": "visible",
                    "reasoning_content": "kept reasoning"
                }
            }]
        });
        let original = body.clone();

        adapter.transform_response(&mut body);

        assert_eq!(body, original);
    }
}
