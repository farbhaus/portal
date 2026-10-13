
//! Formatting helpers that reproduce Python's textual conventions exactly, because the
//! differential compares raw bytes.

use chrono::{DateTime, Utc};

/// `datetime.isoformat()` for an aware UTC datetime: `YYYY-MM-DDTHH:MM:SS.ffffff+00:00`,
/// with the fractional part omitted when it is zero (Python omits trailing-zero micros).
pub fn iso(dt: &DateTime<Utc>) -> String {
    let micros = dt.timestamp_subsec_micros();
    let base = dt.format("%Y-%m-%dT%H:%M:%S").to_string();
    if micros == 0 {
        format!("{base}+00:00")
    } else {
        format!("{base}.{micros:06}+00:00")
    }
}
