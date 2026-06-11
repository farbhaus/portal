"""Admin CRUD for sync rules.

A sync rule watches a Frame.io folder and mirrors new files to a local destination path. Enabling
a rule creates a Frame.io webhook subscription (storing its signing secret + id on the rule);
disabling or deleting tears it down. The download work happens in the ARQ worker; this module just
manages rule config and the subscription, and exposes the per-rule job log + a manual run.
"""

import uuid
from typing import Any

from fastapi import APIRouter, Depends, HTTPException
from pydantic import BaseModel, Field
from sqlalchemy import func, select
from sqlalchemy.ext.asyncio import AsyncSession

from portal.auth.session import require_admin
from portal.db.models import AuditLog, SyncJob, SyncRule, User
from portal.db.session import get_session
from portal.frameio.client import (
    FrameioError,
    FrameioNotConnected,
    FrameioRateLimited,
    get_frameio_client,
)
from portal.lib.config import get_settings
from portal.lib.errors import NotFoundError, PortalError
from portal.lib.logging import get_logger
from portal.storage.base import DestinationConfig
from portal.storage.frameio_backend import FrameioStorageBackend
from portal.sync.queue import enqueue_reconcile_rule

log = get_logger("routes.sync_rules")
router = APIRouter(prefix="/sync-rules", tags=["sync-rules"])

_CONFLICT_POLICIES = {"skip", "overwrite", "rename_suffix"}


class BadRequestError(PortalError):
    status_code = 400
    public_message = "Bad request"


class SyncRuleCreate(BaseModel):
    name: str = Field(min_length=1, max_length=255)
    source: dict[str, Any]
    destination_path: str = Field(min_length=1)
    conflict_policy: str = "rename_suffix"
    path_template: str | None = None
    enabled: bool = True


class SyncRuleUpdate(BaseModel):
    name: str | None = Field(default=None, min_length=1, max_length=255)
    destination_path: str | None = Field(default=None, min_length=1)
    conflict_policy: str | None = None
    path_template: str | None = None
    enabled: bool | None = None


class SyncRuleOut(BaseModel):
    id: str
    name: str
    source: dict[str, Any]
    destination_path: str
    conflict_policy: str
    path_template: str | None
    enabled: bool
    subscription_active: bool
    created_at: str


class SyncJobOut(BaseModel):
    id: str
    frameio_file_id: str
    status: str
    bytes: int | None
    sha256: str | None
    error: str | None
    retry_count: int
    started_at: str | None
    completed_at: str | None
    created_at: str


def _to_out(rule: SyncRule) -> SyncRuleOut:
    return SyncRuleOut(
        id=str(rule.id),
        name=rule.name,
        source=rule.source_config,
        destination_path=rule.destination_path,
        conflict_policy=rule.conflict_policy,
        path_template=rule.path_template,
        enabled=rule.enabled,
        subscription_active=rule.frameio_webhook_id is not None,
        created_at=rule.created_at.isoformat(),
    )


def _destination(rule: SyncRule) -> DestinationConfig:
    return DestinationConfig(type="frameio", data=dict(rule.source_config))


def _callback_url(rule_id: uuid.UUID) -> str:
    return f"{get_settings().base_url.rstrip('/')}/api/webhooks/frameio/{rule_id}"


async def _validate_and_enrich_source(source: dict[str, Any]) -> dict[str, Any]:
    """Resolve project/folder names (for path templating) from the source.

    The admin picker already knows both names, so it sends them and we make zero Frame.io calls
    here — important because the folder endpoints are tightly rate-limited and browsing the picker
    can exhaust the window, which previously turned creation into a confusing 502. We only fall
    back to API lookups (with a clear rate-limit message) when a name is missing, e.g. for
    non-UI/API callers.
    """
    if source.get("type") != "frameio":
        raise BadRequestError("source.type must be 'frameio'")
    try:
        account_id = str(source["account_id"])
        workspace_id = str(source["workspace_id"])
        project_id = str(source["project_id"])
        folder_id = str(source["folder_id"])
    except KeyError as exc:
        raise BadRequestError(f"source missing {exc}") from exc

    folder_name = str(source.get("folder_name") or "")
    project_name = str(source.get("project_name") or "")

    if not folder_name or not project_name:
        client = get_frameio_client()
        try:
            if not folder_name:
                folder_name = (await client.get_folder(account_id, folder_id)).name
            if not project_name:
                projects = await client.list_projects(account_id, workspace_id)
                project_name = next((p.name for p in projects if p.id == project_id), "")
        except FrameioNotConnected as exc:
            raise HTTPException(status_code=409, detail="Frame.io is not connected") from exc
        except FrameioRateLimited as exc:
            raise HTTPException(
                status_code=503, detail="Frame.io is busy, try again in a moment"
            ) from exc
        except FrameioError as exc:
            raise HTTPException(status_code=502, detail="Could not validate the source") from exc

    return {
        "type": "frameio",
        "account_id": account_id,
        "workspace_id": workspace_id,
        "project_id": project_id,
        "folder_id": folder_id,
        "recursive": bool(source.get("recursive", True)),
        "folder_name": folder_name,
        "project_name": project_name,
    }


async def _subscribe(rule: SyncRule) -> None:
    backend = FrameioStorageBackend(client=get_frameio_client())
    try:
        sub = await backend.subscribe_to_changes(
            _destination(rule), callback_url=_callback_url(rule.id)
        )
    except FrameioNotConnected as exc:
        raise HTTPException(status_code=409, detail="Frame.io is not connected") from exc
    except FrameioRateLimited as exc:
        raise HTTPException(
            status_code=503, detail="Frame.io is busy, try again in a moment"
        ) from exc
    except FrameioError as exc:
        raise HTTPException(status_code=502, detail="Could not create the webhook") from exc
    rule.frameio_webhook_id = sub.id
    rule.webhook_secret = sub.backend_data.get("secret")


async def _unsubscribe(rule: SyncRule) -> None:
    if rule.frameio_webhook_id is None:
        return
    backend = FrameioStorageBackend(client=get_frameio_client())
    try:
        await backend.unsubscribe_from_changes(_destination(rule), rule.frameio_webhook_id)
    except FrameioError as exc:
        # Don't block disable/delete on a failed teardown; the stale subscription just won't match
        # a rule on delivery (404), and the user can prune it in Frame.io.
        log.warning("sync.unsubscribe_failed", rule_id=str(rule.id), error=str(exc))
    rule.frameio_webhook_id = None
    rule.webhook_secret = None


async def _get_or_404(db: AsyncSession, rule_id: uuid.UUID) -> SyncRule:
    rule = await db.get(SyncRule, rule_id)
    if rule is None:
        raise NotFoundError("Sync rule not found")
    return rule


@router.get("", response_model=list[SyncRuleOut])
async def list_rules(
    _: User = Depends(require_admin), db: AsyncSession = Depends(get_session)
) -> list[SyncRuleOut]:
    result = await db.execute(select(SyncRule).order_by(SyncRule.created_at.desc()))
    return [_to_out(r) for r in result.scalars().all()]


@router.post("", response_model=SyncRuleOut, status_code=201)
async def create_rule(
    body: SyncRuleCreate,
    user: User = Depends(require_admin),
    db: AsyncSession = Depends(get_session),
) -> SyncRuleOut:
    if body.conflict_policy not in _CONFLICT_POLICIES:
        raise BadRequestError(f"conflict_policy must be one of {sorted(_CONFLICT_POLICIES)}")
    source = await _validate_and_enrich_source(body.source)

    rule = SyncRule(
        name=body.name,
        source_config=source,
        destination_path=body.destination_path,
        conflict_policy=body.conflict_policy,
        path_template=body.path_template,
        enabled=body.enabled,
    )
    db.add(rule)
    await db.flush()  # assign rule.id for the webhook callback URL
    if rule.enabled:
        await _subscribe(rule)
    db.add(
        AuditLog(
            user_id=user.id, action="sync_rule.created", detail={"sync_rule_id": str(rule.id)}
        )
    )
    await db.commit()
    await db.refresh(rule)
    log.info("sync_rule.created", sync_rule_id=str(rule.id), enabled=rule.enabled)
    return _to_out(rule)


@router.get("/{rule_id}", response_model=SyncRuleOut)
async def get_rule(
    rule_id: uuid.UUID,
    _: User = Depends(require_admin),
    db: AsyncSession = Depends(get_session),
) -> SyncRuleOut:
    return _to_out(await _get_or_404(db, rule_id))


@router.patch("/{rule_id}", response_model=SyncRuleOut)
async def update_rule(
    rule_id: uuid.UUID,
    body: SyncRuleUpdate,
    _: User = Depends(require_admin),
    db: AsyncSession = Depends(get_session),
) -> SyncRuleOut:
    rule = await _get_or_404(db, rule_id)
    fields = body.model_dump(exclude_unset=True)

    if "conflict_policy" in fields and fields["conflict_policy"] not in _CONFLICT_POLICIES:
        raise BadRequestError(f"conflict_policy must be one of {sorted(_CONFLICT_POLICIES)}")

    if "enabled" in fields and fields["enabled"] != rule.enabled:
        if fields["enabled"]:
            await _subscribe(rule)
        else:
            await _unsubscribe(rule)

    for key, value in fields.items():
        setattr(rule, key, value)
    await db.commit()
    await db.refresh(rule)
    return _to_out(rule)


@router.delete("/{rule_id}", status_code=204)
async def delete_rule(
    rule_id: uuid.UUID,
    user: User = Depends(require_admin),
    db: AsyncSession = Depends(get_session),
) -> None:
    rule = await _get_or_404(db, rule_id)
    await _unsubscribe(rule)
    await db.delete(rule)
    db.add(
        AuditLog(
            user_id=user.id, action="sync_rule.deleted", detail={"sync_rule_id": str(rule_id)}
        )
    )
    await db.commit()


@router.post("/{rule_id}/run")
async def run_rule(
    rule_id: uuid.UUID,
    _: User = Depends(require_admin),
    db: AsyncSession = Depends(get_session),
) -> dict[str, bool]:
    rule = await _get_or_404(db, rule_id)
    await enqueue_reconcile_rule(rule.id)
    log.info("sync_rule.manual_run", sync_rule_id=str(rule.id))
    return {"ok": True}


@router.get("/{rule_id}/jobs", response_model=list[SyncJobOut])
async def list_jobs(
    rule_id: uuid.UUID,
    _: User = Depends(require_admin),
    db: AsyncSession = Depends(get_session),
    limit: int = 100,
) -> list[SyncJobOut]:
    await _get_or_404(db, rule_id)
    limit = max(1, min(limit, 500))
    result = await db.execute(
        select(SyncJob)
        .where(SyncJob.sync_rule_id == rule_id)
        .order_by(SyncJob.created_at.desc())
        .limit(limit)
    )
    return [
        SyncJobOut(
            id=str(j.id),
            frameio_file_id=j.frameio_file_id,
            status=j.status,
            bytes=j.bytes,
            sha256=j.sha256,
            error=j.error,
            retry_count=j.retry_count,
            started_at=j.started_at.isoformat() if j.started_at else None,
            completed_at=j.completed_at.isoformat() if j.completed_at else None,
            created_at=j.created_at.isoformat(),
        )
        for j in result.scalars().all()
    ]


class JobStats(BaseModel):
    counts: dict[str, int]


@router.get("/{rule_id}/stats", response_model=JobStats)
async def job_stats(
    rule_id: uuid.UUID,
    _: User = Depends(require_admin),
    db: AsyncSession = Depends(get_session),
) -> JobStats:
    await _get_or_404(db, rule_id)
    result = await db.execute(
        select(SyncJob.status, func.count())
        .where(SyncJob.sync_rule_id == rule_id)
        .group_by(SyncJob.status)
    )
    return JobStats(counts={status: count for status, count in result.all()})
