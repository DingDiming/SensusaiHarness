# Archived Web-First Doc

This document is quarantined legacy material from the previous Web-first system.
It conflicts with the current terminal-first Rust architecture and should not guide new implementation work.
Prefer `/docs/ARCHITECTURE.md` in the repository root.

# SensusAI Harness — 架构设计文档

> 私有化 Web 版 long-running Agent Harness，Rust 底座 + Python Harness Runtime + Codex / Responses API 执行层。
> **部署模式**：私有化单机运行，支持多小时自主任务与人工审批混合流程。

---

## 1. 系统定位

SensusAI Harness 不是单纯的“Web 版聊天壳”，而是一个**面向长时间运行软件开发任务的 Agent Harness**。平台需要同时解决两类问题：

- **平台层问题**：鉴权、流式输出、工作区隔离、文件传输、审计、可观测性
- **Harness 层问题**：长任务拆解、上下文控制、QA 验收、审批闸门、失败恢复、断点续跑

系统采用 **Rust 底座 + Python 智能层** 分治架构：

- **前端**：React/Next.js Web UI，提供对话、任务监控、Sprint/QA 视图、审批和恢复入口
- **Rust Core（底座层）**：高性能网关、认证鉴权、资源监管、工作区隔离、流式桥接、审计
- **Python Harness Runtime（智能层）**：planner / generator / evaluator 编排、技能解析、记忆提取、artifact 协议、run 状态机
- **执行层**：OpenAI Responses API / Codex 兼容执行通道，必要时配合本地 CLI、git、测试命令和浏览器工具
- **技能层**：复用 Codex 的 `SKILL.md` 技能体系，Web 端和 CLI 共享同一套技能

系统支持两种运行模式：

- **Chat Mode**：短对话、低延迟、单次任务，偏交互式
- **Autonomous Build Mode**：多小时长任务，按 sprint 推进，带 evaluator QA 和 checkpoint

**为什么分 Rust + Python 两层？**

| 关注点 | Rust 擅长 | Python 擅长 |
|--------|-----------|-------------|
| 网络 IO / 流式传输 | ✅ 零拷贝、低延迟、高并发 | ❌ GIL 制约 |
| 进程管理 / 信号处理 | ✅ 精确控制、无 GC 暂停 | ⚠️ 可用但粗糙 |
| 安全审计 / 沙箱隔离 | ✅ 内存安全、seccomp 集成 | ❌ 运行时漏洞面大 |
| JWT 验证 / Rate Limit | ✅ 微秒级、无分配 | ⚠️ 可用但慢 |
| LLM API 调用 / Prompt 工程 | ❌ 生态薄弱 | ✅ langchain/openai 成熟 |
| 技能解析 / Markdown 处理 | ❌ 非必要 | ✅ 库丰富 |
| 快速迭代业务逻辑 | ❌ 编译周期长 | ✅ 热重载 |

参考项目：
- [DeerFlow](https://github.com/bytedance/deer-flow) — 超级 Agent Harness 架构（LangGraph + Gateway + 前端 + 沙箱）
- [Harness](https://github.com/harness/harness) — 开源 DevOps 平台（Go 核心 + TS 前端，流水线编排）
- [Cloudflare Pingora](https://github.com/cloudflare/pingora) — Rust 高性能代理（架构参考）
- [Anthropic: Harness design for long-running application development](https://www.anthropic.com/engineering/harness-design-long-running-apps) — 长时间运行 harness 的 planner / generator / evaluator、sprint contract、QA loop 设计参考

---

## 2. 整体架构

```
                    ┌─────────────────────────────────────────────┐
                    │            Cloudflare (DNS/CDN/WAF)         │
                    │         DDoS Protection + Edge Caching      │
                    └───────────────────┬─────────────────────────┘
                                        │ HTTPS
    ════════════════════════════════════════════════════════════════
    ║                  Rust Core (sensusai-core)                  ║
    ║                    单二进制，端口 :4000                       ║
    ║  ┌────────────────────────────────────────────────────────┐  ║
    ║  │                   Gateway Layer                        │  ║
    ║  │  TLS Termination · Reverse Proxy · CORS · Compression  │  ║
    ║  └──────┬───────────────────┬────────────────────┬────────┘  ║
    ║         │                   │                    │           ║
    ║    /api/app/*          /api/core/*          /* (static)      ║
    ║    (→ Python)          (Rust 直接处理)      (→ Frontend)     ║
    ║         │                   │                    │           ║
    ║  ┌──────▼──────┐  ┌────────▼─────────┐  ┌───────▼────────┐  ║
    ║  │ HTTP Proxy   │  │  Auth (JWT/OAuth)│  │ Static Server  │  ║
    ║  │ → :8000      │  │  Rate Limiter    │  │ (SPA fallback) │  ║
    ║  └──────────────┘  │  RBAC Enforcer   │  └────────────────┘  ║
    ║                    ├──────────────────┤                      ║
    ║                    │  Process          │                     ║
    ║                    │  Supervisor       │                     ║
    ║                    │  (浏览器/工具监管) │                     ║
    ║                    ├──────────────────┤                      ║
    ║                    │  Stream Bridge    │                     ║
    ║                    │  (SSE Fan-out)    │  ←── 零拷贝转发      ║
    ║                    ├──────────────────┤                      ║
    ║                    │  Workspace Guard  │                     ║
    ║                    │  (隔离 + quota)    │                     ║
    ║                    ├──────────────────┤                      ║
    ║                    │  Audit Logger     │                     ║
    ║                    │  (结构化日志)      │                     ║
    ║                    └──────────────────┘                      ║
    ════════════════════════════════════════════════════════════════
                    │                        │
         ┌──────────▼──────────┐    ┌────────▼───────────┐
         │ Python Harness    │    │  Frontend (Next.js)│
         │ Runtime :8000     │    │  Build Output :3000│
         │                   │    │                    │
         │ ┌───────────────┐ │    │  Chat UI           │
         │ │ Planner       │ │    │  Run Monitor       │
         │ ├───────────────┤ │    │  Diff / QA Viewer  │
         │ │ Generator     │ │    │  Skill Manager     │
         │ ├───────────────┤ │    │  Settings          │
         │ │ Evaluator     │ │    └────────────────────┘
         │ ├───────────────┤ │
         │ │ Artifact /    │ │
         │ │ Budget /      │ │
         │ │ Run State     │ │
         │ └───────────────┘ │
         └──────────┬──────────┘
                    │ (gRPC / Unix Socket)
         ┌──────────▼──────────┐
         │  Codex CLI / API     │
         │  (宿主机进程)         │
         │                      │
         │  由 Rust Process     │
         │  Supervisor 托管     │
         └──────────┬───────────┘
                    │
         ┌──────────▼──────────┐
         │  Workspace (隔离)    │
         │  /data/workspaces/   │
         │  Rust Workspace      │
         │  Guard 管控配额       │
         └─────────────────────┘
```

### 2.1 请求流转路径

```
浏览器
  │ HTTPS
  ▼
Cloudflare (CDN/WAF)
  │
  ▼
Rust Core :4000
  ├─ 静态资源 → 直接返回 (Rust 内置 static file server)
  ├─ /api/core/* → Rust 直接处理 (auth, health, metrics, stream)
  └─ /api/app/* → 鉴权后反代 → Python FastAPI :8000
                                    │
                                    ├─ 业务逻辑处理
                                    ├─ 调用 Responses API / 浏览器 / 测试工具
                                    └─ 归一化为 RunEvent → 写入 Rust Stream Bridge →
                                         Rust fan-out 到浏览器
```

### 2.2 长时间运行 Harness 流转

```
用户输入 1-4 句需求
  │
  ▼
Planner
  ├─ 生成 product_spec.md
  ├─ 定义目标用户 / 核心功能 / 设计语言 / 技术边界
  └─ 拆成 6-12 个 sprint
        │
        ▼
Generator ↔ Evaluator
  ├─ 先协商 sprint_contract.json
  ├─ Generator 只做当前 sprint
  ├─ Evaluator 用 Playwright + 测试 + 规则验收
  ├─ 未达阈值 → qa_report.json → 进入 repair loop
  └─ 达阈值 → checkpoint + git commit
        │
        ▼
Approval Gate（可选）
  ├─ 用户审批 spec / 关键 checkpoint / 最终交付
  └─ 批准后继续下一 sprint
        │
        ▼
Run Complete
  ├─ 交付 artifacts
  ├─ 汇总 run_summary.md
  └─ 保留 resume state 供后续继续
```

长任务默认采用“**结构化 artifact + 多轮 sprint + evaluator 独立验收**”模式，而不是让单个 agent 在一条无限对话里持续输出。上下文控制策略做成可配置：

- 模型稳定时：优先 continuous session + automatic compaction
- 模型在长上下文下容易漂移时：切换为 context reset + handoff artifact
- 两种策略都围绕同一套 run artifact 协议，不影响恢复和审计

---

## 3. 核心模块设计

### 3.1 认证与授权 (Auth) — 🦀 Rust Core

私有化部署，认证保持简单。Rust 层处理 JWT 签发/验证，Python 拿到已验证的 `user_id`。

```
认证方式（从简）：
  - MVP: 简单用户名 + 密码 (Argon2id 哈希)
  - 后续可加: GitHub OAuth
  - 无需 RBAC：私有环境，单角色 admin 即可
  - 无需 Rate Limiting: 仅内网访问

请求流：
  浏览器 → Rust Core
    ├─ JWT 验证 (jsonwebtoken crate)
    ├─ 注入 X-User-Id header
    └─ 反代到 Python
```

### 3.2 会话、线程与 Run 模型 (Session / Thread / Run)

长时间任务必须把“聊天记录”和“可恢复运行状态”分开建模。用户看到的是 `Thread`，系统恢复的是 `Run`。

```
Session (用户会话)
  │
  ├── Thread (对话线程)
  │     ├── Message[]             # 用户消息 + Agent 回复
  │     ├── ThreadSummary         # 对话摘要
  │     ├── Run[]                 # 一次或多次执行
  │     └── UserMemoryRef         # 用户偏好 / 项目惯例
  │
  └── ThreadConfig
        ├── default_mode          # chat | autonomous
        ├── active_skills[]
        ├── workspace_template
        └── approval_policy

Run (可恢复执行单元)
  ├── run_id
  ├── mode                        # interactive | autonomous
  ├── lifecycle_state             # planning / contracting / building / qa / repair / awaiting_approval / completed / failed
  ├── current_sprint
  ├── product_spec_path
  ├── sprint_contract_path
  ├── qa_report_path
  ├── checkpoint_ref              # git commit/tag
  ├── budget_state                # tokens / wall clock / retries
  ├── resume_cursor               # responses continuation / artifact pointer
  └── workspace_path
```

设计原则：

- `Thread` 是用户视角对象，支持聊天、追溯和展示
- `Run` 是机器视角对象，必须可暂停、恢复、重试、审计
- 长期记忆只存“跨任务偏好”，不承担运行态恢复职责
- 每个 `Run` 都有自己的 artifact 目录和状态机

### 3.3 Harness Orchestrator（Planner / Generator / Evaluator）

**确定方案：Responses API 作为执行传输层，Harness Runtime 作为真正的编排层。**

Anthropic 文章里最值得吸收的不是某个模型名，而是编排结构：`planner -> generator -> evaluator`，外加 `sprint contract` 和独立 QA。这里直接落成平台的一等能力。

```
Planner
  - 将 1-4 句用户需求扩展成 product_spec.md
  - 输出目标用户、核心流程、设计语言、技术边界
  - 只定义“做什么”和验收方向，不预写过细实现细节

Generator
  - 一次只做一个 sprint
  - 先提出 sprint_contract.json
  - 在 workspace 内修改代码、运行测试、生成交付物
  - 完成后提交 checkpoint，交给 evaluator

Evaluator
  - 不参与实现，专门做验收
  - 使用 Playwright、自动化测试、静态规则、git diff 审查
  - 输出 qa_report.json 和 fail/pass 判定
  - 若失败，给 generator 明确修复清单
```

Responses API 的角色：

- 负责流式 token 生成和工具调用
- 复用 `~/.codex/auth.json` 或平台 API Key
- 不直接承担“长任务协议”；长任务协议由 Harness Runtime 自己维护

这意味着审批、暂停、恢复都不能依赖模型供应方的隐式会话语义，而要依赖本地 `Run State + Artifact + Checkpoint`。

### 3.4 Sprint Contract 与 QA 闸门

长任务不能直接从高层 spec 跳到编码。每个 sprint 先谈妥“这轮做什么、怎么判断完成”，再开始改代码。

```
sprint_contract.json
  - sprint_id
  - scope_in
  - scope_out
  - files_expected
  - user_flows_to_verify[]
  - tests_to_run[]
  - evaluator_checks[]
  - done_definition
```

执行规则：

1. Planner 产出 product spec
2. Generator 选择下一条 feature / sprint
3. Generator 生成 sprint contract 草案
4. Evaluator 审查 contract 是否可测、是否偏题
5. 双方达成一致后才开始编码
6. Evaluator 对照 contract 产出 pass/fail 结果

QA 采用硬阈值，不允许“整体看起来不错但有关键 bug”就放行。建议最少四类评分：

- `functionality`: 核心功能是否真的可用
- `product_depth`: 功能深度是否达到当前 sprint 预期
- `ux_quality`: 关键流程是否清晰、界面是否可用
- `code_quality`: 结构、可维护性、测试覆盖是否可接受

### 3.5 Artifact 协议与 Handoff

长任务要跨 compaction、重启、断网、人工接管继续运行，关键不在聊天历史，而在结构化 artifact。

```
/data/workspaces/{user_id}/{run_id}/
├── repo/                         # Git 工作目录
├── workspace/                    # 运行中的工作目录
├── artifacts/
│   ├── product_spec.md
│   ├── sprint_contracts/
│   │   └── sprint-01.json
│   ├── qa_reports/
│   │   └── sprint-01.json
│   ├── handoffs/
│   │   └── handoff-2026-04-02T10-30-00Z.md
│   ├── summaries/
│   │   └── run_summary.md
│   └── approvals/
│       └── approval-03.json
├── checkpoints/
│   └── sprint-01.commit
├── uploads/
├── outputs/
└── logs/
```

artifact 原则：

- 人能读：Markdown / JSON，便于人工接管
- 机能读：字段稳定，便于恢复
- 每轮 sprint 都要落盘，不依赖上下文窗口记忆
- handoff 要包含“当前状态、风险、下一步、阻塞、未完成测试”

### 3.6 实时流式输出 (Stream Bridge) — 🦀 Rust Core

全程在 Rust 层处理，这是 Rust 最大的价值点之一。

```
数据流:

  Responses API / Browser Worker / Test Runner
      │ (streaming chunks / structured events)
      ▼
  Python Harness Runtime
      │ 归一化为 RunEvent
      ▼
  Rust broadcast::channel<RunEvent>
      │
      ├── SSE Endpoint (/api/core/runs/{id}/stream)
      ├── Audit Logger
      └── Run Event Store (重播缓冲区)
```

SSE 事件类型统一为 run 级事件：

```
event: run_state       # planning / building / qa / paused / completed
event: message         # agent 文本输出
event: contract        # sprint contract 草案 / 确认
event: tool_call       # 工具调用
event: tool_result     # 工具结果
event: qa_report       # evaluator 验收结果
event: checkpoint      # git commit / tag / artifact 快照
event: approval        # 用户审批请求
event: budget          # token / time / retry 预算变化
event: artifact        # 新 artifact 生成
event: error           # 错误信息
event: done            # run 完成
```

连接恢复机制 (Rust 实现):

```
客户端断开重连:
  1. EventSource 自动重连
  2. 携带 Last-Event-ID header
  3. Rust 从环形缓冲区重播缺失事件
  4. 若事件已裁剪，前端退化到读取 run snapshot
```

### 3.7 技能系统 (Skill Registry)

复用 Codex 的 `SKILL.md` 体系，但要区分“用户技能”和“harness 内部技能”。

```
skills/
├── public/                    # 通用技能
├── custom/                    # 用户自定义技能
└── system/
    ├── planner/               # planner 专用约束
    ├── evaluator/             # evaluator QA 标准
    └── frontend-design/       # 设计语言参考
```

共享机制：

- `skills/` 同时被 CLI 和 Web Backend 读取
- 用户显式选择的技能注入 generator / planner
- evaluator 只能拿到允许的 QA / review 技能，避免角色混淆
- 每个 run 记录实际启用的 skill snapshot，保证可重放

### 3.8 工作区管理 (Workspace Guard) — 🦀 Rust Core

文件系统隔离和资源边界在 Rust 层实现，确保 Python 或模型跑偏时也不会直接污染宿主机。

```
Rust Workspace Guard 职责:
  - 创建/销毁工作区目录 (原子操作)
  - 路径穿越检查 (canonicalize + 前缀匹配)
  - 磁盘配额: 单 run max_size_mb, 单用户总配额
  - CPU / Memory 限额: 进程组级别 watchdog
  - 网络策略: 默认允许，支持 denylist / allowlist
  - inotify/kqueue 监听文件变更 → 通知前端
  - 过期自动清理 (cleanup_after_days)
  - 文件上传安全检查 (文件名消毒、大小限制、MIME 校验)
```

> 私有化单机不代表可以完全放弃资源限制。长任务、浏览器自动化和代码执行天然会出现失控风险，至少要保留进程资源上限和路径边界。

### 3.9 记忆系统 (Memory)

```
短期记忆：
  - 当前 run 的消息历史
  - 当前 sprint 的 contract / qa / handoff
  - 模型上下文压缩摘要

长期记忆：
  - 用户偏好、技术栈偏好、代码风格偏好
  - 常用项目模板、常见约束
  - 不存放可恢复运行状态
```

记忆边界：

- `Run State` 存数据库和 artifact
- `User Memory` 存偏好与稳定事实
- 两者混用会导致恢复污染和误召回

存储决策：

- `Run State`、`Thread Summary`、`User Memory` 统一存同一个 SQLite 数据库
- artifact 正文、截图、日志、输出文件继续落盘到 workspace
- 不再使用 `/data/memory/{user_id}.json` 之类的文件式记忆存储
- memory 提取逻辑可以由 LLM 完成，但持久化必须写入 SQLite 表

### 3.10 网络搜索与浏览 (Browser Engine) — Playwright

浏览器在长任务里不是普通工具，而是 evaluator 的主验收执行器之一。

```
设计原则:
  - 零第三方搜索 API 依赖: 搜索/抓取全走浏览器自动化
  - 单独服务: browser worker 进程池，与主服务解耦
  - Rust 监管: 浏览器进程由 Rust Supervisor 管理
  - 可验证: evaluator 必须能截图、点击、断言 UI 状态和网络行为
```

核心能力：

```
① Research/Search
  - web_search / web_fetch / web_crawl

② Product QA
  - 页面导航、表单填写、点击、截图
  - 关键元素断言
  - 控制台错误抓取
  - Network / API 响应检查

③ Visual Evidence
  - 失败截图
  - 关键流程录像（可选）
  - DOM / accessibility 快照
```

对 evaluator 的约束：

- 必须基于 sprint contract 设计测试步骤
- 必须输出可执行证据，而不是抽象评价
- 遇到失败要提供复现路径、预期结果和实际结果

### 3.11 Checkpoint、Git 与 Resume

Git 在长任务里不是附属工具，而是 checkpoint 协议的一部分。

```
每个 sprint 结束:
  - 运行测试 / lint / evaluator
  - 通过后强制生成 git commit
  - 写入 checkpoint metadata
  - 记录 diff 摘要、关键文件、已知风险
```

恢复策略：

```
resume(run_id)
  1. 读取 run_state + 最新 handoff
  2. checkout 到最后成功 checkpoint
  3. 装载未完成 sprint contract / qa_report
  4. 根据模型策略选择 compaction 或 fresh session
  5. 从下一步继续
```

这套机制保证：

- 浏览器断连不影响后台运行
- Python 服务重启后可从 artifact 恢复
- 人工接管时能明确看到“现在在哪一轮、为什么失败、接下来做什么”

---

## 4. API 设计

### 4.1 路由分层

```
Rust Core 直接处理 (/api/core/*):
  POST   /api/core/auth/login         # 登录 (返回 JWT)
  POST   /api/core/auth/register      # 注册
  POST   /api/core/auth/refresh       # 刷新 token
  GET    /api/core/auth/me            # 当前用户信息
  GET    /api/core/auth/oauth/{provider}  # OAuth 跳转
  GET    /api/core/health             # 健康检查
  GET    /api/core/metrics            # Prometheus 指标
  GET    /api/core/runs/{id}/stream      # Run 级 SSE 流式输出
  POST   /api/core/runs/{id}/uploads     # 文件上传 (直传磁盘)
  GET    /api/core/runs/{id}/artifacts/{path}     # 文件下载
  GET    /api/core/runs/{id}/checkpoints/{name}   # checkpoint 元数据 / patch

Python App 处理 (/api/app/* → 经 Rust 鉴权后反代):
  POST   /api/app/threads                     # 创建线程
  GET    /api/app/threads                     # 列出线程
  GET    /api/app/threads/{id}                # 线程详情
  DELETE /api/app/threads/{id}                # 删除线程
  POST   /api/app/threads/{id}/messages       # Chat Mode 消息
  POST   /api/app/threads/{id}/runs           # 启动 autonomous run
  GET    /api/app/runs                        # run 列表
  GET    /api/app/runs/{id}                   # run 状态 / 元数据
  POST   /api/app/runs/{id}/pause             # 暂停
  POST   /api/app/runs/{id}/resume            # 恢复
  POST   /api/app/runs/{id}/cancel            # 取消
  POST   /api/app/runs/{id}/approve           # 审批当前 gate
  POST   /api/app/runs/{id}/reject            # 拒绝当前 gate
  GET    /api/app/runs/{id}/contracts         # sprint contract 历史
  GET    /api/app/runs/{id}/qa-reports        # QA 历史
  GET    /api/app/runs/{id}/artifacts         # artifacts 索引
  GET    /api/app/runs/{id}/checkpoints       # checkpoint 列表
  GET    /api/app/skills                      # 列出技能
  PUT    /api/app/skills/{name}               # 启用/禁用
  POST   /api/app/skills/install              # 安装技能
  GET    /api/app/models                      # 可用模型
  GET    /api/app/memory                      # 用户记忆
  PUT    /api/app/memory                      # 更新记忆
  GET    /api/app/config                      # 系统配置（脱敏）
  POST   /api/app/browser/search              # 网络搜索
  POST   /api/app/browser/fetch               # 抓取网页
  POST   /api/app/browser/interact            # 页面交互 / QA 辅助
  POST   /api/app/browser/screenshot          # 截图
```

**分层原则**：
- IO 密集 (流式、文件传输、健康检查) → Rust
- 安全敏感 (认证、鉴权、文件安全检查) → Rust
- 业务逻辑 (run 状态机、LLM 编排、技能解析、记忆提取) → Python

### 4.2 Rust ↔ Python 进程间通信

```
请求代理 (Rust → Python):
  Rust ─ HTTP/1.1 reverse proxy ─→ Python FastAPI :8000
  优点: 简单，Python 可独立调试
  连接池: hyper connection pool, keep-alive

事件推送 (Python → Rust):
  Python ─ HTTP POST /internal/runs/{run_id}/events ─→ Rust EventBus
  优点: 无额外依赖，统一 HTTP 协议
  注意: 仅 localhost 访问，无鉴权

  后续可升级: gRPC + Unix Socket (生产)
```

### 4.3 SSE 协议

```
GET /api/core/runs/{id}/stream       # Rust 直接处理
Accept: text/event-stream
Authorization: Bearer <jwt>          # Rust 验证

# 事件格式
id: 42
event: run_state
data: {"state": "planning", "sprint": 0, "timestamp": "..."}

id: 43
event: contract
data: {"sprint": 1, "status": "proposed", "path": "artifacts/sprint_contracts/sprint-01.json"}

id: 44
event: qa_report
data: {"sprint": 1, "result": "fail", "path": "artifacts/qa_reports/sprint-01.json"}

id: 99
event: done
data: {"summary": "run 完成", "artifacts": ["outputs/app.zip"], "checkpoint": "sprint-08"}
```

---

## 5. 前端设计

### 5.1 页面结构

```
/                       # 首页/仪表盘
├── /chat               # 主对话界面
│   ├── 左侧: 线程列表
│   ├── 中间: 对话区域（消息流 + 代码高亮 + diff 展示）
│   └── 右侧: Run 面板（文件树、artifacts、skills、当前 gate）
├── /runs               # 长任务监控（运行中/历史/队列）
│   └── /runs/{id}      # Sprint 进度、QA 报告、checkpoints、budget
├── /skills             # 技能管理
├── /settings           # 设置（模型、API keys、偏好）
└── /admin              # 管理后台（用户管理、系统配置）
```

### 5.2 关键交互

```
Chat Mode：
  - 普通对话: 输入文本 → agent 响应
  - 文件上传: 拖拽上传 → 注入上下文
  - 快捷命令: /new, /model, /skill, /run

Autonomous Build Mode：
  - 短需求启动: 1-4 句 prompt → planner 自动扩 spec
  - Spec 审阅: 用户可在开跑前批准 / 微调 product spec
  - Sprint 面板: 当前 sprint、contract、QA 状态、重试次数、预算
  - 审批流: spec gate / checkpoint gate / final delivery gate
  - 恢复入口: pause / resume / retry from checkpoint

代码与交付展示：
  - 语法高亮 (Monaco Editor / CodeMirror)
  - Diff 视图 (inline / side-by-side)
  - 文件树浏览
  - QA 失败截图 / 控制台日志 / 测试摘要
  - 一键复制 / 下载 / 回滚到 checkpoint
```

---

## 6. 技术栈

```yaml
Frontend:
  framework: Next.js 15 (App Router)
  language: TypeScript
  ui: Tailwind CSS + shadcn/ui
  state: Zustand
  realtime: EventSource (SSE)
  editor: Monaco Editor (代码查看/编辑)
  markdown: react-markdown + remark
  charts: visx / echarts (预算、运行时指标)

Rust Core (sensusai-core):
  language: Rust 1.82+ (2024 edition)
  async_runtime: tokio 1.x (multi-thread)
  http: axum 0.8 + tower middleware
  proxy: hyper 1.x (reverse proxy to Python)
  auth: jsonwebtoken + argon2
  streaming: axum SSE + tokio::sync::broadcast
  logging: tracing + tracing-subscriber (JSON)
  config: config-rs (YAML/TOML)
  db: sqlx + SQLite (WAL mode)
  fs_watch: notify
  resource_guard: process groups + watchdog
  test: cargo test

Python App:
  framework: FastAPI 0.115+
  language: Python 3.12+
  codex: openai SDK (Responses API) / httpx (streaming)
  harness: planner / generator / evaluator runtime
  browser: playwright (Chromium headless, 零 API Key)
  browser_extras: playwright-stealth + readability-lxml
  skills: 自定义 SKILL.md parser
  memory: LLM extraction pipeline + SQLite persistence
  orm: SQLAlchemy 2.0 (共享同一个 SQLite DB)
  queue: asyncio TaskGroup (MVP) → arq/Redis (扩展)
  schemas: Pydantic v2
  git_ops: git CLI + structured checkpoint metadata
  ipc: HTTP (MVP) → gRPC tonic/grpcio (生产)

Infrastructure:
  dns/cdn: Cloudflare (如需外网访问)
  compute: GCE / 裸金属 / Stateful VM
  storage: 本地磁盘 + 可选 GCS/S3 (artifacts, uploads)
  secrets: GCP Secret Manager
  ci/cd: GitHub Actions
  container: Docker + Docker Compose

Codex Integration:
  transport: OpenAI Responses API
  auth: ~/.codex/auth.json 或 OPENAI_API_KEY
  skills: 共享 skills/ 目录
  execution_policy: harness-controlled tools + approval gates
  sandbox: 宿主机执行 + 资源限制 (MVP) → Docker/容器隔离 (生产)
```

---

## 7. 长时间运转 Harness 关键设计补全

这一节吸收 Anthropic 在 2026-03-24 文章里验证过的核心经验，并转成 SensusAI Harness 可实施的方案。

### 7.1 核心原则

```
原则 1: 先拆 sprint，再编码
  - 不让 generator 直接从一句需求跑到最终代码
  - planner 负责扩 spec，generator 一次只做一个 sprint

原则 2: 生成和评估分离
  - generator 负责产出
  - evaluator 负责质疑、验证、打分、卡门槛

原则 3: 聊天历史不是系统状态
  - 可恢复状态必须写入 artifact / DB / checkpoint
  - 避免把 run 生死绑定在单条对话上下文里

原则 4: context 策略按模型可切换
  - 稳定模型走 compaction
  - 容易 context anxiety 的模型走 reset + handoff

原则 5: QA 要有证据链
  - 失败截图、复现步骤、日志、测试结果
  - 不是“感觉还行”的主观通过
```

### 7.2 Run 状态机

```
queued
  ↓
planning
  ↓
contracting
  ↓
building
  ↓
qa
  ├─ pass → checkpointing → next sprint / completed
  ├─ fail → repair
  └─ blocked → awaiting_approval

repair
  ├─ retries_left → building
  └─ exhausted → failed

awaiting_approval
  ├─ approve → resume previous step
  └─ reject  → failed / replanning
```

状态机要求：

- 每个状态切换都写入 DB 和 event stream
- 每个状态都有超时和 watchdog
- `paused`、`cancelled`、`failed`、`completed` 都必须可审计和可恢复判定

### 7.3 Approval Workflow（按 Responses API 重新定义）

Responses API 只是传输层，所以 approval 不能设计成“暂停某个神秘远端进程并等待按钮”。必须做成 harness 自己的 gate。

```
Approval Gate:
  1. generator 完成一个可审阅单元
  2. Harness 写入 patch / diff / checkpoint / summary artifact
  3. Run 状态切到 awaiting_approval
  4. 前端展示本轮改动、测试结果、QA 结论
  5. 用户 approve / reject
  6. Harness 根据结果继续下一步，必要时新开一次 model call
```

默认建议保留三类 gate：

- `spec_gate`: planner 产出 product spec 后
- `checkpoint_gate`: 高风险 sprint 或跨 sprint 大改后
- `delivery_gate`: 最终交付前

### 7.4 Budget、Watchdog 与并发控制

长时间运行要靠预算控制，不是只靠一个总超时。

```
预算维度:
  - max_wall_clock_minutes
  - max_tokens_total
  - max_cost_usd
  - max_sprints
  - max_repairs_per_sprint
  - max_idle_minutes_without_progress

看门狗:
  - 无文件变化 + 无新事件超过 N 分钟 → 标记 stall
  - 同类错误连续出现超过阈值 → 强制升级为 blocked
  - 测试命令运行超时 → kill 子进程并记录失败
```

并发策略：

- 私有单机 MVP 保持 `max_parallel_runs=1~2`
- 浏览器 worker 与 generator run 分开限流
- evaluator 优先级高于新建 run，避免 QA 堵塞后面积压

### 7.5 错误恢复与断点续跑

```
场景                          │ 处理策略
──────────────────────────────┼──────────────────────────────────────
API 限流 / 429                │ 指数退避 + 保留 run state + 继续
浏览器断连                    │ SSE 重连；后台 run 不停止
Python 服务重启               │ 从 DB + artifact + checkpoint 恢复
单次 model call 失败          │ 重试当前 step，不重放整条 run
QA 失败                       │ 进入 repair loop，不丢本轮证据
宿主机重启                    │ 未完成 run 标记 interrupted，支持 resume
上下文膨胀                    │ compaction 或 reset + handoff
SQLite busy / 写锁竞争        │ busy_timeout + 单写队列 + 幂等重试
```

恢复前提：

- checkpoint 一致
- artifact 完整
- 当前 sprint contract 可读
- 最后一个 qa_report 可读

### 7.6 安全基线（私有化最小集）

私有化部署可以简化，但不能把防护降到零。最小基线如下：

```
必须保留:
  - JWT 认证
  - 审计日志
  - 工作区路径白名单
  - 进程 CPU / 内存 / 磁盘配额
  - 文件上传限制
  - shell / tool allowlist

MVP 可暂缓:
  - 多角色 RBAC
  - 完整容器沙箱
  - Cloudflare Access
  - 分布式 rate limit
```

### 7.7 可观测性

```
日志:
  - 结构化日志 (JSON)
  - 每个 run 独立日志文件
  - 每个 sprint 独立 QA / build 子日志

指标:
  - run 成功率 / 失败率 / 中断率
  - 平均 sprint 数
  - QA 首次通过率
  - repair loop 次数
  - token / cost / wall-clock 消耗
  - 浏览器池占用率

追踪:
  - request_id / thread_id / run_id / sprint_id 全链路透传
  - OpenTelemetry 可选接入

SQLite 运行约束:
  - `runs` / `run_events` / `memory` 热路径写入统一由 Python Runtime 串行化
  - Rust Core 不直接落 run 热路径状态到 SQLite，避免双写竞争
  - `run_events` 需要归档和裁剪，避免 WAL 长期膨胀
  - schema migration 只允许一个 owner 执行；建议由 Rust Core 在启动期统一迁移
```

### 7.8 扩展性（预留）

```
当前: 单机私有化
  Rust Core + Python Harness + Browser Worker + SQLite + 本地磁盘

预留扩展点:
  - SQLite WAL + 定时备份 + 归档
  - 热数据 / 冷数据分表或分库
  - 本地 artifacts → GCS / S3
  - 单机 worker → 多 worker 分层
  - evaluator 单实例 → evaluator pool
  - run queue 本地内存 → Redis / durable queue
```

---

## 8. 项目目录结构

```
SensusaiHarness/
├── docs/
│   ├── ARCHITECTURE.md            # 本文档
│   ├── API.md                     # API 详细文档
│   ├── HARNESS_RUNTIME.md         # run 状态机 / artifact 协议
│   ├── RUNBOOK.md                 # 运维与恢复手册
│   ├── SQLITE_SCHEMA.md           # SQLite 表设计与写入策略
│   └── sqlite_schema_v1.sql       # 初版 SQLite DDL
├── core/                           # 🦀 Rust Core
│   ├── src/
│   │   ├── main.rs                # axum 入口 + 路由组装
│   │   ├── config.rs              # AppConfig::from_env
│   │   ├── auth.rs                # JWT 验证 middleware
│   │   ├── proxy.rs               # /api/app/* 反向代理
│   │   ├── stream.rs              # SSE 端点 + Last-Event-ID 重放
│   │   ├── event_bus.rs           # RunEvent broadcast + 环形缓冲
│   │   ├── ingest.rs              # Python→Rust 事件注入端点
│   │   └── db/
│   │       ├── mod.rs
│   │       ├── models.rs          # sqlx FromRow 模型
│   │       └── migrations/
│   ├── Cargo.toml
│   └── Dockerfile
├── backend/                        # 🐍 Python Harness Runtime
│   ├── src/
│   │   ├── main.py              # FastAPI app factory + lifespan
│   │   ├── config.py            # Settings (pydantic-settings, SENSUSAI_ prefix)
│   │   ├── deps.py              # FastAPI 依赖注入 (session, current_user)
│   │   ├── routes/
│   │   │   ├── auth.py          # POST /login
│   │   │   ├── threads.py       # POST/GET /threads
│   │   │   ├── runs.py          # POST runs, GET/pause/resume/cancel
│   │   │   └── approval.py      # POST approve/reject
│   │   ├── schemas/
│   │   │   ├── common.py        # ErrorDetail/ErrorResponse
│   │   │   ├── auth.py          # LoginRequest/TokenResponse
│   │   │   ├── threads.py       # CreateThread/ThreadResponse
│   │   │   └── runs.py          # CreateRun/RunResponse/BudgetView
│   │   ├── harness/
│   │   │   ├── __init__.py      # 状态机 (TRANSITIONS, validate_transition)
│   │   │   ├── runner.py        # RunContext + execute_run (LLM executor)
│   │   │   ├── planner.py       # run_planner (LLM 生成 spec)
│   │   │   ├── generator.py     # draft_contract + run_build_phase
│   │   │   ├── evaluator.py     # run_evaluator (QA 验收)
│   │   │   ├── llm_client.py    # OpenAI / Codex auth + chat_completion
│   │   │   ├── budget.py        # BudgetTracker (token/time/retry)
│   │   │   ├── event_emitter.py # Python→Rust SSE 事件推送
│   │   │   ├── workspace.py     # 工作区目录创建/布局
│   │   │   └── artifacts.py     # 各类 artifact 读写
│   │   ├── repos/
│   │   │   ├── threads.py       # Thread 仓储
│   │   │   ├── runs.py          # Run / Event / Gate 仓储
│   │   │   ├── artifacts.py     # artifact_index 仓储
│   │   │   ├── checkpoints.py   # checkpoint 仓储
│   │   │   └── memory.py        # memory 仓储
│   │   └── models/
│   │       ├── base.py          # SQLAlchemy Base
│   │       ├── db.py            # ORM 模型 (User, Thread, Run...)
│   │       └── session.py       # async session factory
│   ├── pyproject.toml
│   └── Dockerfile
├── frontend/
│   ├── src/
│   │   ├── app/
│   │   │   ├── (auth)/
│   │   │   ├── chat/
│   │   │   ├── runs/
│   │   │   ├── skills/
│   │   │   ├── settings/
│   │   │   └── admin/
│   │   ├── components/
│   │   │   ├── chat/
│   │   │   ├── run-monitor/
│   │   │   ├── qa/
│   │   │   ├── diff/
│   │   │   └── ui/
│   │   ├── hooks/
│   │   │   ├── useRunStream.ts
│   │   │   ├── useThread.ts
│   │   │   └── useAuth.ts
│   │   ├── stores/
│   │   └── lib/
│   └── package.json
├── proto/
│   └── sensusai.proto
├── skills/
│   ├── public/
│   ├── custom/
│   └── system/
├── config/
│   └── config.example.yaml
├── docker/
│   ├── docker-compose.yml
│   ├── docker-compose.prod.yml
│   └── Dockerfile.codex
├── scripts/
│   ├── setup.sh
│   └── deploy.sh
├── Makefile
├── .env.example
└── README.md
```

运行时目录（不入仓）：

```
/data/
├── workspaces/{user_id}/{run_id}/
├── harness.db
├── harness.db-wal
├── harness.db-shm
├── artifacts-cache/
└── screenshots/
```

---

## 9. 配置设计

```yaml
# config.yaml (Rust Core 和 Python Harness 共享读取)

core:
  listen: "0.0.0.0:4000"
  python_upstream: "127.0.0.1:8000"
  static_dir: ./frontend/out
  cors_origins: ["http://localhost:3000"]

auth:
  jwt_secret: $JWT_SECRET
  jwt_expire_minutes: 1440

stream:
  buffer_size: 2000
  heartbeat_interval_seconds: 15
  snapshot_fallback: true

workspace:
  base_path: /data/workspaces
  cleanup_after_days: 30
  disk_quota_mb_per_run: 8192
  disk_quota_mb_per_user: 51200

safety:
  process_cpu_limit_percent: 200
  process_memory_mb: 4096
  shell_allowlist: ["git", "npm", "pnpm", "pytest", "cargo", "uv"]
  network_mode: allow
  writable_roots:
    - /data/workspaces

database:
  url: sqlite:///data/harness.db
  journal_mode: WAL
  synchronous: NORMAL
  busy_timeout_ms: 5000
  foreign_keys: true

harness:
  default_mode: autonomous
  max_parallel_runs: 2
  max_sprints_per_run: 10
  max_repairs_per_sprint: 3
  max_wall_clock_minutes: 360
  max_idle_minutes_without_progress: 15
  approval_gates:
    - spec_gate
    - checkpoint_gate
    - delivery_gate
  context_strategy:
    default: compact
    fallback: reset_with_handoff
  qa_thresholds:
    functionality: 0.75
    product_depth: 0.75
    ux_quality: 0.75
    code_quality: 0.75

models:
  planner_model: $PLANNER_MODEL
  generator_model: $GENERATOR_MODEL
  evaluator_model: $EVALUATOR_MODEL

codex:
  transport: responses_api
  auth_from: ~/.codex/auth.json
  responses_api:
    base_url: https://api.openai.com/v1
    api_key: $OPENAI_API_KEY
  tool_execution: harness_controlled

skills:
  path: ./skills
  auto_discover: true
  snapshot_per_run: true

browser:
  enabled: true
  pool_size: 2
  page_timeout_seconds: 30
  max_pages_per_context: 10
  idle_recycle_seconds: 300
  search_engine: duckduckgo
  stealth: true
  proxy: null
  user_agents_pool_size: 20
  request_delay_ms: 2000
  screenshot_dir: /data/screenshots

memory:
  enabled: true
  storage_backend: sqlite
  user_memory_table: user_memory_entries
  thread_summary_table: thread_summaries
  debounce_seconds: 30
  exclude_run_state: true
```

---

## 10. 实施路线图

### Phase 1: 平台骨架 + Run 基础模型 (2-3 周)

```
目标：能启动一个可跟踪的 run，而不只是发一条消息

✅ Rust Core 骨架 (axum + tokio + config)
✅ Python FastAPI 联通
✅ JWT 认证
✅ Run / Thread / Event 基础表结构
✅ Rust SSE Stream Bridge
✅ Responses API 基础接入
✅ 前端基础聊天页 + run 列表页
✅ SQLite 持久化
✅ SQLite WAL / busy_timeout / 备份策略
✅ Docker Compose 开发环境
```

### Phase 2: Harness Runtime MVP (2-3 周)

```
目标：长任务真的能按 sprint 跑起来

✅ Planner 产出 product spec
✅ Generator / Evaluator 双角色框架
✅ Sprint contract 协商
✅ Artifact 协议（spec / contract / qa / handoff）
✅ Git checkpoint 写入
✅ Playwright evaluator MVP
✅ Pause / resume / cancel
```

### Phase 3: Human-in-the-loop + 稳定化 (2-3 周)

```
目标：可审批、可恢复、可排障

✅ Approval gates
✅ SSE 重连重播 + snapshot fallback
✅ Budget / watchdog / stall detection
✅ 429 重试与 run resume
✅ SQLite 锁冲突处理与事件表归档
✅ 结构化日志 + Prometheus 指标
✅ QA 失败截图、日志和复现证据
✅ CI (cargo test + pytest + frontend tests)
```

### Phase 4: 生产化与增强 (持续)

```
✅ 资源限制加强（容器 / cgroups）
✅ SQLite 备份、归档、压缩与只读分析副本
✅ 多模型策略
✅ MCP Server 集成
✅ 多 evaluator / sub-agent 扩展
✅ 技能管理 UI 和 run 模板
```

---

## 11. 与 Anthropic Harness / DeerFlow 的对齐与差异

| 维度 | Anthropic 长运行 Harness | DeerFlow | SensusAI Harness |
|------|--------------------------|----------|------------------|
| 长任务拆解 | planner + sprint | graph 编排 | **planner + sprint** |
| 评估机制 | 独立 evaluator + Playwright | 外部工具为主 | **独立 evaluator + Playwright + QA 阈值** |
| 上下文策略 | compaction / handoff | LangGraph 状态 | **模型可切换的 compaction / reset + handoff** |
| 交接载体 | 文件 artifact | workflow state | **artifact + DB + git checkpoint** |
| 平台层 | Claude Agent SDK 为主 | 纯 Python | **Rust Core + Python Harness Runtime** |
| 流式输出 | SDK / app 内部流 | Python SSE | **Rust broadcast + SSE** |
| 执行层 | Claude coding harness | 多模型 / LangChain | **Responses API / Codex 兼容执行** |
| 部署定位 | 实验性内部 harness | 通用超级 Agent | **私有化长任务开发平台** |

---

## 12. Rust 核心 Crate 依赖

```toml
# core/Cargo.toml 实际依赖
[dependencies]
# Async Runtime
tokio = { version = "1", features = ["full"] }
tokio-stream = "0.1"

# HTTP Framework
axum = "0.8"
axum-extra = { version = "0.10", features = ["typed-header"] }
tower = "0.5"
tower-http = { version = "0.6", features = ["cors", "trace", "compression-gzip"] }
hyper = { version = "1", features = ["full"] }
hyper-util = { version = "0.1", features = ["client-legacy", "tokio", "http1"] }
http-body-util = "0.1"

# Auth
jsonwebtoken = "9"       # JWT (argon2 密码哈希在 Python 侧)

# Database
sqlx = { version = "0.8", features = ["runtime-tokio", "sqlite"] }

# Serialization
serde = { version = "1", features = ["derive"] }
serde_json = "1"

# SSE Streaming
async-stream = "0.3"

# Observability
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter", "json"] }

# Utilities
uuid = { version = "1", features = ["v4"] }
chrono = { version = "0.4", features = ["serde"] }
```
