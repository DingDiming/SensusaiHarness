"""Harness — State machine transitions."""

from __future__ import annotations

# Valid state transitions
TRANSITIONS: dict[str, list[str]] = {
    "queued": ["planning"],
    "planning": ["awaiting_approval", "contracting", "failed"],
    "awaiting_approval": ["contracting", "completed", "failed"],
    "contracting": ["building", "failed"],
    "building": ["qa", "interrupted", "failed"],
    "qa": ["checkpointing", "repair", "failed"],
    "repair": ["building", "failed"],
    "checkpointing": ["contracting", "awaiting_approval", "completed"],
    "paused": ["queued"],
    "interrupted": ["queued"],
}

TERMINAL_STATES = {"completed", "failed", "cancelled"}


def can_transition(current: str, target: str) -> bool:
    return target in TRANSITIONS.get(current, [])
