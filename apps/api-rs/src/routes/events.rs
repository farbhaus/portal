
//! `GET /api/events` — a server-sent event stream that tells admin pages when something they show
//! has changed (an upload progressed or finished, a file was downloaded, a link was added or
//! removed), so they reload then instead of polling on a timer.
//!
//! One poller per process compares a cheap fingerprint of those tables every couple of seconds,
//! only while at least one admin page is listening, and bumps a version every subscriber watches.
//! Events carry no data beyond that version: pages re-fetch what they show, through the same
//! authenticated endpoints as before.

use std::convert::Infallible;
use std::sync::OnceLock;
use std::time::Duration;

use axum::extract::State;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::IntoResponse;
use futures::stream::{self, Stream, StreamExt};
use sqlx::PgPool;
use tokio::sync::watch;

use crate::auth::Admin;
use crate::state::AppState;

const POLL_EVERY: Duration = Duration::from_secs(2);

/// One row that changes whenever anything a live admin page shows changes.
const FINGERPRINT_SQL: &str = "SELECT concat_ws('|',
    (SELECT max(updated_at) FROM upload_sessions),
    (SELECT count(*) FROM upload_sessions WHERE status = 'in_progress'),
    (SELECT count(*) FROM download_events),
    (SELECT count(*) FROM links),
    (SELECT max(updated_at) FROM links))";

static VERSION: OnceLock<watch::Sender<u64>> = OnceLock::new();

/// The shared version channel, starting the poller on first use.
fn version_channel(db: &PgPool) -> &'static watch::Sender<u64> {
    VERSION.get_or_init(|| {
        let (tx, _) = watch::channel(0u64);
        let db = db.clone();
        let poller = tx.clone();
        tokio::spawn(async move {
            let mut last: Option<String> = None;
            loop {
                tokio::time::sleep(POLL_EVERY).await;
                if poller.receiver_count() == 0 {
                    continue; // nobody listening: don't touch the database
                }
                let Ok(now) = sqlx::query_scalar::<_, String>(FINGERPRINT_SQL).fetch_one(&db).await else {
                    continue;
                };
                if last.as_ref().is_some_and(|l| *l != now) {
                    poller.send_modify(|v| *v += 1);
                }
                last = Some(now);
            }
        });
        tx
    })
}

fn change_stream(rx: watch::Receiver<u64>) -> impl Stream<Item = Result<Event, Infallible>> {
    let hello = stream::once(async { Ok(Event::default().event("ready").data("0")) });
    let changes = stream::unfold(rx, |mut rx| async move {
        rx.changed().await.ok()?;
        let v = *rx.borrow_and_update();
        Some((Ok(Event::default().event("change").data(v.to_string())), rx))
    });
    hello.chain(changes)
}

pub async fn stream_events(State(state): State<AppState>, _: Admin) -> impl IntoResponse {
    let rx = version_channel(&state.db).subscribe();
    Sse::new(change_stream(rx)).keep_alive(KeepAlive::new().interval(Duration::from_secs(20)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn stream_says_ready_then_reports_each_change() {
        let (tx, rx) = watch::channel(0u64);
        let mut s = Box::pin(change_stream(rx));
        let first = s.next().await.unwrap().unwrap();
        assert!(format!("{first:?}").contains("ready"));
        tx.send_modify(|v| *v += 1);
        let second = s.next().await.unwrap().unwrap();
        assert!(format!("{second:?}").contains("change"));
    }
}
