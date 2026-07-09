"""Best-effort resolution and caching of Frame.io folder breadcrumbs for display.

Destinations and sync rules cache the full folder ancestry (`path: [{id, name}, ...]`,
root→leaf) in their JSONB config so list views can show "where does this land" with zero
Frame.io calls. The admin UI already holds the breadcrumb when it creates them and sends it
along; these helpers cover the gaps — non-UI API callers and stale/legacy rows — by resolving
the path from Frame.io on the detail GET. Path data is display-only: resolution failures must
never fail the request, they just leave the cached (or absent) path in place.

The top-most segment is labeled "{Project} (root)" when it is the project's root folder,
matching the create pickers and the download-link /source endpoint.
"""

from datetime import UTC, datetime, timedelta
from typing import Any

from portal.frameio.client import FrameioError, get_frameio_client

PATH_TTL = timedelta(hours=1)

PathSegments = list[dict[str, str]]


def clean_path(raw: Any, folder_id: str) -> PathSegments | None:
    """Validate a client-supplied breadcrumb; None unless it's well-formed and ends at folder_id."""
    if not isinstance(raw, list) or not raw:
        return None
    path: PathSegments = []
    for seg in raw:
        if not isinstance(seg, dict) or not seg.get("id") or not seg.get("name"):
            return None
        path.append({"id": str(seg["id"]), "name": str(seg["name"])})
    if path[-1]["id"] != folder_id:
        return None
    return path


def path_is_fresh(config: dict[str, Any]) -> bool:
    """True when the cached breadcrumb exists and is younger than PATH_TTL."""
    if not config.get("path"):
        return False
    try:
        resolved = datetime.fromisoformat(str(config.get("path_resolved_at")))
    except (TypeError, ValueError):
        return False
    return datetime.now(UTC) - resolved < PATH_TTL


def stamp_path(
    config: dict[str, Any], path: PathSegments, project_name: str | None
) -> dict[str, Any]:
    """Copy of config with the breadcrumb fields set (folder_name only filled when missing)."""
    out = dict(config)
    out["path"] = path
    out.setdefault("folder_name", path[-1]["name"])
    if project_name:
        out["project_name"] = project_name
    out["path_resolved_at"] = datetime.now(UTC).isoformat()
    return out


async def resolve_folder_path(
    account_id: str, project_id: str, folder_id: str
) -> tuple[PathSegments, str | None] | None:
    """Resolve the full breadcrumb + project name from Frame.io; None on any Frame.io error."""
    client = get_frameio_client()
    try:
        ancestry = await client.folder_ancestry(account_id, folder_id)
        if not ancestry:
            return None
        path: PathSegments = [{"id": i.id, "name": i.name} for i in ancestry]
        project_name: str | None = None
        try:
            project = await client.get_project(account_id, project_id)
            project_name = project.name
            if project.root_folder_id and path[0]["id"] == project.root_folder_id:
                path[0] = {"id": path[0]["id"], "name": f"{project.name} (root)"}
        except FrameioError:
            pass
        return path, project_name
    except FrameioError:
        return None


async def refresh_config_path(config: dict[str, Any]) -> dict[str, Any] | None:
    """Refreshed copy of a frameio config's breadcrumb; None when fresh or unresolvable."""
    if config.get("type") != "frameio" or path_is_fresh(config):
        return None
    account_id = config.get("account_id")
    project_id = config.get("project_id")
    folder_id = config.get("folder_id")
    if not (account_id and project_id and folder_id):
        return None
    resolved = await resolve_folder_path(str(account_id), str(project_id), str(folder_id))
    if resolved is None:
        return None
    return stamp_path(config, resolved[0], resolved[1])
