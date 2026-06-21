"""Destinations CRUD.

A destination is a logical upload target, currently always a Frame.io folder. The folder is
picked at create time (via the picker endpoints) and validated against Frame.io so we never
store a destination pointing at a folder we can't reach; its resolved name is cached in the
config for display. The display name and subtitle are editable (the logo and accent come from
global branding); the folder binding is fixed once created. The `config` JSONB is type-tagged so
phase-2 S3 destinations slot in additively.
"""

import uuid
from typing import Any, Literal

from fastapi import APIRouter, Depends, HTTPException
from pydantic import BaseModel, Field
from sqlalchemy import select
from sqlalchemy.ext.asyncio import AsyncSession

from portal.auth.session import require_admin
from portal.db.models import AuditLog, Destination, User
from portal.db.session import get_session
from portal.frameio.client import FrameioError, FrameioNotConnected, get_frameio_client
from portal.lib.errors import NotFoundError
from portal.lib.logging import get_logger

log = get_logger("routes.destinations")
router = APIRouter(prefix="/destinations", tags=["destinations"])


class FrameioConfigIn(BaseModel):
    type: Literal["frameio"] = "frameio"
    account_id: str = Field(min_length=1)
    workspace_id: str = Field(min_length=1)
    project_id: str = Field(min_length=1)
    folder_id: str = Field(min_length=1)


class DestinationCreate(BaseModel):
    display_name: str = Field(min_length=1, max_length=255)
    config: FrameioConfigIn
    subtitle: str | None = None


class DestinationUpdate(BaseModel):
    display_name: str | None = Field(default=None, min_length=1, max_length=255)
    subtitle: str | None = None


class DestinationOut(BaseModel):
    id: str
    display_name: str
    config: dict[str, Any]
    subtitle: str | None
    created_at: str
    updated_at: str


def _to_out(dest: Destination) -> DestinationOut:
    return DestinationOut(
        id=str(dest.id),
        display_name=dest.display_name,
        config=dest.config,
        subtitle=dest.subtitle,
        created_at=dest.created_at.isoformat(),
        updated_at=dest.updated_at.isoformat(),
    )


async def _get_or_404(db: AsyncSession, dest_id: uuid.UUID) -> Destination:
    dest = await db.get(Destination, dest_id)
    if dest is None:
        raise NotFoundError("Destination not found")
    return dest


@router.get("", response_model=list[DestinationOut])
async def list_destinations(
    _: User = Depends(require_admin), db: AsyncSession = Depends(get_session)
) -> list[DestinationOut]:
    result = await db.execute(select(Destination).order_by(Destination.created_at.desc()))
    return [_to_out(d) for d in result.scalars().all()]


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

    config = body.config.model_dump()
    config["folder_name"] = folder.name

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
    await db.refresh(dest)
    log.info("destination.created", destination_id=str(dest.id))
    return _to_out(dest)


@router.get("/{dest_id}", response_model=DestinationOut)
async def get_destination(
    dest_id: uuid.UUID,
    _: User = Depends(require_admin),
    db: AsyncSession = Depends(get_session),
) -> DestinationOut:
    return _to_out(await _get_or_404(db, dest_id))


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
    await db.refresh(dest)
    return _to_out(dest)


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
