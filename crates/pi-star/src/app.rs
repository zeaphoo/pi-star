use std::sync::Arc;

use axum::Router;
use axum::extract::State;
use axum::http::StatusCode;
use axum::middleware;
use axum::response::IntoResponse;
use axum::routing::{get, post, put};
use tower_http::compression::CompressionLayer;
use tower_http::limit::RequestBodyLimitLayer;
use tower_http::trace::TraceLayer;

use crate::auth::security_middleware;
use crate::routes;
use crate::state::AppState;

pub fn router(state: Arc<AppState>) -> Router {
    let api = Router::new()
        .route("/health", get(health))
        .route("/sessions", get(routes::sessions::list))
        .route(
            "/sessions/{id}",
            get(routes::sessions::get_session)
                .patch(routes::sessions::rename)
                .delete(routes::sessions::delete),
        )
        .route("/sessions/delete", post(routes::sessions::delete_many))
        .route("/sessions/stats", post(routes::sessions::stats))
        .route("/sessions/search", get(routes::sessions::search))
        .route(
            "/sessions/ui-state",
            get(routes::sessions::ui_state_get).post(routes::sessions::ui_state_post),
        )
        .route("/sessions/fork", post(routes::sessions::fork_body))
        .route("/sessions/{id}/fork", post(routes::sessions::fork_session))
        .route("/sessions/{id}/state", get(routes::sessions::state))
        .route("/sessions/{id}/context", get(routes::sessions::context))
        .route("/sessions/{id}/export", get(routes::sessions::export))
        .route(
            "/sessions/{id}/auto-name",
            post(routes::sessions::auto_name),
        )
        .route(
            "/sessions/{id}/entries/{entry_id}/tool-result-image",
            get(routes::sessions::tool_result_image),
        )
        .route(
            "/sessions/{id}/entries/{entry_id}/thinking",
            get(routes::sessions::thinking),
        )
        .route("/agent/new", post(routes::agent::new_session))
        .route("/agent/backends", get(agent_backends))
        .route("/agent/running", get(routes::agent::running))
        .route("/agent/debug", get(routes::agent::debug))
        .route(
            "/agent/{id}",
            get(routes::agent::get_state).post(routes::agent::command),
        )
        .route("/agent/{id}/events", get(routes::agent::events))
        .route("/agent/{id}/lease", post(routes::agent::lease))
        .route("/agent/{id}/bash-output", get(routes::agent::bash_output))
        .route(
            "/files/{*path}",
            get(routes::files::get_file).post(routes::files::post_file),
        )
        .route("/file-index", get(routes::files::file_index))
        .route("/git/status", get(routes::git::status))
        .route("/git/diff", get(routes::git::diff))
        .route(
            "/worktrees",
            get(routes::git::worktrees)
                .post(routes::git::add_worktree)
                .delete(routes::git::remove_worktree),
        )
        .route("/terminal", post(routes::terminal::create))
        .route(
            "/terminal/{id}",
            get(routes::terminal::get_terminal)
                .post(routes::terminal::command)
                .delete(routes::terminal::kill),
        )
        .route("/terminal/{id}/events", get(routes::terminal::events))
        .route("/mcp/oauth/callback", get(routes::mcp_oauth::callback))
        .route("/models", get(routes::settings::models))
        .route("/models/default", put(routes::settings::set_default_model))
        .route(
            "/models/enabled",
            get(routes::settings::enabled_models).put(routes::settings::set_enabled_models),
        )
        .route("/models/refresh", post(routes::settings::refresh_models))
        .route(
            "/models-config",
            get(routes::settings::models_config).put(routes::settings::set_models_config),
        )
        .route("/models-config/catalog", get(routes::settings::catalog))
        .route("/models-config/discover", post(routes::settings::discover))
        .route("/models-config/test", post(routes::settings::test_model))
        .route("/auth/providers", get(routes::settings::providers))
        .route(
            "/auth/login/{provider}",
            get(routes::auth::login).post(routes::auth::login_post),
        )
        .route("/auth/logout/{provider}", post(routes::settings::logout))
        .route(
            "/auth/api-key/{provider}",
            post(routes::settings::set_api_key).delete(routes::settings::delete_api_key),
        )
        .route(
            "/tools/settings",
            get(routes::settings::tools).put(routes::settings::set_tools),
        )
        .route(
            "/cache-miss-notices",
            get(routes::settings::cache_miss).put(routes::settings::set_cache_miss),
        )
        .route(
            "/subagents/settings",
            get(routes::settings::subagent_settings).put(routes::settings::set_subagent_settings),
        )
        .route(
            "/subagents/profiles",
            get(routes::settings::subagent_profiles)
                .post(routes::settings::save_subagent_profile)
                .patch(routes::settings::toggle_subagent_profile)
                .delete(routes::settings::delete_subagent_profile),
        )
        .route(
            "/subagents/{id}",
            get(routes::settings::subagent_view).post(routes::settings::subagent_action),
        )
        .route(
            "/project-trust",
            get(routes::settings::project_trust).post(routes::settings::set_project_trust),
        )
        .route(
            "/mcp",
            get(routes::settings::mcp).post(routes::settings::mcp_post),
        )
        .route("/mcp/test", post(routes::settings::mcp_test))
        .route("/mcp/sign-in", post(routes::mcp_oauth::start))
        .route(
            "/mcp/sign-in/{flow_id}",
            get(routes::mcp_oauth::status)
                .post(routes::mcp_oauth::paste)
                .delete(routes::mcp_oauth::cancel),
        )
        .route(
            "/skills",
            get(routes::settings::skills).patch(routes::settings::patch_skills),
        )
        .route("/skills/content", get(routes::settings::skill_content))
        .route("/skills/check", post(routes::settings::skill_check))
        .route("/skills/install", post(routes::skills_install::install))
        .route("/skills/search", post(routes::settings::skill_search))
        .route("/skills/update", post(routes::skills_install::update))
        .route(
            "/plugins",
            get(routes::settings::plugins).post(routes::settings::plugins_post),
        )
        .route("/plugins/check", post(routes::settings::plugins_check))
        .route(
            "/provider-usage/query",
            post(routes::settings::provider_usage),
        )
        .route("/push/config", get(routes::misc::push_config))
        .route("/push/subscribe", post(routes::misc::push_subscribe))
        .route("/app-update", get(routes::misc::app_update))
        .route(
            "/web-auth",
            get(routes::misc::web_auth_status)
                .post(routes::misc::web_auth_login)
                .delete(routes::misc::web_auth_logout),
        )
        .route("/home", get(routes::misc::home))
        .route("/default-cwd", post(routes::misc::default_cwd))
        .route("/cwd/validate", post(routes::misc::validate_cwd))
        .route(
            "/cwd/browse",
            get(routes::misc::browse_cwd).post(routes::misc::create_cwd),
        )
        .route(
            "/open-in-explorer",
            get(routes::misc::open_in_explorer_status).post(routes::misc::open_in_explorer),
        )
        .fallback(api_not_found);

    Router::new()
        .route("/manifest.webmanifest", get(routes::misc::manifest))
        .nest("/api", api)
        .fallback(crate::web_assets::serve)
        .layer(middleware::from_fn(cache_headers))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            security_middleware,
        ))
        .layer(RequestBodyLimitLayer::new(128 * 1024 * 1024))
        .layer(CompressionLayer::new())
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

async fn cache_headers(
    request: axum::extract::Request,
    next: middleware::Next,
) -> axum::response::Response {
    let path = request.uri().path().to_string();
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    if path.starts_with("/assets/") {
        headers.insert(
            axum::http::header::CACHE_CONTROL,
            axum::http::HeaderValue::from_static("public, max-age=31536000, immutable"),
        );
    } else if path == "/sw.js" {
        headers.insert(
            axum::http::header::CACHE_CONTROL,
            axum::http::HeaderValue::from_static("public, max-age=0, must-revalidate"),
        );
        headers.insert(
            "Service-Worker-Allowed",
            axum::http::HeaderValue::from_static("/"),
        );
    } else if path == "/manifest.webmanifest" {
        headers.insert(
            axum::http::header::CACHE_CONTROL,
            axum::http::HeaderValue::from_static("public, max-age=0, must-revalidate"),
        );
    } else if !path.starts_with("/api/") {
        headers.insert(
            axum::http::header::CACHE_CONTROL,
            axum::http::HeaderValue::from_static("private, no-cache, max-age=0, must-revalidate"),
        );
    }
    response
}

async fn health(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let assets = state.web_assets.describe();
    axum::Json(serde_json::json!({
        "ok": true,
        "name": "pi-star",
        "version": env!("CARGO_PKG_VERSION"),
        "uptimeMs": state.started.elapsed().as_millis() as u64,
        "sessions": state.agents.running_ids().len() + state.pi_rpc.running_ids().len(),
        "assets": {
            "source": assets.source,
            "path": assets.path,
            "files": assets.files,
        },
    }))
}

async fn agent_backends() -> impl IntoResponse {
    axum::Json(pi_star_core::agent::backend::options())
}

async fn api_not_found() -> impl IntoResponse {
    (
        StatusCode::NOT_FOUND,
        [(axum::http::header::CONTENT_TYPE, "application/json")],
        "{\"error\":\"Not found\"}",
    )
}
