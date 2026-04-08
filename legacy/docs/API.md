# Archived Web-First Doc

This document is quarantined legacy material from the previous Web-first system.
It is not the source of truth for the current terminal-first Rust product.
Prefer `/docs/ARCHITECTURE.md`, `/docs/OPERATIONS.md`, and `/docs/COMPATIBILITY.md` in the repository root.

# SensusAI Harness API 规格

> 目标：把 [ARCHITECTURE.md](/Users/ddm/Documents/GitHub/SensusaiHarness/docs/ARCHITECTURE.md) 中的接口层设计细化成可直接实现的 HTTP / SSE 契约。
> 范围：MVP 优先覆盖 `thread`、`run`、`approval gate`、`artifact`、`checkpoint`、`SSE stream`。

---

## 1. 设计约定

### 1.1 基础约定

- 所有 JSON API 使用 `application/json; charset=utf-8`
- 所有时间字段使用 UTC RFC3339，例如 `2026-04-02T10:30:00Z`
- 所有主键使用 UUID 字符串，推荐 UUIDv7
- `POST` 创建型接口默认支持幂等键 `Idempotency-Key`
- 列表接口默认支持 `limit` / `cursor`

### 1.2 路由分层

- `/api/core/*`：Rust Core 处理
- `/api/app/*`：Python Harness Runtime 处理

Rust Core 负责：

- JWT 验证 + X-User-Id/X-Request-Id 注入
- run 级 SSE 流
- Python→Rust 事件推送接收 (内部端点)
- 文件上传 / 下载（待实现）
- metrics / health（metrics 待实现）

Python Harness Runtime 负责：

- **auth 登录 / 注册** (MVP 阶段由 Python 直接处理)
- thread / run 业务逻辑
- planner / generator / evaluator 编排
- approval gate、checkpoint、artifact 元数据
- memory / skills / browser 业务封装

状态存储约束：

- `thread`、`run`、`run_events`、`approval_gates`、`user_memory` 统一存同一个 SQLite 数据库
- artifact 正文、日志、截图、输出文件落盘到 workspace
- memory API 只读写 SQLite 中的长期记忆表，不读写 JSON 文件

### 1.3 认证

除登录、注册、health 外，其余接口都要求：

```http
Authorization: Bearer <jwt>
```

Rust Core 验证 JWT 后向 Python 上游注入：

```http
X-User-Id: <user_id>
X-Request-Id: <request_id>
```

### 1.4 错误格式

MVP 使用 FastAPI 默认错误格式：

```json
{
  "detail": "Run not found"
}
```

后续可升级为结构化格式：

```json
{
  "error": {
    "code": "run_not_found",
    "message": "Run does not exist or is not visible to the current user.",
    "retryable": false
  }
}
```

错误码约定：

- `invalid_request`
- `unauthorized`
- `forbidden`
- `thread_not_found`
- `run_not_found`
- `gate_not_found`
- `conflict_state`
- `budget_exceeded`
- `artifact_not_found`
- `upload_too_large`
- `rate_limited`
- `internal_error`

---

## 2. 核心资源模型

### 2.1 Thread

```json
{
  "thread_id": "01hr_thread",
  "title": "Retro game maker",
  "default_mode": "autonomous",
  "summary": "Create a retro game editor app.",
  "active_run_id": "01hr_run",
  "created_at": "2026-04-02T10:00:00Z",
  "updated_at": "2026-04-02T10:05:00Z"
}
```

### 2.2 Run

```json
{
  "run_id": "01hr_run",
  "thread_id": "01hr_thread",
  "mode": "autonomous",
  "state": "building",
  "current_sprint": 2,
  "planned_sprints": 8,
  "active_gate": null,
  "budget": {
    "wall_clock_minutes_used": 34,
    "wall_clock_minutes_limit": 360,
    "tokens_used": 188420,
    "tokens_limit": 2500000,
    "repair_count_current_sprint": 1,
    "repair_limit_current_sprint": 3
  },
  "latest_checkpoint": {
    "name": "sprint-01",
    "commit_sha": "abc123"
  },
  "created_at": "2026-04-02T10:00:00Z",
  "updated_at": "2026-04-02T10:34:00Z"
}
```

### 2.3 Approval Gate

```json
{
  "gate_id": "gate_01",
  "run_id": "01hr_run",
  "gate_type": "checkpoint_gate",
  "status": "awaiting_user",
  "title": "Approve sprint 2 checkpoint",
  "summary": "User auth and project dashboard are ready for review.",
  "artifact_paths": [
    "artifacts/approvals/approval-02.json",
    "artifacts/qa_reports/sprint-02.json"
  ],
  "created_at": "2026-04-02T11:10:00Z"
}
```

### 2.4 Sprint Contract

```json
{
  "contract_id": "contract_02",
  "run_id": "01hr_run",
  "sprint": 2,
  "status": "accepted",
  "scope_in": [
    "Project dashboard",
    "Project creation flow"
  ],
  "scope_out": [
    "Sprite editor"
  ],
  "tests_to_run": [
    "pytest tests/test_projects.py",
    "pnpm test -- dashboard.spec.ts"
  ]
}
```

### 2.5 QA Report

```json
{
  "qa_report_id": "qa_02",
  "run_id": "01hr_run",
  "sprint": 2,
  "result": "fail",
  "scores": {
    "functionality": 0.62,
    "product_depth": 0.81,
    "ux_quality": 0.72,
    "code_quality": 0.78
  },
  "blocking_issues": [
    {
      "title": "Dashboard create button does not persist the new project",
      "severity": "high",
      "repro_steps": [
        "Open dashboard",
        "Create a project",
        "Refresh page"
      ]
    }
  ]
}
```

---

## 3. Rust Core API

### 3.1 `GET /api/core/health`

响应 `200`：

```json
{
  "status": "ok",
  "version": "0.1.0",
  "time": "2026-04-02T10:00:00Z"
}
```

### 3.2 `GET /api/core/runs/{run_id}/stream`

建立 run 级 SSE 流。

请求头：

```http
Accept: text/event-stream
Last-Event-ID: 142
Authorization: Bearer <jwt>
```

响应：

```text
id: 143
event: run_state
data: {"run_id":"01hr_run","state":"qa","current_sprint":2}
```

状态码：

- `200`：流已建立
- `401`：未授权
- `404`：run 不存在

### 3.3 `POST /internal/runs/{run_id}/events`

Python → Rust 内部事件推送（无鉴权，仅 localhost）。

请求：

```json
{
  "event_type": "run_state",
  "data": {"state": "planning", "run_id": "01hr_run"}
}
```

响应 `202`：

```json
{"event_id": 42}
```

### 3.4 `POST /api/core/runs/{run_id}/uploads` （待实现）

向 run 工作区上传文件，`multipart/form-data`。

字段：

- `file`: 二进制文件，可多次出现
- `target`: 可选，默认 `uploads/`

响应 `201`：

```json
{
  "uploaded": [
    {
      "name": "spec.pdf",
      "path": "uploads/spec.pdf",
      "size_bytes": 182934
    }
  ]
}
```

### 3.5 `GET /api/core/runs/{run_id}/artifacts/{path}` （待实现）

下载 artifact 或输出文件。

约束：

- `path` 必须是 run 根目录下的相对路径
- Rust Core 做 canonicalize 和前缀校验

### 3.6 `GET /api/core/runs/{run_id}/checkpoints/{name}` （待实现）

返回 checkpoint 元数据，必要时可附带 patch URL。

响应 `200`：

```json
{
  "name": "sprint-02",
  "commit_sha": "abc123",
  "created_at": "2026-04-02T11:00:00Z",
  "summary": "Project dashboard and create flow",
  "patch_artifact_path": "checkpoints/sprint-02.patch"
}
```

---

## 4. Python App API

### 4.0 Auth

> MVP 阶段登录由 Python 处理（经 Rust 代理）。

#### `POST /api/app/auth/login`

登录并返回 JWT。

请求：

```json
{
  "username": "admin",
  "password": "secret"
}
```

响应 `200`：

```json
{
  "access_token": "eyJ...",
  "expires_at": "2026-04-03T10:00:00Z",
  "user": {
    "user_id": "u_01",
    "username": "admin"
  }
}
```

### 4.1 Threads

#### `POST /api/app/threads`

创建 thread。

请求：

```json
{
  "title": "Retro game maker",
  "default_mode": "autonomous"
}
```

响应 `201`：

```json
{
  "thread_id": "01hr_thread",
  "title": "Retro game maker",
  "default_mode": "autonomous",
  "created_at": "2026-04-02T10:00:00Z",
  "updated_at": "2026-04-02T10:00:00Z"
}
```

#### `GET /api/app/threads`

返回 thread 列表。

#### `GET /api/app/threads/{thread_id}`

返回 thread、最近消息摘要、活跃 run 概况。

#### `DELETE /api/app/threads/{thread_id}`

软删除 thread；已完成 run 保留审计记录。

### 4.2 Chat Messages

#### `POST /api/app/threads/{thread_id}/messages`

Chat Mode 消息入口。

请求：

```json
{
  "message": "Explain the current architecture tradeoffs.",
  "model": "gpt-5.4",
  "skills": ["research"]
}
```

响应 `202`：

```json
{
  "thread_id": "01hr_thread",
  "message_id": "msg_01",
  "interactive_run_id": "run_chat_01",
  "state": "queued"
}
```

### 4.3 Runs

#### `POST /api/app/threads/{thread_id}/runs`

启动 autonomous run。

请求：

```json
{
  "prompt": "Create a 2D retro game maker with a level editor and playable test mode.",
  "planner_model": "planner-model",
  "generator_model": "generator-model",
  "evaluator_model": "evaluator-model",
  "max_sprints": 8,
  "max_wall_clock_minutes": 360,
  "approval_gates": ["spec_gate", "checkpoint_gate", "delivery_gate"],
  "skills": ["frontend-design"]
}
```

响应 `202`：

```json
{
  "run_id": "01hr_run",
  "thread_id": "01hr_thread",
  "mode": "autonomous",
  "state": "queued",
  "current_sprint": 0,
  "planned_sprints": 8,
  "active_gate": null,
  "budget": {
    "wall_clock_minutes_used": 0,
    "wall_clock_minutes_limit": 360,
    "tokens_used": 0,
    "tokens_limit": 2500000,
    "repair_count_current_sprint": 0,
    "repair_limit_current_sprint": 3
  },
  "latest_checkpoint": null,
  "created_at": "2026-04-02T10:00:00Z",
  "updated_at": "2026-04-02T10:00:00Z"
}
```

#### `GET /api/app/runs`

查询当前用户的 run 列表。

过滤参数：

- `state`
- `thread_id`
- `mode`
- `limit`
- `cursor`

#### `GET /api/app/runs/{run_id}`

返回完整 run 视图。

响应 `200`：

```json
{
  "run_id": "01hr_run",
  "thread_id": "01hr_thread",
  "mode": "autonomous",
  "state": "awaiting_approval",
  "current_sprint": 2,
  "planned_sprints": 8,
  "active_gate": {
    "gate_id": "gate_02",
    "gate_type": "checkpoint_gate",
    "status": "awaiting_user"
  },
  "budget": {
    "wall_clock_minutes_used": 58,
    "wall_clock_minutes_limit": 360,
    "tokens_used": 318442,
    "tokens_limit": 2500000,
    "repair_count_current_sprint": 0,
    "repair_limit_current_sprint": 3
  },
  "latest_checkpoint": {
    "name": "sprint-01",
    "commit_sha": "abc123"
  },
  "created_at": "2026-04-02T10:00:00Z",
  "updated_at": "2026-04-02T10:58:00Z"
}
```

#### `POST /api/app/runs/{run_id}/pause`

请求后台进入 `paused`。

请求：

```json
{
  "reason": "User requested manual review"
}
```

响应 `202`：

```json
{
  "run_id": "01hr_run",
  "state": "paused"
}
```

#### `POST /api/app/runs/{run_id}/resume`

恢复 paused 或 interrupted run。

请求：

```json
{
  "resume_from": "latest_checkpoint"
}
```

响应 `202`：

```json
{
  "run_id": "01hr_run",
  "state": "queued"
}
```

#### `POST /api/app/runs/{run_id}/cancel`

取消 run；后台应尽快安全停机。

### 4.4 Approval

#### `POST /api/app/runs/{run_id}/approve`

批准当前 gate。

请求：

```json
{
  "gate_id": "gate_02",
  "note": "Continue to next sprint."
}
```

响应 `200`：

```json
{
  "run_id": "01hr_run",
  "gate_id": "gate_02",
  "decision": "approved",
  "next_state": "checkpointing"
}
```

#### `POST /api/app/runs/{run_id}/reject`

拒绝当前 gate。

请求：

```json
{
  "gate_id": "gate_02",
  "note": "Project creation flow still feels broken."
}
```

### 4.5 Run 子资源

#### `GET /api/app/runs/{run_id}/contracts`

返回 sprint contract 历史。

#### `GET /api/app/runs/{run_id}/qa-reports`

返回 QA 历史。

#### `GET /api/app/runs/{run_id}/artifacts`

返回 artifact 索引。

响应 `200`：

```json
{
  "items": [
    {
      "path": "artifacts/product_spec.md",
      "kind": "product_spec",
      "size_bytes": 4832,
      "created_at": "2026-04-02T10:03:00Z"
    }
  ]
}
```

#### `GET /api/app/runs/{run_id}/checkpoints`

返回 checkpoint 列表。

### 4.6 Skills / Models / Memory

#### `GET /api/app/skills`

返回 skills 列表和启用状态。

#### `PUT /api/app/skills/{name}`

启用或禁用 skill。

请求：

```json
{
  "enabled": true
}
```

#### `POST /api/app/skills/install`

安装 skill。

#### `GET /api/app/models`

返回 planner / generator / evaluator 可选模型。

#### `GET /api/app/memory`

返回当前用户的长期记忆。

响应 `200`：

```json
{
  "user_id": "u_01",
  "preferences": {
    "preferred_stack": "React + FastAPI + SQLite",
    "code_style": "strict typing"
  },
  "facts": [
    {
      "key": "default_database",
      "value": "sqlite",
      "source": "user_confirmed",
      "updated_at": "2026-04-02T12:00:00Z"
    }
  ],
  "updated_at": "2026-04-02T12:00:00Z"
}
```

#### `PUT /api/app/memory`

覆盖或局部更新用户长期记忆。

请求：

```json
{
  "preferences": {
    "preferred_stack": "React + FastAPI + SQLite"
  },
  "facts_upsert": [
    {
      "key": "default_database",
      "value": "sqlite",
      "source": "user_confirmed"
    }
  ]
}
```

说明：

- 后端写入 SQLite 的 `user_memory_entries` 等表
- 不允许通过此接口写 run 恢复状态
- 建议 memory entry 至少带 `source`、`confidence`、`confirmed_by_user`

### 4.7 Browser

#### `POST /api/app/browser/search`

请求：

```json
{
  "query": "retro pixel game editor UX patterns",
  "limit": 5
}
```

#### `POST /api/app/browser/fetch`

请求：

```json
{
  "url": "https://example.com"
}
```

#### `POST /api/app/browser/interact`

供 evaluator 或用户调试使用。

请求：

```json
{
  "url": "http://127.0.0.1:3000",
  "steps": [
    {"action": "click", "selector": "[data-test=create-project]"},
    {"action": "type", "selector": "input[name=name]", "text": "Demo"}
  ]
}
```

#### `POST /api/app/browser/screenshot`

返回截图 artifact 路径。

---

## 5. SSE 事件契约

### 5.1 事件类型

| event | 含义 | 最小字段 |
|------|------|----------|
| `run_state` | run 状态变化 | `run_id`, `state`, `current_sprint` |
| `message` | agent 文本输出 | `role`, `content` |
| `contract` | sprint contract 草案 / 确认 | `sprint`, `status`, `path` |
| `tool_call` | 工具调用开始 | `tool`, `call_id` |
| `tool_result` | 工具调用结束 | `tool`, `call_id`, `ok` |
| `qa_report` | evaluator 结果 | `sprint`, `result`, `path` |
| `checkpoint` | git checkpoint | `name`, `commit_sha` |
| `approval` | 等待人工审批 | `gate_id`, `gate_type` |
| `budget` | 预算更新 | `tokens_used`, `wall_clock_minutes_used` |
| `artifact` | 新 artifact 可读 | `path`, `kind` |
| `error` | 错误 | `code`, `message`, `retryable` |
| `done` | run 结束 | `result`, `summary`, `artifacts` |

### 5.2 事件幂等

- `id` 必须单调递增
- 前端以 `(run_id, event_id)` 去重
- Rust Core 必须支持 `Last-Event-ID` 重放

### 5.3 心跳

无业务事件时，Rust Core 每 15 秒发一条注释心跳：

```text
: keep-alive
```

---

## 6. 状态码和并发语义

### 6.1 推荐状态码

- `200 OK`：同步成功
- `201 Created`：资源创建成功
- `202 Accepted`：后台异步执行已排队
- `204 No Content`：删除成功
- `400 Bad Request`：参数错误
- `401 Unauthorized`：未认证
- `403 Forbidden`：无权访问
- `404 Not Found`：资源不存在
- `409 Conflict`：当前状态不允许该操作
- `413 Payload Too Large`：上传过大
- `429 Too Many Requests`：被限流
- `500 Internal Server Error`：系统错误

### 6.2 状态冲突示例

- 已 `completed` 的 run 调用 `/pause` → `409`
- 非 `awaiting_approval` 的 run 调用 `/approve` → `409`
- `gate_id` 与当前 gate 不匹配 → `409`

---

## 7. MVP 落地建议

优先实现这些接口：

1. `POST /api/app/threads`
2. `POST /api/app/threads/{thread_id}/runs`
3. `GET /api/app/runs/{run_id}`
4. `GET /api/core/runs/{run_id}/stream`
5. `POST /api/app/runs/{run_id}/approve`
6. `POST /api/app/runs/{run_id}/resume`
7. `GET /api/app/runs/{run_id}/contracts`
8. `GET /api/app/runs/{run_id}/qa-reports`
9. `GET /api/app/runs/{run_id}/artifacts`
10. `GET /api/app/runs/{run_id}/checkpoints`

完成这些后，前后端就能跑通一个真正的 long-running harness MVP。
