// Handlers and their helpers return `Result<_, axum::response::Response>`, axum's idiom for an
// early HTTP error; boxing the Response would only add an allocation.
#![allow(clippy::result_large_err)]

mod auth;
mod config;
mod crypto;
mod common;
mod migrate;
mod notify;
mod error;
mod routes;
mod services;
mod seed;
mod downloads;
mod frameio;
mod session;
mod storage;
mod verify;
mod sync;
mod spa;
mod pyfmt;
mod request_log;
mod state;

use std::sync::Arc;

use axum::routing::{get, post};
use axum::Router;
use config::Settings;
use state::AppState;
use tower_http::services::{ServeDir, ServeFile};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let settings = Settings::from_env();
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| settings.log_filter().into()))
        .init();

    // `portal-api migrate` — create or upgrade the schema; run by the entrypoint before seeding.
    if std::env::args().nth(1).as_deref() == Some("migrate") {
        return migrate::migrate(&settings).await;
    }

    let db = sqlx::postgres::PgPoolOptions::new()
        .max_connections(10)
        .connect(&settings.database_url)
        .await?;

    // `portal-api seed` — one-shot admin seed, run once by the container entrypoint.
    if std::env::args().nth(1).as_deref() == Some("seed") {
        settings.require_secure_secrets()?; // fail before seeding an admin with the placeholder password
        seed::seed_admin(&db, &settings).await?;
        seed::seed_config(&db, &settings).await?;
        return Ok(());
    }
    let redis = redis::aio::ConnectionManager::new(
        redis::Client::open(settings.redis_url.clone())?,
    )
    .await?;

    // `portal-api worker` — the sync worker (replaces the arq process).
    if std::env::args().nth(1).as_deref() == Some("worker") {
        let settings = Arc::new(settings);
        let frameio = Arc::new(frameio::FrameioClient::new(db.clone(), settings.clone()));
        let state = AppState { settings, db, redis, frameio };
        return sync::worker::run(state).await;
    }

    settings.require_secure_secrets()?;
    let addr = format!("{}:{}", settings.host, settings.port);
    let settings = Arc::new(settings);
    let frameio = Arc::new(frameio::FrameioClient::new(db.clone(), settings.clone()));
    let state = AppState {
        settings,
        db,
        redis,
        frameio,
    };

    // The API and the static SPA share one origin, so the browser's relative `fetch("/api/…")`
    // calls carry the session cookie with no proxy in between.
    let api = Router::<AppState>::new()
        .route("/health", get(routes::health::health))
        .route("/openapi.yaml", get(routes::docs::openapi))
        .route("/auth/login", post(auth::login))
        .route("/auth/me", get(routes::account::me))
        .route("/auth/logout", post(routes::account::logout))
        .route("/auth/login/totp", post(auth::login_totp))
        .route("/auth/passkeys", get(auth::webauthn::list_passkeys))
        .route("/auth/passkeys/register/begin", post(auth::webauthn::register_begin))
        .route("/auth/passkeys/register/complete", post(auth::webauthn::register_complete))
        .route("/auth/passkeys/login/begin", post(auth::webauthn::login_begin))
        .route("/auth/passkeys/login/complete", post(auth::webauthn::login_complete))
        .route("/auth/passkeys/{passkey_id}", axum::routing::delete(auth::webauthn::delete_passkey))
        .route("/download-links", get(routes::download_links::list_links).post(routes::download_links::create_link))
        .route("/download-links/{link_id}", get(routes::download_links::get_link).patch(routes::download_links::update_link).delete(routes::download_links::delete_link))
        .route("/download-links/{link_id}/revoke", post(routes::download_links::revoke_link))
        .route("/download-links/{link_id}/restore", post(routes::download_links::restore_link))
        .route("/download-links/{link_id}/events", get(routes::download_links::list_events))
        .route("/download-links/{link_id}/stats", get(routes::download_links::stats))
        .route("/download-links/{link_id}/source", get(routes::download_links::source_info))
        .route("/public/links/{token}", get(routes::public::resolve_link))
        .route("/public/links/{token}/request-code", post(routes::public::request_code))
        .route("/public/links/{token}/verify-code", post(routes::public::verify_link_code))
        .route("/public/links/{token}/verify-password", post(routes::public::verify_link_password))
        .route("/public/links/{token}/sessions", post(routes::public::start_session))
        .route("/public/links/{token}/sessions/{session_id}/files", post(routes::public::request_file_upload))
        .route("/public/links/{token}/sessions/{session_id}/files/{file_id}/complete", post(routes::public::complete_file))
        .route("/public/links/{token}/sessions/{session_id}/complete", post(routes::public::complete_session))
        .route("/public/links/{token}/sessions/{session_id}/heartbeat", post(routes::public::session_heartbeat))
        .route("/public/links/{token}/sessions/{session_id}/abandon", post(routes::public::abandon_session))
        .route("/public/downloads/{token}", get(routes::public_downloads::resolve_link))
        .route("/public/downloads/{token}/request-code", post(routes::public_downloads::request_code))
        .route("/public/downloads/{token}/sessions", post(routes::public_downloads::start_session))
        .route("/public/downloads/{token}/sessions/{session_id}/files/{file_id}/url", post(routes::public_downloads::file_download_url))
        .route(
            "/dashboard/transfers",
            get(routes::dashboard::transfers),
        )
        .route("/upload-links/extension-preset", get(routes::upload_links::extension_preset))
        .route("/upload-links", get(routes::upload_links::list_links).post(routes::upload_links::create_link))
        .route("/upload-links/{link_id}", get(routes::upload_links::get_link).patch(routes::upload_links::update_link).delete(routes::upload_links::delete_link))
        .route("/upload-links/{link_id}/revoke", post(routes::upload_links::revoke_link))
        .route("/upload-links/{link_id}/restore", post(routes::upload_links::restore_link))
        .route("/upload-links/{link_id}/sessions", get(routes::upload_links::sessions))
        .route("/upload-links/{link_id}/stats", get(routes::upload_links::stats))
        .route("/branding/logo", get(routes::branding::get_logo))
        .route("/privacy/subject", get(routes::privacy::export_subject))
        .route("/privacy/subject/erase", post(routes::privacy::erase_subject))
        .route("/dashboard/history", get(routes::dashboard::history))
        .route("/links", get(routes::links::list_links))
        .route("/link-presets", get(routes::presets::list).post(routes::presets::create))
        .route("/link-presets/{preset_id}", axum::routing::patch(routes::presets::update).delete(routes::presets::delete))
        .route("/brand-profiles", get(routes::brands::list).post(routes::brands::create))
        .route("/brand-profiles/{profile_id}", axum::routing::patch(routes::brands::update).delete(routes::brands::delete))
        .route(
            "/brand-profiles/{profile_id}/logo",
            post(routes::brands::upload_logo)
                .layer(axum::extract::DefaultBodyLimit::max(state.settings.branding_logo_max_bytes.saturating_mul(2).saturating_add(1 << 20)))
                .delete(routes::brands::delete_logo),
        )
        .route("/nav", get(routes::links::nav_summary))
        .route("/links/email", post(routes::links::email_link))
        .route("/events", get(routes::events::stream_events))
        .route("/sync-rules", get(routes::sync_rules::list_rules).post(routes::sync_rules::create_rule))
        .route("/sync-rules/{rule_id}", get(routes::sync_rules::get_rule).patch(routes::sync_rules::update_rule).delete(routes::sync_rules::delete_rule))
        .route("/sync-rules/{rule_id}/run", post(routes::sync_rules::run_rule))
        .route("/sync-rules/{rule_id}/jobs", get(routes::sync_rules::list_jobs))
        .route("/sync-rules/{rule_id}/stats", get(routes::sync_rules::job_stats))
        .route("/sync-jobs/{job_id}/retry", post(routes::sync_jobs::retry_job))
        .route("/sync-jobs/{job_id}/dismiss", post(routes::sync_jobs::dismiss_job))
        .route("/webhooks/frameio/{rule_id}", post(routes::webhooks::ingest_frameio))
        .route("/webhook-events", get(routes::webhooks::list_events))
        .route("/activity", get(routes::activity::list_activity))
        .route("/activity/actions", get(routes::activity::list_actions))
        .route("/activity/summary", get(routes::activity::summary))
        .route("/settings/security", get(routes::account::security_status))
        .route("/settings/frameio-config", get(routes::settings::frameio_config_status).post(routes::settings::update_frameio_config))
        .route("/settings/email", get(routes::settings::email_status).post(routes::settings::update_email))
        .route("/settings/email/test", post(routes::settings::send_email_test))
        .route("/settings/password", post(routes::settings::change_password))
        .route("/settings/totp/begin", post(routes::security::totp_begin))
        .route("/settings/totp/enable", post(routes::security::totp_enable))
        .route("/settings/totp/disable", post(routes::security::totp_disable))
        .route("/settings/branding", get(routes::security::branding_status).post(routes::security::update_branding))
        .route(
            "/settings/branding/logo",
            // axum caps bodies at 2 MB by default; Starlette doesn't cap them at all. Allow the
            // configured logo size plus room for the multipart envelope, so an oversized logo
            // gets the handler's "exceeds the maximum size" rather than a bare 413.
            post(routes::security::upload_logo)
                .layer(axum::extract::DefaultBodyLimit::max(state.settings.branding_logo_max_bytes.saturating_mul(2).saturating_add(1 << 20)))
                .delete(routes::security::delete_logo),
        )
        .route("/frameio/status", get(routes::account::frameio_status))
        .route("/frameio/oauth/connect", get(routes::frameio::connect))
        .route("/frameio/oauth/callback", get(routes::frameio::callback))
        .route("/frameio/disconnect", post(routes::frameio::disconnect))
        .route("/frameio/accounts", get(routes::frameio::list_accounts))
        .route("/frameio/workspaces", get(routes::frameio::list_workspaces))
        .route("/frameio/projects", get(routes::frameio::list_projects).post(routes::frameio::create_project))
        .route("/frameio/projects/{project_id}", axum::routing::delete(routes::frameio::delete_project))
        .route("/frameio/folders", get(routes::frameio::list_folders).post(routes::frameio::create_folder))
        .route("/frameio/search", get(routes::frameio::search))
        .route("/frameio/path", get(routes::frameio::folder_path))
        .route("/frameio/folders/{folder_id}", axum::routing::delete(routes::frameio::delete_folder))
        .route("/frameio/folders/{folder_id}/move", axum::routing::patch(routes::frameio::move_folder))
        .route("/frameio/folders/{folder_id}/copy", post(routes::frameio::copy_folder))
        .route("/frameio/files", get(routes::frameio::list_files))
        .route("/frameio/files/{file_id}", axum::routing::delete(routes::frameio::delete_file))
        .route("/frameio/files/{file_id}/download-url", get(routes::frameio::file_download_url))
        .route("/frameio/files/{file_id}/move", axum::routing::patch(routes::frameio::move_file))
        .route("/frameio/files/{file_id}/copy", post(routes::frameio::copy_file))
        .route("/frameio/shares", post(routes::frameio::create_share))
        .fallback(error::not_found)
        .layer(axum::middleware::from_fn(request_log::request_id_middleware));

    // SvelteKit's static build. Unknown non-API paths serve index.html for client-side routing.
    // NOTE: `not_found_service` would force a 404 status on the shell; `fallback` serves it 200.
    let web_dir = std::env::var("WEB_DIR").unwrap_or_else(|_| "web/build".to_string());
    let static_service =
        ServeDir::new(&web_dir).fallback(ServeFile::new(format!("{web_dir}/index.html")));

    let app = Router::new()
        .nest("/api", api)
        // Public link pages: the shell, titled and statused per link (see spa::public_page).
        .route("/u/{token}", get(spa::upload_page))
        .route("/d/{token}", get(spa::download_page))
        .fallback_service(static_service)
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            session::session_middleware,
        ))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!("listening on {addr} (web dir: {web_dir})");
    axum::serve(listener, app.into_make_service_with_connect_info::<std::net::SocketAddr>()).await?;
    Ok(())
}
