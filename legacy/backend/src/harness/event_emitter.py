"""Event emitter — pushes events to Rust Core SSE bridge + DB."""
from __future__ import annotations
import json
from datetime import datetime, timezone
import httpx
from ..config import settings
from ..db import get_db

async def emit_event(run_id: str, event_type: str, data: dict) -> None:
    db = await get_db()
    now = datetime.now(timezone.utc).isoformat()
    await db.execute("INSERT INTO run_events (run_id, event_type, data_json, created_at) VALUES (?, ?, ?, ?)",
        (run_id, event_type, json.dumps(data), now))
    await db.commit()
    try:
        async with httpx.AsyncClient() as client:
            await client.post(f"{settings.core_url}/internal/runs/{run_id}/events",
                json={"event_type": event_type, "data": data}, timeout=2.0)
    except Exception:
        pass
