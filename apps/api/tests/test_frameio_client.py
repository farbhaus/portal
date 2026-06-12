import types
from datetime import UTC, datetime, timedelta

import pytest
import pytest_asyncio
from httpx import AsyncClient
from sqlalchemy import delete, select

from portal.db.models import FrameioConnection, User
from portal.db.session import get_sessionmaker
from portal.frameio.client import (
    ChunkUrl,
    FileItem,
    FrameioClient,
    FrameioError,
    FrameioNotConnected,
    PickerItem,
    ProjectItem,
    UploadStatus,
    UploadTarget,
    _provide_token,
)
from portal.lib.config import get_settings
from portal.lib.crypto import TokenCipher
from portal.routes import frameio as frameio_routes
from portal.storage.base import DestinationConfig, RemoteFile, StorageBackend, UploadNotReady
from portal.storage.frameio_backend import FrameioStorageBackend

CREDS = {"email": "admin@example.com", "password": "test-password"}


def ns(**kw: object) -> types.SimpleNamespace:
    return types.SimpleNamespace(**kw)


# ── fake SDK ──────────────────────────────────────────────────────────────────


class _Pager:
    def __init__(self, items: list[object]) -> None:
        self._items = items

    def __aiter__(self) -> "_Pager":
        self._it = iter(self._items)
        return self

    async def __anext__(self) -> object:
        try:
            return next(self._it)
        except StopIteration:
            raise StopAsyncIteration from None


class _IndexResource:
    def __init__(self, items: list[object]) -> None:
        self._items = items

    async def index(self, *args: object, **kwargs: object) -> _Pager:
        return _Pager(self._items)


class _FoldersResource:
    """Serves paginated `list` (subfolders) and `index` (assets) responses.

    `pages` is a list of (data, next_cursor); `after` is the index of the page to return.
    """

    def __init__(self, list_pages: list[tuple], index_pages: list[tuple]) -> None:
        self._list_pages = list_pages
        self._index_pages = index_pages

    async def list(
        self, account_id: str, folder_id: str, *, after: str | None = None, **kwargs: object
    ) -> object:
        data, nxt = self._list_pages[0 if after is None else int(after)]
        return ns(data=data, links=ns(next=nxt))

    async def index(
        self, account_id: str, folder_id: str, *, after: str | None = None, **kwargs: object
    ) -> object:
        data, nxt = self._index_pages[0 if after is None else int(after)]
        return ns(data=data, links=ns(next=nxt))


def _fake_sdk() -> types.SimpleNamespace:
    return ns(
        accounts=_IndexResource([ns(id="a1", display_name="Acct One")]),
        workspaces=_IndexResource([ns(id="w1", name="Workspace")]),
        projects=_IndexResource([ns(id="p1", name="Project", root_folder_id="rf1")]),
        folders=_FoldersResource(
            list_pages=[
                ([ns(id="f1", name="F1"), ns(id="f2", name="F2")], "1"),
                ([ns(id="f3", name="F3")], None),
            ],
            index_pages=[
                ([ns(id="x1", name="clip.mov", file_size=123, media_type="video")], None),
            ],
        ),
    )


@pytest_asyncio.fixture
async def fake_client():
    c = FrameioClient()
    c._client = _fake_sdk()  # type: ignore[assignment]
    yield c
    await c.aclose()


# ── client: picker mapping & pagination ───────────────────────────────────────


async def test_list_accounts(fake_client: FrameioClient) -> None:
    assert await fake_client.list_accounts() == [PickerItem(id="a1", name="Acct One")]


async def test_list_workspaces(fake_client: FrameioClient) -> None:
    assert await fake_client.list_workspaces("a1") == [PickerItem(id="w1", name="Workspace")]


async def test_list_projects_includes_root_folder(fake_client: FrameioClient) -> None:
    assert await fake_client.list_projects("a1", "w1") == [
        ProjectItem(id="p1", name="Project", root_folder_id="rf1")
    ]


async def test_list_subfolders_paginates(fake_client: FrameioClient) -> None:
    items = await fake_client.list_subfolders("a1", "rf1")
    assert [i.id for i in items] == ["f1", "f2", "f3"]


async def test_list_files(fake_client: FrameioClient) -> None:
    assert await fake_client.list_files("a1", "rf1") == [
        FileItem(id="x1", name="clip.mov", file_size=123, media_type="video")
    ]


async def test_error_translation() -> None:
    class _Boom:
        async def index(self, *a: object, **k: object) -> object:
            raise RuntimeError("boom")

    c = FrameioClient()
    c._client = ns(accounts=_Boom())  # type: ignore[assignment]
    with pytest.raises(FrameioError):
        await c.list_accounts()
    await c.aclose()


# ── storage interface contract ────────────────────────────────────────────────


def test_frameio_backend_is_storage_backend() -> None:
    assert issubclass(FrameioStorageBackend, StorageBackend)
    assert FrameioStorageBackend.backend_type == "frameio"


async def test_backend_list_files(fake_client: FrameioClient) -> None:
    backend = FrameioStorageBackend(client=fake_client)
    dest = DestinationConfig(type="frameio", data={"account_id": "a1", "folder_id": "rf1"})
    assert await backend.list_files_in_destination(dest) == [
        RemoteFile(id="x1", name="clip.mov", size=123, media_type="video")
    ]


async def test_backend_missing_config_raises(fake_client: FrameioClient) -> None:
    backend = FrameioStorageBackend(client=fake_client)
    dest = DestinationConfig(type="frameio", data={})
    with pytest.raises(KeyError):
        await backend.list_files_in_destination(dest)


class _StubUploadClient:
    """Stub FrameioClient for backend upload tests (no real SDK/network)."""

    def __init__(self, status: UploadStatus | None = None) -> None:
        self._status = status or UploadStatus(complete=True, failed=False)
        self.ensured: list[tuple[str, list[str]]] = []

    async def ensure_folder_path(self, account_id: str, root: str, parts: list[str]) -> str:
        self.ensured.append((root, parts))
        return "leaf-folder"

    async def create_local_upload(
        self, account_id: str, folder_id: str, *, name: str, file_size: int
    ) -> UploadTarget:
        return UploadTarget(
            file_id="file-1",
            media_type="video/quicktime",
            chunks=[ChunkUrl(size=5, url="https://s3/part1"), ChunkUrl(size=3, url="https://s3/part2")],
        )

    async def get_upload_status(self, account_id: str, file_id: str) -> UploadStatus:
        return self._status


async def test_backend_create_upload_session_and_credentials() -> None:
    stub = _StubUploadClient()
    backend = FrameioStorageBackend(client=stub)  # type: ignore[arg-type]
    dest = DestinationConfig(type="frameio", data={"account_id": "a1", "folder_id": "root"})

    session = await backend.create_upload_session(dest, filename="A-CAM/clip.mov", size=8)
    # Subfolder path mirrored into Frame.io.
    assert stub.ensured == [("root", ["A-CAM"])]
    assert session.backend_data["file_id"] == "file-1"

    creds = await backend.get_upload_credentials(session)
    assert [c.part for c in creds.chunks] == [1, 2]
    assert [c.url for c in creds.chunks] == ["https://s3/part1", "https://s3/part2"]
    # Both signed headers present.
    assert creds.headers == {"x-amz-acl": "private", "Content-Type": "video/quicktime"}


async def test_backend_complete_upload_states() -> None:
    dest = DestinationConfig(type="frameio", data={"account_id": "a1", "folder_id": "root"})
    from portal.storage.base import UploadSession as StorageSession

    sess = StorageSession(
        id="file-1", destination=dest, filename="x", size=1, backend_data={"file_id": "file-1"}
    )

    ready = FrameioStorageBackend(client=_StubUploadClient(UploadStatus(True, False)))  # type: ignore[arg-type]
    assert (await ready.complete_upload(sess)).id == "file-1"

    pending = FrameioStorageBackend(client=_StubUploadClient(UploadStatus(False, False)))  # type: ignore[arg-type]
    with pytest.raises(UploadNotReady):
        await pending.complete_upload(sess)

    failed = FrameioStorageBackend(client=_StubUploadClient(UploadStatus(False, True)))  # type: ignore[arg-type]
    with pytest.raises(FrameioError):
        await failed.complete_upload(sess)


async def test_backend_remaining_methods_deferred(fake_client: FrameioClient) -> None:
    backend = FrameioStorageBackend(client=fake_client)
    dest = DestinationConfig(type="frameio", data={"account_id": "a1", "folder_id": "rf1"})
    # delete_object lands with the phase 2 transit-and-delete lifecycle; still a stub.
    with pytest.raises(NotImplementedError):
        await backend.delete_object(dest, "file-1")


# ── picker routes ─────────────────────────────────────────────────────────────


class _StubClient:
    async def list_accounts(self) -> list[PickerItem]:
        return [PickerItem(id="a1", name="Acct One")]

    async def list_workspaces(self, account_id: str) -> list[PickerItem]:
        return [PickerItem(id="w1", name="Workspace")]

    async def list_projects(self, account_id: str, workspace_id: str) -> list[ProjectItem]:
        return [ProjectItem(id="p1", name="Project", root_folder_id="rf1")]

    async def list_subfolders(self, account_id: str, folder_id: str) -> list[PickerItem]:
        return [PickerItem(id="f1", name="F1")]

    async def create_folder(
        self, account_id: str, parent_folder_id: str, name: str
    ) -> PickerItem:
        return PickerItem(id="new1", name=name)


async def _login(client: AsyncClient) -> None:
    assert (await client.post("/api/auth/login", json=CREDS)).status_code == 200


async def test_accounts_route(client: AsyncClient, monkeypatch) -> None:
    await _login(client)
    monkeypatch.setattr(frameio_routes, "get_frameio_client", lambda: _StubClient())
    resp = await client.get("/api/frameio/accounts")
    assert resp.status_code == 200
    assert resp.json() == [{"id": "a1", "name": "Acct One"}]


async def test_projects_route_includes_root_folder(client: AsyncClient, monkeypatch) -> None:
    await _login(client)
    monkeypatch.setattr(frameio_routes, "get_frameio_client", lambda: _StubClient())
    resp = await client.get(
        "/api/frameio/projects", params={"account_id": "a1", "workspace_id": "w1"}
    )
    assert resp.status_code == 200
    assert resp.json() == [{"id": "p1", "name": "Project", "root_folder_id": "rf1"}]


async def test_picker_route_not_connected_returns_409(client: AsyncClient, monkeypatch) -> None:
    class _NC:
        async def list_accounts(self) -> list[PickerItem]:
            raise FrameioNotConnected("nope")

    await _login(client)
    monkeypatch.setattr(frameio_routes, "get_frameio_client", lambda: _NC())
    assert (await client.get("/api/frameio/accounts")).status_code == 409


async def test_picker_route_upstream_failure_returns_502(client: AsyncClient, monkeypatch) -> None:
    class _Err:
        async def list_accounts(self) -> list[PickerItem]:
            raise FrameioError("boom")

    await _login(client)
    monkeypatch.setattr(frameio_routes, "get_frameio_client", lambda: _Err())
    assert (await client.get("/api/frameio/accounts")).status_code == 502


async def test_create_folder_route(client: AsyncClient, monkeypatch) -> None:
    await _login(client)
    monkeypatch.setattr(frameio_routes, "get_frameio_client", lambda: _StubClient())
    resp = await client.post(
        "/api/frameio/folders",
        json={"account_id": "a1", "parent_folder_id": "rf1", "name": "Dailies"},
    )
    assert resp.status_code == 200
    assert resp.json() == {"id": "new1", "name": "Dailies"}


async def test_create_folder_requires_name(client: AsyncClient, monkeypatch) -> None:
    await _login(client)
    monkeypatch.setattr(frameio_routes, "get_frameio_client", lambda: _StubClient())
    resp = await client.post(
        "/api/frameio/folders",
        json={"account_id": "a1", "parent_folder_id": "rf1", "name": "  "},
    )
    assert resp.status_code == 422


async def test_picker_route_requires_auth(client: AsyncClient) -> None:
    assert (await client.get("/api/frameio/accounts")).status_code == 401


# ── token provider ────────────────────────────────────────────────────────────


@pytest_asyncio.fixture
async def _clear_connections() -> None:
    async with get_sessionmaker()() as db:
        await db.execute(delete(FrameioConnection))
        await db.commit()


async def test_provide_token_no_connection(_clear_connections: None) -> None:
    with pytest.raises(FrameioNotConnected):
        await _provide_token()


async def test_provide_token_returns_valid_token(_clear_connections: None) -> None:
    cipher = TokenCipher(get_settings().token_encryption_key)
    async with get_sessionmaker()() as db:
        user = (await db.execute(select(User))).scalars().first()
        assert user is not None
        db.add(
            FrameioConnection(
                user_id=user.id,
                access_token_encrypted=cipher.encrypt("the-access"),
                refresh_token_encrypted=cipher.encrypt("the-refresh"),
                token_expires_at=datetime.now(UTC) + timedelta(hours=2),
            )
        )
        await db.commit()

    assert await _provide_token() == "the-access"
