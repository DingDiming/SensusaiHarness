# SensusAI Harness Runbook

> 目标：给开发和运维一个最小可执行手册，覆盖 long-running harness 最常见的排障与恢复场景。

---

## 1. 核心原则

- 不直接操作未 checkpoint 的半成品工作树
- 恢复优先从最后一个成功 checkpoint 开始
- 审批中的 run 不应继续推进
- 先看 `run state`，再看日志，最后看工作区

---

## 2. 日常检查

每次值班至少检查：

1. `/api/core/health`
2. `/api/core/metrics`
3. 活跃 run 数和状态分布
4. 最近 1 小时 failed / interrupted run
5. 浏览器 worker 占用
6. `/data/workspaces` 磁盘占用

---

## 3. 常见故障处理

### 3.0 SQLite 写锁或 `database is locked`

检查：

1. 是否开启 WAL
2. 是否设置 `busy_timeout`
3. 是否有长事务未提交
4. `run_events` 写入是否和其他写路径互相竞争

处理：

- 优先缩短事务范围
- 对 run 事件和 memory 写入走串行写队列
- 必要时暂停新 run，只恢复已有 run

### 3.1 Run 卡在 `building`

检查顺序：

1. `last_progress_at` 是否持续更新
2. `logs/` 是否仍有输出
3. 文件树是否还有变化
4. 是否命中 `max_idle_minutes_without_progress`

处理：

- 若仍有进度，继续观察
- 若无进度且超过阈值，pause run 并生成 handoff
- 若工具进程僵死，标记 `interrupted`

### 3.2 Run 卡在 `awaiting_approval`

检查：

1. 当前 gate 类型
2. 对应 `approval-xx.json`
3. 前端是否能看到 gate

处理：

- 通知用户或管理员审批
- 不自动继续
- 超过 12 小时无人处理，保持 `paused`

### 3.3 QA 连续失败

检查：

1. 最近 `qa_report.json`
2. `repair_count_current_sprint`
3. 失败是否同一问题反复出现

处理：

- 未超过 repair 上限：允许继续 repair
- 超过上限：标记 `failed`
- 记录最后一个 handoff，必要时人工接管

### 3.4 服务重启后恢复

步骤：

1. 找出状态为 `interrupted` 的 run
2. 读取最新 checkpoint
3. 校验 `product_spec.md`、`sprint_contract.json`、`handoff.md`
4. 调用 resume
5. 确认 SSE 流恢复正常

---

## 4. 恢复决策表

| 场景 | 处理 |
|------|------|
| Python worker 崩溃 | 标记 `interrupted`，从最新 checkpoint resume |
| 浏览器 worker 崩溃 | 重启 browser worker，重跑当前 QA |
| JWT 过期 | 用户重新登录，不影响后台 run |
| SSE 断开 | 前端重连并用 `Last-Event-ID` 补流 |
| 宿主机磁盘不足 | 暂停新 run，清理过期 workspace |

---

## 5. 人工接管

人工接管时先看：

1. 当前 `run` 记录
2. 最新 `handoff.md`
3. 当前 `sprint_contract.json`
4. 最近 `qa_report.json`
5. 最新 checkpoint metadata

人工接管后：

- 要么 resume run
- 要么终止 run 并保留 artifacts
- 不要在未记录原因的情况下直接删 workspace

---

## 6. 清理策略

- 完成态 run：保留 30 天
- failed / cancelled / interrupted run：保留 14 天
- 上传文件和截图按 run 生命周期清理
- 清理前导出 checkpoint metadata 和 run summary

---

## 7. 需要补自动化的项

- failed / interrupted run 每小时巡检
- stuck run 自动 pause
- workspace 磁盘用量告警
- browser worker 健康告警
- approval gate 长时间无人处理告警
