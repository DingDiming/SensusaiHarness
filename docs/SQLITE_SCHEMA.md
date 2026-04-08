# SensusAI Harness SQLite Schema

> 目标：给当前单机私有化版本落一版可执行的 SQLite schema，并把多智能体记忆共享策略固定成表设计。
> 对应 DDL：`docs/sqlite_schema_v1.sql`

---

## 1. 设计结论

数据库方案固定为：

- 单个 `SQLite` 数据库作为权威状态库
- 开启 `WAL`
- `Python Harness Runtime` 作为热路径单写 owner
- `Rust Core` 不直接写 run 热路径表，只消费事件并提供 SSE / 文件能力

这套 schema 明确采用混合记忆模型：

- **共享事实层**：`user_memory_entries`、`thread_summaries`
- **共享交付层**：`sprint_contracts`、`qa_reports`、`checkpoints`、`artifact_index`
- **私有工作层**：`agent_working_memory`
- **提升队列**：`memory_promotion_queue`

核心原则：

- 共享事实，不共享草稿
- 共享 artifact，不共享推理过程
- evaluator 只读外显产物，不读 generator 私有 working memory

---

## 2. 表分层

### 2.1 认证与用户

- `users`
- `auth_refresh_tokens`

作用：

- 支持私有化用户名密码登录
- 支持 token refresh

### 2.2 对话与运行

- `threads`
- `thread_messages`
- `runs`
- `run_events`
- `approval_gates`

作用：

- 把用户对话和可恢复运行状态分离
- `runs` 只保存当前态
- `run_events` 保存时序事件，用于 SSE 重放和审计

### 2.3 Sprint 与 QA

- `sprint_contracts`
- `qa_reports`
- `checkpoints`
- `artifact_index`

作用：

- 记录每轮 sprint 协议、验收结果和恢复基线
- artifact 正文落磁盘，这些表只存索引和摘要

### 2.4 记忆

- `thread_summaries`
- `user_memory_entries`
- `agent_working_memory`
- `memory_promotion_queue`

作用：

- `thread_summaries`：thread 级摘要
- `user_memory_entries`：稳定偏好与确认事实
- `agent_working_memory`：planner / generator / evaluator 私有草稿
- `memory_promotion_queue`：私有记忆进入共享记忆前的受控提升

---

## 3. 多智能体记忆策略

### 3.1 应共享的内容

应进入共享层的内容：

- 用户明确确认的偏好
- 项目稳定事实
- approved spec
- accepted sprint contract
- QA 结论
- checkpoint 元数据
- thread 级总结

### 3.2 应隔离的内容

只应存在私有 working memory 的内容：

- planner 的拆解草稿
- generator 的临时实现计划
- evaluator 的未确认怀疑和测试笔记
- 尚未通过 QA 的推断

### 3.3 Promotion 规则

只有以下情况允许把私有记忆提升为共享记忆：

- 用户明确确认
- 同类事实在多个独立 run 中重复出现且置信度达阈值
- 系统导入的外部真值配置

`system_inferred` 事实默认不能覆盖 `user_confirmed`。

---

## 4. 写入所有权

### 4.1 Python Harness Runtime 写入

由 Python 统一写入的热路径表：

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

原因：

- SQLite 写锁模型更适合单 owner
- 减少 Rust / Python 双写竞争
- run 生命周期和 memory promotion 都属于 Harness Runtime 业务规则

### 4.2 Rust Core 责任

Rust Core 只负责：

- schema migration owner
- 认证
- 文件上传 / 下载
- SSE fan-out
- 内存态 replay buffer

Rust 不直接更新 `runs.state` 或 `run_events`，通过 IPC 接收 Python 归一化后的事件。

---

## 5. 主表说明

### 5.1 `runs`

关键字段：

- `state`
- `current_sprint`
- `repair_count_current_sprint`
- `active_gate_id`
- `latest_checkpoint_*`
- `tokens_used`
- `wall_clock_minutes_used`
- `last_progress_at`

这是 run 当前态真相表，不记录完整历史。

### 5.2 `run_events`

关键字段：

- `sequence_no`
- `event_type`
- `payload_json`

它是：

- SSE 重放源
- 审计辅助源
- 故障排查辅助源

注意：

- 这是最大增长表
- 必须做归档、裁剪、摘要化

### 5.3 `user_memory_entries`

关键字段：

- `scope_kind`
- `memory_key`
- `value_json`
- `source`
- `confidence`
- `confirmed_by_user`
- `is_active`

建议只保留一条“当前激活版本”，旧版本通过 `is_active = 0` 留存。

### 5.4 `agent_working_memory`

这是多智能体隔离的关键表。

每条记录按这些维度隔离：

- `run_id`
- `agent_role`
- `sprint`

典型用途：

- planner draft
- generator TODO / hypothesis
- evaluator unresolved suspicion

### 5.5 `memory_promotion_queue`

这是 shared/private 边界控制器。

典型流程：

1. agent 在 `agent_working_memory` 记录候选事实
2. runtime 评估是否可提升
3. 若可提升，插入 `memory_promotion_queue`
4. 再根据规则写入 `user_memory_entries` 或 `thread_summaries`

---

## 6. 索引策略

高优先级索引：

- `threads(user_id, updated_at desc)`
- `runs(user_id, state, updated_at desc)`
- `runs(thread_id, created_at desc)`
- `run_events(run_id, sequence_no)`
- `approval_gates(run_id, status, opened_at desc)`
- `sprint_contracts(run_id, sprint, version)`
- `qa_reports(run_id, sprint, generated_at desc)`
- `checkpoints(run_id, sprint)`
- `artifact_index(run_id, path)`
- `user_memory_entries(user_id, scope_kind, scope_id, memory_key)` with active-only uniqueness
- `agent_working_memory(run_id, agent_role, sprint, created_at desc)`
- `memory_promotion_queue(status, created_at)`

避免的模式：

- 大范围 `LIKE '%...%'`
- 在 `run_events.payload_json` 上做临时扫描查询
- 把大文本日志直接塞进表里

---

## 7. SQLite 运行约束

必须开启：

- `journal_mode=WAL`
- `foreign_keys=ON`
- `busy_timeout`

推荐约束：

- 短事务
- checkpoint / run state 更新放在显式事务里
- 大批量归档在低峰执行

不建议：

- 多个进程同时写热路径表
- 在 read transaction 中长时间占着连接
- 把 screenshot、log blob、patch 正文放进 SQLite

---

## 8. 本版 schema 覆盖范围

这一版已经覆盖：

- auth
- thread / message
- run lifecycle
- approval gate
- sprint contract
- QA report
- checkpoint
- artifact index
- user memory
- private agent working memory
- memory promotion queue

尚未覆盖：

- 全文检索
- analytics / BI 专用只读副本
- 跨机复制

---

## 9. 实施建议

按这个顺序落地最稳：

1. 先建 `users`、`threads`、`runs`
2. 再建 `run_events`、`approval_gates`
3. 再建 `sprint_contracts`、`qa_reports`、`checkpoints`、`artifact_index`
4. 最后建 `user_memory_entries`、`agent_working_memory`、`memory_promotion_queue`

实现时的第一批仓储接口建议先做：

- `create_thread`
- `create_run`
- `append_run_event`
- `update_run_state`
- `upsert_approval_gate`
- `insert_checkpoint`
- `list_run_artifacts`
- `upsert_user_memory_entry`
- `append_agent_working_memory`
- `enqueue_memory_promotion`
