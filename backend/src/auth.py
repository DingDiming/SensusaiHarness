"""SensusAI Harness — Auth (JWT + login)."""

from __future__ import annotations

import hashlib
import hmac
import secrets
from datetime import datetime, timezone, timedelta

from jose import jwt, JWTError

from .config import settings

ALGORITHM = "HS256"


def hash_password(password: str) -> str:
    salt = secrets.token_hex(16)
    dk = hashlib.pbkdf2_hmac("sha256", password.encode(), salt.encode(), 100_000)
    return f"{salt}${dk.hex()}"


def verify_password(password: str, password_hash: str) -> bool:
    parts = password_hash.split("$", 1)
    if len(parts) != 2:
        return False
    salt, stored_dk = parts
    dk = hashlib.pbkdf2_hmac("sha256", password.encode(), salt.encode(), 100_000)
    return hmac.compare_digest(dk.hex(), stored_dk)


def create_access_token(user_id: str, username: str) -> tuple[str, str]:
    exp = datetime.now(timezone.utc) + timedelta(hours=settings.jwt_expire_hours)
    payload = {"sub": user_id, "username": username, "exp": exp}
    token = jwt.encode(payload, settings.jwt_secret, algorithm=ALGORITHM)
    return token, exp.isoformat()


def decode_token(token: str) -> dict | None:
    try:
        return jwt.decode(token, settings.jwt_secret, algorithms=[ALGORITHM])
    except JWTError:
        return None
