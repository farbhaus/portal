
//! Destination readiness guard — port of `portal.sync.destination`.

use std::path::{Path, PathBuf};

pub const MARKER_NAME: &str = ".portal-destination";

const MARKER_BODY: &str = "Portal sync destination marker.\n\nPortal refuses to sync into this folder while this file is missing: that is how it tells a mounted\nshare from the empty mountpoint left behind when the share is not mounted, which would otherwise\nfill the server's own disk with files hidden under the mount.\n\nDelete it only if you no longer sync here. To re-enable syncing, recreate it (touch this file).\n";

#[derive(Debug)]
pub struct DestinationUnavailable(pub String);
impl std::fmt::Display for DestinationUnavailable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
impl std::error::Error for DestinationUnavailable {}

pub fn marker_path(root: &str) -> PathBuf {
    Path::new(root).join(MARKER_NAME)
}

fn has_contents(root: &Path) -> bool {
    std::fs::read_dir(root).map(|mut d| d.next().is_some()).unwrap_or(false)
}

/// Raise `DestinationUnavailable` when an armed destination has lost its marker.
pub fn check_destination(root: &str, armed: bool) -> Result<(), DestinationUnavailable> {
    if !armed || marker_path(root).exists() {
        return Ok(());
    }
    if has_contents(Path::new(root)) {
        return Ok(());
    }
    Err(DestinationUnavailable(format!(
        "destination {root} is empty and has no {MARKER_NAME} marker — the share behind it is not mounted; waiting instead of writing into the mountpoint"
    )))
}

/// Drop the marker in a destination that just accepted a write (best-effort).
pub fn arm_destination(root: &str) {
    let path = marker_path(root);
    if path.exists() {
        return;
    }
    if let Err(e) = std::fs::write(&path, MARKER_BODY) {
        tracing::warn!("sync.destination.arm_failed path={} error={e}", path.display());
    } else {
        tracing::info!("sync.destination.armed path={}", path.display());
    }
}
