"""Workspace directory helpers for persistent run artifacts."""
from __future__ import annotations

from pathlib import Path

_SUBDIRS = [
    "repo",
    "workspace",
    "artifacts",
    "artifacts/sprint_contracts",
    "artifacts/qa_reports",
    "artifacts/handoffs",
    "artifacts/approvals",
    "artifacts/summaries",
    "checkpoints",
    "outputs",
    "uploads",
    "logs",
]


def ensure_workspace(workspace_path: str) -> Path:
    """Create the standard run workspace tree and return its root path."""
    root = Path(workspace_path)
    for subdir in _SUBDIRS:
        (root / subdir).mkdir(parents=True, exist_ok=True)
    return root


def artifact_path(workspace_path: str, relative_path: str) -> Path:
    """Resolve a workspace-relative path and reject traversal."""
    root = Path(workspace_path).resolve()
    target = (root / relative_path).resolve()
    if not str(target).startswith(str(root)):
        raise ValueError(f"Path traversal detected: {relative_path}")
    return target
