from httpx import AsyncClient

from portal.notify.email import (
    UploadedFile,
    _format_bytes,
    _format_duration,
    render_upload_completion,
)

CREDS = {"email": "admin@example.com", "password": "test-password"}


def test_format_bytes() -> None:
    assert _format_bytes(512) == "512 B"
    assert _format_bytes(2048) == "2.0 KB"
    assert _format_bytes(5 * 1024**3) == "5.0 GB"


def test_format_duration() -> None:
    assert _format_duration(45) == "45s"
    assert _format_duration(90) == "1m 30s"
    assert _format_duration(3700) == "1h 1m"


def test_render_upload_completion() -> None:
    subject, body = render_upload_completion(
        link_name="DIT Drop",
        uploader_name="Berlin DIT",
        uploader_email="dit@example.com",
        uploader_message="200GB of dailies",
        files=[UploadedFile("A-CAM/clip001.mov", 1024), UploadedFile("audio.wav", None)],
        total_bytes=1024,
        duration_seconds=125,
    )
    assert "DIT Drop" in subject and "2 files" in subject
    assert "Berlin DIT" in body
    assert "dit@example.com" in body
    assert "200GB of dailies" in body
    assert "A-CAM/clip001.mov" in body and "1.0 KB" in body
    assert "audio.wav" in body and "—" in body  # unknown size
    assert "2m 5s" in body


# ── settings endpoints (email not configured in the test env) ─────────────────


async def _login(client: AsyncClient) -> None:
    assert (await client.post("/api/auth/login", json=CREDS)).status_code == 200


async def test_email_status_not_configured(client: AsyncClient) -> None:
    await _login(client)
    resp = await client.get("/api/settings/email")
    assert resp.status_code == 200
    assert resp.json()["configured"] is False


async def test_email_test_requires_config(client: AsyncClient) -> None:
    await _login(client)
    resp = await client.post("/api/settings/email/test")
    assert resp.status_code == 400


async def test_settings_require_auth(client: AsyncClient) -> None:
    assert (await client.get("/api/settings/email")).status_code == 401
    assert (await client.post("/api/settings/email/test")).status_code == 401
