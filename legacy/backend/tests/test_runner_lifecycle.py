from __future__ import annotations

import json
from pathlib import Path

import aiosqlite

from backend.src import db as db_module
from backend.src.harness.llm_client import LLMUsage
from backend.src.harness.role_registry import ensure_default_roles
from backend.src.harness.runner import _drive


async def _make_test_db(tmp_path):
    db_path = tmp_path / "runner.db"
    conn = await aiosqlite.connect(str(db_path))
    conn.row_factory = aiosqlite.Row
    schema_path = Path(__file__).resolve().parents[1] / "migrations" / "001_initial.sql"
    await conn.executescript(schema_path.read_text(encoding="utf-8"))
    await conn.commit()
    db_module._db = conn
    return conn


async def test_runner_drive_persists_full_sprint_lifecycle(monkeypatch, tmp_path):
    test_db = await _make_test_db(tmp_path)
    workspace_path = tmp_path / "workspace"
    now = "2026-04-04T00:00:00+00:00"
    try:
        await test_db.execute(
            """
            INSERT INTO users (user_id, username, password_hash, role, created_at, updated_at)
            VALUES ('user-1', 'tester', 'hash', 'admin', ?, ?)
            """,
            (now, now),
        )
        await test_db.execute(
            """
            INSERT INTO threads (thread_id, user_id, title, default_mode, status, created_at, updated_at)
            VALUES ('thread-1', 'user-1', 'Harness thread', 'autonomous', 'active', ?, ?)
            """,
            (now, now),
        )
        await test_db.execute(
            """
            INSERT INTO runs (
                run_id, thread_id, user_id, prompt, mode, state, config_json, current_sprint,
                planned_sprints, repair_count, max_repairs, workspace_path, created_at, updated_at
            ) VALUES (?, ?, ?, ?, 'autonomous', 'queued', ?, 0, 1, 0, 2, ?, ?, ?)
            """,
            (
                "run-1",
                "thread-1",
                "user-1",
                "Build the acceptance harness",
                json.dumps({"max_sprints": 1, "approval_gates": []}),
                str(workspace_path),
                now,
                now,
            ),
        )
        await test_db.commit()
        await ensure_default_roles()

        async def fake_emit_event(run_id: str, event_type: str, data: dict) -> None:
            await test_db.execute(
                "INSERT INTO run_events (run_id, event_type, data_json, created_at) VALUES (?, ?, ?, ?)",
                (run_id, event_type, json.dumps(data, ensure_ascii=False), now),
            )
            await test_db.commit()

        async def fake_run_planner(prompt: str, user_memory_context: str | None = None):
            assert prompt == "Build the acceptance harness"
            return (
                "# Product Spec\n\nBuild the acceptance harness with durable artifacts.",
                1,
                LLMUsage(),
            )

        async def fake_draft_contract(product_spec: str, sprint: int, total_sprints: int, previous_handoff: str | None = None):
            assert sprint == 1
            assert total_sprints == 1
            assert "Product Spec" in product_spec
            return (
                {
                    "objective": "Ship a verifiable sprint pipeline",
                    "scope_in": ["Persist product spec", "Persist sprint QA"],
                    "scope_out": ["Polish visuals"],
                    "files_expected": ["repo/app.py"],
                    "user_flows_to_verify": ["Open acceptance run detail"],
                    "tests_to_run": ["python -c \"print('ok')\""],
                    "evaluator_checks": ["Artifacts exist", "Run completes"],
                    "done_definition": "One sprint completes and all lifecycle artifacts are saved.",
                },
                LLMUsage(),
            )

        async def fake_run_build_phase(
            product_spec: str,
            contract: dict,
            sprint: int,
            workspace_path: str,
            previous_handoff: str | None = None,
            repair_backlog: list[str] | None = None,
        ):
            repo_dir = Path(workspace_path) / "repo"
            repo_dir.mkdir(parents=True, exist_ok=True)
            (repo_dir / "app.py").write_text("print('acceptance ready')\n", encoding="utf-8")
            return (
                {
                    "summary": "Implemented the sprint and saved artifacts.",
                    "changed_files": ["app.py"],
                    "command_log": [{"command": "python -c \"print('ok')\"", "exit_code": 0}],
                    "commit_sha": "abc123",
                },
                LLMUsage(output_tokens=12),
            )

        async def fake_run_evaluator(contract: dict, build_result: dict, sprint: int, workspace_path: str):
            assert build_result["changed_files"] == ["app.py"]
            return (
                True,
                {
                    "result": "pass",
                    "scores": {
                        "functionality": 0.95,
                        "product_depth": 0.9,
                        "ux_quality": 0.85,
                        "code_quality": 0.9,
                    },
                    "blocking_issues": [],
                    "repair_backlog": [],
                    "evidence_summary": "Artifacts were written and the sprint completed successfully.",
                },
            )

        monkeypatch.setattr("backend.src.harness.runner.emit_event", fake_emit_event)
        monkeypatch.setattr("backend.src.harness.runner.run_planner", fake_run_planner)
        monkeypatch.setattr("backend.src.harness.runner.draft_contract", fake_draft_contract)
        monkeypatch.setattr("backend.src.harness.runner.run_build_phase", fake_run_build_phase)
        monkeypatch.setattr("backend.src.harness.runner.run_evaluator", fake_run_evaluator)

        await _drive("run-1")

        run_row = (
            await test_db.execute_fetchall("SELECT state, current_sprint FROM runs WHERE run_id = 'run-1'")
        )[0]
        assert run_row["state"] == "completed"
        assert run_row["current_sprint"] == 1

        thread_row = (
            await test_db.execute_fetchall("SELECT summary_text FROM threads WHERE thread_id = 'thread-1'")
        )[0]
        assert "One sprint completes and all lifecycle artifacts are saved." in thread_row["summary_text"]

        artifact_rows = await test_db.execute_fetchall(
            "SELECT kind, path FROM artifact_index WHERE run_id = 'run-1' ORDER BY kind"
        )
        artifact_kinds = {row["kind"] for row in artifact_rows}
        assert {
            "product_spec",
            "sprint_contract",
            "qa_report",
            "checkpoint_metadata",
            "handoff",
            "summary",
        }.issubset(artifact_kinds)

        progress_rows = await test_db.execute_fetchall(
            "SELECT phase, outcome FROM progress_snapshots WHERE run_id = 'run-1' ORDER BY started_at"
        )
        progress_phases = [row["phase"] for row in progress_rows]
        assert progress_phases == ["planning", "contracting", "building", "qa", "checkpointing"]
        assert all(row["outcome"] in {"success", "pass"} for row in progress_rows)

        event_rows = await test_db.execute_fetchall(
            "SELECT event_type FROM run_events WHERE run_id = 'run-1' ORDER BY event_id"
        )
        event_types = [row["event_type"] for row in event_rows]
        assert "contract" in event_types
        assert "qa_report" in event_types
        assert "done" in event_types

        assert (workspace_path / "artifacts" / "product_spec.md").exists()
        assert (workspace_path / "artifacts" / "sprint_contracts" / "sprint-01.json").exists()
        assert (workspace_path / "artifacts" / "qa_reports" / "sprint-01.json").exists()
        assert (workspace_path / "artifacts" / "handoffs" / "sprint-01.md").exists()
        assert (workspace_path / "artifacts" / "summaries" / "run_summary.md").exists()
        assert (workspace_path / "checkpoints" / "sprint-01.json").exists()
    finally:
        await test_db.close()
        db_module._db = None
