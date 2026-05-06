use axum::http::{header, HeaderValue, StatusCode, Uri};
use axum::response::{Html, IntoResponse, Response};


use std::path::{Component, Path, PathBuf};



fn dist_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../dist-web-admin")
}

fn read_text(path: &Path) -> Option<String> {
    std::fs::read_to_string(path).ok()
}

fn read_bytes(path: &Path) -> Option<Vec<u8>> {
    std::fs::read(path).ok()
}





fn content_type_for(path: &str) -> &'static str {
    if path.ends_with(".css") {
        "text/css; charset=utf-8"
    } else if path.ends_with(".js") {
        "application/javascript; charset=utf-8"
    } else if path.ends_with(".json") {
        "application/json; charset=utf-8"
    } else if path.ends_with(".svg") {
        "image/svg+xml"
    } else if path.ends_with(".png") {
        "image/png"
    } else if path.ends_with(".jpg") || path.ends_with(".jpeg") {
        "image/jpeg"
    } else {
        "application/octet-stream"
    }
}

fn cache_control_for(path: &str) -> &'static str {
    if path.ends_with("index.html") {
        "no-cache"
    } else {
        "public, max-age=31536000, immutable"
    }
}

fn safe_dist_path(path: &str) -> Option<PathBuf> {
    if path.is_empty() || path == "assets" || path.contains('\\') {
        return None;
    }

    let relative = Path::new(path);
    if !relative.components().all(|component| matches!(component, Component::Normal(_))) {
        return None;
    }

    Some(dist_dir().join(relative))
}

pub async fn admin_index() -> impl IntoResponse {
    let dist = dist_dir();
    let index_path = dist.join("index.html");
    let Some(mut html) = read_text(&index_path) else {
        return StatusCode::NOT_FOUND.into_response();
    };

// 动态资源已在构建产出 (dist) 的 index.html 中直接使用哈希文件名，
// 因此无需在运行时进行占位符替换。若仍然需要确保入口资源存在，可在此处加入检查。
// 当前实现保持原始 HTML 内容，避免硬编码文件名。
// 如需在未来添加自定义资源注入，可在这里使用 `entry_assets()` 的结果。
// 如果后台在单端口模式下重新定位了 Admin 服务，会通过环境变量 ADMIN_BASE_URL 提供新的基路径。
if let Ok(base_url) = std::env::var("ADMIN_BASE_URL") {
    html = html.replace("%ADMIN_BASE_URL%", &base_url);
}

    // 示例（已注释）：
    // if let Some((js, css_list)) = entry_assets() {
    //     // 可在此记录日志或执行其它业务逻辑
    // }
    //
    // 直接返回读取的 HTML。
    //
    // （保持原代码结构不变，仅去除硬编码替换）

    let mut response = Html(html).into_response();
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("no-cache"),
    );
    response
}

pub async fn admin_asset(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches("/admin/");
    if path.is_empty() || path == "assets" {
        return StatusCode::NOT_FOUND.into_response();
    }

    let Some(full_path) = safe_dist_path(path) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let Some(bytes) = read_bytes(&full_path) else {
        return StatusCode::NOT_FOUND.into_response();
    };

    let mut response = Response::new(bytes.into());
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static(content_type_for(path)),
    );
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static(cache_control_for(path)),
    );
    response
}

pub async fn admin_asset_root(uri: Uri) -> Response {
    let path = uri.path();
    
    // Strip root prefixes: /assets/, /logo/, etc.
    let stripped = if let Some(rest) = path.strip_prefix("/assets/") {
        rest
    } else if let Some(rest) = path.strip_prefix("/logo/") {
        rest
    } else if path == "/star.jpg" {
        "star.jpg"
    } else if path == "/favicon.ico" {
        "favicon.ico"
    } else {
        return StatusCode::NOT_FOUND.into_response();
    };

    if stripped.is_empty() || stripped == "assets" {
        return StatusCode::NOT_FOUND.into_response();
    }

    let Some(full_path) = safe_dist_path(stripped) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let Some(bytes) = read_bytes(&full_path) else {
        return StatusCode::NOT_FOUND.into_response();
    };

    let mut response = Response::new(bytes.into());
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static(content_type_for(stripped)),
    );
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static(cache_control_for(stripped)),
    );
    response
}
