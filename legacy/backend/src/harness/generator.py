"""Generator role for contract drafting and Codex-backed build execution."""
from __future__ import annotations

import json
import logging
import subprocess
from pathlib import Path
from typing import Any

from .llm_client import DEFAULT_GENERATOR_MODEL, LLMUsage, chat_completion, codex_exec

logger = logging.getLogger(__name__)

CONTRACT_SYSTEM_PROMPT = """\
You are the Generator for an autonomous software delivery harness.

Draft the next sprint contract as JSON with this exact schema:
{
  "sprint": <int>,
  "status": "accepted",
  "scope_in": ["feature"],
  "scope_out": ["deferred feature"],
  "files_expected": ["path/to/file"],
  "user_flows_to_verify": ["flow description"],
  "tests_to_run": ["command"],
  "evaluator_checks": ["check description"],
  "done_definition": "single sentence"
}

Rules:
- Keep scope_in focused on one sprint.
- Prefer test commands that can actually run in the repo.
- Output JSON only.
"""

BUILD_SYSTEM_PROMPT = """\
You are the Generator for an autonomous software delivery harness.

Implement the current sprint inside the repository.
You may inspect files, edit code, install dependencies when necessary, and run verification commands.

Rules:
- Only modify files inside the working repository.
- Complete scope_in and do not expand scope_out.
- Run the listed tests when possible.
- End with a concise implementation summary and residual risks.
"""


async def draft_contract(
    product_spec: str,
    sprint: int,
    total_sprints: int,
    *,
    previous_handoff: str | None = None,
    model: str | None = None,
) -> tuple[dict, LLMUsage]:
    model = model or DEFAULT_GENERATOR_MODEL
    prompt_parts = [
        f"Product spec:\n{product_spec}",
        f"Current sprint: {sprint} / {total_sprints}",
    ]
    if previous_handoff:
        prompt_parts.append(f"Previous handoff:\n{previous_handoff}")

    raw = await chat_completion(
        [
            {"role": "system", "content": CONTRACT_SYSTEM_PROMPT},
            {"role": "user", "content": "\n\n".join(prompt_parts)},
        ],
        model=model,
        temperature=0.4,
        max_tokens=4096,
    )
    usage = LLMUsage()

    cleaned = raw.strip()
    if cleaned.startswith("```"):
        lines = [line for line in cleaned.splitlines() if not line.strip().startswith("```")]
        cleaned = "\n".join(lines)

    fallback = {
        "sprint": sprint,
        "status": "accepted",
        "scope_in": [f"Implement sprint {sprint} deliverables"],
        "scope_out": [],
        "files_expected": [],
        "user_flows_to_verify": [],
        "tests_to_run": [],
        "evaluator_checks": [],
        "done_definition": f"Sprint {sprint} deliverables complete.",
    }
    try:
        payload = json.loads(cleaned)
    except json.JSONDecodeError:
        logger.warning("Generator returned invalid contract JSON; using fallback contract")
        payload = fallback

    payload["sprint"] = sprint
    payload["status"] = "accepted"
    for key, value in fallback.items():
        payload.setdefault(key, value)
    return payload, usage


def _ensure_repo(repo_dir: Path) -> None:
    repo_dir.mkdir(parents=True, exist_ok=True)
    if not (repo_dir / ".git").exists():
        subprocess.run(["git", "init", "-q"], cwd=repo_dir, check=True)


def _latest_commit(repo_dir: Path) -> str | None:
    proc = subprocess.run(
        ["git", "rev-parse", "HEAD"],
        cwd=repo_dir,
        capture_output=True,
        text=True,
        check=False,
    )
    if proc.returncode != 0:
        return None
    return proc.stdout.strip() or None


def _commit_if_dirty(repo_dir: Path, sprint: int, summary: str) -> str | None:
    status = subprocess.run(
        ["git", "status", "--porcelain"],
        cwd=repo_dir,
        capture_output=True,
        text=True,
        check=False,
    )
    if status.returncode != 0 or not status.stdout.strip():
        return _latest_commit(repo_dir)

    add_proc = subprocess.run(["git", "add", "-A"], cwd=repo_dir, check=False)
    if add_proc.returncode != 0:
        return None

    commit_proc = subprocess.run(
        [
            "git",
            "-c",
            "user.name=SensusAI Harness",
            "-c",
            "user.email=sensusai-harness@local",
            "commit",
            "-m",
            f"sprint-{sprint:02d}: {summary[:72] or 'checkpoint'}",
        ],
        cwd=repo_dir,
        capture_output=True,
        text=True,
        check=False,
    )
    if commit_proc.returncode != 0:
        return None
    return _latest_commit(repo_dir)


async def run_build_phase(
    product_spec: str,
    contract: dict,
    sprint: int,
    workspace_path: str,
    *,
    model: str | None = None,
    previous_handoff: str | None = None,
    repair_backlog: list[str] | None = None,
) -> tuple[dict[str, Any], LLMUsage]:
    model = model or DEFAULT_GENERATOR_MODEL
    repo_dir = Path(workspace_path) / "repo"
    _ensure_repo(repo_dir)

    prompt_parts = [
        BUILD_SYSTEM_PROMPT,
        f"\n## Sprint Contract\n{json.dumps(contract, indent=2, ensure_ascii=False)}",
        f"\n## Product Spec\n{product_spec[:4000]}",
    ]
    if previous_handoff:
        prompt_parts.append(f"\n## Previous Handoff\n{previous_handoff}")
    if repair_backlog:
        prompt_parts.append("\n## Repair Backlog\n" + "\n".join(f"- {item}" for item in repair_backlog))

    prompt_parts.append(
        "\n## Required Output\n"
        "1. Modify the repository to satisfy the sprint contract.\n"
        "2. Run the listed tests when possible.\n"
        "3. Summarize changed files, verification steps, and remaining risks.\n"
    )

    result = await codex_exec(
        "\n".join(prompt_parts),
        model=model,
        cwd=str(repo_dir),
        full_auto=True,
        ephemeral=True,
        timeout_seconds=900,
    )

    commit_sha = _commit_if_dirty(repo_dir, sprint, contract.get("done_definition") or contract.get("objective", ""))
    build_result = {
        "changed_files": [item["path"] for item in result.file_changes],
        "command_log": result.commands,
        "summary": result.text,
        "commit_sha": commit_sha,
        "error": result.error,
    }
    return build_result, result.usage
