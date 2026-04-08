"""Run routes — create, inspect, and control autonomous runs."""
from __future__ import annotations

import asyncio
import json
from datetime import datetime, timezone
from pathlib import Path
from uuid import uuid4

from fastapi import APIRouter, Depends, HTTPException

from ..db import get_db
from ..deps import get_current_user
from ..harness.event_emitter import emit_event
from ..schemas import ApprovalAction, CreateRun, RunDetailResponse, RunResponse

router = APIRouter(tags=["runs"])


def _now_iso() -> str:
    return datetime.now(timezone.utc).isoformat()


def _workspace_path(user_id: str, run_id: str) -> str:
    return str(Path("data") / "workspaces" / user_id / run_id)


def _json_loads(raw: str | None, default):
    if not raw:
        return default
    try:
        return json.loads(raw)
    except json.JSONDecodeError:
        return default


def _make_budget(row) -> dict:
    return {
        "tokens_used": row["tokens_used"],
        "tokens_limit": row["tokens_limit"],
        "wall_clock_seconds": row["wall_clock_seconds"],
        "wall_clock_limit": row["wall_clock_limit"],
        "repair_count": row["repair_count"],
        "max_repairs": row["max_repairs"],
    }


def _normalize_contract(row) -> dict:
    payload = _json_loads(row["contract_json"], {})
    return {
        "contract_id": row["contract_id"],
        "sprint": row["sprint"],
        "status": row["status"],
        "objective": payload.get("objective", ""),
        "scope_in": payload.get("scope_in") or payload.get("tasks") or [],
        "scope_out": payload.get("scope_out") or [],
        "files_expected": payload.get("files_expected") or [],
        "user_flows_to_verify": payload.get("user_flows_to_verify") or [],
        "tests_to_run": payload.get("tests_to_run") or [],
        "evaluator_checks": payload.get("evaluator_checks") or payload.get("success_criteria") or [],
        "done_definition": payload.get("done_definition") or payload.get("objective") or "",
        "raw_contract": payload,
        "created_at": row["created_at"],
    }


def _derive_scores(report: dict, row) -> dict:
    scores = report.get("scores")
    if isinstance(scores, dict):
        return {
            "functionality": float(scores.get("functionality", 0)),
            "product_depth": float(scores.get("product_depth", 0)),
            "ux_quality": float(scores.get("ux_quality", 0)),
            "code_quality": float(scores.get("code_quality", 0)),
        }

    criteria = report.get("criteria_results") or []
    if criteria:
        passed = sum(1 for item in criteria if item.get("pass"))
        ratio = passed / max(len(criteria), 1)
        return {
            "functionality": ratio,
            "product_depth": ratio,
            "ux_quality": ratio,
            "code_quality": ratio,
        }

    base = 0.8 if row["pass"] else 0.35
    return {
        "functionality": base,
        "product_depth": base,
        "ux_quality": base,
        "code_quality": base,
    }


def _derive_blocking_issues(report: dict) -> list[dict]:
    issues = report.get("blocking_issues")
    if isinstance(issues, list):
        return issues

    criteria = report.get("criteria_results") or []
    derived = []
    for item in criteria:
        if item.get("pass"):
            continue
        derived.append(
            {
                "title": item.get("criterion", "Unmet success criterion"),
                "severity": "high",
                "evidence": item.get("notes", ""),
            }
        )
    if derived:
        return derived

    if report.get("raw"):
        return [{"title": "Unstructured QA result", "severity": "high", "evidence": report["raw"][:500]}]
    return []


def _normalize_qa_report(row) -> dict:
    payload = _json_loads(row["report_json"], {})
    result = payload.get("result")
    if result not in {"pass", "fail"}:
        result = "pass" if row["pass"] else "fail"
    scores = _derive_scores(payload, row)
    blocking_issues = _derive_blocking_issues(payload)

    return {
        "report_id": row["qa_id"],
        "sprint": row["sprint"],
        "result": result,
        "scores_json": json.dumps(scores, ensure_ascii=False),
        "blocking_issues_json": json.dumps(blocking_issues, ensure_ascii=False),
        "repair_backlog_json": json.dumps(payload.get("repair_backlog") or [], ensure_ascii=False),
        "summary": payload.get("evidence_summary") or "",
        "raw_report": payload,
        "created_at": row["created_at"],
    }


def _normalize_gate(row) -> dict:
    gate = dict(row)
    if gate.get("status") == "pending":
        gate["status"] = "awaiting_user"
    return gate


def _run_response(row, roles=None) -> RunResponse:
    return RunResponse(
        run_id=row["run_id"],
        thread_id=row["thread_id"],
        prompt=row["prompt"],
        mode=row["mode"],
        state=row["state"],
        current_sprint=row["current_sprint"],
        planned_sprints=row["planned_sprints"],
        roles=roles,
        budget=_make_budget(row),
        created_at=row["created_at"],
        updated_at=row["updated_at"],
    )


@router.post("/threads/{thread_id}/runs", response_model=RunResponse, status_code=202)
async def create_run(thread_id: str, body: CreateRun, user: dict = Depends(get_current_user)):
    db = await get_db()
    rows = await db.execute_fetchall(
        "SELECT thread_id FROM threads WHERE thread_id = ? AND user_id = ?",
        (thread_id, user["user_id"]),
    )
    if not rows:
        raise HTTPException(status_code=404, detail="Thread not found")

    now = _now_iso()
    run_id = str(uuid4())
    config = {
        "max_sprints": body.max_sprints,
        "approval_gates": body.approval_gates,
    }
    if body.role_overrides:
        config["role_overrides"] = body.role_overrides

    await db.execute(
        """
        INSERT INTO runs (
            run_id, thread_id, user_id, prompt, mode, state, config_json, planned_sprints,
            workspace_path, created_at, updated_at
        ) VALUES (?, ?, ?, ?, 'autonomous', 'queued', ?, ?, ?, ?, ?)
        """,
        (
            run_id,
            thread_id,
            user["user_id"],
            body.prompt,
            json.dumps(config, ensure_ascii=False),
            body.max_sprints,
            _workspace_path(user["user_id"], run_id),
            now,
            now,
        ),
    )

    from ..harness.role_registry import assign_roles_for_run

    roles = await assign_roles_for_run(run_id, body.prompt, body.role_overrides)
    await db.execute(
        "UPDATE threads SET active_run_id = ?, updated_at = ? WHERE thread_id = ?",
        (run_id, now, thread_id),
    )
    await db.commit()

    from ..harness.runner import start_run

    asyncio.create_task(start_run(run_id))
    row = (await db.execute_fetchall("SELECT * FROM runs WHERE run_id = ?", (run_id,)))[0]
    return _run_response(row, roles)


@router.get("/runs", response_model=list[RunResponse])
async def list_runs(thread_id: str | None = None, state: str | None = None, user: dict = Depends(get_current_user)):
    db = await get_db()
    query = "SELECT * FROM runs WHERE user_id = ?"
    params: list = [user["user_id"]]
    if thread_id:
        query += " AND thread_id = ?"
        params.append(thread_id)
    if state:
        query += " AND state = ?"
        params.append(state)
    query += " ORDER BY created_at DESC"
    rows = await db.execute_fetchall(query, params)
    return [_run_response(r) for r in rows]


@router.get("/runs/{run_id}", response_model=RunDetailResponse)
async def get_run_detail(run_id: str, user: dict = Depends(get_current_user)):
    db = await get_db()
    rows = await db.execute_fetchall(
        "SELECT * FROM runs WHERE run_id = ? AND user_id = ?",
        (run_id, user["user_id"]),
    )
    if not rows:
        raise HTTPException(status_code=404, detail="Run not found")

    row = rows[0]
    roles_rows = await db.execute_fetchall(
        """
        SELECT ra.role_name, rc.model_id, ra.assigned_reason
        FROM run_role_assignments ra
        JOIN role_configs rc ON ra.config_id = rc.config_id
        WHERE ra.run_id = ?
        ORDER BY ra.role_name
        """,
        (run_id,),
    )
    progress = [
        dict(item)
        for item in await db.execute_fetchall(
            "SELECT * FROM progress_snapshots WHERE run_id = ? ORDER BY started_at",
            (run_id,),
        )
    ]
    events = [
        dict(item)
        for item in await db.execute_fetchall(
            "SELECT * FROM run_events WHERE run_id = ? ORDER BY event_id DESC LIMIT 200",
            (run_id,),
        )
    ]
    contract_rows = await db.execute_fetchall(
        "SELECT * FROM sprint_contracts WHERE run_id = ? ORDER BY sprint, created_at",
        (run_id,),
    )
    qa_rows = await db.execute_fetchall(
        "SELECT * FROM qa_reports WHERE run_id = ? ORDER BY sprint, created_at",
        (run_id,),
    )
    gate_rows = await db.execute_fetchall(
        """
        SELECT * FROM approval_gates
        WHERE run_id = ? AND status IN ('awaiting_user', 'pending')
        ORDER BY created_at DESC
        LIMIT 1
        """,
        (run_id,),
    )
    active_gate = _normalize_gate(gate_rows[0]) if gate_rows else None

    return RunDetailResponse(
        run_id=row["run_id"],
        thread_id=row["thread_id"],
        prompt=row["prompt"],
        mode=row["mode"],
        state=row["state"],
        current_sprint=row["current_sprint"],
        planned_sprints=row["planned_sprints"],
        roles=[dict(item) for item in roles_rows],
        budget=_make_budget(row),
        progress=progress,
        events=events,
        contracts=[_normalize_contract(item) for item in contract_rows],
        qa_reports=[_normalize_qa_report(item) for item in qa_rows],
        active_gate=active_gate,
        created_at=row["created_at"],
        updated_at=row["updated_at"],
    )


@router.post("/runs/{run_id}/pause")
async def pause_run(run_id: str, user: dict = Depends(get_current_user)):
    db = await get_db()
    rows = await db.execute_fetchall(
        "SELECT state FROM runs WHERE run_id = ? AND user_id = ?",
        (run_id, user["user_id"]),
    )
    if not rows:
        raise HTTPException(status_code=404, detail="Run not found")
    current_state = rows[0]["state"]
    if current_state in {"completed", "failed", "cancelled", "paused"}:
        raise HTTPException(status_code=409, detail="Cannot pause run in current state")

    now = _now_iso()
    await db.execute(
        "UPDATE runs SET state = 'paused', updated_at = ? WHERE run_id = ?",
        (now, run_id),
    )
    await db.commit()
    await emit_event(run_id, "state_change", {"from": current_state, "state": "paused"})
    return {"run_id": run_id, "state": "paused"}


@router.post("/runs/{run_id}/resume")
async def resume_run(run_id: str, user: dict = Depends(get_current_user)):
    db = await get_db()
    rows = await db.execute_fetchall(
        "SELECT state FROM runs WHERE run_id = ? AND user_id = ?",
        (run_id, user["user_id"]),
    )
    if not rows:
        raise HTTPException(status_code=404, detail="Run not found")
    current_state = rows[0]["state"]
    if current_state not in {"paused", "interrupted"}:
        raise HTTPException(status_code=409, detail="Run is not paused/interrupted")

    now = _now_iso()
    await db.execute(
        "UPDATE runs SET state = 'queued', updated_at = ? WHERE run_id = ?",
        (now, run_id),
    )
    await db.commit()
    await emit_event(run_id, "state_change", {"from": current_state, "state": "queued"})

    from ..harness.runner import start_run

    asyncio.create_task(start_run(run_id))
    return {"run_id": run_id, "state": "queued"}


@router.post("/runs/{run_id}/cancel")
async def cancel_run(run_id: str, user: dict = Depends(get_current_user)):
    db = await get_db()
    rows = await db.execute_fetchall(
        "SELECT state FROM runs WHERE run_id = ? AND user_id = ?",
        (run_id, user["user_id"]),
    )
    if not rows:
        raise HTTPException(status_code=404, detail="Run not found")
    current_state = rows[0]["state"]
    if current_state in {"completed", "failed", "cancelled"}:
        raise HTTPException(status_code=409, detail="Cannot cancel run in current state")

    now = _now_iso()
    await db.execute(
        "UPDATE runs SET state = 'cancelled', updated_at = ?, completed_at = ? WHERE run_id = ?",
        (now, now, run_id),
    )
    await db.commit()
    await emit_event(run_id, "state_change", {"from": current_state, "state": "cancelled"})
    await emit_event(run_id, "done", {"result": "cancelled", "summary": "Run cancelled by user."})
    return {"run_id": run_id, "state": "cancelled"}


@router.post("/runs/{run_id}/approve")
async def approve_gate(run_id: str, body: ApprovalAction, user: dict = Depends(get_current_user)):
    db = await get_db()
    now = _now_iso()
    gate_rows = await db.execute_fetchall(
        """
        SELECT gate_id, gate_type, sprint, status, title, summary
        FROM approval_gates
        WHERE gate_id = ? AND run_id = ? AND status IN ('awaiting_user', 'pending')
        """,
        (body.gate_id, run_id),
    )
    if not gate_rows:
        raise HTTPException(status_code=404, detail="Gate not found or already decided")

    result = await db.execute(
        """
        UPDATE approval_gates
        SET status = 'approved', decision_note = ?, decided_at = ?
        WHERE gate_id = ? AND run_id = ?
        """,
        (body.note, now, body.gate_id, run_id),
    )
    if result.rowcount == 0:
        raise HTTPException(status_code=404, detail="Gate not found or already decided")

    run_rows = await db.execute_fetchall(
        "SELECT state FROM runs WHERE run_id = ? AND user_id = ?",
        (run_id, user["user_id"]),
    )
    if not run_rows:
        raise HTTPException(status_code=404, detail="Run not found")
    current_state = run_rows[0]["state"]
    await db.execute(
        "UPDATE runs SET state = 'queued', updated_at = ? WHERE run_id = ?",
        (now, run_id),
    )
    await db.commit()

    gate = dict(gate_rows[0])
    gate["status"] = "approved"
    await emit_event(run_id, "approval", gate)
    await emit_event(run_id, "state_change", {"from": current_state, "state": "queued"})

    from ..harness.runner import start_run

    asyncio.create_task(start_run(run_id))
    return {"run_id": run_id, "gate_id": body.gate_id, "decision": "approved"}


@router.post("/runs/{run_id}/reject")
async def reject_gate(run_id: str, body: ApprovalAction, user: dict = Depends(get_current_user)):
    db = await get_db()
    now = _now_iso()
    gate_rows = await db.execute_fetchall(
        """
        SELECT gate_id, gate_type, sprint, status, title, summary
        FROM approval_gates
        WHERE gate_id = ? AND run_id = ? AND status IN ('awaiting_user', 'pending')
        """,
        (body.gate_id, run_id),
    )
    if not gate_rows:
        raise HTTPException(status_code=404, detail="Gate not found or already decided")

    result = await db.execute(
        """
        UPDATE approval_gates
        SET status = 'rejected', decision_note = ?, decided_at = ?
        WHERE gate_id = ? AND run_id = ?
        """,
        (body.note, now, body.gate_id, run_id),
    )
    if result.rowcount == 0:
        raise HTTPException(status_code=404, detail="Gate not found or already decided")

    run_rows = await db.execute_fetchall(
        "SELECT state FROM runs WHERE run_id = ? AND user_id = ?",
        (run_id, user["user_id"]),
    )
    if not run_rows:
        raise HTTPException(status_code=404, detail="Run not found")
    current_state = run_rows[0]["state"]
    failure_message = body.note or "Approval rejected by user."
    await db.execute(
        """
        UPDATE runs
        SET state = 'failed', error_message = ?, updated_at = ?, completed_at = ?
        WHERE run_id = ?
        """,
        (failure_message, now, now, run_id),
    )
    await db.commit()

    gate = dict(gate_rows[0])
    gate["status"] = "rejected"
    await emit_event(run_id, "approval", gate)
    await emit_event(run_id, "state_change", {"from": current_state, "state": "failed"})
    await emit_event(run_id, "done", {"result": "failed", "summary": failure_message})
    return {"run_id": run_id, "gate_id": body.gate_id, "decision": "rejected"}
