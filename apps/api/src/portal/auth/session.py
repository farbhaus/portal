import uuid

from fastapi import Depends, Request
from sqlalchemy import select
from sqlalchemy.ext.asyncio import AsyncSession

from portal.db.models import User
from portal.db.session import get_session
from portal.lib.errors import AuthError

SESSION_USER_KEY = "user_id"


def login_session(request: Request, user: User) -> None:
    request.session[SESSION_USER_KEY] = str(user.id)


def clear_session(request: Request) -> None:
    request.session.clear()


async def get_current_user(
    request: Request, db: AsyncSession = Depends(get_session)
) -> User | None:
    raw_id = request.session.get(SESSION_USER_KEY)
    if not raw_id:
        return None
    try:
        user_id = uuid.UUID(raw_id)
    except ValueError:
        return None
    result = await db.execute(select(User).where(User.id == user_id, User.is_active.is_(True)))
    return result.scalar_one_or_none()


async def require_admin(user: User | None = Depends(get_current_user)) -> User:
    if user is None:
        raise AuthError()
    return user
