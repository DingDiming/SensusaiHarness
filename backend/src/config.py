"""SensusAI Harness — Configuration."""

from __future__ import annotations

import os
from dataclasses import dataclass, field


@dataclass
class Settings:
    database_url: str = field(default_factory=lambda: os.getenv("DATABASE_URL", "data/sensusai.db"))
    jwt_secret: str = field(default_factory=lambda: os.getenv("JWT_SECRET", "dev-secret-change-in-prod"))
    jwt_expire_hours: int = 24
    host: str = "0.0.0.0"
    port: int = 8000
    core_url: str = field(default_factory=lambda: os.getenv("CORE_URL", "http://127.0.0.1:4000"))
    openai_api_key: str = field(default_factory=lambda: os.getenv("OPENAI_API_KEY", ""))
    default_model: str = field(default_factory=lambda: os.getenv("DEFAULT_MODEL", "gpt-5.4-mini"))
    use_codex: bool = field(default_factory=lambda: os.getenv("USE_CODEX", "true").lower() in ("1", "true", "yes"))
    codex_home: str = field(default_factory=lambda: os.getenv("CODEX_HOME", os.path.expanduser("~/.codex")))
    debug: bool = field(default_factory=lambda: os.getenv("DEBUG", "").lower() in ("1", "true"))
    bootstrap_admin_username: str = field(default_factory=lambda: os.getenv("BOOTSTRAP_ADMIN_USERNAME", ""))
    bootstrap_admin_password: str = field(default_factory=lambda: os.getenv("BOOTSTRAP_ADMIN_PASSWORD", ""))


settings = Settings()
