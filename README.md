# code-review

`code-review` 是一个 Rust CLI，用来把 diff、业务规则、上下文代码整理成更可靠的 AI code review prompt，并通过本机安装的 `copilot` CLI 执行真实审查。

## 现在支持什么

- `prompt`：从结构化输入生成 review prompt
- `assemble`：预览自动装配结果（含 Jira enrich）
- `run`：从 `git diff` 自动提取改动并生成 prompt，并自动扩展部分关联上下文
- `deep-review`：执行两阶段 review，第一轮先审，第二轮基于高风险点自动扩展上下文再深挖
- `validate`：检查 review 输入是否充分
- `template`：输出 review 模板
- `auth login`：调用真实 `copilot login`
- `auth status`：检测本地 session + 实测 `copilot -p` 登录状态
- `auth refresh`：重新探测并刷新本地 session 元信息
- `auth logout`：删除本地 session；可选尝试调用 `copilot logout`
- `auth whoami`：显示当前账号/来源/主机/脱敏 token 预览
- `review`：在已登录状态下，把 prompt 交给真实 `copilot` 执行一次审查

## 设计说明

### 1. 认证策略

这里不再保留任何 mock 登录逻辑，直接走真实 `copilot` CLI：

- 登录：`copilot login`
- 状态探测：`copilot --no-ask-user -p "reply with exactly OK"`
- 审查执行：`copilot --no-ask-user -p "...review prompt..."`

### 2. session 存储

本地 session 写到：

`~/.config/code-review/session.json`

这里保存的是**本地元信息**，比如：

- provider/source
- host
- user
- updated_at / last_probe_at
- 最近一次探测错误

不会在终端打印明文 secret。token 只显示脱敏预览或 `configured`。

### 3. token 来源

优先读取这些环境变量（由 `copilot` 官方支持）：

- `COPILOT_GITHUB_TOKEN`
- `GH_TOKEN`
- `GITHUB_TOKEN`

如果没有环境变量，但本地 `~/.copilot/config.json` 存在，则显示为 `configured`。

## 安装与构建

```bash
cargo build
cargo test
```

## 配置文件

默认配置文件路径：

```bash
~/.config/code-review/config.toml
```

示例：

```toml
[llm]
provider = "copilot"
model = "gpt-5.4"
models = ["gpt-5.4", "opus", "gpt-5"]

[jira]
provider = "native"
base_url = "https://your-company.atlassian.net"

[review]
mode = "standard"
include_context = true
context_budget_bytes = 48000
context_file_max_bytes = 12000
```

## 常见命令

### 登录

```bash
code-review auth login
```

### 查看状态

```bash
code-review auth status
code-review auth whoami
code-review auth refresh
```

### 查看可用模型

```bash
code-review models
code-review models --format json
```

### 登出

```bash
code-review auth logout
code-review auth logout --clear-remote
```

### 生成 prompt

支持三种 mode：
- `lite`：轻量版，适合日常 PR 快速筛错
- `standard`：标准版，适合常规团队 review
- `critical`：高价值版，适合核心业务，建议补更多 focus / 风险信息

```bash
code-review prompt \
  --mode standard \
  --stack "Rust + Axum + PostgreSQL" \
  --goal "修复重复下单" \
  --why "线上偶发重复提交" \
  --rule "一个订单只能支付一次" \
  --rule "幂等键必须生效" \
  --expected-normal "首次提交成功" \
  --expected-error "重复提交返回冲突" \
  --expected-edge "网络重试不应产生双写" \
  --diff-file /tmp/change.diff
```

### 预览 Jira / 自动装配结果

```bash
code-review assemble \
  --jira PROJ-123 \
  --jira-base-url https://your-company.atlassian.net
```

如果你想接自己的脚本、lib 或 opencli，当成 provider 用：

```bash
code-review assemble \
  --jira PROJ-123 \
  --jira-provider command \
  --jira-command 'my-jira-fetcher "{issue}"'
```

外部命令需要输出 JSON，例如：

```json
{
  "key": "PROJ-123",
  "summary": "修复重复下单",
  "description": "支付接口在网络重试下出现重复创建订单",
  "acceptance": ["一个订单只能支付一次", "重复提交返回冲突"],
  "comments": ["QA: 正常下单已通过", "测试: 幂等重试场景待补"],
  "labels": ["backend", "payment"],
  "components": ["order-service"],
  "issue_type": "Bug",
  "priority": "High"
}
```

### 从 git diff 自动生成

```bash
code-review run \
  --repo . \
  --git HEAD~1..HEAD \
  --include-context \
  --jira PROJ-123 \
  --stack "Rust + Axum + PostgreSQL"
```

### 执行真实 review

```bash
code-review review --prompt "请审查下面的变更，重点看边界条件、错误处理、并发和事务一致性。"
```

或者：

```bash
code-review review \
  --stack "Rust + Axum + PostgreSQL" \
  --goal "修复重复下单" \
  --rule "一个订单只能支付一次" \
  --expected-normal "首次提交成功" \
  --diff-file /tmp/change.diff
```

### 两阶段深度 review

`deep-review` 会先跑第一轮 review，然后自动从第一轮输出中提取：
- 文件路径
- 高风险点
- 不确定点
- 疑似关键函数/方法

并在第二轮里自动补更多关联上下文文件（test / dto / model / contract / 同目录高价值文件），更适合抓业务问题和实现逻辑问题。

```bash
code-review deep-review \
  --repo . \
  --git origin/main...HEAD \
  --include-context \
  --jira PROJ-123 \
  --jira-base-url https://your-company.atlassian.net
```

如果你不接 Jira，也可以手工传上下文：

```bash
code-review deep-review \
  --repo . \
  --git HEAD~1..HEAD \
  --include-context \
  --stack "Java 17 + Spring Boot 3" \
  --goal "修复重复下单"
```

## Prompt 策略

现在的 prompt 强调：

- 必须给出**文件/函数/代码片段定位**
- 必须说明**风险等级与原因**
- 必须给出**触发条件 / 影响范围 / 修复建议**
- 证据不足时明确写**“不确定，需要补充上下文”**
- 忽略纯格式、命名风格、无关紧要的建议

## 大上下文保护

为了避免一把把整个仓库塞进 prompt：

- 跳过二进制文件
- 跳过非 UTF-8 文件
- 跳过超大文件
- 对总上下文大小做预算裁剪
- 在 prompt 里附带 skipped / truncated 摘要

## 现场验证建议

你现在可以直接自己跑：

```bash
cd /home/delta/code-review
cargo build
cargo test
cargo run -- auth status
cargo run -- auth login
cargo run -- auth whoami
cargo run -- review --prompt "请用简短要点检查一个 Rust CLI 的错误处理、边界条件和状态管理。"
```

## 故障排查

### 1. `auth login` 偶发报 `fetch failed`

先别急着判断登录坏了，先探测当前会话：

```bash
cargo run -- auth status
```

如果这里仍然显示 `logged_in: true`，说明已有登录态可用，那次只是 `copilot login` 本身的网络/远端波动。

### 2. `review` 很慢或看起来像卡住

现在 `review` 已经加了超时控制，不会无限挂住。若输入很大，程序会自动把 prompt 落到临时文件，再用 `@file` 形式传给 `copilot`，避免 argv 过长。

如果还是超时，优先检查：

- 当前 Copilot 登录态是否有效
- 网络是否正常
- prompt 是否过大
- `copilot` CLI 本身是否在等待远端响应

### 3. session 里为什么没有明文 token

这是刻意的。`~/.config/code-review/session.json` 只保存脱敏预览或 `configured`，避免把 secret 落盘。

## 集成验证清单

交付前建议至少跑这几条：

```bash
cargo run -- auth status
cargo run -- auth refresh
cargo run -- auth whoami
cargo run -- review --prompt "请只输出三条，检查这个 CLI 的错误处理和边界条件风险。"
```

## 已知限制

- 依赖本机已安装并可用的 `copilot` CLI
- `whoami` 不调用私有用户信息 API，只展示本地和环境可确认的信息
- `refresh` 是重新探测登录态，不是直接刷新某个私有 token
