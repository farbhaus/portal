"""Match sync rules to destinations by their Frame.io folder.

There is no FK between the two — a destination and a sync rule each store their own Frame.io
folder config — so "is this destination synced?" is answered by comparing IDs: a rule covers a
destination when it watches the same folder (exact), or when it recursively watches an ancestor
from the destination's cached `path` breadcrumb. Without a cached path, matching degrades to
exact-only. Pure and DB-only: never calls Frame.io.
"""

from collections.abc import Iterable
from typing import Any

from portal.db.models import SyncRule


def rules_covering(
    dest_config: dict[str, Any], rules: Iterable[SyncRule]
) -> list[tuple[SyncRule, bool]]:
    """Sync rules whose watched folder covers this destination's folder, as (rule, exact) pairs."""
    account_id = dest_config.get("account_id")
    folder_id = dest_config.get("folder_id")
    if not account_id or not folder_id:
        return []
    ancestor_ids = {
        seg.get("id")
        for seg in (dest_config.get("path") or [])[:-1]
        if isinstance(seg, dict) and seg.get("id")
    }
    covering: list[tuple[SyncRule, bool]] = []
    for rule in rules:
        src = rule.source_config or {}
        if src.get("account_id") != account_id:
            continue
        rule_folder = src.get("folder_id")
        if rule_folder == folder_id:
            covering.append((rule, True))
        elif bool(src.get("recursive", True)) and rule_folder in ancestor_ids:
            covering.append((rule, False))
    return covering
