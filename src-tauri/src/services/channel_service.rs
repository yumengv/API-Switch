use crate::admin::{
    ERROR_CODE_EMPTY_MODEL_LIST, ERROR_CODE_ENDPOINT_CORRECTION_FAILED,
    ERROR_CODE_ENDPOINT_UNREACHABLE, ERROR_CODE_ENDPOINT_VALIDATION_FAILED,
    ERROR_CODE_FETCH_MODELS_FAILED, ERROR_CODE_HTTP_CLIENT_ERROR,
    ERROR_CODE_INVALID_CREDENTIALS, ERROR_CODE_INVALID_URL, ERROR_CODE_RATE_LIMITED,
    ERROR_CODE_TIMEOUT, ERROR_CODE_UNSUPPORTED_PROVIDER,
};
use crate::database::{Channel, Database, ModelInfo};
use crate::error::AppError;
use crate::proxy::protocol::get_adapter;
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
pub struct ModelCatalogMetaInput {
    pub model: String,
    pub provider_logo: String,
    pub release_date: String,
    pub model_meta_zh: String,
    pub model_meta_en: String,
}

#[derive(Clone)]
struct ProbeSuccess {
    models: Vec<ModelInfo>,
    corrected_base_url: String,
}

#[derive(Debug, Clone)]
enum ModelsEndpointError {
    Network(String),
    Timeout(String),
    Auth(u16),
    RateLimited(u16),
    Http(u16),
    Parse(String),
    Empty,
}

impl ModelsEndpointError {
    fn is_blocking(&self) -> bool {
        matches!(self, Self::Network(_) | Self::Timeout(_) | Self::Auth(_) | Self::RateLimited(_))
    }

    fn code(&self) -> &'static str {
        match self {
            Self::Network(_) => ERROR_CODE_ENDPOINT_UNREACHABLE,
            Self::Timeout(_) => ERROR_CODE_TIMEOUT,
            Self::Auth(_) => ERROR_CODE_INVALID_CREDENTIALS,
            Self::RateLimited(_) => ERROR_CODE_RATE_LIMITED,
            Self::Http(_) => ERROR_CODE_UNSUPPORTED_PROVIDER,
            Self::Parse(_) => ERROR_CODE_UNSUPPORTED_PROVIDER,
            Self::Empty => ERROR_CODE_EMPTY_MODEL_LIST,
        }
    }

    fn message(&self) -> String {
        match self {
            Self::Network(msg) => format!("Endpoint unreachable: {msg}"),
            Self::Timeout(msg) => format!("Endpoint timeout: {msg}"),
            Self::Auth(status) => format!("Invalid credentials: HTTP {status}"),
            Self::RateLimited(status) => format!("Rate limited: HTTP {status}"),
            Self::Http(status) => format!("Provider returned unsupported response: HTTP {status}"),
            Self::Parse(msg) => format!("Provider returned unsupported response: {msg}"),
            Self::Empty => "Provider returned an empty model list".to_string(),
        }
    }

    fn to_operation_error(&self) -> ChannelOperationError {
        let details = match self {
            Self::Auth(status) | Self::Http(status) | Self::RateLimited(status) => {
                Some(serde_json::json!({ "status_code": status }))
            }
            Self::Network(raw_message) | Self::Timeout(raw_message) | Self::Parse(raw_message) => {
                Some(serde_json::json!({ "raw_message": raw_message }))
            }
            Self::Empty => None,
        };

        let mut error = ChannelOperationError::new(self.code(), self.message());
        error.details = details;
        error
    }
}

#[derive(Clone)]
struct EndpointGuess {
    detected_type: String,
    corrected_base_url: String,
}

#[derive(Deserialize)]
pub struct CreateChannelParams {
    pub name: String,
    pub api_type: String,
    pub base_url: String,
    pub api_key: String,
    pub notes: Option<String>,
}

#[derive(Deserialize)]
pub struct UpdateChannelParams {
    pub id: String,
    pub name: Option<String>,
    pub api_type: Option<String>,
    pub base_url: Option<String>,
    pub api_key: Option<String>,
    pub enabled: Option<bool>,
    pub notes: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct UpdateResponseMsParams {
    #[serde(rename = "channelId")]
    pub channel_id: String,
    #[serde(rename = "responseMs")]
    pub response_ms: String,
}

#[derive(Serialize)]
pub struct ChannelOperationError {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
}

impl ChannelOperationError {
    fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            details: None,
        }
    }

    fn with_details(mut self, details: serde_json::Value) -> Self {
        self.details = Some(details);
        self
    }

    fn endpoint_validation_failed(message: impl Into<String>) -> Self {
        Self {
            code: ERROR_CODE_ENDPOINT_VALIDATION_FAILED.into(),
            message: message.into(),
            details: None,
        }
    }
}

#[derive(Serialize)]
pub struct ProbeResult {
    pub reachable: bool,
    pub status_code: Option<u16>,
    pub latency_ms: u64,
    pub detected_type: Option<String>,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ChannelOperationError>,
}

#[derive(Serialize)]
pub struct FetchModelsResult {
    pub detected_type: String,
    pub corrected_base_url: String,
    pub models: Vec<ModelInfo>,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ChannelOperationError>,
    #[serde(default)]
    pub endpoint_corrected: bool,
    #[serde(default)]
    pub auto_saved: bool,
}

// Service functions – thin wrappers around existing logic

pub fn update_channel_response_ms(db: &Database, params: UpdateResponseMsParams) -> Result<(), AppError> {
    db.update_channel_response_ms(&params.channel_id, &params.response_ms)
}

pub fn list_channels(db: &Database) -> Result<Vec<Channel>, AppError> {
    db.list_channels()
}

pub fn create_channel(db: &Database, params: CreateChannelParams) -> Result<Channel, AppError> {
    db.create_channel(
        &params.name,
        &params.api_type,
        &params.base_url,
        &params.api_key,
        params.notes.as_deref(),
    )
}

pub fn update_channel(db: &Database, app: Option<&tauri::AppHandle>, params: UpdateChannelParams) -> Result<Channel, AppError> {
    if let Some(false) = params.enabled {
        db.disable_entries_for_channel(&params.id)?;
    }
    db.update_channel(
        &params.id,
        params.name.as_deref(),
        params.api_type.as_deref(),
        params.base_url.as_deref(),
        params.api_key.as_deref(),
        params.enabled,
        params.notes.as_deref(),
    )?;
    if let Some(app) = app {
        crate::refresh_tray_if_enabled(app);
    }
    db.get_channel(&params.id)
}

pub fn delete_channel(db: &Database, app: Option<&tauri::AppHandle>, id: String) -> Result<(), AppError> {
    db.delete_channel(&id)?;
    if let Some(app) = app {
        crate::refresh_tray_if_enabled(app);
    }
    Ok(())
}

pub async fn probe_url(url: String) -> Result<ProbeResult, AppError> {
    // identical implementation from original command
    let url = url.trim_end_matches('/').trim();
    if url.is_empty() {
        return Ok(ProbeResult { reachable: false, status_code: None, latency_ms: 0,
            detected_type: None, message: "Empty URL".into(),
            error: Some(ChannelOperationError::new(ERROR_CODE_INVALID_URL, "Empty URL")) });
    }
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .danger_accept_invalid_certs(true)
        .build()
        .map_err(|e| AppError::Network(format!("HTTP client: {e}")))?;

    let start = std::time::Instant::now();
    match client.head(url).send().await {
        Ok(r) => {
            let s = r.status().as_u16();
            let ms = start.elapsed().as_millis() as u64;
            Ok(ProbeResult { reachable: s < 500, status_code: Some(s), latency_ms: ms,
                detected_type: None, message: format!("{s} ({ms}ms)"), error: None })
        }
        Err(_) => {
            let _start2 = std::time::Instant::now();
            match client.get(url).send().await {
                Ok(r) => {
                    let s = r.status().as_u16();
                    let ms = start.elapsed().as_millis() as u64;
                    Ok(ProbeResult { reachable: s < 500, status_code: Some(s), latency_ms: ms,
                        detected_type: None, message: format!("{s} ({ms}ms)"), error: None })
                }
                Err(e) => {
                    let ms = start.elapsed().as_millis() as u64;
                    let error = if e.is_timeout() {
                        ChannelOperationError::new(ERROR_CODE_TIMEOUT, e.to_string())
                    } else {
                        ChannelOperationError::new(ERROR_CODE_ENDPOINT_UNREACHABLE, e.to_string())
                    }
                    .with_details(serde_json::json!({ "url": url }));
                    Ok(ProbeResult { reachable: false, status_code: None, latency_ms: ms,
                        detected_type: None, message: e.to_string(), error: Some(error) })
                }
            }
        }
    }
}

pub async fn fetch_models_direct(
    api_type: String,
    base_url: String,
    api_key: String,
    verified: Option<bool>,
) -> Result<FetchModelsResult, AppError> {
    let base_url = normalize_base_url(&base_url);
    if base_url.is_empty() {
        return Ok(FetchModelsResult {
            detected_type: api_type,
            corrected_base_url: base_url,
            models: Vec::new(),
            message: "Empty URL".into(),
            warning: None,
            error: Some(ChannelOperationError::new(ERROR_CODE_INVALID_URL, "Empty URL")),
            endpoint_corrected: false,
            auto_saved: false,
        });
    }
    smart_fetch_models(&api_type, &base_url, &api_key, verified.unwrap_or(false))
        .await
        .map_err(|e| AppError::Network(e.message))
}

pub async fn fetch_models(db: &Database, channel_id: String) -> Result<FetchModelsResult, AppError> {
    let channel = db.get_channel(&channel_id)?;
    let original_base_url = normalize_base_url(&channel.base_url);
    let endpoint_guess = detect_endpoint_guess(&channel.api_type, &channel.base_url, &channel.api_key).await;
    let Some(guess) = endpoint_guess else {
        return Ok(FetchModelsResult {
            detected_type: channel.api_type,
            corrected_base_url: original_base_url,
            models: Vec::new(),
            message: "Could not validate endpoint. Check network, URL, API type, and API key.".into(),
            warning: None,
            error: Some(ChannelOperationError::new(
                ERROR_CODE_ENDPOINT_CORRECTION_FAILED,
                "Could not validate endpoint. Check network, URL, API type, and API key.",
            )),
            endpoint_corrected: false,
            auto_saved: false,
        });
    };

    let endpoint_corrected = channel.api_type != guess.detected_type || original_base_url != guess.corrected_base_url;

    let result = match fetch_models_result_with_fallback(&guess.detected_type, &guess.corrected_base_url, &channel.api_key).await {
        Ok((models, _actual_type, _actual_base_url)) => {
            let count = models.len();
            let message = if endpoint_corrected {
                format!("Detected: {} ({count} models). Endpoint correction is ready to save.", guess.detected_type)
            } else {
                format!("Detected: {} ({count} models)", guess.detected_type)
            };
            FetchModelsResult {
                detected_type: guess.detected_type.clone(),
                corrected_base_url: guess.corrected_base_url.clone(),
                models,
                message,
                warning: endpoint_corrected.then(|| {
                    format!(
                        "Endpoint correction suggested: {} {} → {} {}. Review and save manually.",
                        channel.api_type,
                        original_base_url,
                        guess.detected_type,
                        guess.corrected_base_url
                    )
                }),
                error: None,
                endpoint_corrected,
                auto_saved: false,
            }
        }
        Err(error) => {
            let warning = endpoint_corrected.then(|| {
                format!(
                    "Endpoint correction suggested: {} {} → {} {}. Models were not saved automatically because fetch failed.",
                    channel.api_type,
                    original_base_url,
                    guess.detected_type,
                    guess.corrected_base_url
                )
            });
            return Ok(FetchModelsResult {
                detected_type: guess.detected_type.clone(),
                corrected_base_url: guess.corrected_base_url.clone(),
                models: Vec::new(),
                message: error.message.clone(),
                warning,
                error: Some(error),
                endpoint_corrected,
                auto_saved: false,
            });
        }
    };

    db.update_channel_models(&channel_id, &result.models, &[])?;
    Ok(result)
}

// The remaining helper functions are required for model fetch flows.

async fn smart_fetch_models(
    api_type: &str,
    base_url: &str,
    api_key: &str,
    verified: bool,
) -> Result<FetchModelsResult, ChannelOperationError> {
    let base_url = normalize_base_url(base_url);

    let endpoint_guess = if verified {
        Some(EndpointGuess {
            detected_type: api_type.to_string(),
            corrected_base_url: base_url.clone(),
        })
    } else {
        detect_endpoint_guess(api_type, &base_url, api_key).await
    };

    if !verified && endpoint_guess.is_none() {
        return Err(ChannelOperationError::endpoint_validation_failed(
            "Could not validate endpoint. HTTP 200 model list is required.",
        ));
    }

    let fetch_seed_type = endpoint_guess
        .as_ref()
        .map(|g| g.detected_type.as_str())
        .unwrap_or(api_type);
    let fetch_seed_base_url = endpoint_guess
        .as_ref()
        .map(|g| g.corrected_base_url.as_str())
        .unwrap_or(base_url.as_str());

    let (models, actual_type, actual_base_url) = fetch_models_result_with_fallback(fetch_seed_type, fetch_seed_base_url, api_key).await?;

    let corrected_type = endpoint_guess
        .as_ref()
        .map(|g| g.detected_type.clone())
        .unwrap_or_else(|| resolve_detected_type(actual_type, &actual_base_url));
    let corrected_base_url = endpoint_guess
        .as_ref()
        .map(|g| g.corrected_base_url.clone())
        .unwrap_or_else(|| actual_base_url.clone());
    let count = models.len();

    Ok(FetchModelsResult {
        message: format!("Detected: {corrected_type} ({count} models)"),
        detected_type: corrected_type,
        corrected_base_url,
        models,
        warning: None,
        error: None,
        endpoint_corrected: endpoint_guess
            .as_ref()
            .map(|g| g.detected_type != api_type || g.corrected_base_url != base_url)
            .unwrap_or(false),
        auto_saved: false,
    })
}

async fn detect_endpoint_guess(
    api_type: &str,
    base_url: &str,
    api_key: &str,
) -> Option<EndpointGuess> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .danger_accept_invalid_certs(true)
        .build()
        .ok()?;

    let original_url = normalize_base_url(&base_url);
    let base_site = extract_base_site(&original_url).unwrap_or_else(|| original_url.clone());

    let phase1_base_url = if api_type == "custom" { &original_url } else { &base_site };
    match detect_type_with_base_url(&client, api_type, phase1_base_url, api_key, true).await {
        DetectionResult::Found(guess) => return Some(guess),
        DetectionResult::Blocked(err) => {
            log::warn!("[detect_endpoint] Stop after selected type failed with blocking error: {}", err.message());
            return None;
        }
        DetectionResult::NotFound => {}
    }

    for current_type in ["custom", "openai", "claude", "gemini", "azure"] {
        let candidate_base_url = if current_type == "custom" { &original_url } else { &base_site };
        match detect_type_with_base_url(&client, current_type, candidate_base_url, api_key, false).await {
            DetectionResult::Found(guess) => return Some(guess),
            DetectionResult::Blocked(err) => {
                log::warn!("[detect_endpoint] Stop correction flow after blocking error: {}", err.message());
                return None;
            }
            DetectionResult::NotFound => {}
        }
    }

    None
}

enum DetectionResult {
    Found(EndpointGuess),
    NotFound,
    Blocked(ModelsEndpointError),
}

async fn detect_type_with_base_url(
    client: &reqwest::Client,
    api_type: &str,
    base_url: &str,
    api_key: &str,
    respect_selected_type: bool,
) -> DetectionResult {
    let adapter = get_adapter(api_type);
    let urls = build_models_url_variants(adapter.as_ref(), base_url, api_key);
    for url in &urls {
        match try_models_endpoint(client, adapter.as_ref(), url, api_key).await {
            Ok(models) => {
                if !models.is_empty() {
                    if !is_authoritative_detection_success(api_type, url) {
                        continue;
                    }
                    let corrected_base_url = canonical_base_url_for_success(api_type, base_url, url);
                    let detected_type = if respect_selected_type {
                        api_type.to_string()
                    } else {
                        resolve_detected_type(api_type, &corrected_base_url)
                    };
                    log::info!("[detect_endpoint] OK via {url}, type={detected_type}, base_url={corrected_base_url}");
                    return DetectionResult::Found(EndpointGuess {
                        detected_type,
                        corrected_base_url,
                    });
                }
            }
            Err(err) if err.is_blocking() => return DetectionResult::Blocked(err),
            Err(_) => {}
        }
    }

    DetectionResult::NotFound
}

async fn fetch_models_result_with_fallback(
    preferred_type: &str,
    preferred_base_url: &str,
    api_key: &str,
) -> Result<(Vec<ModelInfo>, &'static str, String), ChannelOperationError> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .danger_accept_invalid_certs(true)
        .build()
        .map_err(|e| {
            ChannelOperationError::new(ERROR_CODE_HTTP_CLIENT_ERROR, format!("HTTP client: {e}"))
        })?;

    let candidates = build_base_url_candidates(preferred_base_url);
    let try_types = build_try_types(preferred_type);
    let mut last_error: Option<ChannelOperationError> = None;

    for candidate_base_url in candidates {
        for current_type in &try_types {
            let adapter = get_adapter(current_type);
            let urls = build_models_url_variants(adapter.as_ref(), &candidate_base_url, api_key);
            for url in &urls {
                match try_models_endpoint(&client, adapter.as_ref(), url, api_key).await {
                    Ok(models) if !models.is_empty() => {
                        let corrected_base_url = canonical_base_url_for_success(current_type, &candidate_base_url, url);
                        let models = dedup_models(models);
                        log::info!("[fetch_models] OK via {url}, type={current_type}, base_url={} ({} models)", corrected_base_url, models.len());
                        return Ok((models, current_type, corrected_base_url));
                    }
                    Ok(_) => {}
                    Err(err) if err.is_blocking() => {
                        let details = serde_json::json!({
                            "api_type": current_type,
                            "base_url": candidate_base_url,
                            "models_url": url,
                        });
                        return Err(err.to_operation_error().with_details(details));
                    }
                    Err(err) => {
                        last_error = Some(err.to_operation_error().with_details(serde_json::json!({
                            "api_type": current_type,
                            "base_url": candidate_base_url,
                            "models_url": url,
                        })));
                    }
                }
            }
        }
    }

    Err(last_error.unwrap_or_else(|| {
        ChannelOperationError::new(
            ERROR_CODE_FETCH_MODELS_FAILED,
            "Could not fetch models. Check URL and API Key.",
        )
    }))
}

fn build_try_types(preferred_type: &str) -> Vec<&'static str> {
    let mut seen = std::collections::HashSet::new();
    let normalized: &'static str = match preferred_type {
        "openai" => "openai",
        "gemini" => "gemini",
        "claude" => "claude",
        "azure" => "azure",
        "custom" => "custom",
        _ => "custom",
    };
    let mut v = Vec::new();
    for t in [normalized, "custom", "openai", "claude", "gemini", "azure"] {
        if seen.insert(t) {
            v.push(t);
        }
    }
    v
}

fn is_authoritative_detection_success(api_type: &str, success_url: &str) -> bool {
    match api_type {
        "gemini" => success_url.contains("/v1beta/openai/"),
        "azure" => success_url.contains("/openai/deployments"),
        _ => true,
    }
}

fn normalize_base_url(base_url: &str) -> String {
    let trimmed = base_url.trim().trim_end_matches('/');
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        trimmed.to_string()
    } else if trimmed.is_empty() {
        String::new()
    } else {
        format!("https://{trimmed}")
    }
}

fn build_base_url_candidates(base_url: &str) -> Vec<String> {
    let normalized = normalize_base_url(base_url);
    let mut candidates = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for candidate in [normalized.clone(), trim_known_api_suffix(&normalized)] {
        if !candidate.is_empty() && seen.insert(candidate.clone()) {
            candidates.push(candidate);
        }
    }

    if let Some(scheme_end) = normalized.find("://") {
        let after_scheme = &normalized[scheme_end + 3..];
        if let Some(slash) = after_scheme.find('/') {
            let base_site = format!("{}://{}", &normalized[..scheme_end], &after_scheme[..slash]);
            if seen.insert(base_site.clone()) {
                candidates.push(base_site);
            }
        }
    }

    candidates
}

fn extract_base_site(base_url: &str) -> Option<String> {
    let normalized = normalize_base_url(base_url);
    let scheme_end = normalized.find("://")?;
    let after_scheme = &normalized[scheme_end + 3..];
    if let Some(slash) = after_scheme.find('/') {
        Some(format!("{}://{}", &normalized[..scheme_end], &after_scheme[..slash]))
    } else {
        Some(normalized)
    }
}

pub(crate) fn canonical_base_url_for_success(api_type: &str, fallback_base_url: &str, success_url: &str) -> String {
    let success = success_url.trim();
    let success_lower = success.to_ascii_lowercase();

    if api_type == "gemini" {
        if let Some(idx) = success_lower.find("/v1beta/openai/") {
            let base = &success[..idx];
            return base.trim_end_matches('/').to_string();
        }
    }

    if api_type == "claude" {
        if let Some(idx) = success_lower.find("/v1/") {
            let base = &success[..idx];
            return base.trim_end_matches('/').to_string();
        }
    }

    if api_type == "azure" {
        if let Some(idx) = success_lower.find("/openai/deployments") {
            let base = &success[..idx];
            return base.trim_end_matches('/').to_string();
        }
    }

    if api_type == "openai" {
        if let Some(idx) = success_lower.find("/v1/") {
            let base = &success[..idx];
            return base.trim_end_matches('/').to_string();
        }
    }

    if api_type == "custom" {
        for suffix in ["/models", "/chat/completions"] {
            if success_lower.ends_with(suffix) {
                let stripped = &success[..success.len() - suffix.len()];
                return stripped.trim_end_matches('/').to_string();
            }
        }
    }

    normalize_base_url(fallback_base_url)
}

fn trim_known_api_suffix(base_url: &str) -> String {
    let base = base_url.trim_end_matches('/');
    let lower = base.to_ascii_lowercase();
    let suffixes = [
        "/v1/chat/completions",
        "/chat/completions",
        "/v1/messages",
        "/v1/models",
        "/models",
        "/v1beta/openai/chat/completions",
        "/v1beta/openai/models",
        "/openai/deployments",
    ];
    for suffix in suffixes {
        if lower.ends_with(suffix) {
            let stripped = &base[..base.len() - suffix.len()];
            return stripped.trim_end_matches('/').to_string();
        }
    }
    base.to_string()
}

fn resolve_detected_type(detected: &str, base_url: &str) -> String {
    let _ = base_url;
    detected.into()
}

fn build_models_url_variants(
    adapter: &(dyn crate::proxy::protocol::ProtocolAdapter + Send + Sync),
    base_url: &str,
    api_key: &str,
) -> Vec<String> {
    let mut urls = vec![adapter.build_models_url(base_url, api_key)];
    let base = base_url.trim_end_matches('/');
    for v in &["/models", "/v1/models", "/api/models", "/api/v1/models", "/v2/models"] {
        let u = format!("{base}{v}");
        if !urls.contains(&u) {
            urls.push(u);
        }
    }
    urls
}

async fn try_models_endpoint(
    client: &reqwest::Client,
    adapter: &(dyn crate::proxy::protocol::ProtocolAdapter + Send + Sync),
    url: &str,
    api_key: &str,
) -> Result<Vec<ModelInfo>, ModelsEndpointError> {
    let resp = adapter
        .apply_auth(client.get(url), api_key)
        .send()
        .await
        .map_err(|e| {
            if e.is_timeout() {
                ModelsEndpointError::Timeout(e.to_string())
            } else if e.is_connect() || e.is_request() {
                ModelsEndpointError::Network(e.to_string())
            } else {
                ModelsEndpointError::Network(e.to_string())
            }
        })?;
    let status = resp.status();
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        return Err(ModelsEndpointError::Auth(status.as_u16()));
    }
    if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
        return Err(ModelsEndpointError::RateLimited(status.as_u16()));
    }
    if status != reqwest::StatusCode::OK {
        return Err(ModelsEndpointError::Http(status.as_u16()));
    }
    let body: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| ModelsEndpointError::Parse(e.to_string()))?;
    let models: Vec<ModelInfo> = adapter
        .parse_models_response(&body)
        .into_iter()
        .map(|(id, owned_by)| ModelInfo { name: id.clone(), id, owned_by })
        .collect();
    if models.is_empty() {
        Err(ModelsEndpointError::Empty)
    } else {
        Ok(models)
    }
}

fn extract_models_from_json(body: &str) -> Option<Vec<ModelInfo>> {
    let json: serde_json::Value = serde_json::from_str(body).ok()?;
    let arr = json.get("data")?.as_array()?;
    let models: Vec<ModelInfo> = arr
        .iter()
        .filter_map(|m| {
            let id = m.get("id")?.as_str()?.to_string();
            if id.eq_ignore_ascii_case("auto") {
                return None;
            }
            let owned = m.get("owned_by").and_then(|v| v.as_str()).unwrap_or("unknown").to_string();
            Some(ModelInfo { name: id.clone(), id, owned_by: Some(owned) })
        })
        .collect();
    if models.is_empty() { None } else { Some(models) }
}

async fn try_chat_probe(
    client: &reqwest::Client,
    adapter: &(dyn crate::proxy::protocol::ProtocolAdapter + Send + Sync),
    base_url: &str,
    api_key: &str,
    api_type: &str,
) -> Option<ProbeSuccess> {
    let test_model = match api_type {
        "claude" => "claude-3-5-sonnet-20241022",
        "gemini" => "gemini-2.0-flash",
        _ => "gpt-4o-mini",
    };
    let chat_url = adapter.build_chat_url(base_url, test_model);
    let body = serde_json::json!({"model": test_model, "messages": [{"role":"user","content":"hi"}], "max_tokens": 1});
    let req = adapter.apply_auth(client.post(&chat_url).header("Content-Type", "application/json"), api_key);
    match req.json(&body).send().await {
        Ok(resp) => {
            let s = resp.status().as_u16();
            if s < 500 {
                let corrected_base_url = canonical_base_url_for_success(api_type, base_url, &chat_url);
                if let Ok(text) = resp.text().await {
                    if let Some(m) = extract_models_from_json(&text) {
                        return Some(ProbeSuccess { models: m, corrected_base_url });
                    }
                }
                return Some(ProbeSuccess { models: known_models_for_type(api_type), corrected_base_url });
            }
            None
        }
        Err(_) => None,
    }
}

fn known_models_for_type(api_type: &str) -> Vec<ModelInfo> {
    let list: &[(&str, &str)] = match api_type {
        "openai" => &[("gpt-4o","openai"),("gpt-4o-mini","openai"),("gpt-4-turbo","openai"),("gpt-3.5-turbo","openai"),("o1","openai"),("o1-mini","openai"),("o1-preview","openai"),("o3-mini","openai"),("o4-mini","openai")],
        "claude" => &[("claude-sonnet-4-20250514","anthropic"),("claude-3-5-sonnet-20241022","anthropic"),("claude-3-5-haiku-20241022","anthropic"),("claude-3-opus-20240229","anthropic")],
        "gemini" => &[("gemini-2.5-pro-preview-05-06","google"),("gemini-2.0-flash","google"),("gemini-1.5-pro","google"),("gemini-1.5-flash","google")],
        "azure" => &[("gpt-4o","azure"),("gpt-4o-mini","azure"),("gpt-4-turbo","azure")],
        _ => &[("gpt-4o","openai"),("gpt-4o-mini","openai"),("gpt-3.5-turbo","openai"),("claude-3-5-sonnet-20241022","anthropic"),("claude-3-5-haiku-20241022","anthropic"),("gemini-2.0-flash","google"),("deepseek-chat","deepseek"),("deepseek-reasoner","deepseek"),("qwen-turbo","alibaba"),("glm-4-flash","zhipu")],
    };
    list.iter().map(|&(name, owner)| ModelInfo { name: name.into(), id: name.into(), owned_by: Some(owner.into()) }).collect()
}

fn dedup_models(models: Vec<ModelInfo>) -> Vec<ModelInfo> {
    let mut seen = std::collections::HashSet::new();
    models
        .into_iter()
        .filter(|m| !m.id.eq_ignore_ascii_case("auto") && !m.name.eq_ignore_ascii_case("auto"))
        .filter(|m| seen.insert(m.name.clone()))
        .collect()
}

