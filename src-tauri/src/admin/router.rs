use crate::admin::auth::require_auth;
use crate::admin::channel_handlers;
use crate::admin::cors::apply_admin_cors;
use crate::admin::handlers;
use crate::admin::state::AdminState;
use axum::middleware;
use axum::routing::{get, post, put};
use axum::Router;

pub fn build_admin_router(state: AdminState) -> Router {
    let protected = Router::new()
        .route("/admin/logout", post(handlers::logout))
        .route("/admin/status", get(handlers::status))
        .route("/admin/audit-logs", get(handlers::audit_logs))
        .route(
            "/admin/settings",
            get(handlers::get_settings).put(handlers::update_settings),
        )
        // Channel API routes – all require auth
        .route(
            "/admin/channels",
            get(channel_handlers::list).post(channel_handlers::create),
        )
        .route(
            "/admin/channels/:id",
            get(channel_handlers::list)
                .put(channel_handlers::update)
                .delete(channel_handlers::delete),
        )
        .route(
            "/admin/channels/:id/fetch-models",
            post(channel_handlers::fetch_models),
        )
        .route(
            "/admin/channels/fetch-models-direct",
            post(channel_handlers::fetch_models_direct),
        )
        .route(
            "/admin/channels/probe-url",
            post(channel_handlers::probe_url),
        )
        .route(
            "/admin/channels/:id/select-models",
            post(channel_handlers::select_models),
        )
        .route(
            "/admin/channels/:id/response-ms",
            put(channel_handlers::update_response_ms),
        )
        .route_layer(middleware::from_fn_with_state(state.clone(), require_auth));

    Router::new()
        .route("/admin", get(crate::admin::static_files::admin_index))
        .route("/admin/", get(crate::admin::static_files::admin_index))
        .route(
            "/admin/assets/*path",
            get(crate::admin::static_files::admin_asset),
        )
        .route(
            "/admin/star.jpg",
            get(crate::admin::static_files::admin_asset),
        )
        .route(
            "/admin/favicon.ico",
            get(crate::admin::static_files::admin_asset),
        )
        .route(
            "/admin/logo/*path",
            get(crate::admin::static_files::admin_asset),
        )
        .route("/admin/login", post(handlers::login))
        .route("/admin/health", get(handlers::health))
        .route("/admin/login", axum::routing::options(|| async {}))
        .route("/admin/health", axum::routing::options(|| async {}))
        .route("/admin/logout", axum::routing::options(|| async {}))
        .route("/admin/status", axum::routing::options(|| async {}))
        .route("/admin/audit-logs", axum::routing::options(|| async {}))
        .route("/admin/settings", axum::routing::options(|| async {}))
        .merge(protected)
        .with_state(state)
        .route_layer(middleware::from_fn(apply_admin_cors))
}
