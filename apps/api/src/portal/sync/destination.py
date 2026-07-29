"""Destination readiness guard for sync writes.

A sync rule's destination is usually a network share (NAS) mounted on the host and bind-mounted
into the container. When that share isn't mounted the mountpoint is still an ordinary writable
directory, so downloads land on the container's own disk and then vanish *underneath* the share the
moment it mounts again — invisible to ``ls``/``du`` while still consuming the system disk. One
outage of a few days silently filled a production root filesystem with 40 GB of files nobody could
see.

``os.path.ismount`` cannot separate the two cases from inside the container: the destination sits
within the ``/mnt`` bind mount whether or not the share is up, so the path always looks "on a
mount". Instead each rule *arms itself* — the first job that writes successfully drops a marker file
in the destination root, and from then on a missing marker means the share is gone, so the job waits
instead of writing to a bare mountpoint. The marker lives on the share, so it disappears exactly
when the share does, whichever filesystem backs it (NFS, SMB, a local disk).

A rule counts as armed once it has a completed job, so there is no schema change and nothing for an
operator to configure: existing rules arm themselves on their next successful sync. The first sync
of a brand-new rule is necessarily unprotected — there is nothing yet to distinguish an empty
destination from an absent one.

A rule that already has completed jobs but no marker yet (any rule that predates this guard, or one
whose marker was deleted) falls back to a one-time check of whether the destination holds anything
at all: a mounted share still contains what that rule wrote before, whereas an unmounted mountpoint
is an empty directory. Without that fallback such a rule could never arm itself — the guard would
block the very write that creates the marker.
"""

from __future__ import annotations

from pathlib import Path

from portal.lib.logging import get_logger

log = get_logger("sync.destination")

MARKER_NAME = ".portal-destination"

_MARKER_BODY = """\
Portal sync destination marker.

Portal refuses to sync into this folder while this file is missing: that is how it tells a mounted
share from the empty mountpoint left behind when the share is not mounted, which would otherwise
fill the server's own disk with files hidden under the mount.

Delete it only if you no longer sync here. To re-enable syncing, recreate it (touch this file).
"""


class DestinationUnavailable(Exception):
    """An armed destination lost its marker — the share behind it isn't mounted."""


def marker_path(root: str | Path) -> Path:
    return Path(root) / MARKER_NAME


def _has_contents(root: Path) -> bool:
    try:
        return any(root.iterdir())
    except OSError:
        return False  # missing or unreadable — either way, not somewhere to write


def check_destination(root: str | Path, *, armed: bool) -> None:
    """Raise ``DestinationUnavailable`` when an armed destination has lost its marker."""
    if not armed or marker_path(root).exists():
        return
    # No marker, but this rule has synced before: either it predates the marker (arm it on this
    # run) or the share is gone. Anything already in the destination means it is really there.
    if _has_contents(Path(root)):
        return
    raise DestinationUnavailable(
        f"destination {root} is empty and has no {MARKER_NAME} marker — the share behind it is "
        f"not mounted; waiting instead of writing into the mountpoint"
    )


def arm_destination(root: str | Path) -> None:
    """Drop the marker in a destination that just accepted a write.

    Best-effort: a share that refuses the write (read-only export, tight permissions) must not fail
    an otherwise good sync, but it does mean the rule stays unarmed, so say so loudly enough to be
    findable in the logs.
    """
    path = marker_path(root)
    try:
        if path.exists():
            return
        path.write_text(_MARKER_BODY, encoding="utf-8")
    except OSError as exc:
        log.warning("sync.destination.arm_failed", path=str(path), error=str(exc))
        return
    log.info("sync.destination.armed", path=str(path))
