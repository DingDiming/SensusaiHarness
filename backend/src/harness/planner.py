"""Planner role for generating product specs from short user requests."""
from __future__ import annotations

import logging

from .llm_client import DEFAULT_PLANNER_MODEL, LLMUsage, chat_completion

logger = logging.getLogger(__name__)

PLANNER_SYSTEM_PROMPT = """\
You are the Planner for a long-running autonomous software development harness.

Expand a short user request into a concise product specification.
Output Markdown with these sections only:

# Product Spec

## Product Goal
## Target Users
## Core Features
## Visual / UX Direction
## Technical Boundaries
## Sprint Breakdown
## Risks and Open Questions

Rules:
- Keep it under 800 words.
- The sprint breakdown should have 3-8 numbered sprints.
- Do not write implementation code.
"""


async def run_planner(
    user_prompt: str,
    *,
    model: str | None = None,
    user_memory_context: str | None = None,
) -> tuple[str, int, LLMUsage]:
    model = model or DEFAULT_PLANNER_MODEL
    prompt_parts = []
    if user_memory_context:
        prompt_parts.append(f"Known user/project context:\n{user_memory_context}")
    prompt_parts.append(user_prompt)

    spec_content = await chat_completion(
        [
            {"role": "system", "content": PLANNER_SYSTEM_PROMPT},
            {"role": "user", "content": "\n\n".join(prompt_parts)},
        ],
        model=model,
        temperature=0.6,
        max_tokens=4096,
    )
    usage = LLMUsage()
    planned_sprints = _count_sprints(spec_content)
    logger.info("Planner produced spec with %d planned sprint(s)", planned_sprints)
    return spec_content, planned_sprints, usage


def _count_sprints(spec: str) -> int:
    count = 0
    in_section = False
    for line in spec.splitlines():
        lower = line.lower().strip()
        if "sprint breakdown" in lower:
            in_section = True
            continue
        if in_section:
            if lower.startswith("#"):
                break
            if lower.startswith(("- sprint", "sprint")) or (lower and lower[0].isdigit() and "sprint" in lower):
                count += 1
    return max(count, 1)
