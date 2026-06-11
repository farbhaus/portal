"""Admin activity log — a read view over the append-only audit_log, plus dashboard counts."""

from typing import Any

from fastapi import APIRouter, Depends
from pydantic import BaseModel
from sqlalchemy import func, select
from sqlalchemy.ext.asyncio import AsyncSession

from portal.auth.session import require_admin
from portal.db.models import (
    AuditLog,
    Destination,
    DownloadLink,
    SyncRule,
    UploadLink,
    User,
)
from portal.db.session import get_session

router = APIRouter(prefix="/activity", tags=["activity"])


class ActivityOut(BaseModel):
    id: str
    action: str
    user_email: str | None
    detail: dict[str, Any] | None
    created_at: str


class SummaryOut(BaseModel):
    destinations: int
    upload_links: int
    download_links: int
    sync_rules: int


@router.get("", response_model=list[ActivityOut])
async def list_activity(
    _: User = Depends(require_admin),
    db: AsyncSession = Depends(get_session),
    action: str | None = None,
    limit: int = 100,
) -> list[ActivityOut]:
    limit = max(1, min(limit, 500))
    stmt = (
        select(AuditLog, User.email)
        .join(User, AuditLog.user_id == User.id, isouter=True)
        .order_by(AuditLog.created_at.desc())
        .limit(limit)
    )
    if action:
        stmt = stmt.where(AuditLog.action == action)
    result = await db.execute(stmt)
    return [
        ActivityOut(
            id=str(row.AuditLog.id),
            action=row.AuditLog.action,
            user_email=row.email,
            detail=row.AuditLog.detail,
            created_at=row.AuditLog.created_at.isoformat(),
        )
        for row in result.all()
    ]


@router.get("/actions", response_model=list[str])
async def list_actions(
    _: User = Depends(require_admin), db: AsyncSession = Depends(get_session)
) -> list[str]:
    """Distinct action names, for the filter dropdown."""
    result = await db.execute(select(AuditLog.action).distinct().order_by(AuditLog.action))
    return list(result.scalars().all())


@router.get("/summary", response_model=SummaryOut)
async def summary(
    _: User = Depends(require_admin), db: AsyncSession = Depends(get_session)
) -> SummaryOut:
    async def _count(model: type[Any]) -> int:
        return await db.scalar(select(func.count()).select_from(model)) or 0

    return SummaryOut(
        destinations=await _count(Destination),
        upload_links=await _count(UploadLink),
        download_links=await _count(DownloadLink),
        sync_rules=await _count(SyncRule),
    )
