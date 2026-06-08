from functools import lru_cache

from pydantic import Field
from pydantic_settings import BaseSettings, SettingsConfigDict


class Settings(BaseSettings):
    model_config = SettingsConfigDict(env_file=".env", env_file_encoding="utf-8", extra="ignore")

    # Database / cache
    database_url: str = Field(
        default="postgresql+asyncpg://portal:portal@portal_postgres:5432/portal"
    )
    redis_url: str = Field(default="redis://portal_redis:6379/0")

    # Ingress
    portal_port: int = Field(default=18080)
    base_url: str = Field(default="http://127.0.0.1:18080")

    # Secrets
    session_secret: str = Field(default="change-me-session-secret")
    token_encryption_key: str = Field(default="change-me-token-encryption-key")

    # Admin seed (single admin in v1)
    admin_email: str = Field(default="admin@example.com")
    admin_password: str = Field(default="change-me")

    # Session cookie
    session_cookie_secure: bool = Field(default=True)
    session_cookie_name: str = Field(default="portal_session")

    log_level: str = Field(default="INFO")


@lru_cache
def get_settings() -> Settings:
    return Settings()
