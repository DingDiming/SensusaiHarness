"""FastAPI application entry point."""
from __future__ import annotations
from contextlib import asynccontextmanager
from fastapi import FastAPI
from fastapi.middleware.cors import CORSMiddleware
from .config import settings
from .db import get_db, run_migrations
from .auth import hash_password
from .harness.role_registry import ensure_default_roles
from .routes import router as auth_router
from .routes.threads import router as threads_router
from .routes.messages import router as messages_router
from .routes.runs import router as runs_router
from .routes.roles import router as roles_router
from datetime import datetime, timezone
from uuid import uuid4

@asynccontextmanager
async def lifespan(app: FastAPI):
    await run_migrations()
    # Only create a bootstrap admin when credentials are explicitly provided.
    if settings.bootstrap_admin_username and settings.bootstrap_admin_password:
        db = await get_db()
        rows = await db.execute_fetchall(
            "SELECT user_id FROM users WHERE username = ?",
            (settings.bootstrap_admin_username,),
        )
        if not rows:
            now = datetime.now(timezone.utc).isoformat()
            await db.execute(
                """
                INSERT INTO users (user_id, username, password_hash, role, created_at, updated_at)
                VALUES (?, ?, ?, 'admin', ?, ?)
                """,
                (
                    str(uuid4()),
                    settings.bootstrap_admin_username,
                    hash_password(settings.bootstrap_admin_password),
                    now,
                    now,
                ),
            )
            await db.commit()
    await ensure_default_roles()
    yield

app = FastAPI(title="SensusAI Harness", version="2.0.0", lifespan=lifespan)

app.add_middleware(CORSMiddleware,
    allow_origins=["*"], allow_credentials=True,
    allow_methods=["*"], allow_headers=["*"])

app.include_router(auth_router, prefix="/api", tags=["auth"])
app.include_router(threads_router, prefix="/api", tags=["threads"])
app.include_router(messages_router, prefix="/api", tags=["messages"])
app.include_router(runs_router, prefix="/api", tags=["runs"])
app.include_router(roles_router, prefix="/api", tags=["roles"])

@app.get("/api/health")
async def health():
    return {"status": "ok", "version": "2.0.0"}
