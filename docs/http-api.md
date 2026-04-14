# HTTP API Guide

`code-review` 现在支持本地 HTTP API，适合：
- 本地脚本调用
- Web 前端调用
- 其他服务集成

> 当前版本是**同步 API**。`review` / `deep-review` 会阻塞直到模型执行结束。

---

OpenAPI YAML 草案见：`docs/openapi.yaml`

## 1. 启动服务

默认监听：

```bash
cargo run -- serve
```

自定义监听地址：

```bash
cargo run -- serve --bind 0.0.0.0:3000
```

默认地址示例：

- Base URL: `http://127.0.0.1:3000`

---

## 2. 通用响应约定

### 2.1 成功响应
- `200 OK`
- body 为 JSON

### 2.2 错误响应
- `400 Bad Request`
- body 结构：

```json
{
  "error": "错误信息"
}
```

### 2.3 exit_code 语义
用于 `review` / `deep-review` 等接口返回中：

- `0`：执行成功，且无明显阻断问题
- `2`：执行成功，但存在高风险或需要人工复核
- `3`：输入不满足准入要求
- `4`：输出结构化校验失败，repair 后仍不合格
- `5`：运行时异常

---

## 3. 接口列表

- `GET /api/health`
- `GET /api/models`
- `POST /api/validate`
- `POST /api/prompt`
- `POST /api/assemble`
- `POST /api/run`
- `POST /api/review`
- `POST /api/deep-review`

---

## 4. Schema 概览

### 4.1 PromptArgs
很多接口共用这份结构：

```json
{
  "mode": "standard",
  "stack": "Rust + Axum + PostgreSQL",
  "goal": "修复重复下单",
  "why": "线上偶发重复提交",
  "rules": ["一个订单只能支付一次", "幂等键必须生效"],
  "risks": ["并发", "事务一致性"],
  "expected_normal": "首次提交成功",
  "expected_error": "重复提交返回冲突",
  "expected_edge": "网络重试不应双写",
  "issue": "支付接口在网络重试下出现重复创建订单",
  "test_results": ["订单单测通过"],
  "jira": "PROJ-123",
  "jira_base_url": "https://your-company.atlassian.net",
  "jira_provider": "native",
  "jira_command": null,
  "diff_file": null,
  "context_files": [],
  "files": ["src/order/service.rs"],
  "focus": ["事务一致性"],
  "baseline_files": [],
  "change_type": "server",
  "format": "json"
}
```

说明：
- `mode`: `lite | standard | critical`
- `format`: `text | json`
- `change_type`: 常见值 `server | db | frontend | infra`

### 4.2 RunArgs

```json
{
  "git": "HEAD~1..HEAD",
  "repo": ".",
  "prompt": { "...PromptArgs...": "..." },
  "include_context": true,
  "context_budget_bytes": 48000,
  "context_file_max_bytes": 12000
}
```

### 4.3 ReviewArgs

```json
{
  "prompt": null,
  "model": "gpt-5.4",
  "prompt_args": { "...PromptArgs...": "..." }
}
```

### 4.4 DeepReviewArgs

```json
{
  "git": "HEAD~1..HEAD",
  "repo": ".",
  "model": "gpt-5.4",
  "prompt": { "...PromptArgs...": "..." },
  "include_context": true,
  "context_budget_bytes": 48000,
  "context_file_max_bytes": 12000
}
```

---

## 5. 核心响应结构

### 5.1 AdmissionResult (`/api/validate`)

```json
{
  "ok": true,
  "level": "pass",
  "score": 75,
  "confidence": "medium",
  "missing_p0": [],
  "missing_p1": ["test_results"],
  "missing_p2": [],
  "warnings": [],
  "block_reasons": [],
  "suggestions": ["补充测试结果，方便判断风险是否已覆盖"]
}
```

### 5.2 PromptOutput (`/api/prompt`, `/api/run`)

```json
{
  "ok": true,
  "score": 75,
  "prompt": "完整 prompt 文本...",
  "summary": {
    "stack": "Rust + Axum + PostgreSQL",
    "goal": "修复重复下单",
    "issue": "支付接口在网络重试下出现重复创建订单",
    "rules_count": 2,
    "risks": ["并发"],
    "test_results_count": 1,
    "files": ["src/order/service.rs"],
    "context_files": [],
    "has_diff": false
  }
}
```

### 5.3 ReviewResult (`/api/review`, `/api/deep-review` 内部 stage)

```json
{
  "mode": "standard",
  "input_ok": true,
  "input_level": "pass",
  "input_score": 80,
  "confidence": "high",
  "high_risk": [
    {
      "title": "重复提交可能导致双写",
      "file": "src/order/service.rs",
      "location": "create_order",
      "reason": "缺少幂等校验",
      "trigger": "并发重试",
      "impact": "重复订单/重复扣款",
      "suggestion": "补充唯一约束或幂等键保护"
    }
  ],
  "medium_risk": [],
  "low_risk": [],
  "missing_tests": [],
  "summary": "发现 1 个高风险问题，建议人工复核。",
  "needs_human_review": true,
  "used_rules": ["一个订单只能支付一次", "幂等键必须生效"],
  "impact_scope": ["接口字段变化可能影响调用方兼容性"],
  "release_checks": ["发布前确认回滚方案"],
  "risk_hints": [
    {
      "title": "API / 契约变更风险",
      "detail": "检测到 DTO / API 相关文件改动，需确认兼容性。",
      "source": "file-path"
    }
  ],
  "validation_report": {
    "ok": true,
    "repaired": false,
    "findings": []
  },
  "repair_attempted": false,
  "repair_succeeded": false,
  "raw_text": "模型原始输出..."
}
```

---

## 6. 接口详解与 curl 示例

### 6.1 GET /api/health

```bash
curl -s http://127.0.0.1:3000/api/health | jq
```

### 6.2 GET /api/models

```bash
curl -s http://127.0.0.1:3000/api/models | jq
```

### 6.3 POST /api/validate

```bash
curl -s -X POST http://127.0.0.1:3000/api/validate \
  -H 'Content-Type: application/json' \
  -d '{
    "mode": "standard",
    "stack": "Rust + Axum + PostgreSQL",
    "goal": "修复重复下单",
    "rules": ["一个订单只能支付一次", "幂等键必须生效"],
    "files": ["src/order/service.rs"],
    "format": "json"
  }' | jq
```

### 6.4 POST /api/prompt

```bash
curl -s -X POST http://127.0.0.1:3000/api/prompt \
  -H 'Content-Type: application/json' \
  -d '{
    "mode": "standard",
    "stack": "Rust + Axum + PostgreSQL",
    "goal": "修复重复下单",
    "why": "线上偶发重复提交",
    "rules": ["一个订单只能支付一次", "幂等键必须生效"],
    "expected_normal": "首次提交成功",
    "expected_error": "重复提交返回冲突",
    "expected_edge": "网络重试不应双写",
    "files": ["src/order/service.rs"],
    "format": "json"
  }' | jq
```

### 6.5 POST /api/assemble

```bash
curl -s -X POST http://127.0.0.1:3000/api/assemble \
  -H 'Content-Type: application/json' \
  -d '{
    "mode": "standard",
    "jira": "PROJ-123",
    "jira_base_url": "https://your-company.atlassian.net",
    "jira_provider": "native",
    "format": "json"
  }' | jq
```

### 6.6 POST /api/run

```bash
curl -s -X POST http://127.0.0.1:3000/api/run \
  -H 'Content-Type: application/json' \
  -d '{
    "git": "HEAD~1..HEAD",
    "repo": ".",
    "prompt": {
      "mode": "standard",
      "stack": "Rust + Axum + PostgreSQL",
      "goal": "修复重复下单",
      "rules": ["一个订单只能支付一次", "幂等键必须生效"],
      "format": "json"
    },
    "include_context": true,
    "context_budget_bytes": 48000,
    "context_file_max_bytes": 12000
  }' | jq
```

### 6.7 POST /api/review

> 需要先在本机完成 `code-review auth login`

```bash
curl -s -X POST http://127.0.0.1:3000/api/review \
  -H 'Content-Type: application/json' \
  -d '{
    "model": "gpt-5.4",
    "prompt_args": {
      "mode": "standard",
      "stack": "Rust + Axum + PostgreSQL",
      "goal": "修复重复下单",
      "rules": ["一个订单只能支付一次", "幂等键必须生效"],
      "expected_normal": "首次提交成功",
      "files": ["src/order/service.rs"],
      "format": "json"
    }
  }' | jq
```

### 6.8 POST /api/deep-review

```bash
curl -s -X POST http://127.0.0.1:3000/api/deep-review \
  -H 'Content-Type: application/json' \
  -d '{
    "git": "HEAD~1..HEAD",
    "repo": ".",
    "model": "gpt-5.4",
    "prompt": {
      "mode": "critical",
      "stack": "Rust + Axum + PostgreSQL",
      "goal": "修复重复下单",
      "rules": ["一个订单只能支付一次", "幂等键必须生效"],
      "focus": ["支付安全", "事务一致性"],
      "format": "json"
    },
    "include_context": true,
    "context_budget_bytes": 48000,
    "context_file_max_bytes": 12000
  }' | jq
```

---

## 7. 限制说明

- 当前是本地服务化入口，不是多租户平台
- 仍依赖本机 git 仓库和本机 `copilot` 登录态
- `review` / `deep-review` 没有任务队列，耗时会直接阻塞请求
- 还没有正式 OpenAPI 生成器，这份文档是当前接口契约草案
