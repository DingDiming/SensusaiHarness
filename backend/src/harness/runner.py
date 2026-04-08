"""Run orchestrator with persistent artifacts, approvals, and resumable sprint flow."""
from __future__ import annotations

import asyncio
import json
import traceback
from datetime import datetime, timezone
from pathlib import Path
from uuid import uuid4

from ..db import get_db
from . import TERMINAL_STATES
from .artifacts import (
    write_approval_snapshot,
    write_checkpoint_metadata,
    write_handoff,
    write_product_spec,
    write_qa_report,
    write_run_summary,
    write_sprint_contract,
)
from .evaluator import run_evaluator
from .event_emitter import emit_event
from .generator import draft_contract, run_build_phase
from .llm_client import chat_completion as chat
from .planner import run_planner
from .progress import end_phase, start_phase
from .role_registry import assign_roles_for_run
from .workspace import ensure_workspace

_ACTIVE_RUN_TASKS: dict[str, asyncio.Task[None]] = {}
_PHASE_ROLE = {
    "planning": "planner",
    "contracting": "planner",
    "building": "generator",
    "repair": "generator",
    "qa": "evaluator",
    "checkpointing": "system",
}
_MAX_CONTEXT_MESSAGES = 8
_MAX_MEMORY_ENTRIES = 8


def _now() -> datetime:
    return datetime.now(timezone.utc)


def _now_iso() -> str:
    return _now().isoformat()


def _json_loads(raw: str | None, default):
    if not raw:
        return default
    try:
        return json.loads(raw)
    except json.JSONDecodeError:
        return default


def _default_workspace_path(user_id: str, run_id: str) -> str:
    return str(Path("data") / "workspaces" / user_id / run_id)


async def start_run(run_id: str) -> None:
    existing = _ACTIVE_RUN_TASKS.get(run_id)
    if existing and not existing.done():
        return

    task = asyncio.create_task(_drive(run_id), name=f"run:{run_id}")
    _ACTIVE_RUN_TASKS[run_id] = task

    def _cleanup(_: asyncio.Task[None]) -> None:
        _ACTIVE_RUN_TASKS.pop(run_id, None)

    task.add_done_callback(_cleanup)


async def _refresh_run(run_id: str) -> dict | None:
    db = await get_db()
    rows = await db.execute_fetchall("SELECT * FROM runs WHERE run_id = ?", (run_id,))
    return dict(rows[0]) if rows else None


async def _update_run(run_id: str, **fields) -> None:
    if not fields:
        return

    db = await get_db()
    assignments = ", ".join(f"{column} = ?" for column in fields)
    values = list(fields.values()) + [run_id]
    await db.execute(f"UPDATE runs SET {assignments} WHERE run_id = ?", values)
    await db.commit()


def _budget_payload(run_row: dict) -> dict:
    return {
        "tokens_used": run_row.get("tokens_used", 0),
        "tokens_limit": run_row.get("tokens_limit"),
        "wall_clock_seconds": run_row.get("wall_clock_seconds", 0),
        "wall_clock_limit": run_row.get("wall_clock_limit"),
        "repair_count": run_row.get("repair_count", 0),
        "max_repairs": run_row.get("max_repairs", 3),
    }


async def _transition(run_id: str, new_state: str, **extra_fields) -> None:
    run_row = await _refresh_run(run_id)
    if not run_row:
        return

    old_state = run_row["state"]
    now = _now()
    fields = {"state": new_state, "updated_at": now.isoformat(), **extra_fields}

    started_at = run_row.get("started_at")
    if not started_at and new_state not in {"queued", "paused", "awaiting_approval"}:
        started_at = now.isoformat()
        fields["started_at"] = started_at

    if started_at:
        try:
            started_dt = datetime.fromisoformat(str(started_at))
            fields["wall_clock_seconds"] = max(0, int((now - started_dt).total_seconds()))
        except ValueError:
            pass

    if new_state in TERMINAL_STATES:
        fields.setdefault("completed_at", now.isoformat())

    await _update_run(run_id, **fields)
    latest = await _refresh_run(run_id)
    await emit_event(
        run_id,
        "state_change",
        {"from": old_state, "state": new_state, "current_sprint": latest.get("current_sprint", 0)},
    )
    await emit_event(run_id, "budget", _budget_payload(latest))

    role_name = _PHASE_ROLE.get(new_state)
    if role_name:
        await emit_event(run_id, "role_switch", {"role": role_name, "phase": new_state})


async def _register_artifact(
    run_id: str,
    kind: str,
    path: str,
    *,
    sprint: int | None = None,
    size_bytes: int | None = None,
    producer_role: str | None = None,
) -> None:
    db = await get_db()
    await db.execute(
        """
        INSERT INTO artifact_index (
            artifact_id, run_id, sprint, kind, path, size_bytes, producer_role, created_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)
        """,
        (str(uuid4()), run_id, sprint, kind, path, size_bytes, producer_role, _now_iso()),
    )
    await db.commit()
    await emit_event(run_id, "artifact", {"kind": kind, "path": path, "sprint": sprint})


async def _upsert_checkpoint(run_id: str, sprint: int, summary: str) -> None:
    db = await get_db()
    existing = await db.execute_fetchall(
        "SELECT checkpoint_id FROM checkpoints WHERE run_id = ? AND sprint = ?",
        (run_id, sprint),
    )
    if existing:
        await db.execute(
            "UPDATE checkpoints SET summary = ?, created_at = ? WHERE checkpoint_id = ?",
            (summary, _now_iso(), existing[0]["checkpoint_id"]),
        )
    else:
        await db.execute(
            """
            INSERT INTO checkpoints (checkpoint_id, run_id, sprint, name, summary, created_at)
            VALUES (?, ?, ?, ?, ?, ?)
            """,
            (str(uuid4()), run_id, sprint, f"sprint-{sprint:02d}", summary, _now_iso()),
        )
    await db.commit()
    await emit_event(
        run_id,
        "checkpoint",
        {"sprint": sprint, "name": f"sprint-{sprint:02d}", "summary": summary},
    )


async def _ensure_roles_for_run(run_id: str, prompt: str, overrides: dict[str, str] | None) -> list[dict]:
    db = await get_db()
    existing = await db.execute_fetchall(
        """
        SELECT ra.role_name, rc.model_id, ra.assigned_reason
        FROM run_role_assignments ra
        JOIN role_configs rc ON rc.config_id = ra.config_id
        WHERE ra.run_id = ?
        ORDER BY ra.role_name
        """,
        (run_id,),
    )
    if existing:
        return [dict(row) for row in existing]
    return await assign_roles_for_run(run_id, prompt, overrides)


async def _wait_for_control(run_id: str) -> str | None:
    while True:
        run_row = await _refresh_run(run_id)
        if not run_row:
            return "missing"

        state = run_row["state"]
        if state == "paused":
            await asyncio.sleep(1)
            continue
        if state in TERMINAL_STATES:
            return state
        return None


async def _find_gate(run_id: str, gate_type: str, sprint: int | None = None) -> dict | None:
    db = await get_db()
    if sprint is None:
        rows = await db.execute_fetchall(
            """
            SELECT * FROM approval_gates
            WHERE run_id = ? AND gate_type = ?
            ORDER BY created_at DESC
            LIMIT 1
            """,
            (run_id, gate_type),
        )
    else:
        rows = await db.execute_fetchall(
            """
            SELECT * FROM approval_gates
            WHERE run_id = ? AND gate_type = ? AND sprint = ?
            ORDER BY created_at DESC
            LIMIT 1
            """,
            (run_id, gate_type, sprint),
        )
    return dict(rows[0]) if rows else None


async def _wait_for_gate(run_id: str, gate_type: str, title: str, summary: str, *, sprint: int | None = None) -> str:
    gate_row = await _find_gate(run_id, gate_type, sprint)
    db = await get_db()

    if gate_row is None or gate_row["status"] not in {"awaiting_user", "approved", "rejected"}:
        gate_id = str(uuid4())
        created_at = _now_iso()
        await db.execute(
            """
            INSERT INTO approval_gates (gate_id, run_id, sprint, gate_type, status, title, summary, created_at)
            VALUES (?, ?, ?, ?, 'awaiting_user', ?, ?, ?)
            """,
            (gate_id, run_id, sprint, gate_type, title, summary, created_at),
        )
        await db.commit()
        gate_row = {
            "gate_id": gate_id,
            "run_id": run_id,
            "sprint": sprint,
            "gate_type": gate_type,
            "status": "awaiting_user",
            "title": title,
            "summary": summary,
        }
        run_row = await _refresh_run(run_id)
        workspace_path = run_row.get("workspace_path") or _default_workspace_path(run_row["user_id"], run_id)
        approval_path, size_bytes, _ = write_approval_snapshot(workspace_path, gate_id, gate_row)
        await _register_artifact(
            run_id,
            "approval",
            approval_path,
            sprint=sprint,
            size_bytes=size_bytes,
            producer_role="system",
        )
        await emit_event(run_id, "approval", gate_row)

    if gate_row["status"] == "approved":
        return "approved"
    if gate_row["status"] == "rejected":
        return "rejected"

    await _transition(run_id, "awaiting_approval")
    while True:
        gate_row = await _find_gate(run_id, gate_type, sprint)
        if gate_row:
            if gate_row["status"] == "approved":
                return "approved"
            if gate_row["status"] == "rejected":
                return "rejected"

        control = await _wait_for_control(run_id)
        if control in TERMINAL_STATES:
            return "rejected"

        if not gate_row:
            await asyncio.sleep(1)
            continue
        await asyncio.sleep(1)


async def _load_recent_memory(user_id: str) -> list[str]:
    db = await get_db()
    rows = await db.execute_fetchall(
        "SELECT content FROM user_memory WHERE user_id = ? ORDER BY created_at DESC LIMIT ?",
        (user_id, _MAX_MEMORY_ENTRIES),
    )
    return [row["content"] for row in rows if row["content"]]


async def _load_thread_summary(thread_id: str) -> str:
    db = await get_db()
    rows = await db.execute_fetchall(
        "SELECT summary_text FROM threads WHERE thread_id = ? LIMIT 1",
        (thread_id,),
    )
    if not rows:
        return ""
    return rows[0]["summary_text"] or ""


async def _load_recent_messages(thread_id: str) -> list[dict]:
    db = await get_db()
    rows = await db.execute_fetchall(
        """
        SELECT role, content
        FROM thread_messages
        WHERE thread_id = ?
        ORDER BY created_at DESC
        LIMIT ?
        """,
        (thread_id, _MAX_CONTEXT_MESSAGES),
    )
    messages = [dict(row) for row in reversed(rows)]
    return messages


async def _load_previous_contracts(run_id: str, sprint: int) -> list[dict]:
    db = await get_db()
    rows = await db.execute_fetchall(
        """
        SELECT sprint, contract_json
        FROM sprint_contracts
        WHERE run_id = ? AND sprint < ?
        ORDER BY sprint DESC
        LIMIT 2
        """,
        (run_id, sprint),
    )
    return [
        {"sprint": row["sprint"], **_json_loads(row["contract_json"], {})}
        for row in rows
    ]


async def _load_previous_reports(run_id: str, sprint: int) -> list[dict]:
    db = await get_db()
    rows = await db.execute_fetchall(
        """
        SELECT sprint, report_json, pass
        FROM qa_reports
        WHERE run_id = ? AND sprint < ?
        ORDER BY sprint DESC
        LIMIT 2
        """,
        (run_id, sprint),
    )
    result = []
    for row in rows:
        payload = _json_loads(row["report_json"], {})
        payload.setdefault("result", "pass" if row["pass"] else "fail")
        payload["sprint"] = row["sprint"]
        result.append(payload)
    return result


async def _build_thread_context(run_row: dict, sprint: int) -> str:
    thread_summary = await _load_thread_summary(run_row["thread_id"])
    messages = await _load_recent_messages(run_row["thread_id"])
    memory_entries = await _load_recent_memory(run_row["user_id"])
    previous_contracts = await _load_previous_contracts(run_row["run_id"], sprint)
    previous_reports = await _load_previous_reports(run_row["run_id"], sprint)

    sections: list[str] = []
    if thread_summary:
        sections.append(f"Thread summary:\n{thread_summary}")
    if memory_entries:
        sections.append("Stable user/project memory:\n" + "\n".join(f"- {item}" for item in memory_entries))
    if messages:
        formatted = "\n".join(f"{item['role']}: {item['content']}" for item in messages)
        sections.append(f"Recent conversation:\n{formatted}")
    if previous_contracts:
        lines = []
        for item in reversed(previous_contracts):
            objective = item.get("done_definition") or item.get("objective") or ", ".join(item.get("scope_in", [])[:2])
            lines.append(f"- Sprint {item['sprint']}: {objective}")
        sections.append("Previous sprint goals:\n" + "\n".join(lines))
    if previous_reports:
        lines = []
        for item in reversed(previous_reports):
            issues = item.get("blocking_issues") or []
            issue_title = issues[0].get("title") if issues and isinstance(issues[0], dict) else ""
            lines.append(f"- Sprint {item['sprint']}: {item.get('result', 'fail')} {issue_title}".strip())
        sections.append("Previous QA outcomes:\n" + "\n".join(lines))

    return "\n\n".join(section for section in sections if section).strip()


async def _run_planning(run_row: dict, sprint: int, thread_context: str) -> dict:
    phase_id = await start_phase(run_row["run_id"], sprint, "planning")
    cfg = await _get_role_config(run_row["run_id"], "planner")
    user_prompt = (
        f"User request:\n{run_row['prompt']}\n\n"
        f"Context gathered from prior work:\n{thread_context or '(none)'}\n\n"
        "Draft the next sprint contract as JSON with this schema:\n"
        "{\n"
        '  "objective": "string",\n'
        '  "scope_in": ["deliverable"],\n'
        '  "scope_out": ["deferred"],\n'
        '  "files_expected": ["path/to/file"],\n'
        '  "user_flows_to_verify": ["flow"],\n'
        '  "tests_to_run": ["command"],\n'
        '  "evaluator_checks": ["check"],\n'
        '  "done_definition": "clear definition of done"\n'
        "}\n\n"
        "Rules:\n"
        "- Make scope_in concrete and sprint-sized.\n"
        "- Keep done_definition directly testable.\n"
        "- Prefer existing repository paths when you can infer them from context.\n"
        "- Output JSON only."
    )
    messages = [
        {"role": "system", "content": cfg.get("system_prompt", "You are a planner for autonomous software delivery.")},
        {"role": "user", "content": user_prompt},
    ]
    result = await chat(
        messages,
        model=cfg.get("model_id"),
        temperature=cfg.get("temperature", 0.4),
        max_tokens=cfg.get("max_tokens", 4096),
    )
    await end_phase(phase_id, "success")

    fallback = {
        "objective": f"Sprint {sprint} delivery",
        "scope_in": [run_row["prompt"]],
        "scope_out": [],
        "files_expected": [],
        "user_flows_to_verify": [],
        "tests_to_run": [],
        "evaluator_checks": [],
        "done_definition": "Implement the requested change and verify the primary flow works.",
    }
    try:
        payload = json.loads(result)
    except json.JSONDecodeError:
        payload = fallback

    payload.setdefault("objective", fallback["objective"])
    payload.setdefault("scope_in", fallback["scope_in"])
    payload.setdefault("scope_out", fallback["scope_out"])
    payload.setdefault("files_expected", fallback["files_expected"])
    payload.setdefault("user_flows_to_verify", fallback["user_flows_to_verify"])
    payload.setdefault("tests_to_run", fallback["tests_to_run"])
    payload.setdefault("evaluator_checks", payload.get("success_criteria", fallback["evaluator_checks"]))
    payload.setdefault("done_definition", payload.get("objective", fallback["done_definition"]))
    return payload


async def _run_building(run_id: str, sprint: int, plan: dict, thread_context: str, repair_notes: list[str] | None) -> str:
    phase_id = await start_phase(run_id, sprint, "building")
    cfg = await _get_role_config(run_id, "generator")
    repair_block = ""
    if repair_notes:
        repair_block = "\n\nFix these QA issues before anything else:\n" + "\n".join(f"- {item}" for item in repair_notes)
    messages = [
        {"role": "system", "content": cfg.get("system_prompt", "You are a code generator.")},
        {
            "role": "user",
            "content": (
                f"Working context:\n{thread_context or '(none)'}\n\n"
                f"Sprint contract:\n{json.dumps(plan, ensure_ascii=False)}"
                f"{repair_block}\n\n"
                "Produce a concise implementation report covering:\n"
                "1. What changed.\n"
                "2. Which files or modules should be touched.\n"
                "3. Which checks should pass.\n"
                "4. Remaining risks."
            ),
        },
    ]
    result = await chat(
        messages,
        model=cfg.get("model_id"),
        temperature=cfg.get("temperature", 0.3),
        max_tokens=cfg.get("max_tokens", 8192),
    )
    await end_phase(phase_id, "success")
    return result


async def _run_qa(run_id: str, sprint: int, plan: dict, build_result: str, thread_context: str) -> tuple[bool, dict]:
    phase_id = await start_phase(run_id, sprint, "qa")
    cfg = await _get_role_config(run_id, "evaluator")
    messages = [
        {"role": "system", "content": cfg.get("system_prompt", "You are a QA evaluator.")},
        {
            "role": "user",
            "content": (
                f"Context:\n{thread_context or '(none)'}\n\n"
                f"Contract:\n{json.dumps(plan, ensure_ascii=False)}\n\n"
                f"Implementation report:\n{build_result}\n\n"
                "Return JSON only with schema:\n"
                "{\n"
                '  "result": "pass" | "fail",\n'
                '  "scores": {"functionality": 0.0, "product_depth": 0.0, "ux_quality": 0.0, "code_quality": 0.0},\n'
                '  "blocking_issues": [{"title": "issue", "severity": "high|medium|low", "evidence": "why"}],\n'
                '  "repair_backlog": ["specific fix"],\n'
                '  "evidence_summary": "what you checked"\n'
                "}\n\n"
                "Be strict. If evidence is weak or missing, fail the sprint."
            ),
        },
    ]
    result = await chat(
        messages,
        model=cfg.get("model_id"),
        temperature=cfg.get("temperature", 0.2),
        max_tokens=cfg.get("max_tokens", 4096),
    )

    fallback = {
        "result": "fail",
        "scores": {
            "functionality": 0.35,
            "product_depth": 0.4,
            "ux_quality": 0.4,
            "code_quality": 0.4,
        },
        "blocking_issues": [{"title": "Evaluator returned invalid JSON", "severity": "high", "evidence": result[:500]}],
        "repair_backlog": ["Provide a structured, verifiable implementation update."],
        "evidence_summary": "Fallback QA report because evaluator output could not be parsed.",
    }
    try:
        report = json.loads(result)
    except json.JSONDecodeError:
        report = fallback

    scores = report.get("scores") or fallback["scores"]
    report["scores"] = {
        "functionality": float(scores.get("functionality", 0)),
        "product_depth": float(scores.get("product_depth", 0)),
        "ux_quality": float(scores.get("ux_quality", 0)),
        "code_quality": float(scores.get("code_quality", 0)),
    }
    report.setdefault("blocking_issues", [])
    report.setdefault("repair_backlog", [])
    report.setdefault("evidence_summary", "")
    report["result"] = report.get("result", "fail")
    if report["result"] not in {"pass", "fail"}:
        report["result"] = "fail"

    await end_phase(phase_id, "pass" if report["result"] == "pass" else "fail")
    return report["result"] == "pass", report


async def _run_repair(run_id: str, sprint: int, plan: dict, qa_report: dict) -> list[str]:
    phase_id = await start_phase(run_id, sprint, "repair")
    cfg = await _get_role_config(run_id, "generator")
    messages = [
        {"role": "system", "content": cfg.get("system_prompt", "You are a code generator.")},
        {
            "role": "user",
            "content": (
                f"Contract:\n{json.dumps(plan, ensure_ascii=False)}\n\n"
                f"QA report:\n{json.dumps(qa_report, ensure_ascii=False)}\n\n"
                "Return only a JSON array of the concrete fixes that should happen next."
            ),
        },
    ]
    result = await chat(
        messages,
        model=cfg.get("model_id"),
        temperature=cfg.get("temperature", 0.3),
        max_tokens=cfg.get("max_tokens", 4096),
    )
    await end_phase(phase_id, "success")

    try:
        payload = json.loads(result)
    except json.JSONDecodeError:
        payload = qa_report.get("repair_backlog") or []

    if isinstance(payload, list):
        return [str(item) for item in payload if str(item).strip()]
    return [str(payload)]


async def _get_role_config(run_id: str, role_name: str) -> dict:
    db = await get_db()
    rows = await db.execute_fetchall(
        """
        SELECT rc.*
        FROM run_role_assignments ra
        JOIN role_configs rc ON ra.config_id = rc.config_id
        WHERE ra.run_id = ? AND ra.role_name = ?
        """,
        (run_id, role_name),
    )
    if rows:
        return dict(rows[0])
    return {
        "model_id": None,
        "system_prompt": f"You are the {role_name}.",
        "temperature": 0.5,
        "max_tokens": 4096,
    }


async def _latest_contract(run_id: str, sprint: int) -> dict | None:
    db = await get_db()
    rows = await db.execute_fetchall(
        """
        SELECT * FROM sprint_contracts
        WHERE run_id = ? AND sprint = ?
        ORDER BY created_at DESC
        LIMIT 1
        """,
        (run_id, sprint),
    )
    return dict(rows[0]) if rows else None


async def _latest_qa_report(run_id: str, sprint: int) -> dict | None:
    db = await get_db()
    rows = await db.execute_fetchall(
        """
        SELECT * FROM qa_reports
        WHERE run_id = ? AND sprint = ?
        ORDER BY created_at DESC
        LIMIT 1
        """,
        (run_id, sprint),
    )
    return dict(rows[0]) if rows else None


async def _latest_checkpoint_sprint(run_id: str) -> int:
    db = await get_db()
    rows = await db.execute_fetchall(
        "SELECT MAX(sprint) AS sprint FROM checkpoints WHERE run_id = ?",
        (run_id,),
    )
    return int(rows[0]["sprint"] or 0) if rows else 0


async def _has_checkpoint(run_id: str, sprint: int) -> bool:
    db = await get_db()
    rows = await db.execute_fetchall(
        "SELECT checkpoint_id FROM checkpoints WHERE run_id = ? AND sprint = ? LIMIT 1",
        (run_id, sprint),
    )
    return bool(rows)


def _build_summary_text(run_row: dict, contracts: list[dict], qa_reports: list[dict]) -> str:
    lines = [
        "# Run Summary",
        "",
        f"- Prompt: {run_row['prompt']}",
        f"- Final state: {run_row['state']}",
        f"- Completed sprints: {len(contracts)}",
    ]

    if contracts:
        lines.append("")
        lines.append("## Sprint Goals")
        for contract in contracts:
            objective = contract.get("done_definition") or contract.get("objective") or ", ".join(contract.get("scope_in", [])[:2])
            lines.append(f"- Sprint {contract['sprint']}: {objective}")

    if qa_reports:
        lines.append("")
        lines.append("## QA Outcomes")
        for report in qa_reports:
            issues = report.get("blocking_issues") or []
            top_issue = issues[0].get("title") if issues and isinstance(issues[0], dict) else ""
            suffix = f" ({top_issue})" if top_issue else ""
            lines.append(f"- Sprint {report['sprint']}: {report.get('result', 'fail')}{suffix}")

    return "\n".join(lines)


def _read_text_if_exists(path: Path) -> str | None:
    if not path.exists():
        return None
    try:
        return path.read_text(encoding="utf-8")
    except OSError:
        return None


def _read_product_spec(workspace_path: str) -> str | None:
    return _read_text_if_exists(Path(workspace_path) / "artifacts" / "product_spec.md")


def _read_handoff(workspace_path: str, sprint: int) -> str | None:
    return _read_text_if_exists(Path(workspace_path) / "artifacts" / "handoffs" / f"sprint-{sprint:02d}.md")


async def _write_thread_summary(run_row: dict) -> None:
    db = await get_db()
    contract_rows = await db.execute_fetchall(
        "SELECT sprint, contract_json FROM sprint_contracts WHERE run_id = ? ORDER BY sprint",
        (run_row["run_id"],),
    )
    qa_rows = await db.execute_fetchall(
        "SELECT sprint, report_json, pass FROM qa_reports WHERE run_id = ? ORDER BY sprint",
        (run_row["run_id"],),
    )
    contracts = [
        {"sprint": row["sprint"], **_json_loads(row["contract_json"], {})}
        for row in contract_rows
    ]
    reports = []
    for row in qa_rows:
        payload = _json_loads(row["report_json"], {})
        payload.setdefault("result", "pass" if row["pass"] else "fail")
        payload["sprint"] = row["sprint"]
        reports.append(payload)

    summary_text = _build_summary_text(run_row, contracts, reports)
    workspace_path = run_row.get("workspace_path") or _default_workspace_path(run_row["user_id"], run_row["run_id"])
    relative_path, size_bytes, _ = write_run_summary(workspace_path, summary_text)
    await _register_artifact(
        run_row["run_id"],
        "summary",
        relative_path,
        size_bytes=size_bytes,
        producer_role="system",
    )

    await db.execute(
        "UPDATE threads SET summary_text = ?, updated_at = ? WHERE thread_id = ?",
        (summary_text[:1000], _now_iso(), run_row["thread_id"]),
    )
    await db.commit()


async def _fail_run(run_id: str, error: str, tb: str) -> None:
    await _transition(run_id, "failed", error_message=error)
    await emit_event(run_id, "error", {"message": error, "traceback": tb})
    await emit_event(run_id, "done", {"result": "failed", "summary": error})


async def _drive(run_id: str) -> None:
    try:
        run_row = await _refresh_run(run_id)
        if not run_row or run_row["state"] in TERMINAL_STATES:
            return

        config = _json_loads(run_row.get("config_json"), {})
        approval_gates = config.get("approval_gates")
        if approval_gates is None and config.get("require_approval"):
            approval_gates = ["spec_gate", "delivery_gate"]
        approval_gates = approval_gates or []

        workspace_path = run_row.get("workspace_path") or _default_workspace_path(run_row["user_id"], run_id)
        ensure_workspace(workspace_path)
        if run_row.get("workspace_path") != workspace_path:
            await _update_run(run_id, workspace_path=workspace_path)
            run_row = await _refresh_run(run_id)

        roles = await _ensure_roles_for_run(run_id, run_row["prompt"], config.get("role_overrides"))
        if roles:
            await emit_event(run_id, "roles_assigned", {"roles": roles})

        max_sprints = max(1, int(config.get("max_sprints") or run_row.get("planned_sprints") or 1))
        completed_sprint = await _latest_checkpoint_sprint(run_id)
        next_sprint = max(completed_sprint + 1, run_row.get("current_sprint") or 1)
        previous_handoff = _read_handoff(workspace_path, completed_sprint) if completed_sprint > 0 else None

        product_spec = _read_product_spec(workspace_path)
        if not product_spec:
            planner_memory = await _load_recent_memory(run_row["user_id"])
            thread_summary = await _load_thread_summary(run_row["thread_id"])
            planner_context = "\n".join(
                item for item in [thread_summary, *planner_memory] if item
            )
            planning_phase_id = await start_phase(run_id, 1, "planning")
            await _transition(run_id, "planning", current_sprint=1, planned_sprints=max_sprints)
            product_spec, detected_sprints, _ = await run_planner(
                run_row["prompt"],
                user_memory_context=planner_context or None,
            )
            await end_phase(planning_phase_id, "success")
            planned_sprints = min(max_sprints, detected_sprints) if max_sprints else detected_sprints
            await _update_run(run_id, planned_sprints=planned_sprints)
            relative_path, size_bytes, _ = write_product_spec(workspace_path, product_spec)
            await _register_artifact(
                run_id,
                "product_spec",
                relative_path,
                size_bytes=size_bytes,
                producer_role="planner",
            )
        else:
            planned_sprints = max_sprints

        run_row = await _refresh_run(run_id)
        max_sprints = max(1, int(run_row.get("planned_sprints") or max_sprints))

        if run_row.get("repair_count") is None:
            await _update_run(run_id, repair_count=0)

        while next_sprint <= max_sprints:
            control = await _wait_for_control(run_id)
            if control in TERMINAL_STATES:
                return

            await _update_run(run_id, current_sprint=next_sprint, planned_sprints=max_sprints, repair_count=0)
            run_row = await _refresh_run(run_id)

            contract_row = await _latest_contract(run_id, next_sprint)
            contract = _json_loads(contract_row["contract_json"], {}) if contract_row else None

            if not contract:
                await _transition(run_id, "contracting", current_sprint=next_sprint, planned_sprints=max_sprints)
                contract_phase_id = await start_phase(run_id, next_sprint, "contracting")
                contract, _ = await draft_contract(
                    product_spec,
                    next_sprint,
                    max_sprints,
                    previous_handoff=previous_handoff,
                )
                contract_row = {
                    "contract_id": str(uuid4()),
                    "run_id": run_id,
                    "sprint": next_sprint,
                    "status": "accepted",
                    "contract_json": json.dumps(contract, ensure_ascii=False),
                    "created_at": _now_iso(),
                }
                db = await get_db()
                await db.execute(
                    """
                    INSERT INTO sprint_contracts (contract_id, run_id, sprint, status, contract_json, created_at)
                    VALUES (?, ?, ?, 'accepted', ?, ?)
                    """,
                    (
                        contract_row["contract_id"],
                        run_id,
                        next_sprint,
                        contract_row["contract_json"],
                        contract_row["created_at"],
                    ),
                )
                await db.commit()
                relative_path, size_bytes, _ = write_sprint_contract(workspace_path, next_sprint, contract)
                await _register_artifact(
                    run_id,
                    "sprint_contract",
                    relative_path,
                    sprint=next_sprint,
                    size_bytes=size_bytes,
                    producer_role="planner",
                )
                await emit_event(
                    run_id,
                    "contract",
                    {
                        "sprint": next_sprint,
                        "status": "accepted",
                        "done_definition": contract.get("done_definition") or contract.get("objective", ""),
                    },
                )
                await end_phase(contract_phase_id, "success")

            if next_sprint == 1 and "spec_gate" in approval_gates:
                decision = await _wait_for_gate(
                    run_id,
                    "spec_gate",
                    "Approve initial sprint contract",
                    contract.get("done_definition") or contract.get("objective", "Review the first sprint scope."),
                    sprint=next_sprint,
                )
                if decision != "approved":
                    await _fail_run(run_id, "Initial sprint contract was rejected.", "")
                    return

            if await _has_checkpoint(run_id, next_sprint):
                previous_handoff = _read_handoff(workspace_path, next_sprint) or previous_handoff
                next_sprint += 1
                continue

            repair_notes: list[str] | None = None
            last_report: dict | None = None
            max_repairs = int(run_row.get("max_repairs") or 3)

            while True:
                control = await _wait_for_control(run_id)
                if control in TERMINAL_STATES:
                    return

                await _transition(run_id, "building", current_sprint=next_sprint)
                build_phase_id = await start_phase(run_id, next_sprint, "building")
                try:
                    build_result, _ = await run_build_phase(
                        product_spec,
                        contract,
                        next_sprint,
                        workspace_path,
                        previous_handoff=previous_handoff,
                        repair_backlog=repair_notes,
                    )
                except Exception:
                    await end_phase(build_phase_id, "fail")
                    raise
                await end_phase(build_phase_id, "success")
                await emit_event(
                    run_id,
                    "message",
                    {
                        "content": build_result.get("summary", ""),
                        "phase": "building",
                        "sprint": next_sprint,
                    },
                )

                control = await _wait_for_control(run_id)
                if control in TERMINAL_STATES:
                    return

                await _transition(run_id, "qa", current_sprint=next_sprint)
                qa_phase_id = await start_phase(run_id, next_sprint, "qa")
                try:
                    qa_ok, last_report = await run_evaluator(
                        contract,
                        build_result,
                        next_sprint,
                        workspace_path,
                    )
                except Exception:
                    await end_phase(qa_phase_id, "fail")
                    raise
                await end_phase(qa_phase_id, "pass" if qa_ok else "fail")
                last_report["run_id"] = run_id
                qa_id = str(uuid4())
                db = await get_db()
                await db.execute(
                    """
                    INSERT INTO qa_reports (qa_id, run_id, sprint, pass, report_json, created_at)
                    VALUES (?, ?, ?, ?, ?, ?)
                    """,
                    (qa_id, run_id, next_sprint, int(qa_ok), json.dumps(last_report, ensure_ascii=False), _now_iso()),
                )
                await db.commit()
                relative_path, size_bytes, _ = write_qa_report(workspace_path, next_sprint, last_report)
                await _register_artifact(
                    run_id,
                    "qa_report",
                    relative_path,
                    sprint=next_sprint,
                    size_bytes=size_bytes,
                    producer_role="evaluator",
                )
                await emit_event(
                    run_id,
                    "qa_report",
                    {
                        "sprint": next_sprint,
                        "result": last_report.get("result", "fail"),
                        "blocking_issues": last_report.get("blocking_issues", []),
                    },
                )

                if qa_ok:
                    break

                latest = await _refresh_run(run_id)
                next_repair_count = int(latest.get("repair_count") or 0) + 1
                await _update_run(run_id, repair_count=next_repair_count)
                await emit_event(run_id, "budget", _budget_payload(await _refresh_run(run_id)))

                if next_repair_count >= max_repairs:
                    await _fail_run(
                        run_id,
                        f"Sprint {next_sprint} failed QA after {max_repairs} repair attempt(s).",
                        "",
                    )
                    return

                await _transition(run_id, "repair", current_sprint=next_sprint)
                repair_notes = await _run_repair(run_id, next_sprint, contract, last_report)

            checkpoint_phase_id = await start_phase(run_id, next_sprint, "checkpointing")
            await _transition(run_id, "checkpointing", current_sprint=next_sprint)
            checkpoint_summary = contract.get("done_definition") or contract.get("objective") or f"Sprint {next_sprint} checkpoint"
            await _upsert_checkpoint(run_id, next_sprint, checkpoint_summary)
            checkpoint_payload = {
                "name": f"sprint-{next_sprint:02d}",
                "run_id": run_id,
                "sprint": next_sprint,
                "commit_sha": build_result.get("commit_sha"),
                "summary": checkpoint_summary,
                "artifact_refs": [
                    f"artifacts/sprint_contracts/sprint-{next_sprint:02d}.json",
                    f"artifacts/qa_reports/sprint-{next_sprint:02d}.json",
                ],
                "created_at": _now_iso(),
            }
            checkpoint_path, checkpoint_size, _ = write_checkpoint_metadata(workspace_path, next_sprint, checkpoint_payload)
            await _register_artifact(
                run_id,
                "checkpoint_metadata",
                checkpoint_path,
                sprint=next_sprint,
                size_bytes=checkpoint_size,
                producer_role="system",
            )
            handoff_content = (
                f"# Handoff — Sprint {next_sprint}\n\n"
                f"Status: QA pass\n"
                f"Done definition: {checkpoint_summary}\n\n"
                f"Summary:\n{build_result.get('summary', '').strip() or '(no summary)'}\n\n"
                f"Next step: Sprint {next_sprint + 1 if next_sprint < max_sprints else 'delivery'}\n"
            )
            handoff_path, handoff_size, _ = write_handoff(workspace_path, next_sprint, handoff_content)
            await _register_artifact(
                run_id,
                "handoff",
                handoff_path,
                sprint=next_sprint,
                size_bytes=handoff_size,
                producer_role="generator",
            )
            await end_phase(checkpoint_phase_id, "success")
            previous_handoff = handoff_content

            if "checkpoint_gate" in approval_gates:
                decision = await _wait_for_gate(
                    run_id,
                    "checkpoint_gate",
                    f"Approve sprint {next_sprint} checkpoint",
                    checkpoint_summary,
                    sprint=next_sprint,
                )
                if decision != "approved":
                    await _fail_run(run_id, f"Sprint {next_sprint} checkpoint was rejected.", "")
                    return

            next_sprint += 1

        if "delivery_gate" in approval_gates:
            decision = await _wait_for_gate(
                run_id,
                "delivery_gate",
                "Approve final delivery",
                "All planned sprints are complete. Review the final delivery summary.",
                sprint=None,
            )
            if decision != "approved":
                await _fail_run(run_id, "Final delivery was rejected.", "")
                return

        await _transition(run_id, "completed", current_sprint=max_sprints)
        final_row = await _refresh_run(run_id)
        if final_row:
            await _write_thread_summary(final_row)
        await emit_event(
            run_id,
            "done",
            {"result": "completed", "summary": f"Completed {max_sprints} sprint(s)."},
        )
    except Exception as exc:
        await _fail_run(run_id, str(exc), traceback.format_exc())
