"""SensusAI Harness — Pydantic schemas."""

from __future__ import annotations

from pydantic import BaseModel, Field
from typing import Any


class LoginRequest(BaseModel):
    username: str
    password: str

class TokenResponse(BaseModel):
    access_token: str
    expires_at: str
    user: dict

class CreateThread(BaseModel):
    title: str
    default_mode: str = "autonomous"

class ThreadResponse(BaseModel):
    thread_id: str
    title: str
    default_mode: str
    status: str
    active_run_id: str | None = None
    summary_text: str | None = None
    created_at: str
    updated_at: str

class SendMessage(BaseModel):
    message: str
    model: str | None = None

class MessageResponse(BaseModel):
    message_id: str
    thread_id: str
    role: str
    content: str
    created_at: str

class CreateRun(BaseModel):
    prompt: str
    max_sprints: int = 6
    approval_gates: list[str] = Field(default_factory=lambda: ["spec_gate", "delivery_gate"])
    role_overrides: dict[str, str] | None = None

class RunResponse(BaseModel):
    run_id: str
    thread_id: str
    prompt: str
    mode: str
    state: str
    current_sprint: int
    planned_sprints: int | None
    roles: list[dict] | None = None
    budget: dict | None = None
    created_at: str
    updated_at: str

class RunDetailResponse(RunResponse):
    progress: list[dict] = Field(default_factory=list)
    events: list[dict] = Field(default_factory=list)
    contracts: list[dict] = Field(default_factory=list)
    qa_reports: list[dict] = Field(default_factory=list)
    active_gate: dict | None = None

class RoleConfigCreate(BaseModel):
    role_name: str
    model_id: str
    system_prompt: str | None = None
    temperature: float = 0.7
    max_tokens: int = 4096
    tool_permissions: list[str] = Field(default_factory=list)
    is_default: bool = False

class RoleConfigResponse(BaseModel):
    config_id: str
    role_name: str
    model_id: str
    system_prompt: str | None
    temperature: float
    max_tokens: int
    tool_permissions: list[str]
    is_default: bool
    created_at: str

class RoleSuggestion(BaseModel):
    role_name: str
    suggested_config_id: str
    suggested_model: str
    reason: str

class ApprovalAction(BaseModel):
    gate_id: str
    note: str = ""
