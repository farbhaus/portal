"""Public branding asset serving — the uploaded logo shown on /u and /d pages + login.

No auth: the public uploader/recipient pages and the login page are unauthenticated. Admin upload
lives in routes/security.py.
"""

from fastapi import APIRouter, Depends, HTTPException, Request
from fastapi.responses import Response
from sqlalchemy.ext.asyncio import AsyncSession

from portal.db.session import get_session
from portal.services.app_settings import get_app_settings

router = APIRouter(prefix="/branding", tags=["branding"])


@router.get("/logo")
async def get_logo(request: Request, db: AsyncSession = Depends(get_session)) -> Response:
    row = await get_app_settings(db)
    if row is None or not row.logo_png:
        raise HTTPException(status_code=404, detail="No logo set")
    etag = f'"{int(row.logo_updated_at.timestamp())}"' if row.logo_updated_at else None
    if etag and request.headers.get("if-none-match") == etag:
        return Response(status_code=304)
    headers = {"Cache-Control": "public, max-age=300"}
    if etag:
        headers["ETag"] = etag
    return Response(
        content=row.logo_png,
        media_type=row.logo_content_type or "image/png",
        headers=headers,
    )
