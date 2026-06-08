from sqlalchemy import func, select

from portal.auth.passwords import hash_password
from portal.db.models import User
from portal.db.session import get_sessionmaker
from portal.lib.config import get_settings
from portal.lib.logging import get_logger

log = get_logger("seed")


async def seed_admin() -> None:
    """Create the single admin from env if no users exist. Idempotent."""
    settings = get_settings()
    async with get_sessionmaker()() as db:
        count = await db.scalar(select(func.count()).select_from(User))
        if count:
            return
        admin = User(
            email=settings.admin_email,
            password_hash=hash_password(settings.admin_password),
        )
        db.add(admin)
        await db.commit()
        log.info("seed.admin_created", email=settings.admin_email)
