from __future__ import annotations

import json
from pathlib import Path

from backend.src.harness.evaluator import run_evaluator
from backend.src.harness.generator import draft_contract, run_build_phase
from backend.src.harness.llm_client import CodexExecResult, LLMUsage
from backend.src.harness.workspace import ensure_workspace


async def _fake_chat_completion(*args, **kwargs):
    return json.dumps(
        {
            "sprint": 1,
            "status": "accepted",
            "scope_in": ["Build dashboard"],
            "scope_out": [],
            "files_expected": ["app.py"],
            "user_flows_to_verify": ["Open dashboard"],
            "tests_to_run": ["python -c 'print(1)'"],
            "evaluator_checks": ["Dashboard exists"],
            "done_definition": "Dashboard works",
        }
    )


async def test_draft_contract_parses_generator_json(monkeypatch):
    monkeypatch.setattr("backend.src.harness.generator.chat_completion", _fake_chat_completion)

    contract, usage = await draft_contract("# Product Spec", 1, 3)

    assert contract["scope_in"] == ["Build dashboard"]
    assert contract["done_definition"] == "Dashboard works"
    assert usage.total_tokens == 0


async def test_run_build_phase_collects_changes_and_commit(monkeypatch, tmp_path):
    workspace = ensure_workspace(str(tmp_path))
    repo_dir = Path(workspace) / "repo"

    async def fake_codex_exec(*args, **kwargs):
        target = repo_dir / "app.py"
        target.write_text("print('hello')\n", encoding="utf-8")
        return CodexExecResult(
            text="Implemented dashboard",
            commands=[{"command": "python -m pytest", "output": "ok", "exit_code": 0, "status": "completed"}],
            file_changes=[{"status": "??", "path": "app.py"}],
            usage=LLMUsage(output_tokens=10),
        )

    monkeypatch.setattr("backend.src.harness.generator.codex_exec", fake_codex_exec)

    build_result, usage = await run_build_phase(
        "# Product Spec",
        {"scope_in": ["Create app"], "tests_to_run": []},
        1,
        str(workspace),
    )

    assert build_result["changed_files"] == ["app.py"]
    assert build_result["command_log"][0]["exit_code"] == 0
    assert build_result["commit_sha"]
    assert usage.output_tokens == 10


async def test_run_evaluator_forces_failure_when_test_command_fails(monkeypatch, tmp_path):
    workspace = ensure_workspace(str(tmp_path))
    repo_dir = Path(workspace) / "repo"
    repo_dir.mkdir(parents=True, exist_ok=True)
    (repo_dir / "app.py").write_text("print('hello')\n", encoding="utf-8")

    async def fake_eval_chat(*args, **kwargs):
        return json.dumps(
            {
                "result": "pass",
                "scores": {
                    "functionality": 0.95,
                    "product_depth": 0.95,
                    "ux_quality": 0.95,
                    "code_quality": 0.95,
                },
                "thresholds": {
                    "functionality": 0.75,
                    "product_depth": 0.75,
                    "ux_quality": 0.75,
                    "code_quality": 0.75,
                },
                "blocking_issues": [],
                "repair_backlog": [],
                "evidence_summary": "All good",
            }
        )

    monkeypatch.setattr("backend.src.harness.evaluator.chat_completion", fake_eval_chat)

    report, usage = await run_evaluator(
        {"tests_to_run": ["python -c \"import sys; sys.exit(1)\""]},
        {"changed_files": ["app.py"], "summary": "done", "command_log": []},
        1,
        str(workspace),
    )

    assert report["result"] == "fail"
    assert report["blocking_issues"]
    assert usage.total_tokens == 0
