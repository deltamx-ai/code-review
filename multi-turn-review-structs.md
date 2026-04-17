# 多轮对话 Code Review：详细表结构 / Rust Struct 草案

这份文档目标很直接：

- 先把“会话式 review”需要保存什么讲清楚
- 再给出一套可以直接落地的 Rust 数据结构
- 最后补一版 API DTO 草案，方便 CLI / HTTP / Web UI 共用

默认设计方向：
- **本地优先**
- **文件存储可跑，后续可切 SQLite**
- **先支持单会话串行执行，后续再扩并发/流式**
- **兼容当前 `ReviewResult`、`PromptArgs`、`ReviewMode` 等已有类型**

---

# 一、推荐的存储模型

建议先按“两层模型”设计：

## 1. 逻辑层对象
逻辑层是代码里真正使用的对象：
- ReviewSession
- ReviewTurn
- ReviewMessage
- ReviewFinding
- ReviewArtifact
- ReviewSnapshot

这些对象不需要一开始就和数据库表 1:1 绑定，但要稳定。

## 2. 持久层对象
持久层可以先用：
- `sessions/<id>/session.json`
- `sessions/<id>/messages.jsonl`
- `sessions/<id>/findings.json`
- `sessions/<id>/artifacts/`

后面如果切 SQLite，可以自然映射到表：
- `review_sessions`
- `review_turns`
- `review_messages`
- `review_findings`
- `review_artifacts`
- `review_checkpoints`

所以我下面给你两套：
1. **推荐表结构**
2. **推荐 Rust struct**

---

# 二、详细表结构设计

> 即使你现在先不上数据库，这套表结构也值得先定下来。因为它能反向约束你的 domain model。

---

## 表 1：`review_sessions`

一条记录代表一个完整的 review 会话。

### 字段

| 字段 | 类型 | 说明 |
|---|---|---|
| id | TEXT / UUID | 会话 ID |
| status | TEXT | 会话状态：created/running/waiting_input/completed/failed/cancelled |
| review_mode | TEXT | standard / critical 等 |
| strategy | TEXT | single_turn / deep_review / conversation |
| repo_root | TEXT | 仓库根目录 |
| base_ref | TEXT NULL | 基线分支/commit |
| head_ref | TEXT NULL | 对比分支/commit |
| title | TEXT NULL | 会话标题，可用于 UI |
| created_by | TEXT NULL | 来源，如 cli/api/web |
| provider | TEXT | copilot/openai/ollama/... |
| model | TEXT | 模型名 |
| temperature | REAL NULL | 可选 |
| current_turn | INTEGER | 当前轮次 |
| total_turns | INTEGER | 累计轮次 |
| admission_level | TEXT NULL | P0/P1/P2 |
| admission_score | INTEGER NULL | 准入评分 |
| admission_ok | BOOLEAN NULL | 是否通过准入 |
| final_summary | TEXT NULL | 最终总结 |
| final_report_json | TEXT NULL | 最终结构化结果 JSON |
| last_error | TEXT NULL | 最近错误 |
| created_at | TEXT | 创建时间 |
| updated_at | TEXT | 更新时间 |
| completed_at | TEXT NULL | 完成时间 |

### 说明

这一张表是总表。

它不负责保存每条消息，只负责：
- 这次 review 是谁
- 审到哪了
- 用了哪个模型
- 当前是不是完成了
- 最终结论是什么

---

## 表 2：`review_turns`

一条记录代表会话中的一轮。

### 字段

| 字段 | 类型 | 说明 |
|---|---|---|
| id | TEXT / UUID | turn ID |
| session_id | TEXT | 所属会话 |
| turn_no | INTEGER | 第几轮，从 1 开始 |
| turn_kind | TEXT | discovery / deep_dive / business_check / final_report / manual_followup |
| status | TEXT | pending/running/completed/failed/skipped |
| input_summary | TEXT NULL | 本轮输入摘要 |
| instruction | TEXT NULL | 本轮附加指令 |
| requested_files_json | TEXT NULL | 本轮请求补充的文件列表 |
| attached_files_json | TEXT NULL | 本轮实际附加的文件列表 |
| focus_findings_json | TEXT NULL | 本轮重点检查的问题 ID 列表 |
| prompt_text | TEXT NULL | 兼容旧实现，可保存完整 prompt |
| response_text | TEXT NULL | 原始模型输出 |
| parsed_result_json | TEXT NULL | 本轮结构化结果 |
| token_input | INTEGER NULL | 输入 token |
| token_output | INTEGER NULL | 输出 token |
| latency_ms | INTEGER NULL | 耗时 |
| started_at | TEXT NULL | 开始时间 |
| completed_at | TEXT NULL | 完成时间 |
| created_at | TEXT | 创建时间 |
| updated_at | TEXT | 更新时间 |

### 说明

`review_turns` 是“多轮对话”最关键的一张表。

因为你最后会发现：
- session 是总容器
- messages 是对话细节
- turns 才是“业务阶段”的边界

也就是说，一轮里可能有多条 message，但业务上仍然算同一轮。

---

## 表 3：`review_messages`

保存真正发给模型/从模型返回的消息。

### 字段

| 字段 | 类型 | 说明 |
|---|---|---|
| id | TEXT / UUID | message ID |
| session_id | TEXT | 所属会话 |
| turn_id | TEXT NULL | 所属轮次 |
| seq_no | INTEGER | 在整个会话中的顺序 |
| role | TEXT | system/user/assistant/tool |
| author | TEXT NULL | 来源，如 orchestrator/user/provider/tool |
| content | TEXT | 消息正文 |
| content_format | TEXT | text/markdown/json |
| meta_json | TEXT NULL | 扩展元信息 |
| created_at | TEXT | 创建时间 |

### 说明

这一张表保存真正的对话历史。

后面如果 provider 支持 chat(messages)，请求就可以从这张表回放。

`meta_json` 可以存：
- 本条消息关联的文件 ID
- 本条消息关联的 finding ID
- 模型 finish_reason
- provider 返回的 request_id

---

## 表 4：`review_findings`

保存问题项，供多轮持续追踪。

### 字段

| 字段 | 类型 | 说明 |
|---|---|---|
| id | TEXT / UUID | finding ID |
| session_id | TEXT | 所属会话 |
| source_turn_id | TEXT NULL | 首次发现它的 turn |
| code | TEXT NULL | 可读编号，如 F-001 |
| severity | TEXT | critical/high/medium/low/info |
| category | TEXT | logic/performance/security/compatibility/testability/release/data |
| title | TEXT | 标题 |
| description | TEXT | 问题描述 |
| rationale | TEXT NULL | 原因解释 |
| suggestion | TEXT NULL | 修复建议 |
| confidence | REAL NULL | 置信度 0~1 |
| status | TEXT | suspected/confirmed/dismissed/fixed/accepted_risk |
| owner | TEXT NULL | 责任人，可选 |
| file_path | TEXT NULL | 主文件 |
| line_start | INTEGER NULL | 起始行 |
| line_end | INTEGER NULL | 结束行 |
| function_name | TEXT NULL | 相关函数/方法 |
| evidence_json | TEXT NULL | 证据列表 |
| related_files_json | TEXT NULL | 关联文件 |
| tags_json | TEXT NULL | 标签 |
| last_seen_turn | INTEGER NULL | 最后一次在哪轮出现 |
| created_at | TEXT | 创建时间 |
| updated_at | TEXT | 更新时间 |
| resolved_at | TEXT NULL | 解决时间 |

### 说明

这是整个多轮系统最值钱的一张表。

因为多轮 review 的实质不是聊天，而是：

> 一批问题项在多个轮次里被提出、验证、升级、驳回、收敛。

所以你一定要把 finding 单独建模，而不是把它埋在一堆纯文本里。

---

## 表 5：`review_artifacts`

保存每轮引用或生成的工件。

### 字段

| 字段 | 类型 | 说明 |
|---|---|---|
| id | TEXT / UUID | artifact ID |
| session_id | TEXT | 所属会话 |
| turn_id | TEXT NULL | 来源轮次 |
| artifact_type | TEXT | diff/context_file/prompt/response/report/jira/test_result |
| name | TEXT | 名称 |
| path | TEXT NULL | 文件路径 |
| content | TEXT NULL | 内容快照，小文件可直接存 |
| mime_type | TEXT NULL | 类型 |
| size_bytes | INTEGER NULL | 大小 |
| hash | TEXT NULL | 内容 hash |
| meta_json | TEXT NULL | 元数据 |
| created_at | TEXT | 创建时间 |

### 说明

工件表用来解决两个问题：
- 你到底把哪些内容喂给了模型
- 最终结果是基于哪些证据得出的

比如：
- 原始 diff
- 自动扩展的上下文文件内容
- Jira 摘要
- 测试结果
- 生成的最终报告

都可以走 artifact。

---

## 表 6：`review_checkpoints`

保存阶段性快照，方便恢复与调试。

### 字段

| 字段 | 类型 | 说明 |
|---|---|---|
| id | TEXT / UUID | checkpoint ID |
| session_id | TEXT | 所属会话 |
| turn_id | TEXT NULL | 关联轮次 |
| checkpoint_type | TEXT | before_turn/after_turn/final |
| snapshot_json | TEXT | 状态快照 |
| created_at | TEXT | 创建时间 |

### 说明

这个表不是必须，但我很建议保留。

因为多轮系统调试时最痛苦的是：
- 为什么第 2 轮丢了上下文
- 为什么 finding 被错误降级
- 为什么最终报告缺了 release checks

如果有 checkpoint，就很容易回放。

---

# 三、文件存储版本目录结构

如果你先不用 SQLite，建议目录这样设计：

```text
.alma/review-sessions/
  <session_id>/
    session.json
    turns/
      0001.json
      0002.json
    messages.jsonl
    findings.json
    checkpoints/
      before-turn-1.json
      after-turn-1.json
    artifacts/
      diff.patch
      src_order_service.rs.txt
      jira-ABC-123.md
      final-report.json
```

这样切数据库也很顺。

---

# 四、Rust Domain Struct 草案

下面这套 struct 尽量贴近你现有项目风格，默认用 `serde` 可序列化。

---

## 1. 基础枚举

```rust
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ConversationStatus {
    Created,
    Running,
    WaitingInput,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TurnKind {
    Discovery,
    DeepDive,
    BusinessCheck,
    FinalReport,
    ManualFollowup,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TurnStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Skipped,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MessageRole {
    System,
    User,
    Assistant,
    Tool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ContentFormat {
    Text,
    Markdown,
    Json,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FindingSeverity {
    Critical,
    High,
    Medium,
    Low,
    Info,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FindingStatus {
    Suspected,
    Confirmed,
    Dismissed,
    Fixed,
    AcceptedRisk,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FindingCategory {
    Logic,
    Security,
    Performance,
    Compatibility,
    Data,
    Testability,
    Release,
    Maintainability,
    Style,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactType {
    Diff,
    ContextFile,
    Prompt,
    Response,
    Report,
    Jira,
    TestResult,
    Snapshot,
    Other,
}
```

---

## 2. 会话主结构

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewSession {
    pub id: String,
    pub title: Option<String>,
    pub status: ConversationStatus,
    pub review_mode: crate::cli::ReviewMode,
    pub strategy: String,

    pub repo_root: PathBuf,
    pub base_ref: Option<String>,
    pub head_ref: Option<String>,

    pub provider: String,
    pub model: String,
    pub temperature: Option<f32>,

    pub current_turn: u32,
    pub total_turns: u32,

    pub admission: Option<AdmissionSnapshot>,
    pub state: ReviewConversationState,

    pub final_summary: Option<String>,
    pub final_report: Option<crate::review_schema::ReviewResult>,
    pub last_error: Option<String>,

    pub created_by: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub completed_at: Option<String>,
}
```

---

## 3. Admission 快照

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdmissionSnapshot {
    pub ok: bool,
    pub level: String,
    pub score: u8,
    pub confidence: Option<f32>,
    pub block_reasons: Vec<String>,
    pub missing_required: Vec<String>,
}
```

这个结构建议和现有 `AdmissionResult` 对齐，但不要直接把运行态对象硬塞进会话对象里。

---

## 4. 会话内部状态

```rust
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ReviewConversationState {
    pub requested_files: Vec<String>,
    pub attached_files: Vec<String>,
    pub findings: Vec<ReviewFinding>,
    pub pending_finding_ids: Vec<String>,
    pub confirmed_finding_ids: Vec<String>,
    pub dismissed_finding_ids: Vec<String>,
    pub release_checks: Vec<String>,
    pub impact_scope: Vec<String>,
    pub extra: BTreeMap<String, String>,
}
```

这里的 `findings` 可以先直接放全量对象，后面如果量大再改成只放 ID + 从 store 取详情。

---

## 5. 单轮结构

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewTurn {
    pub id: String,
    pub session_id: String,
    pub turn_no: u32,
    pub kind: TurnKind,
    pub status: TurnStatus,

    pub input_summary: Option<String>,
    pub instruction: Option<String>,

    pub requested_files: Vec<String>,
    pub attached_files: Vec<String>,
    pub focus_finding_ids: Vec<String>,

    pub prompt_text: Option<String>,
    pub response_text: Option<String>,
    pub parsed_result: Option<crate::review_schema::ReviewResult>,

    pub token_input: Option<u32>,
    pub token_output: Option<u32>,
    pub latency_ms: Option<u64>,

    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}
```

---

## 6. 消息结构

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewMessage {
    pub id: String,
    pub session_id: String,
    pub turn_id: Option<String>,
    pub seq_no: u64,
    pub role: MessageRole,
    pub author: Option<String>,
    pub content: String,
    pub format: ContentFormat,
    pub meta: BTreeMap<String, String>,
    pub created_at: String,
}
```

这个结构要尽量简单。因为它会被频繁 append。

---

## 7. 问题项结构

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewFinding {
    pub id: String,
    pub code: Option<String>,
    pub session_id: String,
    pub source_turn_id: Option<String>,

    pub severity: FindingSeverity,
    pub category: FindingCategory,
    pub status: FindingStatus,

    pub title: String,
    pub description: String,
    pub rationale: Option<String>,
    pub suggestion: Option<String>,
    pub confidence: Option<f32>,

    pub location: Option<CodeLocation>,
    pub evidence: Vec<FindingEvidence>,
    pub related_files: Vec<String>,
    pub tags: Vec<String>,

    pub last_seen_turn: Option<u32>,
    pub created_at: String,
    pub updated_at: String,
    pub resolved_at: Option<String>,
}
```

---

## 8. 代码定位结构

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeLocation {
    pub file_path: String,
    pub line_start: Option<u32>,
    pub line_end: Option<u32>,
    pub symbol: Option<String>,
}
```

---

## 9. 证据结构

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FindingEvidence {
    pub kind: String, // diff_snippet / file_snippet / model_reasoning / test_result / jira
    pub summary: String,
    pub content: Option<String>,
    pub artifact_id: Option<String>,
}
```

---

## 10. 工件结构

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewArtifact {
    pub id: String,
    pub session_id: String,
    pub turn_id: Option<String>,
    pub artifact_type: ArtifactType,
    pub name: String,
    pub path: Option<PathBuf>,
    pub content: Option<String>,
    pub mime_type: Option<String>,
    pub size_bytes: Option<u64>,
    pub hash: Option<String>,
    pub meta: BTreeMap<String, String>,
    pub created_at: String,
}
```

---

## 11. Checkpoint 结构

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewCheckpoint {
    pub id: String,
    pub session_id: String,
    pub turn_id: Option<String>,
    pub checkpoint_type: String,
    pub snapshot_json: String,
    pub created_at: String,
}
```

如果你想更类型安全，也可以把 `snapshot_json: String` 改成：

```rust
pub struct SessionSnapshot {
    pub session: ReviewSession,
    pub turns: Vec<ReviewTurn>,
    pub findings: Vec<ReviewFinding>,
}
```

但刚开始我反而建议先存 string/json，别过早复杂化。

---

# 五、Chat Provider 相关 Struct 草案

这部分是把当前 `copilot.rs` 升级成 provider 抽象时需要的。

## 1. Chat Request / Response

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<ChatInputMessage>,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
    pub metadata: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatInputMessage {
    pub role: MessageRole,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatResponse {
    pub content: String,
    pub finish_reason: Option<String>,
    pub usage: Option<TokenUsage>,
    pub raw: Option<String>,
    pub request_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenUsage {
    pub input_tokens: Option<u32>,
    pub output_tokens: Option<u32>,
    pub total_tokens: Option<u32>,
}
```

## 2. Provider Trait

```rust
pub trait LlmProvider: Send + Sync {
    fn name(&self) -> &str;
    fn chat(&self, request: &ChatRequest) -> anyhow::Result<ChatResponse>;
}
```

如果后面要做 streaming：

```rust
pub trait StreamingLlmProvider: LlmProvider {
    fn chat_stream(&self, request: &ChatRequest) -> anyhow::Result<()>;
}
```

先别急着做，主路径先跑通最重要。

---

# 六、Orchestrator 层 Struct 草案

多轮不是 provider 做的，是 orchestrator 做的。

## 1. 发起会话请求

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StartReviewSessionRequest {
    pub repo_root: PathBuf,
    pub review_mode: crate::cli::ReviewMode,
    pub provider: Option<String>,
    pub model: Option<String>,

    pub base_ref: Option<String>,
    pub head_ref: Option<String>,
    pub diff_text: Option<String>,

    pub prompt_args: crate::cli::PromptArgs,
    pub initial_instruction: Option<String>,
}
```

## 2. 继续下一轮请求

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContinueReviewTurnRequest {
    pub session_id: String,
    pub instruction: Option<String>,
    pub attached_files: Vec<String>,
    pub extra_context: Vec<String>,
    pub focus_finding_ids: Vec<String>,
    pub generate_final_report: bool,
}
```

## 3. 编排结果

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewOrchestrationResult {
    pub session: ReviewSession,
    pub turn: ReviewTurn,
    pub new_messages: Vec<ReviewMessage>,
    pub new_findings: Vec<ReviewFinding>,
    pub final_report: Option<crate::review_schema::ReviewResult>,
}
```

---

# 七、API DTO 草案

这部分给 HTTP API 用，尽量不要直接暴露内部 domain model。

---

## 1. 创建会话请求

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateReviewSessionApiRequest {
    pub repo_root: String,
    pub review_mode: String,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub base_ref: Option<String>,
    pub head_ref: Option<String>,
    pub diff_text: Option<String>,
    pub files: Vec<String>,
    pub context_files: Vec<String>,
    pub focus: Vec<String>,
    pub goal: Option<String>,
    pub why: Option<String>,
    pub rules: Vec<String>,
    pub risks: Vec<String>,
}
```

## 2. 创建会话响应

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateReviewSessionApiResponse {
    pub session_id: String,
    pub status: String,
    pub current_turn: u32,
    pub requested_files: Vec<String>,
    pub findings: Vec<FindingSummaryDto>,
    pub summary: Option<String>,
}
```

## 3. 追加一轮请求

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppendReviewTurnApiRequest {
    pub instruction: Option<String>,
    pub attached_files: Vec<String>,
    pub extra_context: Vec<String>,
    pub focus_finding_ids: Vec<String>,
    pub finalize: Option<bool>,
}
```

## 4. 会话详情响应

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewSessionDetailApiResponse {
    pub session_id: String,
    pub status: String,
    pub review_mode: String,
    pub provider: String,
    pub model: String,
    pub current_turn: u32,
    pub total_turns: u32,
    pub messages: Vec<ReviewMessageDto>,
    pub findings: Vec<FindingDetailDto>,
    pub final_report: Option<crate::review_schema::ReviewResult>,
    pub created_at: String,
    pub updated_at: String,
}
```

## 5. DTO 示例

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewMessageDto {
    pub role: String,
    pub content: String,
    pub turn_no: Option<u32>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FindingSummaryDto {
    pub id: String,
    pub severity: String,
    pub status: String,
    pub title: String,
    pub file_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FindingDetailDto {
    pub id: String,
    pub severity: String,
    pub category: String,
    pub status: String,
    pub title: String,
    pub description: String,
    pub suggestion: Option<String>,
    pub confidence: Option<f32>,
    pub location: Option<CodeLocation>,
    pub related_files: Vec<String>,
}
```

---

# 八、和现有项目的映射关系

你现在项目里已有这些对象：
- `PromptArgs`
- `ReviewArgs`
- `DeepReviewArgs`
- `ReviewResult`
- `AdmissionResult`

建议映射方式是：

## 1. `PromptArgs`
继续保留，作为“启动会话时的初始输入”。

不要让它承担会话状态。

## 2. `ReviewResult`
继续保留，作为“单轮解析结果”和“最终报告”的统一结构。

但要注意：
- `ReviewResult` 是结果
- `ReviewFinding` 是过程跟踪对象

它们不是一回事。

可以理解成：
- `ReviewResult` 偏“报告视图”
- `ReviewFinding` 偏“领域实体”

## 3. `AdmissionResult`
在会话启动时生成一次，存进 `AdmissionSnapshot`。

不要每轮都重新覆盖，除非你明确支持“补上下文后重新准入”。

---

# 九、最小可落地版本建议

如果你不想一下子做太大，我建议第一版只做下面这些字段。

## 必做对象
- `ReviewSession`
- `ReviewTurn`
- `ReviewMessage`
- `ReviewFinding`
- `ChatRequest`
- `ChatResponse`
- `LlmProvider`

## 第一版可以先不做
- owner
- checkpoint 表
- token 精确统计
- artifact hash
- resolved_at
- 流式输出

## 第一版最小目录
```text
src/
  conversation.rs
  conversation_store.rs
  orchestrator.rs
  providers/
    mod.rs
    copilot.rs
```

## 第一版最小流程
1. 创建 session
2. 写入 system + user message
3. provider.chat(messages)
4. 解析成 `ReviewResult`
5. 提取 findings
6. 保存 turn / messages / findings
7. 返回 session 状态

这样你就已经从“单次 review 工具”升级成“有会话能力的 review 系统雏形”了。

---

# 十、我建议你优先采用的最终版本

如果你要一个比较稳的结论，我会建议：

## 内部主模型
- `ReviewSession`
- `ReviewTurn`
- `ReviewMessage`
- `ReviewFinding`

## 结果模型
- 继续沿用 `ReviewResult`

## 模型调用接口
- `LlmProvider::chat(ChatRequest) -> ChatResponse`

## 存储策略
- 第一版先用文件存储
- 接口按表结构设计
- 后续平滑切 SQLite

这样做的好处是：
- 改造成本可控
- 不会一次性重写太多旧代码
- 以后接 Web UI、历史回放、人工确认都很顺

---

# 十一、一句话收口

如果只挑最关键的一句：

> `ReviewResult` 负责“输出报告”，`ReviewFinding` 负责“多轮追踪”，`ReviewMessage` 负责“上下文连续性”，`ReviewTurn` 负责“业务阶段边界”。

这四个对象一旦定稳，后面的系统就不会乱。
