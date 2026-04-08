"""Role configuration routes."""
from __future__ import annotations
import json
from datetime import datetime, timezone
from uuid import uuid4
from fastapi import APIRouter, Depends, HTTPException
from ..db import get_db
from ..deps import get_current_user
from ..schemas import RoleConfigCreate, RoleConfigResponse, RoleSuggestion

router = APIRouter(prefix="/roles", tags=["roles"])

@router.get("", response_model=list[RoleConfigResponse])
async def list_roles(user: dict = Depends(get_current_user)):
    db = await get_db()
    rows = await db.execute_fetchall("SELECT * FROM role_configs ORDER BY role_name, is_default DESC")
    result = []
    for r in rows:
        perms = json.loads(r["tool_permissions_json"]) if r["tool_permissions_json"] else []
        result.append(RoleConfigResponse(
            config_id=r["config_id"], role_name=r["role_name"], model_id=r["model_id"],
            system_prompt=r["system_prompt"], temperature=r["temperature"], max_tokens=r["max_tokens"],
            tool_permissions=perms, is_default=bool(r["is_default"]), created_at=r["created_at"],
        ))
    return result

@router.post("", response_model=RoleConfigResponse, status_code=201)
async def create_role(body: RoleConfigCreate, user: dict = Depends(get_current_user)):
    db = await get_db()
    now = datetime.now(timezone.utc).isoformat()
    config_id = str(uuid4())
    await db.execute(
        "INSERT INTO role_configs (config_id, role_name, model_id, system_prompt, temperature, max_tokens, tool_permissions_json, is_default, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        (config_id, body.role_name, body.model_id, body.system_prompt, body.temperature, body.max_tokens, json.dumps(body.tool_permissions), int(body.is_default), now, now),
    )
    await db.commit()
    return RoleConfigResponse(config_id=config_id, role_name=body.role_name, model_id=body.model_id,
        system_prompt=body.system_prompt, temperature=body.temperature, max_tokens=body.max_tokens,
        tool_permissions=body.tool_permissions, is_default=body.is_default, created_at=now)
