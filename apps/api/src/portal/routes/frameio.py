import secrets
from datetime import datetime

from fastapi import APIRouter, Depends, HTTPException, Request
from fastapi.responses import RedirectResponse
from pydantic import BaseModel
from sqlalchemy.ext.asyncio import AsyncSession

from portal.auth.session import get_current_user, require_admin
from portal.db.models import AuditLog, User
from portal.db.session import get_session
from portal.frameio import connection as conn_svc
from portal.frameio import oauth
from portal.frameio.client import (
    FrameioError,
    FrameioNotConnected,
    FrameioNotFound,
    FrameioRateLimited,
    get_frameio_client,
)
from portal.lib.config import get_settings
from portal.lib.logging import get_logger
from portal.services.runtime_config import get_oauth_config

log = get_logger("routes.frameio")
router = APIRouter(prefix="/frameio", tags=["frameio"])

_STATE_SESSION_KEY = "frameio_oauth_state"


class ConnectionStatus(BaseModel):
    connected: bool
    adobe_email: str | None = None
    expires_at: str | None = None
    needs_refresh: bool | None = None
    scopes: str | None = None


def _web_redirect(suffix: str) -> RedirectResponse:
    base = get_settings().base_url.rstrip("/")
    return RedirectResponse(url=f"{base}{suffix}", status_code=302)


@router.get("/oauth/connect")
async def connect(
    request: Request,
    _: User = Depends(require_admin),
    db: AsyncSession = Depends(get_session),
) -> RedirectResponse:
    oauth_config = await get_oauth_config(db)
    if not oauth_config.configured:
        return _web_redirect("/settings?frameio=unconfigured")
    state = secrets.token_urlsafe(32)
    request.session[_STATE_SESSION_KEY] = state
    return RedirectResponse(url=oauth.build_authorize_url(oauth_config, state), status_code=302)


@router.get("/oauth/callback")
async def callback(
    request: Request,
    state: str | None = None,
    code: str | None = None,
    error: str | None = None,
    db: AsyncSession = Depends(get_session),
    user: User | None = Depends(get_current_user),
) -> RedirectResponse:
    expected = request.session.pop(_STATE_SESSION_KEY, None)
    if user is None:
        return _web_redirect("/login")
    if error or not code or not state or state != expected:
        log.warning("frameio.callback.rejected", has_code=bool(code), state_ok=state == expected)
        return _web_redirect("/settings?frameio=error")
    try:
        oauth_config = await get_oauth_config(db)
        tokens = await oauth.exchange_code(oauth_config, code)
        identity = await oauth.fetch_identity(tokens.access_token)
        await conn_svc.upsert_connection(db, user.id, tokens, identity)
    except oauth.FrameioOAuthError:
        return _web_redirect("/settings?frameio=error")

    db.add(AuditLog(user_id=user.id, action="frameio.connected", detail={"email": identity.email}))
    await db.commit()
    return _web_redirect("/settings?frameio=connected")


@router.get("/status", response_model=ConnectionStatus)
async def status(
    user: User = Depends(require_admin), db: AsyncSession = Depends(get_session)
) -> ConnectionStatus:
    conn = await conn_svc.get_connection(db, user.id)
    if conn is None:
        return ConnectionStatus(connected=False)
    return ConnectionStatus(
        connected=True,
        adobe_email=conn.adobe_email,
        expires_at=conn.token_expires_at.isoformat() if conn.token_expires_at else None,
        needs_refresh=conn_svc.needs_refresh(conn),
        scopes=get_settings().frameio_scopes,
    )


@router.post("/disconnect")
async def disconnect(
    user: User = Depends(require_admin), db: AsyncSession = Depends(get_session)
) -> dict[str, bool]:
    conn = await conn_svc.get_connection(db, user.id)
    if conn is not None:
        await db.delete(conn)
        db.add(AuditLog(user_id=user.id, action="frameio.disconnected"))
        await db.commit()
    return {"ok": True}


# ── pickers (configure destinations by browsing the Frame.io account tree) ─────


class PickerItemOut(BaseModel):
    id: str
    name: str


class ProjectItemOut(BaseModel):
    id: str
    name: str
    root_folder_id: str | None = None


def _picker_error(exc: Exception) -> HTTPException:
    if isinstance(exc, FrameioNotConnected):
        return HTTPException(status_code=409, detail="Frame.io is not connected")
    if isinstance(exc, FrameioRateLimited):
        return HTTPException(status_code=503, detail="Frame.io is busy, try again in a moment")
    if isinstance(exc, FrameioNotFound):
        return HTTPException(status_code=404, detail="Frame.io item not found")
    return HTTPException(status_code=502, detail="Frame.io request failed")


@router.get("/accounts", response_model=list[PickerItemOut])
async def list_accounts(_: User = Depends(require_admin)) -> list[PickerItemOut]:
    try:
        items = await get_frameio_client().list_accounts()
    except FrameioError as exc:
        raise _picker_error(exc) from exc
    return [PickerItemOut(id=i.id, name=i.name) for i in items]


@router.get("/workspaces", response_model=list[PickerItemOut])
async def list_workspaces(
    account_id: str, _: User = Depends(require_admin)
) -> list[PickerItemOut]:
    try:
        items = await get_frameio_client().list_workspaces(account_id)
    except FrameioError as exc:
        raise _picker_error(exc) from exc
    return [PickerItemOut(id=i.id, name=i.name) for i in items]


@router.get("/projects", response_model=list[ProjectItemOut])
async def list_projects(
    account_id: str, workspace_id: str, _: User = Depends(require_admin)
) -> list[ProjectItemOut]:
    try:
        items = await get_frameio_client().list_projects(account_id, workspace_id)
    except FrameioError as exc:
        raise _picker_error(exc) from exc
    return [
        ProjectItemOut(id=i.id, name=i.name, root_folder_id=i.root_folder_id) for i in items
    ]


class CreateProjectRequest(BaseModel):
    account_id: str
    workspace_id: str
    name: str
    restricted: bool = False


@router.post("/projects", response_model=ProjectItemOut)
async def create_project(
    body: CreateProjectRequest, _: User = Depends(require_admin)
) -> ProjectItemOut:
    """Create a project in a workspace (explorer New-project action)."""
    name = body.name.strip()
    if not name:
        raise HTTPException(status_code=422, detail="Project name is required")
    try:
        item = await get_frameio_client().create_project(
            body.account_id, body.workspace_id, name=name, restricted=body.restricted
        )
    except FrameioError as exc:
        raise _picker_error(exc) from exc
    return ProjectItemOut(id=item.id, name=item.name, root_folder_id=item.root_folder_id)


@router.delete("/projects/{project_id}", status_code=204)
async def delete_project(
    project_id: str, account_id: str, _: User = Depends(require_admin)
) -> None:
    """Delete a project and all its contents from Frame.io (explorer)."""
    try:
        await get_frameio_client().delete_project(account_id, project_id)
    except FrameioError as exc:
        raise _picker_error(exc) from exc


@router.get("/folders", response_model=list[PickerItemOut])
async def list_folders(
    account_id: str, folder_id: str, _: User = Depends(require_admin)
) -> list[PickerItemOut]:
    """Direct child folders of `folder_id` (pass a project's root_folder_id to start)."""
    try:
        items = await get_frameio_client().list_subfolders(account_id, folder_id)
    except FrameioError as exc:
        raise _picker_error(exc) from exc
    return [PickerItemOut(id=i.id, name=i.name) for i in items]


class CreateFolderRequest(BaseModel):
    account_id: str
    parent_folder_id: str
    name: str


@router.post("/folders", response_model=PickerItemOut)
async def create_folder(
    body: CreateFolderRequest, _: User = Depends(require_admin)
) -> PickerItemOut:
    """Create a new subfolder under a parent folder — used by the picker's New-folder action."""
    name = body.name.strip()
    if not name:
        raise HTTPException(status_code=422, detail="Folder name is required")
    client = get_frameio_client()
    try:
        item = await client.create_folder(body.account_id, body.parent_folder_id, name)
    except FrameioError as exc:
        raise _picker_error(exc) from exc
    return PickerItemOut(id=item.id, name=item.name)


class FileItemOut(BaseModel):
    id: str
    name: str
    file_size: int | None = None
    media_type: str | None = None


@router.get("/files", response_model=list[FileItemOut])
async def list_files(
    account_id: str, folder_id: str, _: User = Depends(require_admin)
) -> list[FileItemOut]:
    """Direct child files of `folder_id` (for download-link source selection)."""
    try:
        items = await get_frameio_client().list_files(account_id, folder_id)
    except FrameioError as exc:
        raise _picker_error(exc) from exc
    return [
        FileItemOut(id=i.id, name=i.name, file_size=i.file_size, media_type=i.media_type)
        for i in items
    ]


class DownloadUrlOut(BaseModel):
    url: str


@router.get("/files/{file_id}/download-url", response_model=DownloadUrlOut)
async def file_download_url(
    file_id: str, account_id: str, _: User = Depends(require_admin)
) -> DownloadUrlOut:
    """Mint a short-lived signed download URL for a file (explorer download action)."""
    try:
        url = await get_frameio_client().get_download_url(account_id, file_id)
    except FrameioError as exc:
        raise _picker_error(exc) from exc
    return DownloadUrlOut(url=url)


@router.delete("/files/{file_id}", status_code=204)
async def delete_file(
    file_id: str, account_id: str, user: User = Depends(require_admin)
) -> None:
    """Delete a file from Frame.io (explorer)."""
    try:
        await get_frameio_client().delete_file(account_id, file_id)
    except FrameioError as exc:
        raise _picker_error(exc) from exc


@router.delete("/folders/{folder_id}", status_code=204)
async def delete_folder(
    folder_id: str, account_id: str, user: User = Depends(require_admin)
) -> None:
    """Delete a folder (and its contents) from Frame.io (explorer)."""
    try:
        await get_frameio_client().delete_folder(account_id, folder_id)
    except FrameioError as exc:
        raise _picker_error(exc) from exc


# ── move / copy (clipboard Cut/Copy → Paste in the explorer) ───────────────────


class MoveCopyRequest(BaseModel):
    account_id: str
    parent_id: str


@router.patch("/files/{file_id}/move", status_code=204)
async def move_file(
    file_id: str, body: MoveCopyRequest, _: User = Depends(require_admin)
) -> None:
    """Move a file into another folder (Cut → Paste)."""
    try:
        await get_frameio_client().move_file(body.account_id, file_id, body.parent_id)
    except FrameioError as exc:
        raise _picker_error(exc) from exc


@router.post("/files/{file_id}/copy", response_model=FileItemOut)
async def copy_file(
    file_id: str, body: MoveCopyRequest, _: User = Depends(require_admin)
) -> FileItemOut:
    """Copy a file into another folder (Copy → Paste). Returns the new file."""
    try:
        item = await get_frameio_client().copy_file(body.account_id, file_id, body.parent_id)
    except FrameioError as exc:
        raise _picker_error(exc) from exc
    return FileItemOut(
        id=item.id, name=item.name, file_size=item.file_size, media_type=item.media_type
    )


@router.patch("/folders/{folder_id}/move", status_code=204)
async def move_folder(
    folder_id: str, body: MoveCopyRequest, _: User = Depends(require_admin)
) -> None:
    """Move a folder (and its contents) into another folder (Cut → Paste)."""
    try:
        await get_frameio_client().move_folder(body.account_id, folder_id, body.parent_id)
    except FrameioError as exc:
        raise _picker_error(exc) from exc


@router.post("/folders/{folder_id}/copy", response_model=PickerItemOut)
async def copy_folder(
    folder_id: str, body: MoveCopyRequest, _: User = Depends(require_admin)
) -> PickerItemOut:
    """Copy a folder into another folder (Copy → Paste). Returns the new folder."""
    try:
        item = await get_frameio_client().copy_folder(body.account_id, folder_id, body.parent_id)
    except FrameioError as exc:
        raise _picker_error(exc) from exc
    return PickerItemOut(id=item.id, name=item.name)


# ── Frame.io share links (native review page over files/folders) ───────────────


class CreateShareRequest(BaseModel):
    account_id: str
    project_id: str
    name: str
    asset_ids: list[str]
    access: str = "public"
    downloading_enabled: bool = True
    expiration: datetime | None = None
    passphrase: str | None = None


class ShareOut(BaseModel):
    id: str
    name: str | None = None
    short_url: str | None = None
    access: str | None = None
    passphrase: str | None = None
    expiration: str | None = None
    downloading_enabled: bool
    enabled: bool


@router.post("/shares", response_model=ShareOut)
async def create_share(
    body: CreateShareRequest, _: User = Depends(require_admin)
) -> ShareOut:
    """Create a Frame.io share link over the given files/folders in a project."""
    name = body.name.strip()
    if not name:
        raise HTTPException(status_code=422, detail="Share name is required")
    if not body.asset_ids:
        raise HTTPException(status_code=422, detail="Select at least one file or folder to share")
    if body.access not in ("public", "secure"):
        raise HTTPException(status_code=422, detail="access must be 'public' or 'secure'")
    try:
        share = await get_frameio_client().create_share(
            body.account_id,
            body.project_id,
            name=name,
            asset_ids=body.asset_ids,
            access=body.access,
            downloading_enabled=body.downloading_enabled,
            expiration=body.expiration,
            passphrase=body.passphrase or None,
        )
    except FrameioError as exc:
        raise _picker_error(exc) from exc
    return ShareOut(
        id=share.id,
        name=share.name,
        short_url=share.short_url,
        access=share.access,
        passphrase=share.passphrase,
        expiration=share.expiration,
        downloading_enabled=share.downloading_enabled,
        enabled=share.enabled,
    )
