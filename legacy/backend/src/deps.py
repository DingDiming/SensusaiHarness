"""SensusAI Harness — FastAPI dependencies."""

from __future__ import annotations

from fastapi import Depends, HTTPException, Request

from .auth import decode_token
from .db import get_db


async def get_current_user(request: Request) -> dict:
    """Extract user from JWT (header or X-User-Id from Rust proxy)."""
    # Rust proxy injects X-User-Id after JWT validation
    user_id = request.headers.get("X-User-Id")
    if user_id:
        db = await get_db()
        row = await db.execute_fetchall("SELECT user_id, username FROM users WHERE user_id = ?", (user_id,))
        if row:
            return {"user_id": row[0]["user_id"], "username": row[0]["username"]}

    # Fallback: validate JWT directly (for direct Python access)
    auth = request.headers.get("Authorization", "")
    if not auth.startswith("Bearer "):
        raise HTTPException(status_code=401, detail="Not authenticated")

    payload = decode_token(auth[7:])
    if not payload:
        raise HTTPException(status_code=401, detail="Invalid token")

    return {"user_id": payload["sub"], "username": payload.get("username", "")}
