# Archived Planning Doc

This document is quarantined historical planning material from the earlier rewrite effort.
It is no longer the active roadmap for the repository.
Prefer `/task.md` and the root `/docs` directory.

# SensusAI Harness — 重写计划

> 基于 v1 经验教训的全面重建。目标：从 "可演示 MVP" 升级为 "真正可用的产品"。

---

## 经验教训（v1 问题总结）

1. **后端先行、前端缺位** — 堆了大量 API 但前端几乎没消费
2. **集成碎片化** — SSE 鉴权不通、聊天单轮、skills 参数未接线
3. **无进度可视化** — 用户无法直观看到项目推进过程
4. **角色硬编码** — planner/generator/evaluator 流水线固定，不可配置
5. **过度设计** — 文档设计宏大但实现跟不上，代码与文档脱节
6. **缺乏端到端验证** — 每个模块独立可用但串起来有断裂

---

## 重写原则

1. **前端驱动** — 每个后端功能都必须有对应的 UI 消费者
2. **可见即可信** — 进度、角色、状态必须在界面上实时可见
3. **简洁优先** — 砍掉不需要的抽象层，代码量减半
4. **端到端验证** — 每个 Phase 必须全栈可测
5. **智能分配** — 角色和模型不再硬编码，支持动态选择和配置

---

## 新架构设计

### 整体结构（简化版）

```
浏览器 (Next.js 15 + shadcn/ui)
  │ HTTPS
  ▼
Rust Core :4000
  ├── /api/core/health
  ├── /api/core/runs/{id}/stream  (SSE，token 参数鉴权)
  ├── /api/app/*  → 反代 Python :8000
  └── /*  → 静态文件 (SPA)
  │
  ▼
Python Harness :8000
  ├── Auth (JWT)
  ├── Threads / Messages (多轮聊天)
  ├── Runs (状态机 + 编排)
  ├── Role Registry (智能角色分配)
  ├── Progress Tracker (进度追踪)
  └── Artifacts / Checkpoints
```

### 关键改进

| v1 | v2 |
|----|-----|
| SSE 用 Authorization header | SSE 用 `?token=` query 参数（EventSource 兼容） |
| 聊天只传单条消息 | 完整对话历史 + 上下文窗口管理 |
| 仅文字 "sprint 2/8" | Sprint 时间线 + 甘特图 + 实时事件流 |
| 硬编码 planner/generator/evaluator | Role Registry：按任务类型动态分配角色+模型 |
| 前端单页无路由 | 完整路由：Dashboard / Thread / Run Detail / Settings |
| 5 秒轮询 | SSE 实时推送 + 断线重连 |
| 前端吞错 | Toast 通知 + 错误边界 + 降级提示 |

---

## 新数据库 Schema（v2 关键变更）

保留 v1 的好设计（runs 状态机、sprint contracts、qa reports），新增：

```sql
-- 角色配置表：支持自定义角色和模型映射
CREATE TABLE role_configs (
  config_id TEXT PRIMARY KEY,
  role_name TEXT NOT NULL,              -- planner / generator / evaluator / researcher / designer
  model_id TEXT NOT NULL,               -- 模型标识，如 gpt-4o, claude-sonnet-4
  system_prompt TEXT,                   -- 角色专属 system prompt
  temperature REAL DEFAULT 0.7,
  max_tokens INTEGER DEFAULT 4096,
  tool_permissions_json TEXT,           -- 角色允许的工具列表
  is_default INTEGER DEFAULT 0,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

-- Run 角色分配表：每次 run 可以独立配置角色
CREATE TABLE run_role_assignments (
  assignment_id TEXT PRIMARY KEY,
  run_id TEXT NOT NULL,
  role_name TEXT NOT NULL,
  config_id TEXT NOT NULL,
  assigned_reason TEXT,                 -- 为什么选这个角色/模型
  created_at TEXT NOT NULL,
  FOREIGN KEY (run_id) REFERENCES runs(run_id) ON DELETE CASCADE,
  FOREIGN KEY (config_id) REFERENCES role_configs(config_id),
  UNIQUE (run_id, role_name)
);

-- 进度快照表：记录 run 的关键时间节点
CREATE TABLE progress_snapshots (
  snapshot_id TEXT PRIMARY KEY,
  run_id TEXT NOT NULL,
  sprint INTEGER NOT NULL,
  phase TEXT NOT NULL,                  -- planning / contracting / building / qa / repair
  started_at TEXT NOT NULL,
  completed_at TEXT,
  duration_seconds INTEGER,
  tokens_used INTEGER DEFAULT 0,
  outcome TEXT,                         -- success / fail / skipped
  details_json TEXT,
  FOREIGN KEY (run_id) REFERENCES runs(run_id) ON DELETE CASCADE
);
```

---

## 实现阶段

### Phase 1: 基础框架 + 数据库（全栈可启动）

**后端**
- [ ] 新 `backend/` — FastAPI app，SQLite (aiosqlite)，完整 schema 迁移
- [ ] Auth: login + JWT 签发
- [ ] Config: 环境配置

**Rust Core**
- [ ] 新 `core/` — axum 入口，health，JWT 验证(query param 支持)，反代
- [ ] SSE endpoint (支持 `?token=` 鉴权)

**前端**
- [ ] 新 `frontend/` — Next.js 15 + App Router + shadcn/ui + Tailwind v4
- [ ] 路由结构: `/login`, `/`, `/threads/[id]`, `/runs/[id]`, `/settings`
- [ ] 登录页（真实接 API）
- [ ] API client（fetch wrapper + token 管理 + 错误处理）

**验证**: 能登录 → 看到空 dashboard → Rust 代理到 Python 正常

---

### Phase 2: Thread + 聊天（多轮上下文）

**后端**
- [ ] Threads CRUD
- [ ] Messages: POST（保存 + LLM 调用 + 流式回复）
- [ ] 多轮对话上下文：自动截断 + rolling summary
- [ ] 聊天流式输出通过 SSE 推送

**前端**
- [ ] Dashboard: Thread 列表 + 新建 Thread
- [ ] ThreadView: 消息列表 + 输入框 + 流式回复展示
- [ ] SSE 消费：实时接收聊天回复

**验证**: 创建 thread → 发消息 → 实时看到 AI 流式回复 → 多轮对话连贯

---

### Phase 3: Run 编排 + 智能角色分配

**后端**
- [ ] Role Registry: CRUD 角色配置，默认提供 planner/generator/evaluator
- [ ] 智能分配逻辑: 根据 prompt 分析推荐角色+模型组合
- [ ] Run 创建: 支持自选或自动分配角色
- [ ] 状态机: queued → planning → contracting → building → qa → checkpointing → completed
- [ ] Runner: 异步执行，每个阶段间检查取消/暂停
- [ ] Planner / Generator / Evaluator（精简版，先接 LLM 小步验证）

**前端**
- [ ] Run 创建对话框：输入需求 → 显示智能分配建议 → 可调整 → 启动
- [ ] Run 列表（Dashboard 里）

**验证**: 创建 run → 看到角色分配 → run 开始执行

---

### Phase 4: 进度仪表盘 + Run 详情

**后端**
- [ ] Progress Tracker: 每个阶段写入 progress_snapshots
- [ ] Run 子资源 API: contracts, qa-reports, artifacts, events, progress
- [ ] Budget 实时跟踪

**前端**
- [ ] **Run 详情页** — 核心体验页面:
  - Sprint 时间线（哪个 sprint，在做什么，用了多长时间）
  - 实时事件流（SSE 驱动的事件时间线）
  - 角色工作面板（当前是哪个角色在工作，做了什么）
  - Contract 查看器（当前 sprint 的合约内容）
  - QA 报告查看器（评分雷达图 + blocking issues）
  - Artifact 浏览器（产出物列表 + 预览）
  - Budget 仪表盘（token/时间/repair 用量环形图）
  - 控制栏（暂停/恢复/取消/审批按钮）
- [ ] **Dashboard 进度概览**:
  - 每个 run 卡片显示进度条 + 当前阶段 + 负载角色
  - 活跃 run 高亮

**验证**: run 执行过程中，前端实时看到 sprint 推进、角色切换、事件流滚动

---

### Phase 5: 审批闸门 + Pause/Resume

**后端**
- [ ] Approval gates: spec_gate / checkpoint_gate / delivery_gate
- [ ] Pause/Resume/Cancel 真正通知 runner

**前端**
- [ ] 审批通知（badge + inline gate 面板）
- [ ] Approve/Reject 按钮 + 备注输入
- [ ] Pause/Resume 按钮状态即时响应

**验证**: run 到达审批点 → 前端弹出审批面板 → approve → run 继续

---

### Phase 6: 记忆系统 + Settings

**后端**
- [ ] User memory CRUD
- [ ] Planner/Generator 注入记忆
- [ ] Thread summary 自动生成

**前端**
- [ ] Settings 页: 角色配置管理、模型选择、记忆查看/编辑
- [ ] Thread 摘要展示

---

## 文件结构（v2）

```
SensusaiHarness/
├── backend/
│   ├── pyproject.toml
│   ├── src/
│   │   ├── main.py               # FastAPI app + lifespan
│   │   ├── config.py             # 环境配置
│   │   ├── db.py                 # SQLite 连接 + 迁移
│   │   ├── auth.py               # JWT + 登录
│   │   ├── deps.py               # FastAPI 依赖
│   │   ├── routes/
│   │   │   ├── threads.py
│   │   │   ├── messages.py
│   │   │   ├── runs.py
│   │   │   ├── roles.py          # NEW: 角色配置
│   │   │   └── settings.py       # NEW: 用户设置/记忆
│   │   ├── harness/
│   │   │   ├── runner.py         # Run 编排器
│   │   │   ├── state_machine.py  # 状态转移表
│   │   │   ├── planner.py
│   │   │   ├── generator.py
│   │   │   ├── evaluator.py
│   │   │   ├── role_registry.py  # NEW: 角色注册+智能分配
│   │   │   ├── progress.py       # NEW: 进度追踪
│   │   │   ├── llm_client.py     # LLM 调用封装
│   │   │   ├── budget.py
│   │   │   ├── workspace.py
│   │   │   ├── artifacts.py
│   │   │   └── event_emitter.py
│   │   └── schemas/
│   │       ├── common.py
│   │       ├── threads.py
│   │       ├── runs.py
│   │       ├── roles.py          # NEW
│   │       └── auth.py
│   └── migrations/
│       └── 001_initial.sql
├── core/
│   ├── Cargo.toml
│   └── src/
│       ├── main.rs
│       ├── config.rs
│       ├── auth.rs               # JWT 验证 (支持 query param)
│       ├── proxy.rs
│       ├── stream.rs             # SSE + replay
│       ├── event_bus.rs
│       └── ingest.rs
├── frontend/
│   ├── package.json
│   └── src/
│       ├── app/
│       │   ├── layout.tsx
│       │   ├── page.tsx          # → redirect to /dashboard
│       │   ├── login/page.tsx
│       │   ├── dashboard/page.tsx
│       │   ├── threads/[id]/page.tsx
│       │   ├── runs/[id]/page.tsx    # NEW: Run 详情页
│       │   └── settings/page.tsx     # NEW
│       ├── components/
│       │   ├── ui/               # shadcn components
│       │   ├── auth-provider.tsx
│       │   ├── sidebar.tsx
│       │   ├── thread-list.tsx
│       │   ├── chat-view.tsx
│       │   ├── run-card.tsx      # Dashboard run 卡片
│       │   ├── run-detail/       # Run 详情页组件组
│       │   │   ├── sprint-timeline.tsx
│       │   │   ├── event-stream.tsx
│       │   │   ├── role-panel.tsx
│       │   │   ├── contract-viewer.tsx
│       │   │   ├── qa-report-viewer.tsx
│       │   │   ├── artifact-browser.tsx
│       │   │   ├── budget-gauge.tsx
│       │   │   └── control-bar.tsx
│       │   ├── role-config.tsx    # 角色配置组件
│       │   └── approval-panel.tsx
│       ├── lib/
│       │   ├── api.ts            # API client + error handling
│       │   ├── sse.ts            # SSE client (token param)
│       │   ├── auth.ts           # Auth context
│       │   └── utils.ts
│       └── hooks/
│           ├── use-sse.ts
│           ├── use-run.ts
│           └── use-threads.ts
├── docs/                         # 保留设计文档
│   ├── ARCHITECTURE.md
│   ├── API.md
│   └── SCHEMA.md
├── docker-compose.yml
└── package.json
```

---

## 智能角色分配系统设计

### 核心思路

不再 planner/generator/evaluator 写死三个模型。用 **Role Registry** 管理：

```python
# 默认角色配置
DEFAULT_ROLES = [
    {
        "role_name": "planner",
        "model_id": "o3",
        "system_prompt": "You are a product planner...",
        "temperature": 0.8,
        "tool_permissions": ["web_search", "file_read"],
    },
    {
        "role_name": "generator",
        "model_id": "claude-sonnet-4-20250514",
        "system_prompt": "You are a code generator...",
        "temperature": 0.3,
        "tool_permissions": ["file_read", "file_write", "shell_exec"],
    },
    {
        "role_name": "evaluator",
        "model_id": "o3",
        "system_prompt": "You are a QA evaluator...",
        "temperature": 0.2,
        "tool_permissions": ["file_read", "shell_exec", "browser"],
    },
    {
        "role_name": "researcher",
        "model_id": "gpt-4o-search-preview",
        "system_prompt": "You research technical topics...",
        "temperature": 0.5,
        "tool_permissions": ["web_search"],
    },
    {
        "role_name": "designer",
        "model_id": "gpt-4o",
        "system_prompt": "You are a UI/UX designer...",
        "temperature": 0.6,
        "tool_permissions": ["file_read", "file_write", "browser"],
    },
]
```

### 智能分配流程

1. 用户输入需求 prompt
2. 系统分析 prompt → 推荐角色组合（如：纯代码任务不需要 designer，研究任务需要 researcher）
3. 用户可调整角色和模型
4. 每个 run 记录实际使用的角色分配

---

## 进度追踪系统设计

### 数据流

```
Runner 每个阶段 start/end → progress_snapshots 表
  │
  ├── Phase 开始 → insert snapshot (started_at)
  ├── Phase 完成 → update snapshot (completed_at, duration, outcome)
  ├── 同时 → SSE event: progress_update
  │
  ▼
前端 Sprint Timeline 组件实时渲染
```

### 前端展示

```
Run #abc123  ━━━━━━━━━━━━━━━━━━━━━━━━━━━━

Sprint 1/6  ✅ Planning (2m) → ✅ Contracting (1m) → ✅ Building (8m) → ✅ QA (3m) → ✅ Checkpoint
Sprint 2/6  ✅ Contracting (1m) → 🔄 Building (5m elapsed) → ○ QA → ○ Checkpoint
Sprint 3/6  ○ ...
Sprint 4/6  ○ ...

[当前角色: generator (claude-sonnet-4-20250514)]  [Token: 185K/2.5M]  [时间: 34m/360m]
```

---

## 开始实施

从 Phase 1 开始，每个 Phase 完成后全栈验证。
