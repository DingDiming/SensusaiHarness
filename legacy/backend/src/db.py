"""SensusAI Harness — Database layer."""

from __future__ import annotations

import aiosqlite
from pathlib import Path

from .config import settings

_db: aiosqlite.Connection | None = None

MIGRATIONS_DIR = Path(__file__).resolve().parent.parent / "migrations"


async def get_db() -> aiosqlite.Connection:
    global _db
    if _db is None:
        db_path = Path(settings.database_url)
        db_path.parent.mkdir(parents=True, exist_ok=True)
        _db = await aiosqlite.connect(str(db_path))
        _db.row_factory = aiosqlite.Row
        await _db.execute("PRAGMA journal_mode = WAL")
        await _db.execute("PRAGMA foreign_keys = ON")
        await _db.execute("PRAGMA busy_timeout = 5000")
    return _db


async def run_migrations() -> None:
    db = await get_db()
    migration_file = MIGRATIONS_DIR / "001_initial.sql"
    if migration_file.exists():
        sql = migration_file.read_text()
        await db.executescript(sql)
    await db.commit()


async def close_db() -> None:
    global _db
    if _db is not None:
        await _db.close()
        _db = None
