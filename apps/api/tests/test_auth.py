from httpx import AsyncClient


async def test_login_success_and_me(client: AsyncClient) -> None:
    resp = await client.post(
        "/api/auth/login",
        json={"email": "admin@example.com", "password": "test-password"},
    )
    assert resp.status_code == 200
    assert resp.json()["email"] == "admin@example.com"

    me = await client.get("/api/auth/me")
    assert me.status_code == 200
    assert me.json()["email"] == "admin@example.com"


async def test_login_wrong_password_rejected(client: AsyncClient) -> None:
    resp = await client.post(
        "/api/auth/login",
        json={"email": "admin@example.com", "password": "nope"},
    )
    assert resp.status_code == 401


async def test_me_requires_auth(client: AsyncClient) -> None:
    resp = await client.get("/api/auth/me")
    assert resp.status_code == 401


async def test_logout_clears_session(client: AsyncClient) -> None:
    await client.post(
        "/api/auth/login",
        json={"email": "admin@example.com", "password": "test-password"},
    )
    assert (await client.get("/api/auth/me")).status_code == 200
    await client.post("/api/auth/logout")
    assert (await client.get("/api/auth/me")).status_code == 401
