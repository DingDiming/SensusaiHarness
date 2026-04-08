# Archived Planning Doc

This document is quarantined legacy planning material from the previous Web-first rewrite path.
It does not describe the active roadmap for the current terminal-first product.
Prefer `/task.md` and the root `/docs` directory.

# SensusAI Harness — 二阶段开发计划

> 一阶段交付了完整的编排骨架（状态机、DB、事件流、审批闸门、artifact 协议、SSE 流式）。
> 二阶段目标：**填充执行层，让 harness 能真正产出代码并可靠地长时间运行。**

---

## 现状总结（一阶段交付物）

| 层 | 状态 | 说明 |
|----|------|------|
| Rust Core (gateway/auth/proxy/SSE) | ✅ 完成 | 端到端验证通过 |
| Python 路由 (auth/threads/runs/approval) | ✅ 完成 | 11 个 API 端点 |
| 状态机 + Runner 编排 | ✅ 完成 | 13 状态 + approval gates |
| Artifact 协议 + Workspace | ✅ 完成 | 6 类 artifact 落盘 |
| Budget 追踪 | ✅ 完成 | token + wall-clock + repair |
| SSE 事件推送 | ✅ 完成 | Python→Rust 环形缓冲 |
| LLM Client | ⚠️ 仅 chat completions | 无 tool_use |
| Generator (build phase) | ⚠️ 纯叙事模拟 | 不改文件、不跑测试 |
| Evaluator (QA) | ⚠️ 纯 LLM 凭空打分 | 无真实验证 |
| Contract 协商 | ⚠️ 单步自动 accept | 无 evaluator 审查 |
| Pause/Resume/Cancel | ⚠️ 只改 DB | 不通知 runner |
| Memory 系统 | ⚠️ repo 层完整 | 未接入 harness |
| 前端 | ❌ 未开始 | |

---

## 二阶段任务清单

### P0 — 执行层核心（不完成则 harness 空转）

#### P0.1 LLM Client 集成 Responses API
- [x] `llm_client.py` 新增 `responses_create()` — 调用 OpenAI Responses API
- [x] 支持 `tools` 参数传入（file_read, file_write, shell_exec 等）
- [x] 支持多轮 tool_use 循环（模型调 tool → 执行 → 返回结果 → 模型继续）
- [x] 统一 usage 追踪（input/output tokens 跨多轮累加）
- [x] 复用现有 auth 逻辑（OPENAI_API_KEY / codex auth.json）
- [x] 旧版 SDK 自动降级为 Chat Completions with tools

#### P0.2 Generator 真实代码生成
- [x] `generator.py` 重写 `run_build_phase()` — 用 Responses API + tool_use
- [x] 内置 tool 定义：`file_read`, `file_write`, `file_list`, `shell_exec`（沙箱限定 `repo/` 目录）
- [x] tool 执行器 `WorkspaceToolExecutor`：真正操作文件系统、执行命令（测试等）
- [x] 命令输出写入 `logs/`，路径遍历防护（含 macOS symlink 兼容）
- [x] build 完成后产出结构化 build_result（changed_files, command_log, summary）
- [x] 支持 repair_backlog 输入（从 QA 失败回传修复清单）

#### P0.3 Evaluator 真实验证
- [x] `evaluator.py` 重写 — 基于真实文件和测试结果做 QA
- [x] 新增 `_run_tests()` — subprocess 执行 contract 中的 `tests_to_run`
- [x] 新增 `_collect_changed_files_content()` — 读取变更文件内容
- [x] 新增 `_list_repo_structure()` — 生成项目结构树
- [x] LLM 评估基于真实证据（测试结果 + 文件内容 + 目录结构 + 命令日志）
- [x] qa_report 中包含 `test_results` 和 `evidence` 字段
- [x] 测试失败自动注入 blocking_issues
- [x] JSON 解析失败默认 fail（不再默认 pass）
- [x] Playwright 验证预留接口（P2 实现）

### P1 — 可靠性（多小时运行必须）

#### P1.1 Contract 协商循环
- [x] Runner contracting 阶段：generator 出草案 → evaluator review → 修改/accept
- [x] 最多 3 轮协商，超出则 `failed` + `conflict_state`
- [x] `evaluator.py` 新增 `review_contract()` 方法

#### P1.2 Runner Pause/Cancel/Resume
- [x] Runner 每个阶段间检查 DB 中的 run state（`_check_cancellation()`）
- [x] 外部 cancel 检测 → 设置 `ctx.cancelled`
- [x] 外部 pause 检测 → 挂起轮询直到 resume 或 cancel
- [x] 进程启动时扫描 active runs 并标记 `interrupted`（`_recover_interrupted_runs()`）
- [x] `qa` 状态新增 `interrupted` 转移

#### P1.3 Stall Watchdog
- [x] BudgetTracker 增加 `max_idle_minutes` 维度（默认 15 分钟）
- [x] `record_usage()` 自动更新 `_last_progress_time`
- [x] 增加 `record_progress()` 手动标记进展
- [x] `stalled` 属性 + `exceeded_reason` 属性
- [x] Runner 区分 stall（→ interrupted）vs budget（→ failed）

### P2 — 功能完善（可用性）

#### P2.1 Memory 接入
- [x] Planner prompt 注入 user_memory
- [x] Generator prompt 注入 working_memory
- [x] Run 完成后写 thread_summary

#### P2.2 子资源路由
- [x] `GET /runs/{id}/contracts`
- [x] `GET /runs/{id}/qa-reports`
- [x] `GET /runs/{id}/artifacts`
- [x] `GET /runs/{id}/checkpoints`

#### P2.3 Chat Mode
- [x] `POST /threads/{id}/messages` + `GET /threads/{id}/messages`
- [x] Interactive run (single-turn LLM call with conversation history)

#### P2.5 LLM 层重写：Codex CLI
- [x] 移除 OpenAI SDK 依赖，全部调用走本地 `codex exec --json`
- [x] Generator build phase 使用 `codex exec --full-auto --cd repo/`
- [x] Planner / Evaluator / Chat 使用 `chat_completion()` → `codex exec --ephemeral`

#### P2.4 前端 MVP
- [x] Next.js 15 + shadcn/ui 脚手架
- [x] 登录 + Thread 列表 + Run 监控
- [x] Approval gate 审批 UI
- [x] Chat 对话界面
- [x] SSE 实时更新

---

## 完成标准

**P0 完成标准**：给定一句需求 prompt，harness 能：
1. Planner 生成 product spec
2. Generator 在 workspace/repo/ 下真正创建/修改文件
3. Generator 执行 contract 中定义的测试命令
4. Evaluator 基于真实测试结果和文件 diff 做 QA 评估
5. 整个编排循环产出真实的代码 artifact

**P1 完成标准**：一个多 sprint run 能可靠地：
1. 被外部 pause/cancel 中断
2. 从 checkpoint 恢复执行
3. Contract 有 evaluator 审查
4. 卡住时自动标记 interrupted
