//! `portal-api worker` — the background housekeeping loop: marks upload sessions whose page went
//! quiet as abandoned, sends due expiry reminders, and runs the daily retention sweep.

use std::time::{Duration, Instant};

use crate::state::AppState;

pub async fn run(state: AppState) -> anyhow::Result<()> {
    tracing::info!("worker.startup");
    let mut last_abandon = Instant::now();
    let mut last_retention = Instant::now();
    let mut last_reminders = Instant::now() - Duration::from_secs(10 * 60); // check once at startup

    loop {
        if last_abandon.elapsed() >= Duration::from_secs(15 * 60) {
            last_abandon = Instant::now();
            let _ = crate::services::upload_sessions::abandon_stale_upload_sessions(&state.db, &state.settings).await;
        }
        if last_reminders.elapsed() >= Duration::from_secs(10 * 60) {
            last_reminders = Instant::now();
            crate::services::reminders::send_due(&state).await;
        }
        if last_retention.elapsed() >= Duration::from_secs(24 * 60 * 60) {
            last_retention = Instant::now();
            let _ = crate::services::retention::purge_expired(&state.db, &state.settings).await;
        }

        tokio::time::sleep(Duration::from_secs(30)).await;
    }
}
