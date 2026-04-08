"""Artifact writers for contracts, QA reports, and run summaries."""
from __future__ import annotations

import hashlib
import json
from pathlib import Path

from .workspace import artifact_path


def _write_text(path: Path, content: str) -> int:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(content, encoding="utf-8")
    return len(content.encode("utf-8"))


def _write_json(path: Path, data: dict | list) -> int:
    content = json.dumps(data, indent=2, ensure_ascii=False)
    return _write_text(path, content)


def _sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(8192), b""):
            digest.update(chunk)
    return digest.hexdigest()


def write_sprint_contract(workspace_path: str, sprint: int, contract: dict) -> tuple[str, int, str]:
    relative_path = f"artifacts/sprint_contracts/sprint-{sprint:02d}.json"
    target = artifact_path(workspace_path, relative_path)
    size_bytes = _write_json(target, contract)
    return relative_path, size_bytes, _sha256(target)


def write_product_spec(workspace_path: str, content: str) -> tuple[str, int, str]:
    relative_path = "artifacts/product_spec.md"
    target = artifact_path(workspace_path, relative_path)
    size_bytes = _write_text(target, content)
    return relative_path, size_bytes, _sha256(target)


def write_qa_report(workspace_path: str, sprint: int, report: dict) -> tuple[str, int, str]:
    relative_path = f"artifacts/qa_reports/sprint-{sprint:02d}.json"
    target = artifact_path(workspace_path, relative_path)
    size_bytes = _write_json(target, report)
    return relative_path, size_bytes, _sha256(target)


def write_approval_snapshot(workspace_path: str, gate_id: str, payload: dict) -> tuple[str, int, str]:
    relative_path = f"artifacts/approvals/{gate_id}.json"
    target = artifact_path(workspace_path, relative_path)
    size_bytes = _write_json(target, payload)
    return relative_path, size_bytes, _sha256(target)


def write_handoff(workspace_path: str, sprint: int, content: str) -> tuple[str, int, str]:
    relative_path = f"artifacts/handoffs/sprint-{sprint:02d}.md"
    target = artifact_path(workspace_path, relative_path)
    size_bytes = _write_text(target, content)
    return relative_path, size_bytes, _sha256(target)


def write_checkpoint_metadata(workspace_path: str, sprint: int, payload: dict) -> tuple[str, int, str]:
    relative_path = f"checkpoints/sprint-{sprint:02d}.json"
    target = artifact_path(workspace_path, relative_path)
    size_bytes = _write_json(target, payload)
    return relative_path, size_bytes, _sha256(target)


def write_run_summary(workspace_path: str, content: str) -> tuple[str, int, str]:
    relative_path = "artifacts/summaries/run_summary.md"
    target = artifact_path(workspace_path, relative_path)
    size_bytes = _write_text(target, content)
    return relative_path, size_bytes, _sha256(target)
