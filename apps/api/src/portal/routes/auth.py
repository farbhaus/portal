from fastapi import APIRouter, Depends, Request
from pydantic import BaseModel, EmailStr
from sqlalchemy import select
from sqlalchemy.ext.asyncio import AsyncSession

from portal.auth.passwords import verify_password
from portal.auth.session import clear_session, login_session, require_admin
from portal.db.models import User
from portal.db.session import get_session
from portal.lib.errors import AuthError

router = APIRouter(prefix="/auth", tags=["auth"])


class LoginRequest(BaseModel):
    email: EmailStr
    password: str


class UserOut(BaseModel):
    id: str
    email: str


@router.post("/login", response_model=UserOut)
async def login(
    body: LoginRequest, request: Request, db: AsyncSession = Depends(get_session)
) -> UserOut:
    result = await db.execute(select(User).where(User.email == body.email))
    user = result.scalar_one_or_none()
    if user is None or not user.is_active or not verify_password(body.password, user.password_hash):
        raise AuthError("Invalid email or password")
    login_session(request, user)
    return UserOut(id=str(user.id), email=user.email)


@router.post("/logout")
async def logout(request: Request) -> dict[str, bool]:
    clear_session(request)
    return {"ok": True}


@router.get("/me", response_model=UserOut)
async def me(user: User = Depends(require_admin)) -> UserOut:
    return UserOut(id=str(user.id), email=user.email)
