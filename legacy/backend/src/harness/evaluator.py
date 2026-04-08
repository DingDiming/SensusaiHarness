"""Evaluator role for evidence-based QA on the repository workspace."""
from __future__ import annotations

import asyncio
import json
import logging
import os
from pathlib import Path
from typing import Any

from .llm_client import DEFAULT_EVALUATOR_MODEL, LLMUsage, chat_completion

logger = logging.getLogger(__name__)

EVALUATOR_SYSTEM_PROMPT = """\
You are the Evaluator for an autonomous software delivery harness.

Review the sprint deliverables against the sprint contract using evidence from:
- changed files
- repository structure
- verification command output
- implementation summary

Output JSON only with this schema:
{
  "result": "pass" or "fail",
  "scores": {
    "functionality": 0.0,
    "product_depth": 0.0,
    "ux_quality": 0.0,
    "code_quality": 0.0
  },
  "thresholds": {
    "functionality": 0.75,
    "product_depth": 0.75,
    "ux_quality": 0.75,
    "code_quality": 0.75
  },
  "blocking_issues": [
    {
      "title": "issue",
      "severity": "high|medium|low",
      "expected": "expected behavior",
      "actual": "actual behavior",
      "evidence": "specific evidence"
    }
  ],
  "repair_backlog": ["specific fix"],
  "evidence_summary": "short summary"
}

Be strict: failed tests or missing evidence should fail the sprint.
"""

PASS_THRESHOLDS = {
    "functionality": 0.75,
    "product_depth": 0.75,
    "ux_quality": 0.75,
    "code_quality": 0.75,
}


async def _run_tests(repo_root: Path, commands: list[str]) -> list[dict[str, Any]]:
    results: list[dict[str, Any]] = []
    for command in commands:
        if not command.strip():
            continue
        try:
            proc = await asyncio.create_subprocess_shell(
                command,
                cwd=str(repo_root),
                stdout=asyncio.subprocess.PIPE,
                stderr=asyncio.subprocess.STDOUT,
                env={**os.environ, "CI": "true"},
            )
            stdout, _ = await asyncio.wait_for(proc.communicate(), timeout=180)
            results.append(
                {
                    "command": command,
                    "exit_code": proc.returncode,
                    "passed": proc.returncode == 0,
                    "output": stdout.decode("utf-8", errors="replace")[:4000],
                }
            )
        except asyncio.TimeoutError:
            results.append(
                {
                    "command": command,
                    "exit_code": -1,
                    "passed": False,
                    "output": "TIMEOUT",
                }
            )
    return results


def _list_repo_structure(repo_root: Path, max_depth: int = 3) -> str:
    if not repo_root.exists():
        return "(repo directory does not exist)"

    lines: list[str] = []
    for dirpath, dirnames, filenames in os.walk(repo_root):
        depth = len(Path(dirpath).relative_to(repo_root).parts)
        if depth >= max_depth:
            dirnames.clear()
            continue
        indent = "  " * depth
        relative = Path(dirpath).relative_to(repo_root)
        lines.append(f"{indent}{relative}/")
        for filename in sorted(filenames)[:20]:
            lines.append(f"{indent}  {filename}")
        if len(lines) > 150:
            lines.append("... (truncated)")
            break
    return "\n".join(lines)


def _collect_changed_files(repo_root: Path, changed_files: list[str], max_chars: int = 2500) -> str:
    parts: list[str] = []
    for file_path in changed_files[:20]:
        full_path = (repo_root / file_path).resolve()
        if not str(full_path).startswith(str(repo_root.resolve())):
            continue
        if not full_path.exists():
            parts.append(f"### {file_path}\n(deleted or missing)")
            continue
        try:
            content = full_path.read_text(encoding="utf-8", errors="replace")
        except OSError:
            parts.append(f"### {file_path}\n(unreadable)")
            continue
        if len(content) > max_chars:
            content = content[:max_chars] + "\n... (truncated)"
        parts.append(f"### {file_path}\n```\n{content}\n```")
    return "\n\n".join(parts) if parts else "(no changed files)"


async def run_evaluator(
    contract: dict,
    build_result: dict[str, Any],
    sprint: int,
    workspace_path: str,
    *,
    model: str | None = None,
) -> tuple[dict, LLMUsage]:
    model = model or DEFAULT_EVALUATOR_MODEL
    repo_root = Path(workspace_path) / "repo"
    test_results = await _run_tests(repo_root, contract.get("tests_to_run") or [])
    repo_tree = _list_repo_structure(repo_root)
    changed_files = build_result.get("changed_files") or []
    file_contents = _collect_changed_files(repo_root, changed_files)

    prompt_parts = [
        f"Contract:\n{json.dumps(contract, indent=2, ensure_ascii=False)}",
        f"\nBuild result:\n{json.dumps(build_result, indent=2, ensure_ascii=False)}",
        f"\nChanged file contents:\n{file_contents}",
        f"\nRepository structure:\n```\n{repo_tree}\n```",
    ]
    if test_results:
        prompt_parts.append("\nTest results:\n" + json.dumps(test_results, indent=2, ensure_ascii=False))
    else:
        prompt_parts.append("\nTest results:\n[]")

    raw = await chat_completion(
        [
            {"role": "system", "content": EVALUATOR_SYSTEM_PROMPT},
            {"role": "user", "content": "\n".join(prompt_parts)},
        ],
        model=model,
        temperature=0.2,
        max_tokens=4096,
    )
    usage = LLMUsage()

    cleaned = raw.strip()
    if cleaned.startswith("```"):
        lines = [line for line in cleaned.splitlines() if not line.strip().startswith("```")]
        cleaned = "\n".join(lines)

    fallback = {
        "result": "fail",
        "scores": {key: 0.35 for key in PASS_THRESHOLDS},
        "thresholds": PASS_THRESHOLDS,
        "blocking_issues": [{"title": "Evaluator returned invalid JSON", "severity": "high", "expected": "Structured QA report", "actual": raw[:300], "evidence": raw[:500]}],
        "repair_backlog": ["Return a structured QA report with concrete issues and evidence."],
        "evidence_summary": "Fallback QA report because evaluator output was invalid.",
    }
    try:
        report = json.loads(cleaned)
    except json.JSONDecodeError:
        report = fallback

    report.setdefault("scores", fallback["scores"])
    report.setdefault("thresholds", PASS_THRESHOLDS)
    report.setdefault("blocking_issues", [])
    report.setdefault("repair_backlog", [])
    report.setdefault("evidence_summary", "")
    report["test_results"] = test_results
    report["sprint"] = sprint

    if any(not item["passed"] for item in test_results):
        report["result"] = "fail"
        report.setdefault("blocking_issues", []).append(
            {
                "title": "One or more verification commands failed",
                "severity": "high",
                "expected": "All verification commands should pass",
                "actual": "At least one command returned non-zero",
                "evidence": json.dumps(test_results, ensure_ascii=False)[:800],
            }
        )

    scores = report.get("scores") or {}
    all_scores_pass = all(float(scores.get(key, 0)) >= threshold for key, threshold in PASS_THRESHOLDS.items())
    has_blockers = any(issue.get("severity") == "high" for issue in report.get("blocking_issues", []))
    report["result"] = "pass" if all_scores_pass and not has_blockers else "fail"
    return report, usage
