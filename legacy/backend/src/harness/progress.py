"""Progress tracker — records phase timings."""
from __future__ import annotations
from datetime import datetime, timezone
from uuid import uuid4
from ..db import get_db

async def start_phase(run_id: str, sprint: int, phase: str) -> str:
    db = await get_db()
    snapshot_id = str(uuid4())
    now = datetime.now(timezone.utc).isoformat()
    await db.execute("INSERT INTO progress_snapshots (snapshot_id, run_id, sprint, phase, started_at) VALUES (?, ?, ?, ?, ?)",
        (snapshot_id, run_id, sprint, phase, now))
    await db.commit()
    return snapshot_id

async def end_phase(snapshot_id: str, outcome: str, tokens_used: int = 0) -> None:
    db = await get_db()
    now = datetime.now(timezone.utc).isoformat()
    await db.execute(
        "UPDATE progress_snapshots SET completed_at = ?, outcome = ?, tokens_used = ?, duration_seconds = CAST((julianday(?) - julianday(started_at)) * 86400 AS INTEGER) WHERE snapshot_id = ?",
        (now, outcome, tokens_used, now, snapshot_id))
    await db.commit()
