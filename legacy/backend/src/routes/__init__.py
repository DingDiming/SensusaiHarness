"""Auth routes."""
from __future__ import annotations
from fastapi import APIRouter, HTTPException
from ..auth import verify_password, create_access_token
from ..db import get_db
from ..schemas import LoginRequest, TokenResponse

router = APIRouter(prefix="/auth", tags=["auth"])

@router.post("/login", response_model=TokenResponse)
async def login(body: LoginRequest):
    db = await get_db()
    rows = await db.execute_fetchall(
        "SELECT user_id, username, password_hash FROM users WHERE username = ?",
        (body.username,),
    )
    if not rows or not verify_password(body.password, rows[0]["password_hash"]):
        raise HTTPException(status_code=401, detail="Invalid credentials")
    user = rows[0]
    token, expires_at = create_access_token(user["user_id"], user["username"])
    return TokenResponse(
        access_token=token,
        expires_at=expires_at,
        user={"user_id": user["user_id"], "username": user["username"]},
    )
