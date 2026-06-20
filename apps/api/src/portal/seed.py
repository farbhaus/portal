from sqlalchemy import func, select

from portal.auth.passwords import hash_password
from portal.db.models import User
from portal.db.session import get_sessionmaker
from portal.lib.config import get_settings
from portal.lib.logging import get_logger
from portal.services.runtime_config import bootstrap_config

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


async def seed_config() -> None:
    """Import any legacy .env email/Frame.io config into app_settings once. Idempotent."""
    async with get_sessionmaker()() as db:
        await bootstrap_config(db)


def main() -> None:
    """One-shot entrypoint: run seeding once before the API workers start."""
    import asyncio

    from portal.lib.config import get_settings
    from portal.lib.logging import configure_logging

    configure_logging(get_settings().log_level)

    async def _run() -> None:
        await seed_admin()
        await seed_config()

    asyncio.run(_run())


if __name__ == "__main__":
    main()
