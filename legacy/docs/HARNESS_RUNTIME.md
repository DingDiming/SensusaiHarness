# SensusAI Harness Runtime 规格

> 目标：定义 long-running harness 的执行协议，而不是只描述“会有 planner / generator / evaluator”。
> 这份文档是实现 `backend/src/harness/*` 和 `core/src/runs/*` 的直接依据。

---

## 1. 设计目标

SensusAI Harness Runtime 必须满足四件事：

1. 把一句高层需求稳定拆成多轮 sprint
2. 把生成和评估分离，避免 agent 自评过宽
3. 把可恢复状态落到 artifact / DB / git checkpoint，而不是聊天历史
4. 在多小时运行过程中支持暂停、恢复、审批、失败修复和断点续跑

非目标：

- 不追求一次模型调用完成整项工作
- 不把用户长期记忆当作运行态存储
- 不依赖某个模型供应商的隐式 session 语义

---

## 2. 术语

- `Thread`：用户视角的对话容器
- `Run`：机器视角的可恢复执行单元
- `Sprint`：run 中的最小交付增量
- `Contract`：本轮 sprint 的范围与验收标准
- `Checkpoint`：可恢复的 git 快照
- `Gate`：等待人工决策的暂停点
- `Artifact`：为人和机器共同消费而落盘的结构化文件
- `Handoff`：跨 compaction / reset / restart 的交接文件

---

## 3. 运行角色

### 3.1 Planner

职责：

- 把用户短需求扩展为 `product_spec.md`
- 给出产品目标、核心用户、关键流程、设计语言、技术边界
- 初步拆分 sprint

禁止事项：

- 不预写过细的技术实现步骤
- 不直接改代码
- 不给 evaluator 评分

### 3.2 Generator

职责：

- 一次只处理一个 sprint
- 起草 `sprint_contract.json`
- 改代码、跑测试、生成输出
- 写 handoff 和 checkpoint metadata

禁止事项：

- 不判定最终 pass/fail
- 不跳过 contract 直接编码
- 不跨 sprint 偷做未来范围

### 3.3 Evaluator

职责：

- 检查 contract 是否清晰、可测、没有偏题
- 使用 Playwright、测试命令、日志、git diff 做验收
- 输出 `qa_report.json`
- 给出明确 repair backlog

禁止事项：

- 不直接改代码
- 不以“整体感觉不错”代替证据
- 不放过 blocking issue

### 3.4 Human Operator

职责：

- 在 approval gate 做判断
- 必要时 pause / resume / cancel run
- 在失败后选择继续修复、回退 checkpoint 或终止

---

## 4. Run 生命周期

### 4.1 状态枚举

| 状态 | 含义 |
|------|------|
| `queued` | run 已创建，等待执行 |
| `planning` | planner 生成 product spec |
| `awaiting_approval` | 等待人工决策 |
| `contracting` | generator / evaluator 协商当前 sprint contract |
| `building` | generator 正在实现当前 sprint |
| `qa` | evaluator 正在验收 |
| `repair` | generator 基于 qa_report 修复 |
| `checkpointing` | 写 git checkpoint 和 checkpoint metadata |
| `paused` | 用户或系统暂时停机 |
| `interrupted` | 进程崩溃、宿主机重启等异常中断 |
| `completed` | run 完成 |
| `failed` | run 终止且不可继续 |
| `cancelled` | 用户取消 |

### 4.2 状态机

```text
queued
  -> planning
planning
  -> awaiting_approval   (spec_gate enabled)
  -> contracting         (spec_gate disabled or auto-approved)
awaiting_approval
  -> contracting         (approved spec gate)
  -> checkpointing       (approved checkpoint gate)
  -> completed           (approved delivery gate)
  -> failed              (rejected spec gate)
  -> repair              (rejected checkpoint gate with revise policy)
contracting
  -> building
  -> failed              (contract negotiation exhausted)
building
  -> qa
  -> interrupted         (worker crash / host crash)
  -> failed              (budget exhausted)
qa
  -> checkpointing       (pass)
  -> repair              (fail and repairs left)
  -> failed              (fail and repairs exhausted)
repair
  -> building
  -> failed
checkpointing
  -> awaiting_approval   (checkpoint gate enabled)
  -> contracting         (next sprint exists)
  -> awaiting_approval   (delivery gate enabled and last sprint done)
  -> completed           (last sprint done and no delivery gate)
paused
  -> queued              (resume)
interrupted
  -> queued              (resume)
```

### 4.3 状态切换不变量

- 每次切换都必须先写数据库，再发 SSE 事件
- 每个状态切换都必须记录 `request_id`、`thread_id`、`run_id`
- `current_sprint` 只能单调递增，repair 不增加 sprint 序号
- `awaiting_approval` 时不允许后台继续修改代码

---

## 5. Run State 持久化模型

数据库表 `runs` 推荐最少包含：

```json
{
  "run_id": "01hr_run",
  "thread_id": "01hr_thread",
  "user_id": "u_01",
  "mode": "autonomous",
  "state": "qa",
  "current_sprint": 2,
  "planned_sprints": 8,
  "repair_count_current_sprint": 1,
  "approval_gate_type": null,
  "approval_gate_id": null,
  "workspace_path": "/data/workspaces/u_01/01hr_run",
  "product_spec_path": "artifacts/product_spec.md",
  "current_contract_path": "artifacts/sprint_contracts/sprint-02.json",
  "current_qa_report_path": null,
  "latest_handoff_path": "artifacts/handoffs/handoff-2026-04-02T10-30-00Z.md",
  "latest_checkpoint_name": "sprint-01",
  "latest_checkpoint_commit_sha": "abc123",
  "tokens_used": 184220,
  "tokens_limit": 2500000,
  "wall_clock_minutes_used": 38,
  "wall_clock_minutes_limit": 360,
  "last_progress_at": "2026-04-02T10:35:00Z",
  "created_at": "2026-04-02T10:00:00Z",
  "updated_at": "2026-04-02T10:35:00Z"
}
```

说明：

- `runs` 表是当前态索引
- 历史事件单独写入 `run_events`
- artifact 文件本体不塞进数据库，只存路径和摘要

### 5.1 SQLite 存储策略

本项目的权威状态库固定为单个 SQLite 数据库。

建议最少表：

- `threads`
- `thread_messages`
- `thread_summaries`
- `runs`
- `run_events`
- `approval_gates`
- `sprint_contracts`
- `qa_reports`
- `checkpoints`
- `artifact_index`
- `user_memory_entries`
- `agent_working_memory`
- `memory_promotion_queue`

约束：

- 开启 WAL 模式
- 设置 `busy_timeout`
- 写操作通过单写队列或显式事务串行化，避免长任务下写锁抖动
- 大文本 artifact、日志、截图不写入 SQLite，只写索引和路径
- `runs`、`run_events`、`approval_gates`、`user_memory_entries` 的热路径写入由 Python Harness Runtime 统一负责
- Rust Core 不直接写 run 热路径状态，只消费 IPC 事件并维护内存重播缓冲区

### 5.2 Memory 持久化边界

memory 系统也使用同一个 SQLite 数据库，但必须和 run 恢复态隔离。

允许存入 `user_memory_entries` / `thread_summaries` 的内容：

- 用户偏好
- 稳定事实
- thread 级摘要

推荐字段：

- `key`
- `value`
- `source` (`user_confirmed` / `system_inferred` / `imported`)
- `confidence`
- `confirmed_by_user`
- `updated_at`

禁止存入 memory 表的内容：

- 当前 run 的临时恢复游标
- 当前 sprint 的半成品状态
- 未 checkpoint 的工作树快照

写入规则：

- `user_confirmed` 事实优先级最高，不能被 `system_inferred` 覆盖
- `system_inferred` 事实默认只作为软偏好参与提示，不作为强约束
- 只有用户明确确认，或同类事实在多次独立 run 中重复出现且置信度达阈值，才允许升级为稳定记忆

---

## 6. Artifact 协议

### 6.1 目录布局

```text
/data/workspaces/{user_id}/{run_id}/
├── repo/
├── workspace/
├── artifacts/
│   ├── product_spec.md
│   ├── sprint_contracts/
│   │   ├── sprint-01.json
│   │   └── sprint-02.json
│   ├── qa_reports/
│   │   ├── sprint-01.json
│   │   └── sprint-02.json
│   ├── handoffs/
│   │   ├── handoff-2026-04-02T10-10-00Z.md
│   │   └── handoff-2026-04-02T10-50-00Z.md
│   ├── approvals/
│   │   └── approval-02.json
│   └── summaries/
│       └── run_summary.md
├── checkpoints/
│   ├── sprint-01.json
│   └── sprint-02.json
├── outputs/
├── uploads/
└── logs/
```

### 6.2 `product_spec.md`

建议使用 Markdown + frontmatter。

```md
---
run_id: 01hr_run
created_at: 2026-04-02T10:03:00Z
planner_model: planner-model
planned_sprints: 8
status: approved
---

# Product Goal

...
```

最少章节：

- Product Goal
- Users and Primary Flows
- Visual / UX Direction
- Technical Boundaries
- Sprint Breakdown
- Risks and Open Questions

### 6.3 `sprint_contract.json`

```json
{
  "contract_id": "contract_02",
  "run_id": "01hr_run",
  "sprint": 2,
  "status": "accepted",
  "scope_in": ["Project dashboard", "Project creation flow"],
  "scope_out": ["Sprite editor"],
  "files_expected": ["frontend/src/app/runs/page.tsx"],
  "user_flows_to_verify": [
    "Create project from dashboard",
    "Persist project after refresh"
  ],
  "tests_to_run": [
    "pytest tests/test_projects.py",
    "pnpm playwright test dashboard.spec.ts"
  ],
  "evaluator_checks": [
    "Dashboard should render existing projects",
    "Create flow should persist to database"
  ],
  "done_definition": "Dashboard and create flow work end to end and pass tests.",
  "created_by": "generator",
  "reviewed_by": "evaluator",
  "accepted_at": "2026-04-02T10:40:00Z"
}
```

### 6.4 `qa_report.json`

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
  "thresholds": {
    "functionality": 0.75,
    "product_depth": 0.75,
    "ux_quality": 0.75,
    "code_quality": 0.75
  },
  "blocking_issues": [
    {
      "title": "Created project disappears after refresh",
      "severity": "high",
      "expected": "Project persists in database and remains visible.",
      "actual": "Project appears in UI but disappears after refresh.",
      "repro_steps": [
        "Open dashboard",
        "Create project",
        "Refresh page"
      ],
      "evidence": [
        "artifacts/screenshots/sprint-02-dashboard-fail.png"
      ]
    }
  ],
  "repair_backlog": [
    "Fix project persistence write path",
    "Add end-to-end regression test"
  ],
  "generated_at": "2026-04-02T11:12:00Z"
}
```

### 6.5 `handoff.md`

handoff 必须是人和模型都能快速消费的短文档。

建议结构：

- 当前状态
- 已完成内容
- 当前阻塞 / 失败原因
- 下一步动作
- 未完成测试
- 风险提示

### 6.6 `approval-xx.json`

```json
{
  "gate_id": "gate_02",
  "run_id": "01hr_run",
  "gate_type": "checkpoint_gate",
  "status": "awaiting_user",
  "summary": "Approve sprint 2 checkpoint",
  "checkpoint_name": "sprint-02",
  "related_artifacts": [
    "artifacts/qa_reports/sprint-02.json",
    "checkpoints/sprint-02.json"
  ]
}
```

---

## 7. Contract 协商协议

### 7.1 协商步骤

1. generator 读取 `product_spec.md`
2. generator 选择下一 sprint
3. generator 产出 contract 草案
4. evaluator 检查以下问题：
   - scope 是否过大
   - scope 是否偏题
   - done definition 是否可测
   - tests 是否足够验证
5. evaluator 返回接受或修改意见
6. 达成一致后将 contract 状态置为 `accepted`

### 7.2 协商失败策略

出现以下情况直接失败：

- contract 连续 3 次无法收敛
- generator 一直试图跨 sprint 加 scope
- evaluator 无法给出可执行验收标准

失败时：

- run 状态切到 `failed`
- 写 `handoff.md`
- 记录 `conflict_state` 错误

---

## 8. Generator 构建协议

进入 `building` 后，generator 必须遵守：

1. 只处理当前 sprint contract 内的 `scope_in`
2. 所有代码修改必须在 `repo/` 内完成
3. 关键命令输出写入 `logs/`
4. 关键文件变化、测试结果和未解风险写入 `handoff.md`
5. 编码结束后必须显式提交“ready for QA”

推荐输出：

- `artifacts/handoffs/handoff-<ts>.md`
- 最新测试摘要
- 如有 UI 变更，可附截图

---

## 9. Evaluator QA 协议

### 9.1 评分维度

每轮 QA 至少检查：

- `functionality`
- `product_depth`
- `ux_quality`
- `code_quality`

规则：

- 任意一项低于阈值即判定失败
- `blocking_issues` 非空即失败
- 没有证据链的 issue 不能写入 blocking list

### 9.2 证据来源

允许使用：

- Playwright 页面操作
- 前后端测试命令
- 控制台日志
- 网络请求日志
- 文件 diff
- 截图 / 可选录像

### 9.3 QA 输出要求

每份 `qa_report.json` 都必须包含：

- `result`
- `scores`
- `thresholds`
- `blocking_issues`
- `repair_backlog`
- `evidence`

---

## 10. Checkpoint 协议

### 10.1 生成时机

只有在以下条件都满足时才生成 checkpoint：

- sprint contract 已 `accepted`
- generator 完成构建
- evaluator QA `pass`
- 当前 gate 未阻塞

### 10.2 Git 约束

每个 sprint 至少一个 commit：

```text
sprint-02: dashboard and project creation flow
```

checkpoint metadata 文件示例：

```json
{
  "name": "sprint-02",
  "run_id": "01hr_run",
  "sprint": 2,
  "commit_sha": "abc123",
  "summary": "Project dashboard and create flow",
  "artifact_refs": [
    "artifacts/sprint_contracts/sprint-02.json",
    "artifacts/qa_reports/sprint-02.json"
  ],
  "created_at": "2026-04-02T11:20:00Z"
}
```

### 10.2.1 Checkpoint 与 DB 状态一致性

checkpoint 写入顺序必须固定：

1. 先完成 git commit
2. 再写 checkpoint metadata 文件
3. 最后在一个 SQLite 事务中更新 `runs.latest_checkpoint_*`、插入 `checkpoints`、追加 `run_events`

如果 1 成功但 2 或 3 失败：

- run 状态切到 `interrupted`
- 写 `checkpoint_incomplete` 错误事件
- 恢复流程优先做 reconcile，而不是盲目继续下一 sprint

### 10.3 恢复基线

恢复永远从“最后一个成功 checkpoint”开始，而不是从半成品工作树开始。

这样做的原因：

- 避免工作树脏状态污染恢复
- evaluator 结论始终对应确定代码快照
- approval gate 展示内容可重放

---

## 11. Approval Gate 协议

### 11.1 Gate 类型

- `spec_gate`
- `checkpoint_gate`
- `delivery_gate`

### 11.2 Gate 进入规则

`spec_gate`：

- planner 完成 spec
- spec 已落盘
- run 暂停等待用户确认

`checkpoint_gate`：

- QA 已 pass
- 已生成 checkpoint metadata
- sprint 影响高风险模块或配置要求必须人工确认

`delivery_gate`：

- 最后一个 sprint 完成
- `run_summary.md` 已生成

### 11.3 Gate 决策规则

approve：

- 记录决策和用户 note
- 发 `approval` 事件
- 跳转到下一个合法状态

reject：

- `spec_gate` 默认失败
- `checkpoint_gate` 可配置为 `repair` 或 `failed`
- `delivery_gate` 默认回到 `repair` 或新增收尾 sprint

### 11.4 Gate 超时

长时间无人决策的 gate 默认不自动继续。

建议策略：

- 12 小时无人处理 → 保持 `paused`
- 前端明确展示“等待人工决策，不在运行”

---

## 12. Budget 与 Watchdog

### 12.1 预算维度

```json
{
  "max_wall_clock_minutes": 360,
  "max_tokens_total": 2500000,
  "max_cost_usd": 200,
  "max_sprints": 10,
  "max_repairs_per_sprint": 3,
  "max_idle_minutes_without_progress": 15
}
```

### 12.2 进度定义

以下任一发生视为“有进度”：

- 文件树有实质修改
- 新 artifact 产生
- 测试通过数增加
- checkpoint 成功写入
- evaluator 生成新证据

### 12.3 Stall 检测

触发条件：

- 超过 `max_idle_minutes_without_progress`
- 无新事件、无文件变化、无日志增量

处理：

1. 写 `error` 事件
2. 自动收集 handoff
3. 进入 `paused` 或 `failed`

---

## 13. 恢复与断点续跑

### 13.1 Resume 前置检查

恢复前必须检查：

- run 当前状态为 `paused` 或 `interrupted`
- 最新 checkpoint 存在
- `product_spec.md` 存在
- 当前 sprint contract 存在
- 最近 handoff 可读

### 13.2 Resume 算法

```text
resume(run_id):
  1. load run row
  2. load latest successful checkpoint
  3. git checkout checkpoint commit
  4. load latest handoff
  5. if model strategy == compact:
       continue with summarized context
     else:
       create fresh session from handoff + artifacts
  6. set state = queued
  7. emit run_state event
```

### 13.3 Interrupted 处理

当 Python worker 崩溃、机器重启或依赖服务中断时：

- run 标记为 `interrupted`
- 当前半成品工作树不作为恢复基线
- 恢复时回到最后成功 checkpoint

---

## 14. Context 策略

### 14.1 Compact 模式

适用：

- 当前模型长上下文表现稳定
- 同一会话连续推进多个 sprint 效果更好

要求：

- 仍然必须写 artifact
- compact 后不允许丢弃 contract / qa / handoff

### 14.2 Reset With Handoff 模式

适用：

- 模型出现 context anxiety
- 上下文窗口膨胀
- 长时间运行后目标漂移

要求：

- 新 session 只能基于 handoff + artifact + checkpoint 恢复
- 不能依赖旧消息历史全文

---

## 15. MVP 实现切线

第一版先实现这些能力：

1. `queued -> planning -> contracting -> building -> qa -> checkpointing -> completed`
2. `spec_gate` 和 `delivery_gate`
3. 单 evaluator
4. 单浏览器 worker
5. 单仓库、单分支、单用户
6. `resume from latest checkpoint`

可以后补的能力：

- 多 evaluator 并行评估
- 中途动态插入 sprint
- evaluator pool
- delivery diff 审批高级视图
- 跨机分布式恢复

---

## 16. 对实现的硬要求

如果后续代码实现偏离以下要求，就会损伤长任务稳定性：

- 不允许跳过 contract 直接做大范围编码
- 不允许 evaluator 改代码
- 不允许 approval gate 期间继续改工作树
- 不允许恢复时基于未 checkpoint 的半成品工作树
- 不允许把 run state 混进长期记忆

这五条是 harness 质量底线。
