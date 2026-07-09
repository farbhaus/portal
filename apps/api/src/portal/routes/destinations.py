"""Destinations CRUD.

A destination is a logical upload target, currently always a Frame.io folder. The folder is
picked at create time (via the picker endpoints) and validated against Frame.io so we never
store a destination pointing at a folder we can't reach; its resolved name — and, for display,
the full breadcrumb `path` (sent by the picker, so creation makes no extra Frame.io calls) —
are cached in the config. The display name and subtitle are editable (the logo and accent come
from global branding); the folder binding is fixed once created. The `config` JSONB is
type-tagged so phase-2 S3 destinations slot in additively.

Responses embed the sync rules covering the destination's folder and the upload links pointing
at it, so the admin UI can show links ↔ destination ↔ sync in one place.
"""

import uuid
from collections.abc import Sequence
from typing import Any, Literal

from fastapi import APIRouter, Depends, HTTPException
from pydantic import BaseModel, Field
from sqlalchemy import select
from sqlalchemy.ext.asyncio import AsyncSession
from sqlalchemy.orm import selectinload

from portal.auth.session import require_admin
from portal.db.models import AuditLog, Destination, SyncRule, UploadLink, User
from portal.db.session import get_session
from portal.frameio.client import FrameioError, FrameioNotConnected, get_frameio_client
from portal.lib.errors import NotFoundError
from portal.lib.logging import get_logger
from portal.services.frameio_paths import (
    clean_path,
    refresh_config_path,
    resolve_folder_path,
    stamp_path,
)
from portal.services.sync_coverage import rules_covering
from portal.uploads.links import link_state

log = get_logger("routes.destinations")
router = APIRouter(prefix="/destinations", tags=["destinations"])


class PathSegmentIn(BaseModel):
    id: str = Field(min_length=1)
    name: str = Field(min_length=1)


class FrameioConfigIn(BaseModel):
    type: Literal["frameio"] = "frameio"
    account_id: str = Field(min_length=1)
    workspace_id: str = Field(min_length=1)
    project_id: str = Field(min_length=1)
    folder_id: str = Field(min_length=1)
    # Full breadcrumb (root→leaf) + project name as known to the picker; display-only.
    path: list[PathSegmentIn] | None = None
    project_name: str | None = None


class DestinationCreate(BaseModel):
    display_name: str = Field(min_length=1, max_length=255)
    config: FrameioConfigIn
    subtitle: str | None = None


class DestinationUpdate(BaseModel):
    display_name: str | None = Field(default=None, min_length=1, max_length=255)
    subtitle: str | None = None


class DestinationSyncRuleOut(BaseModel):
    id: str
    name: str
    destination_path: str
    enabled: bool
    recursive: bool
    exact: bool  # False = covered via a recursive rule on an ancestor folder


class DestinationUploadLinkOut(BaseModel):
    id: str
    label: str
    state: str


class DestinationOut(BaseModel):
    id: str
    display_name: str
    config: dict[str, Any]
    subtitle: str | None
    sync_rules: list[DestinationSyncRuleOut]
    upload_links: list[DestinationUploadLinkOut]
    created_at: str
    updated_at: str


def _to_out(
    dest: Destination, rules: Sequence[SyncRule], links: Sequence[UploadLink]
) -> DestinationOut:
    return DestinationOut(
        id=str(dest.id),
        display_name=dest.display_name,
        config=dest.config,
        subtitle=dest.subtitle,
        sync_rules=[
            DestinationSyncRuleOut(
                id=str(rule.id),
                name=rule.name,
                destination_path=rule.destination_path,
                enabled=rule.enabled,
                recursive=bool(rule.source_config.get("recursive", True)),
                exact=exact,
            )
            for rule, exact in rules_covering(dest.config, rules)
        ],
        upload_links=[
            DestinationUploadLinkOut(
                id=str(link.id),
                label=link.brand_display_name or "Untitled",
                state=link_state(link),
            )
            for link in sorted(links, key=lambda li: li.created_at, reverse=True)
        ],
        created_at=dest.created_at.isoformat(),
        updated_at=dest.updated_at.isoformat(),
    )


async def _all_rules(db: AsyncSession) -> Sequence[SyncRule]:
    return (await db.execute(select(SyncRule))).scalars().all()


async def _get_or_404(db: AsyncSession, dest_id: uuid.UUID) -> Destination:
    result = await db.execute(
        select(Destination)
        .where(Destination.id == dest_id)
        .options(selectinload(Destination.upload_links))
    )
    dest = result.scalar_one_or_none()
    if dest is None:
        raise NotFoundError("Destination not found")
    return dest


@router.get("", response_model=list[DestinationOut])
async def list_destinations(
    _: User = Depends(require_admin), db: AsyncSession = Depends(get_session)
) -> list[DestinationOut]:
    result = await db.execute(
        select(Destination)
        .options(selectinload(Destination.upload_links))
        .order_by(Destination.created_at.desc())
    )
    dests = result.scalars().all()
    rules = await _all_rules(db)
    return [_to_out(d, rules, d.upload_links) for d in dests]


@router.post("", response_model=DestinationOut, status_code=201)
async def create_destination(
    body: DestinationCreate,
    user: User = Depends(require_admin),
    db: AsyncSession = Depends(get_session),
) -> DestinationOut:
    # Validate the picked folder against Frame.io and capture its name for display.
    try:
        folder = await get_frameio_client().get_folder(
            body.config.account_id, body.config.folder_id
        )
    except FrameioNotConnected as exc:
        raise HTTPException(status_code=409, detail="Frame.io is not connected") from exc
    except FrameioError as exc:
        raise HTTPException(
            status_code=400, detail="Frame.io folder not found or inaccessible"
        ) from exc

    config = body.config.model_dump(exclude={"path", "project_name"})
    config["folder_name"] = folder.name

    path = clean_path(
        [seg.model_dump() for seg in body.config.path] if body.config.path else None,
        body.config.folder_id,
    )
    project_name = (body.config.project_name or "").strip() or None
    if path is None:
        # Non-UI callers don't know the breadcrumb; resolve it best-effort, display-only.
        resolved = await resolve_folder_path(
            body.config.account_id, body.config.project_id, body.config.folder_id
        )
        if resolved is not None:
            path, project_name = resolved
    if path is not None:
        config = stamp_path(config, path, project_name)

    dest = Destination(
        display_name=body.display_name,
        config=config,
        subtitle=body.subtitle,
    )
    db.add(dest)
    await db.flush()
    db.add(
        AuditLog(
            user_id=user.id,
            action="destination.created",
            detail={"destination_id": str(dest.id), "display_name": dest.display_name},
        )
    )
    await db.commit()
    dest = await _get_or_404(db, dest.id)
    log.info("destination.created", destination_id=str(dest.id))
    return _to_out(dest, await _all_rules(db), dest.upload_links)


@router.get("/{dest_id}", response_model=DestinationOut)
async def get_destination(
    dest_id: uuid.UUID,
    _: User = Depends(require_admin),
    db: AsyncSession = Depends(get_session),
) -> DestinationOut:
    dest = await _get_or_404(db, dest_id)
    # Lazily backfill/refresh the cached breadcrumb (legacy rows, folder renames); best-effort.
    refreshed = await refresh_config_path(dest.config)
    if refreshed is not None:
        dest.config = refreshed
        await db.commit()
        dest = await _get_or_404(db, dest_id)
    return _to_out(dest, await _all_rules(db), dest.upload_links)


@router.patch("/{dest_id}", response_model=DestinationOut)
async def update_destination(
    dest_id: uuid.UUID,
    body: DestinationUpdate,
    _: User = Depends(require_admin),
    db: AsyncSession = Depends(get_session),
) -> DestinationOut:
    dest = await _get_or_404(db, dest_id)
    fields = body.model_dump(exclude_unset=True)
    for key, value in fields.items():
        setattr(dest, key, value)
    await db.commit()
    dest = await _get_or_404(db, dest_id)
    return _to_out(dest, await _all_rules(db), dest.upload_links)


@router.delete("/{dest_id}", status_code=204)
async def delete_destination(
    dest_id: uuid.UUID,
    user: User = Depends(require_admin),
    db: AsyncSession = Depends(get_session),
) -> None:
    dest = await _get_or_404(db, dest_id)
    await db.delete(dest)
    db.add(
        AuditLog(
            user_id=user.id,
            action="destination.deleted",
            detail={"destination_id": str(dest_id)},
        )
    )
    await db.commit()
