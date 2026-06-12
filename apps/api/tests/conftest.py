import os

# Test settings must be set before any portal module reads them.
os.environ.setdefault(
    "DATABASE_URL", "postgresql+asyncpg://portal:portal@localhost:55432/portal"
)
os.environ.setdefault("REDIS_URL", "redis://localhost:56379/0")
os.environ.setdefault("SESSION_SECRET", "test-session-secret")
os.environ.setdefault("TOKEN_ENCRYPTION_KEY", "test-token-encryption-key")
os.environ.setdefault("ADMIN_EMAIL", "admin@example.com")
os.environ.setdefault("ADMIN_PASSWORD", "test-password")
os.environ.setdefault("SESSION_COOKIE_SECURE", "false")
os.environ.setdefault("RATE_LIMIT_ENABLED", "false")

from collections.abc import AsyncIterator  # noqa: E402

import pytest_asyncio  # noqa: E402
from httpx import ASGITransport, AsyncClient  # noqa: E402

from portal.db.base import Base  # noqa: E402
from portal.db.session import get_engine, get_sessionmaker  # noqa: E402
from portal.main import app  # noqa: E402
from portal.seed import seed_admin  # noqa: E402


@pytest_asyncio.fixture(scope="session", autouse=True)
async def _schema() -> AsyncIterator[None]:
    engine = get_engine()
    async with engine.begin() as conn:
        await conn.run_sync(Base.metadata.drop_all)
        await conn.run_sync(Base.metadata.create_all)
    await seed_admin()
    yield
    async with engine.begin() as conn:
        await conn.run_sync(Base.metadata.drop_all)


@pytest_asyncio.fixture
async def client() -> AsyncIterator[AsyncClient]:
    transport = ASGITransport(app=app)
    async with AsyncClient(transport=transport, base_url="http://test") as ac:
        yield ac


@pytest_asyncio.fixture(autouse=True)
async def _reset_sessionmaker() -> None:
    # Ensure the sessionmaker is initialised against the test engine.
    get_sessionmaker()
