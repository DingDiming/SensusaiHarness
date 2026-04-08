"""LLM client helpers for lightweight chat calls and Codex-backed execution."""
from __future__ import annotations

import asyncio
import json
import os
import subprocess
import tempfile
from dataclasses import dataclass, field
from pathlib import Path

from ..config import settings

DEFAULT_PLANNER_MODEL = "gpt-5.4-mini"
DEFAULT_GENERATOR_MODEL = "gpt-5.4"
DEFAULT_EVALUATOR_MODEL = "gpt-5.4-mini"


@dataclass(slots=True)
class LLMUsage:
    input_tokens: int = 0
    cached_input_tokens: int = 0
    output_tokens: int = 0

    @property
    def total_tokens(self) -> int:
        return self.input_tokens + self.output_tokens


@dataclass(slots=True)
class CodexExecResult:
    text: str = ""
    commands: list[dict] = field(default_factory=list)
    file_changes: list[dict] = field(default_factory=list)
    usage: LLMUsage = field(default_factory=LLMUsage)
    error: str | None = None
    raw_events: list[dict] = field(default_factory=list)


def _codex_available() -> bool:
    import shutil

    return shutil.which("codex") is not None


def _compose_prompt(prompt: str, system: str | None = None) -> str:
    if not system:
        return prompt
    return f"[system]\n{system}\n\n{prompt}"


def _make_env() -> dict[str, str]:
    env = {**os.environ, "NO_COLOR": "1"}
    codex_data = os.getenv("CODEX_DATA_DIR", "")
    if codex_data:
        env["CODEX_HOME"] = codex_data
    return env


def _parse_jsonl_output(raw: str) -> list[dict]:
    events: list[dict] = []
    for line in raw.splitlines():
        line = line.strip()
        if not line or not line.startswith("{"):
            continue
        try:
            events.append(json.loads(line))
        except json.JSONDecodeError:
            continue
    return events


def _collect_git_file_changes(cwd: str | None) -> list[dict]:
    if not cwd:
        return []

    try:
        proc = subprocess.run(
            ["git", "status", "--porcelain"],
            cwd=cwd,
            capture_output=True,
            text=True,
            check=False,
        )
    except OSError:
        return []

    if proc.returncode != 0:
        return []

    changes: list[dict] = []
    for line in proc.stdout.splitlines():
        if len(line) < 4:
            continue
        status = line[:2].strip() or "??"
        path = line[3:].strip()
        if " -> " in path:
            path = path.split(" -> ", 1)[1]
        changes.append({"status": status, "path": path})
    return changes


async def _call_codex(prompt: str, system: str | None = None, model: str | None = None) -> str:
    model = model or settings.default_model
    cmd = ["codex", "exec", "--ephemeral", "--skip-git-repo-check", "-m", model]

    with tempfile.NamedTemporaryFile(mode="w", suffix=".txt", delete=False) as out:
        out_path = out.name

    try:
        cmd += ["-o", out_path, "--", _compose_prompt(prompt, system)]
        proc = await asyncio.create_subprocess_exec(
            *cmd,
            stdout=asyncio.subprocess.PIPE,
            stderr=asyncio.subprocess.PIPE,
            env=_make_env(),
        )
        _, stderr = await asyncio.wait_for(proc.communicate(), timeout=300)
        if proc.returncode != 0:
            err = stderr.decode(errors="replace").strip()
            raise RuntimeError(f"codex exec failed (rc={proc.returncode}): {err}")
        result = Path(out_path).read_text(encoding="utf-8").strip()
        return result or "No output from codex"
    finally:
        Path(out_path).unlink(missing_ok=True)


async def codex_exec(
    prompt: str,
    *,
    system_prompt: str | None = None,
    model: str | None = None,
    cwd: str | None = None,
    full_auto: bool = False,
    ephemeral: bool = True,
    skip_git_check: bool = False,
    timeout_seconds: int = 600,
    add_dirs: list[str] | None = None,
) -> CodexExecResult:
    """Run Codex in JSON mode and collect command/file-change evidence."""
    model = model or settings.default_model
    cmd = ["codex", "exec", "--json", "-m", model]
    if ephemeral:
        cmd.append("--ephemeral")
    if full_auto:
        cmd.append("--full-auto")
    if cwd:
        cmd += ["-C", cwd]
    if skip_git_check:
        cmd.append("--skip-git-repo-check")
    if add_dirs:
        for directory in add_dirs:
            cmd += ["--add-dir", directory]
    cmd += ["--", _compose_prompt(prompt, system_prompt)]

    proc = await asyncio.create_subprocess_exec(
        *cmd,
        stdout=asyncio.subprocess.PIPE,
        stderr=asyncio.subprocess.PIPE,
        env=_make_env(),
    )
    stdout, stderr = await asyncio.wait_for(proc.communicate(), timeout=timeout_seconds)

    stdout_text = stdout.decode("utf-8", errors="replace")
    stderr_text = stderr.decode("utf-8", errors="replace")
    events = _parse_jsonl_output(stdout_text)

    result = CodexExecResult(raw_events=events)
    if proc.returncode != 0:
        result.error = (stderr_text or stdout_text or f"codex exec failed with rc={proc.returncode}").strip()
        return result

    for event in events:
        event_type = event.get("type")
        if event_type == "item.completed":
            item = event.get("item", {})
            item_type = item.get("type")
            if item_type == "agent_message":
                result.text = item.get("text", result.text)
            elif item_type == "command_execution":
                result.commands.append(
                    {
                        "command": item.get("command", ""),
                        "output": item.get("aggregated_output", ""),
                        "exit_code": item.get("exit_code"),
                        "status": item.get("status"),
                    }
                )
        elif event_type == "turn.completed":
            usage = event.get("usage", {})
            result.usage = LLMUsage(
                input_tokens=int(usage.get("input_tokens", 0) or 0),
                cached_input_tokens=int(usage.get("cached_input_tokens", 0) or 0),
                output_tokens=int(usage.get("output_tokens", 0) or 0),
            )

    result.file_changes = _collect_git_file_changes(cwd)
    return result


async def _call_openai(messages: list[dict], model: str, temperature: float, max_tokens: int) -> str:
    import openai

    client = openai.AsyncOpenAI(api_key=settings.openai_api_key or os.getenv("OPENAI_API_KEY"))
    response = await client.chat.completions.create(
        model=model,
        messages=messages,
        temperature=temperature,
        max_tokens=max_tokens,
    )
    return response.choices[0].message.content or ""


async def chat_completion(
    messages: list[dict],
    model: str | None = None,
    temperature: float = 0.7,
    max_tokens: int = 4096,
) -> str:
    model = model or settings.default_model
    if settings.use_codex and _codex_available():
        system = None
        prompt_parts = []
        for msg in messages:
            if msg["role"] == "system":
                system = msg["content"]
            else:
                prompt_parts.append(f"[{msg['role']}]\n{msg['content']}")
        prompt = "\n\n".join(prompt_parts)
        return await _call_codex(prompt, system=system, model=model)
    return await _call_openai(messages, model, temperature, max_tokens)
