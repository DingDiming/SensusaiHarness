"""Thread routes."""
from __future__ import annotations
from datetime import datetime, timezone
from uuid import uuid4
from fastapi import APIRouter, Depends, HTTPException
from ..db import get_db
from ..deps import get_current_user
from ..schemas import CreateThread, ThreadResponse

router = APIRouter(prefix="/threads", tags=["threads"])

@router.post("", response_model=ThreadResponse, status_code=201)
async def create_thread(body: CreateThread, user: dict = Depends(get_current_user)):
    db = await get_db()
    now = datetime.now(timezone.utc).isoformat()
    thread_id = str(uuid4())
    await db.execute(
        "INSERT INTO threads (thread_id, user_id, title, default_mode, status, created_at, updated_at) VALUES (?, ?, ?, ?, 'active', ?, ?)",
        (thread_id, user["user_id"], body.title, body.default_mode, now, now),
    )
    await db.commit()
    return ThreadResponse(thread_id=thread_id, title=body.title, default_mode=body.default_mode, status="active", created_at=now, updated_at=now)

@router.get("", response_model=list[ThreadResponse])
async def list_threads(user: dict = Depends(get_current_user)):
    db = await get_db()
    rows = await db.execute_fetchall(
        "SELECT * FROM threads WHERE user_id = ? AND status = 'active' ORDER BY updated_at DESC",
        (user["user_id"],),
    )
    return [ThreadResponse(**dict(r)) for r in rows]

@router.get("/{thread_id}", response_model=ThreadResponse)
async def get_thread(thread_id: str, user: dict = Depends(get_current_user)):
    db = await get_db()
    rows = await db.execute_fetchall(
        "SELECT * FROM threads WHERE thread_id = ? AND user_id = ?",
        (thread_id, user["user_id"]),
    )
    if not rows:
        raise HTTPException(status_code=404, detail="Thread not found")
    return ThreadResponse(**dict(rows[0]))
