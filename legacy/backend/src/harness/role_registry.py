"""Role Registry — intelligent role assignment."""
from __future__ import annotations
import json
from datetime import datetime, timezone
from uuid import uuid4
from ..db import get_db
from ..schemas import RoleSuggestion

DEFAULT_ROLES = [
    {"role_name": "planner", "model_id": "gpt-5.4-mini", "system_prompt": "You are a product planner. Expand the user request into a detailed product specification. Respond with JSON containing: objective, tasks (list), success_criteria (list).", "temperature": 0.8, "max_tokens": 8192, "tool_permissions": ["web_search"]},
    {"role_name": "generator", "model_id": "gpt-5.4", "system_prompt": "You are a code generator. Implement one sprint at a time based on the contract.", "temperature": 0.3, "max_tokens": 16384, "tool_permissions": ["file_read", "file_write", "shell_exec"]},
    {"role_name": "evaluator", "model_id": "gpt-5.4-mini", "system_prompt": "You are a QA evaluator. Review implementation against the contract. Never modify code. Respond with JSON: {pass: bool, criteria_results: [{criterion, pass, notes}]}.", "temperature": 0.2, "max_tokens": 4096, "tool_permissions": ["file_read", "shell_exec"]},
]

ROLE_TRIGGERS = {
    "researcher": {"keywords": ["research", "analyze", "investigate", "survey", "compare"], "model_id": "gpt-5.4-mini", "system_prompt": "You research technical topics.", "temperature": 0.5, "tool_permissions": ["web_search"]},
    "designer": {"keywords": ["design", "ui", "ux", "interface", "layout", "visual"], "model_id": "gpt-5.4", "system_prompt": "You are a UI/UX designer.", "temperature": 0.6, "tool_permissions": ["file_read", "file_write"]},
}

async def ensure_default_roles() -> None:
    db = await get_db()
    for role in DEFAULT_ROLES:
        rows = await db.execute_fetchall("SELECT config_id FROM role_configs WHERE role_name = ? AND is_default = 1", (role["role_name"],))
        if not rows:
            now = datetime.now(timezone.utc).isoformat()
            await db.execute(
                "INSERT INTO role_configs (config_id, role_name, model_id, system_prompt, temperature, max_tokens, tool_permissions_json, is_default, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, 1, ?, ?)",
                (str(uuid4()), role["role_name"], role["model_id"], role["system_prompt"], role["temperature"], role["max_tokens"], json.dumps(role["tool_permissions"]), now, now))
    await db.commit()

async def suggest_roles_for_prompt(prompt: str) -> list[RoleSuggestion]:
    db = await get_db()
    suggestions = []
    for role_name in ["planner", "generator", "evaluator"]:
        rows = await db.execute_fetchall("SELECT config_id, model_id FROM role_configs WHERE role_name = ? AND is_default = 1 LIMIT 1", (role_name,))
        if rows:
            suggestions.append(RoleSuggestion(role_name=role_name, suggested_config_id=rows[0]["config_id"],
                suggested_model=rows[0]["model_id"], reason=f"Core {role_name} role"))
    prompt_lower = prompt.lower()
    for extra_role, config in ROLE_TRIGGERS.items():
        if any(kw in prompt_lower for kw in config["keywords"]):
            rows = await db.execute_fetchall("SELECT config_id, model_id FROM role_configs WHERE role_name = ? LIMIT 1", (extra_role,))
            if rows:
                suggestions.append(RoleSuggestion(role_name=extra_role, suggested_config_id=rows[0]["config_id"],
                    suggested_model=rows[0]["model_id"], reason=f"Prompt mentions {extra_role}-related tasks"))
    return suggestions

async def assign_roles_for_run(run_id: str, prompt: str, overrides: dict[str, str] | None = None) -> list[dict]:
    db = await get_db()
    suggestions = await suggest_roles_for_prompt(prompt)
    now = datetime.now(timezone.utc).isoformat()
    roles = []
    for s in suggestions:
        config_id = s.suggested_config_id
        if overrides and s.role_name in overrides:
            config_id = overrides[s.role_name]
        await db.execute("INSERT OR REPLACE INTO run_role_assignments (assignment_id, run_id, role_name, config_id, assigned_reason, created_at) VALUES (?, ?, ?, ?, ?, ?)",
            (str(uuid4()), run_id, s.role_name, config_id, s.reason, now))
        roles.append({"role_name": s.role_name, "model_id": s.suggested_model, "reason": s.reason})
    await db.commit()
    return roles
