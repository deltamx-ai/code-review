# 多轮会话详情（multi-chat details）实施计划

> 本计划是对 `multi-turn-review-design.md` / `multi-turn-review-structs.md` 两份设计稿的 **落地侧**补充。
> 设计稿已把 domain model、表结构、API 草案定下来，后端核心对象也写进了 `src/conversation.rs`、`src/conversation_store.rs`、`src/orchestrator.rs`、`src/providers/`，HTTP 侧挂了三个最基本的 session 接口。但：
>
> - **后端**还缺 list / delete / finding 状态更新 / CLI 子命令 / OpenAPI 文档同步 / 若干编排漏洞；
> - **前端**还是一个大单页表单，没有任何会话相关 UI；
> - **前后端**没有串起来。
>
> 本计划目标是把「多轮会话详情」这件事真正落到用户能点开、能看、能追问、能标记的程度，而不是停在接口草案层面。
>
> 对齐风格：参考 `implementation-plan.md`。

---

## 0. 一句话目标

让用户能在浏览器里：

1. 从 `/` 发起一轮全新的 review 会话；
2. 在 `/sessions` 看到所有历史会话；
3. 在 `/sessions/:id` 看到完整的消息流 + 各轮摘要 + findings 跟踪；
4. 在详情页底部**聊天式追问**继续推进；
5. 标记 findings 为 `confirmed / dismissed / fixed`；
6. 能收尾成最终报告。

整条链路跑通之后，「多轮会话」才算真的有用户面。

---

## 1. 当前基线与缺口盘点

### 1.1 后端已经具备

| 模块 | 状态 | 位置 |
|---|---|---|
| 会话 / 轮次 / 消息 / finding / artifact / checkpoint 领域模型 | 已有 | `src/conversation.rs` |
| 文件存储（`~/.alma/review-sessions/<id>/`） | 已有 | `src/conversation_store.rs` |
| `LlmProvider` trait + `CopilotCliProvider` | 已有 | `src/providers/mod.rs`, `src/providers/copilot.rs` |
| `start_session` / `continue_session` 编排 | 已有 | `src/orchestrator.rs` |
| `POST /api/review-sessions` / `GET /api/review-sessions/:id` / `POST /api/review-sessions/:id/turns` | 已有 | `src/api.rs:61-63` |
| SQL schema 草案 | 已有但未落地 | `docs/review_sessions.sql` |

### 1.2 后端缺口

| 缺口 | 影响 |
|---|---|
| `GET /api/review-sessions` 列表接口缺失 | 前端没法做会话列表页 |
| `DELETE /api/review-sessions/:id` 缺失 | 不能清理历史 |
| `PATCH /api/review-sessions/:id/findings/:fid` 缺失 | 无法驱动「多轮跟踪收敛」 |
| `start_session` 没调用 `attach_admission` | Admission 快照始终为空，前端拿不到准入摘要 |
| `continue_session` 没把 diff / context 文件传入下一轮 | 追问时上下文被丢掉，模型等于重零 |
| `orchestrator` 没落盘 `ReviewArtifact` | prompt / response / diff 三类证据都没留档 |
| `ConversationStore` 无 `list_sessions` / `delete_session` / `update_finding` | 上面的 API 无底层方法可调 |
| `cli.rs` 没有 `sessions` 子命令 | 无法脱离前端做本地排查 |
| `docs/openapi.yaml` / `docs/http-api.md` 未同步 session 接口 | 其他集成方不知道契约 |
| `api.rs:368` 错误分类仍用 `contains` | 新的 `session not found / conflict` 路径得显式映射到 404/409，不能混在通用分支 |

### 1.3 前端已经具备

| 模块 | 状态 | 位置 |
|---|---|---|
| Vite + React 19 + Tailwind v4 工程脚手架 | 已有 | `package.json` |
| 单页表单 UI（Prompt / Run / Review / Deep Review / Analyze） | 已有 | `src/App.tsx` |
| `fetchJson` + `ApiClientError` 错误分类 | 已有 | `src/lib/api.ts` |
| Vite dev 代理 `/api → 127.0.0.1:3000` | 已有 | `vite.config.ts` |

### 1.4 前端缺口

| 缺口 | 影响 |
|---|---|
| 没有 router | 无法拆出 `/sessions` / `/sessions/:id` |
| 没有任何 session 相关 UI | 多轮能力对用户不可见 |
| 没有共享类型定义 | 每个组件自己 `any` 处理 |
| `App.tsx` 已经 770 行、逻辑和视图全耦合 | 继续堆新特性会炸 |
| API client 层太薄，没有按资源聚类（session / review / prompt） | 重复代码 |
| 没有列表/详情/追问专用组件 | 需要从零搭 |

---

## 2. 实施原则

- **先后端补齐、再前端搭页面、最后联调**：三块如果并行会频繁改接口契约。
- **小步提交**：一次 PR 只动一个主题（API / store / CLI / 前端页面 / 前端组件）。
- **契约先锁**：任何新增 API 先改 `docs/openapi.yaml`，再写后端，再写前端。
- **不引入会话持久层的重构**：这次仍用文件存储，不碰 SQLite。迁移数据库是下一阶段的事。
- **兼容已有单页表单**：`/` 路径保留现有单轮 review 能力，不做破坏性改动。
- **聊天式 UI 优先**：详情页底部聊天输入框 + 折叠附加项；表单参数藏在 disclosure 里。

---

## 3. 总体分期

```
Phase 1  后端补齐（list / delete / finding 状态 / orchestrator 修复 / CLI / docs）
Phase 2  前端基础（router / 目录重构 / 类型共享 / API client 重构）
Phase 3  前端会话列表页 + 新建会话
Phase 4  前端会话详情页（消息时间线 + 轮次摘要 + findings）
Phase 5  前端追问 composer + finding 状态切换
Phase 6  联调、空/错/加载态打磨、文档与截图
```

Phase 1 和 Phase 2 可以并行；Phase 3~5 在 Phase 1 + 2 完成后顺序推进。

---

## 4. Phase 1 — 后端补齐

### Task B1：`GET /api/review-sessions` 列表接口

#### 目标
提供分页 + 基础筛选的会话列表，支撑前端 `/sessions` 页。

#### 需要解决的问题
- 当前只能按 id 反查单会话；
- 前端需要「最近」「按 repo」「按 status」等常见过滤；
- 不能一次把 messages 全量吐出来，会膨胀。

#### 实现内容

1. `ConversationStore::list_sessions(filter: SessionListFilter) -> Vec<SessionSummary>`
   - 扫描 `<root>/*/session.json`；
   - 按 `updated_at desc` 排序；
   - 支持过滤：`repo_root`、`status`、`review_mode`；
   - 支持 `limit` / `offset`。
2. 新增 `SessionSummary` DTO（不含 messages / turns / findings 全量，仅给列表用）：
   ```rust
   pub struct SessionSummary {
       pub id: String,
       pub title: Option<String>,
       pub status: String,
       pub review_mode: String,
       pub repo_root: String,
       pub provider: String,
       pub model: String,
       pub current_turn: u32,
       pub total_turns: u32,
       pub finding_counts: FindingCounts, // high / medium / low / confirmed
       pub updated_at: String,
       pub completed_at: Option<String>,
   }
   ```
3. 新增接口：
   ```
   GET /api/review-sessions
     ?repo=...&status=...&mode=...&limit=20&offset=0
   ```
   返回：
   ```json
   {
     "items": [SessionSummary, ...],
     "total": 42,
     "limit": 20,
     "offset": 0
   }
   ```
4. `api.rs` 新增 handler、路由、错误映射。

#### 验收标准
- `curl .../api/review-sessions?limit=5` 返回按 updated_at 倒序的 summary 列表；
- `?status=completed` 过滤生效；
- `total` 字段反映未分页时的总数；
- 100 个会话目录时响应 < 300ms（文件存储阶段可接受）。

#### 风险
- 文件目录多了之后 `read_dir` + 逐个 `read_to_string` 会慢；先加 `head_limit` 保底，未来切 SQLite 再优化。

---

### Task B2：`DELETE /api/review-sessions/:id`

#### 目标
支持在列表页删除会话。

#### 实现内容
- `ConversationStore::delete_session(id)`：`fs::remove_dir_all(session_dir)`，并做 path safety 校验，禁止 `..` / 空 id。
- `DELETE /api/review-sessions/:id` handler，成功返回 `204 No Content`；不存在返回 `404`。

#### 验收标准
- 已删除会话在 `GET /api/review-sessions` 中消失；
- 对不存在 id 返回 404；
- 用 `../etc` 之类 id 不会越界删目录（需要测试）。

#### 风险
- **破坏性操作**：必须做 path 校验；删除前后端不设硬确认，交给前端在 UI 里二次确认。

---

### Task B3：`PATCH /api/review-sessions/:id/findings/:fid` 更新 finding 状态

#### 目标
让用户在详情页把一个 finding 标记为 `confirmed / dismissed / fixed / accepted_risk`，这是多轮跟踪「收敛」的关键动作。

#### 需要解决的问题
- `ReviewFinding.status` 现在只能被 orchestrator 内部改；
- 没有接口把人工确认结果写回去；
- findings 之间的收敛率也就无从度量。

#### 实现内容
1. `ConversationStore::update_finding(session_id, finding_id, patch)`：
   - 读 `findings.json`；
   - 根据 id 定位；
   - 合并 patch（status / owner / resolved_at / tags）；
   - `updated_at` 刷新；
   - 回写。
2. 请求体：
   ```json
   {
     "status": "confirmed",
     "owner": "alice",
     "tags": ["manual-reviewed"]
   }
   ```
3. 返回 `ReviewFinding` 全量对象。
4. 状态流转校验：参考 `implementation-plan.md` 里的问题分级思路，保证：
   - `dismissed → suspected` 允许（已发现 orchestrator 里有这条路径）
   - `fixed / accepted_risk` 必须同时提供 `resolved_at`（服务端自动填）。

#### 验收标准
- 切换状态后 `session.state.pending_finding_ids / confirmed_finding_ids / dismissed_finding_ids` 三个列表也同步（这是 `orchestrator::apply_findings_to_session` 的逻辑，要抽出来复用）；
- 详情接口返回的 findings.status 正确；
- 非法 status 字符串返回 400。

#### 风险
- `apply_findings_to_session` 目前写死在 orchestrator 里，调用点要小心不要把统计逻辑漏算。

---

### Task B4：`orchestrator::start_session` 修补 admission 与 artifacts

#### 目标
补齐两个明显漏洞。

#### 需要解决的问题
1. `start_session` 创建会话时没有调用 `session.attach_admission(...)`，所以 `session.admission` 始终是 `None`。前端没法展示准入摘要；
2. `start_session` 和 `continue_session` 都没把 diff、prompt、response 落成 `ReviewArtifact`。`multi-turn-review-structs.md` 里工件表是被明确推荐保留的，当前是「跑完即销」。

#### 实现内容
1. 在 `start_session` 内：
   - 依赖现有 `execute_validate` / `check_admission`（参考 `services/review_service.rs`）先跑一次 admission；
   - 拿到 `AdmissionResult` 后 `session.attach_admission(&admission)`；
   - 若 `level == Block`：直接返回 `ConversationStatus::Failed` 会话，不跑模型。
2. 新增 `ConversationStore::save_artifact(artifact)` + `load_artifacts(session_id)` + `list_artifacts_by_turn(turn_id)`：
   - 小工件（prompt / response / diff 文本）直接写 `<session_dir>/artifacts/<artifact_id>.json`；
   - 工件 id 用 uuid。
3. 在 `start_session` / `continue_session` 的结尾，把以下内容落成 artifact：
   - `ArtifactType::Prompt`（渲染后的 message 列表文本）
   - `ArtifactType::Response`（模型原文）
   - `ArtifactType::Diff`（仅 `start_session` 有 diff_text 时）

#### 验收标准
- 新建会话返回的 `session.admission` 不为 null；
- `GET /api/review-sessions/:id` 的 turns 里有对应 turn 生成的 artifact 引用（新增 `ReviewSessionDetailApiResponse.artifacts`）；
- 详情页能按 turn 展开 prompt 原文与模型原文。

#### 风险
- 要小心 diff 过大时直接写 artifact 文件的体积。先加 1MB 上限，超过就截断 + 标记 `truncated=true`。

---

### Task B5：`continue_session` 上下文延续 + 每轮模型覆盖

#### 目标
让追问轮次真的基于完整历史 + 本次补充材料推理，而不是只靠一句 `instruction`。同时支持用户在追问时指定不同 model。

#### 需要解决的问题
- 当前 `continue_session` 构造 `user_text` 只拼了 instruction + focus + attached file 名 + extra context 字符串；
- **没读文件内容、没复用 session 的 diff**；
- 模型在追问轮看到的上下文严重缩水。

#### 实现内容
1. 新增 `fn read_attached_file_contents(repo_root: &Path, files: &[String], budget_bytes: usize)`：
   - 复用 `src/context.rs` 的 `read_repo_context_with_budget` 预算逻辑；
   - 返回 `Vec<(String, String)>`（相对路径 + 内容）。
2. 在 `build_continue_user_prompt` 里：
   - 如果有 attached_files，把文件内容以 `## 补充文件：<path>\n\`\`\`\n...\n\`\`\`` 的格式塞进 user message；
   - 文件内容超出预算时按 utf-8 边界截断，附 `-- 省略，已截断 --` 提示；
3. 在 session 上记录最后一次已加载的文件 hash（避免重复塞）：
   - `session.state.extra.insert("last_attached_hash_<turn>", ...)`
4. `generate_final_report=true` 时把所有 session.state.impact_scope / release_checks 重新喂给模型，要求生成最终结论。
5. **模型覆盖**：`ContinueReviewTurnRequest` 新增 `model: Option<String>`；非空时本轮用该 model，session.model 不改。UI 侧 Composer 传入；用于让用户随时切换到更强/更快的模型。

#### 验收标准
- 追问轮的 prompt 文本里出现补充文件内容；
- 连续三轮 review 后 token_input 单调增长，不出现第二轮比第一轮小的情况（说明历史真在堆上去）；
- finalize 轮产出的 `final_report.summary` 非空。

#### 风险
- 文件内容大会撑爆上下文，要严格走预算控制；
- 同一个文件在多轮被重复 attach 要去重。

---

### Task B6：CLI `sessions` 子命令

#### 目标
给研发本地一条不走浏览器的排查路径，不用每次都 `curl`。

#### 实现内容
在 `src/cli.rs` 新增：

```
code-review sessions list [--repo ...] [--status ...] [--limit 20]
code-review sessions show <id> [--with-messages] [--with-findings]
code-review sessions continue <id> --instruction "..." [--file ...]* [--finalize]
code-review sessions delete <id>
code-review sessions finding <session-id> <finding-id> --status confirmed
```

每个命令都直接调 `ConversationStore` + `orchestrator`，不走 HTTP。

#### 验收标准
- 本地跑一轮完整流程（create → list → show → continue → finding confirm → finalize → show）不报错；
- `show` 输出格式是结构化 JSON，方便 `jq`。

#### 风险
- `continue` 需要 CLI 实例化 `CopilotCliProvider`，这点已有 API 层示例可参考。

---

### Task B7：`docs/openapi.yaml` 与 `docs/http-api.md` 同步

#### 目标
不要让契约文档和代码脱节。

#### 实现内容
1. `openapi.yaml` 增加：
   - `/api/review-sessions` (GET/POST)
   - `/api/review-sessions/{id}` (GET/DELETE)
   - `/api/review-sessions/{id}/turns` (POST)
   - `/api/review-sessions/{id}/findings/{finding_id}` (PATCH)
   - schemas：`SessionSummary`, `SessionListResponse`, `ReviewSession`, `ReviewTurn`, `ReviewMessage`, `ReviewFinding`, `FindingPatch`, `CreateReviewSessionApiRequest`, `AppendReviewTurnApiRequest`, `ReviewSessionDetailApiResponse`。
2. 顺手修复 `openapi.yaml` 末尾（`tView'` 起）那一段明显是 merge 事故造成的文件尾部残骸。
3. `http-api.md` 补「多轮会话」小节，给出四条核心 curl 示例：
   - create session
   - append turn
   - list sessions
   - update finding status

#### 验收标准
- `swagger-cli validate docs/openapi.yaml` 通过；
- `http-api.md` 的 curl 能在本地直接复制执行跑通。

---

### Task B8：`api.rs` 错误映射收紧

#### 目标
避免 session 路径的新错误被误判成 500。

#### 实现内容
- 给 `ApiError` 加一个可选的 `hint_status: Option<StatusCode>`，让业务层能显式指定；
- 或者保留 `api_error()`，但把 `session not found` / `finding not found` / `invalid status transition` 几条新增字符串加到现有分支里；
- `DELETE` 对不存在资源返回 404 而不是 500。

#### 验收标准
- 所有新接口错误返回码：400（参数错）/ 404（找不到）/ 409（状态非法流转）/ 500（其他）。

---

## 5. Phase 2 — 前端基础

### Task F1：引入 `react-router-dom` 并拆页面目录

#### 目标
给多页面 UI 铺路。

#### 实现内容
1. `package.json` 新增 `react-router-dom@^7`；
2. 新建目录结构：
   ```
   src/
     main.tsx                 # RouterProvider
     App.tsx                  # 顶层 Layout（导航栏）
     routes.tsx               # 路由配置
     pages/
       HomePage.tsx           # 原 App.tsx 内容迁入
       SessionsPage.tsx       # 会话列表
       SessionDetailPage.tsx  # 会话详情
       NotFoundPage.tsx
     components/
       Layout.tsx
       NavBar.tsx
       (后续 Phase 再加)
     lib/
       api.ts                 # 已有
       sessions.ts            # 新增 session 专用 API
       http.ts                # 把 fetchJson 从 api.ts 抽出来
     types/
       session.ts
       review.ts
   ```
3. 路由：
   - `/` → HomePage（原表单）
   - `/sessions` → SessionsPage
   - `/sessions/:id` → SessionDetailPage
   - `*` → NotFoundPage
4. 顶部 NavBar 提供三个 tab：`Single Review` / `Chat Sessions` / 外链 `GitHub`。

#### 验收标准
- `npm run dev` 后 `/`、`/sessions`、`/sessions/abc` 三个 URL 可达；
- 现有单轮 review 表单行为完全不变；
- 切换路由无整页刷新。

#### 风险
- `App.tsx` 770 行一次性搬迁容易出错：建议先原样复制到 `HomePage.tsx`，本 task 不改任何逻辑，纯搬家。

---

### Task F2：共享类型定义

#### 目标
消灭 `any`，前后端字段对齐。

#### 实现内容
在 `src/types/session.ts` 定义：

```ts
export type ReviewMode = 'lite' | 'standard' | 'critical'
export type SessionStatus = 'created' | 'running' | 'waiting_input' | 'completed' | 'failed' | 'cancelled'
export type TurnKind = 'discovery' | 'deep_dive' | 'business_check' | 'final_report' | 'manual_followup'
export type TurnStatus = 'pending' | 'running' | 'completed' | 'failed' | 'skipped'
export type MessageRole = 'system' | 'user' | 'assistant' | 'tool'
export type FindingSeverity = 'critical' | 'high' | 'medium' | 'low' | 'info'
export type FindingStatus = 'suspected' | 'confirmed' | 'dismissed' | 'fixed' | 'accepted_risk'

export type SessionSummary = { ... }
export type ReviewSession = { ... }
export type ReviewTurn = { ... }
export type ReviewMessage = { ... }
export type ReviewFinding = { ... }
export type AdmissionSnapshot = { ... }
export type ReviewSessionDetail = {
  session: ReviewSession
  turns: ReviewTurn[]
  messages: ReviewMessage[]
  findings: ReviewFinding[]
}
```

字段命名严格对齐 `src/conversation.rs` 的 serde 输出（`snake_case`）。

#### 验收标准
- 后续组件不再出现 `any`；
- `tsc --noEmit` 通过。

---

### Task F3：API client 按资源拆分

#### 目标
避免 `App.tsx` 里又一次堆满 `fetchJson('.../api/review-sessions', body)`。

#### 实现内容
`src/lib/sessions.ts`：

```ts
export async function listSessions(base: string, q?: {...}): Promise<SessionListResponse>
export async function getSession(base: string, id: string): Promise<ReviewSessionDetail>
export async function createSession(base: string, body: CreateReviewSessionPayload): Promise<ReviewSessionDetail>
export async function appendTurn(base: string, id: string, body: AppendReviewTurnPayload): Promise<ReviewSessionDetail>
export async function deleteSession(base: string, id: string): Promise<void>
export async function updateFinding(base: string, sid: string, fid: string, patch: FindingPatch): Promise<ReviewFinding>
```

保留 `api.ts` 里 `fetchJson` 不动，新 client 都基于它。

#### 验收标准
- 所有组件/页面 import 只走 `lib/sessions.ts`；
- `fetchJson` 的错误分类对 session 相关 404 / 409 正确分流。

---

## 6. Phase 3 — 会话列表页 + 新建

### Task F4：`SessionsPage.tsx` 列表

#### 目标
展示所有会话，支持过滤和跳转。

#### 实现内容
1. 顶部筛选条：repo（文本框）、status（select）、mode（select）、刷新按钮；
2. 主体表格列：
   - `title || id`（点击跳 `/sessions/:id`）
   - `review_mode`（pill）
   - `status`（color chip）
   - `current_turn / total_turns`
   - `finding_counts.high / medium / low / confirmed`
   - `updated_at`（相对时间）
   - 操作：删除（二次确认 modal）
3. 空状态、加载状态、错误状态三类占位。
4. URL query 绑定：`?repo=...&status=...&page=2`。

#### 组件拆分
- `components/SessionListTable.tsx`
- `components/SessionListFilters.tsx`
- `components/StatusChip.tsx`
- `components/ModeBadge.tsx`
- `components/ConfirmDialog.tsx`（删除确认复用）

#### 验收标准
- 后端返回 0 条时显示「还没有会话，先去 Single Review 发起一轮」；
- 删除后刷新不再出现；
- 筛选变化时 URL 同步。

---

### Task F5：「新建会话」入口

#### 目标
让用户能在 UI 里直接新开一个会话。

#### 实现内容
1. 列表页右上角「新建会话」按钮，打开 `NewSessionDialog`；
2. 对话框表单字段（精简版，不要照搬 HomePage 那一整页）：
   - repo_root（必填）
   - review_mode
   - provider（可选）
   - **model 下拉选择器**（必填）：页面初次挂载时调 `GET /api/models` 拉列表；默认选 `default_model`；用户可覆盖。失败时显示文本输入框兜底。
   - base_ref / head_ref（可选）
   - diff_text（textarea 选填）
   - goal / rules（两三个核心 prompt_args 字段）
   - initial_instruction
3. 提交时调用 `createSession()`，成功后跳转 `/sessions/:id`。

#### 验收标准
- 填最小字段（repo_root + review_mode）能创建成功；
- 创建后 300ms 内落到详情页；
- 创建失败时错误提示具体（admission 失败时列出 missing）。

#### 风险
- HomePage 表单太重，这里只暴露必要字段，高级参数留到详情页「上下文编辑」里再补。

---

## 7. Phase 4 — 会话详情页

### Task F6：`SessionDetailPage.tsx` 主骨架

#### 目标
把 `GET /api/review-sessions/:id` 的内容用三栏视图呈现。

#### 布局
```
┌─────────────────────────────────────────────────────────────┐
│  HeaderCard：title / status / mode / provider / turns       │
│              admission 摘要 / final summary                  │
├─────────────────────────────────────┬───────────────────────┤
│                                     │                       │
│  MessageTimeline                    │  RightPanel           │
│  ┌─ turn 1 (discovery) ──────────┐  │  ┌─ FindingsList ─┐  │
│  │ system ...                    │  │  │ high / med / .. │  │
│  │ user   ...                    │  │  └─────────────────┘  │
│  │ assistant (parsed summary)    │  │  ┌─ TurnsIndex ───┐  │
│  └───────────────────────────────┘  │  │ turn list      │  │
│  ┌─ turn 2 (deep_dive) ──────────┐  │  └─────────────────┘  │
│  │ ...                           │  │  ┌─ ContextPanel ─┐  │
│  └───────────────────────────────┘  │  │ requested /    │  │
│                                     │  │ attached files  │  │
│                                     │  └─────────────────┘  │
├─────────────────────────────────────┴───────────────────────┤
│  ContinueTurnComposer（固定底部）                            │
└─────────────────────────────────────────────────────────────┘
```

#### 实现内容
- 初次加载调 `getSession(id)`；
- 切路由时用 `useParams`；
- 错误态区分 404（「该会话不存在」）和 500（「加载失败，重试」）；
- 每次追问成功后用返回值直接 replace 本地 state（避免再 GET）。

#### 验收标准
- 正常加载 < 500ms；
- 刷新后滚动位置不保留可以；
- 三栏在窄屏下折叠成 tab。

---

### Task F7：`MessageTimeline`

#### 目标
把一条条 message 按 turn 分组，渲染成聊天气泡。

#### 实现内容
- 消息按 `turn_id` 分组；每组上方显示 `turn.kind + turn_no + status + started/completed`；
- `system` 消息默认折叠（一行 preview，点击展开）；
- `user` 消息靠左灰底；
- `assistant` 消息靠左白底 + 边框；长内容超过 400px 高度出现「展开」；
- 支持 Markdown（简单选一下 `marked` 或 `react-markdown`，这次我倾向 `react-markdown` 因为已经有 React 19 生态）。

#### 验收标准
- 100 条消息滚动流畅；
- 代码块带语法高亮（如果时间允许）；否则先保留 monospace。

#### 风险
- Markdown 安全：assistant 输出是可信后端生成，但仍要关闭 raw HTML；
- 新依赖要锁版本。

---

### Task F8：`FindingsList` + 状态切换

#### 目标
展示所有 findings，支持一键改状态。

#### 实现内容
- 顶部小 tabs：`全部 / 待核 / 已确认 / 已排除 / 已修复`；
- 每条 finding 卡片：severity chip、title、file:line（点击在 MessageTimeline 里定位相关 turn —— 需要 findings.source_turn_id）、description、suggestion、`confidence`；
- 右上角按钮组：Confirm / Dismiss / Fixed；
- 点击后乐观更新 + 调 `updateFinding()` + 错误回滚。

#### 验收标准
- 切换状态后 FindingsList 分组立刻更新；
- 错误时弹 toast 回滚 UI；
- 不同状态的 finding 颜色清晰可分。

---

### Task F9：`TurnsIndex` + `ContextPanel`

#### 目标
复用详情数据的其他角度。

#### 实现内容
- `TurnsIndex`：表格式 turn 列表（turn_no / kind / status / latency_ms / tokens / finding_count）；点击跳到 MessageTimeline 对应位置（用 `scrollIntoView`）。
- `ContextPanel`：展示 `session.state.requested_files` / `attached_files` / `impact_scope` / `release_checks`。

#### 验收标准
- 点 turn 行滚动定位；
- 文件列表空时显示占位文案。

---

## 8. Phase 5 — 追问 composer

### Task F10：`ContinueTurnComposer` 聊天式输入

#### 目标
实现本计划最重要的交互：详情页底部聊天框，结构对齐前面 ASCII mock。

#### UI 结构
```
┌────────────────────────────────────────────────────┐
│ ▾ 附加 (0 files / 0 focus / 0 context)             │
│   ┌ Attached files ─────────────────────┐           │
│   │ tag input: 输入路径回车确认         │           │
│   └─────────────────────────────────────┘           │
│   ┌ Focus finding IDs ──────────────────┐           │
│   │ 从 FindingsList 里勾选 → 自动填     │           │
│   └─────────────────────────────────────┘           │
│   ┌ Extra context ───────────────────────┐          │
│   │ textarea (多行)                       │          │
│   └──────────────────────────────────────┘          │
├────────────────────────────────────────────────────┤
│ ┌ textarea ─────────────────────────────────────┐  │
│ │ 继续追问...                                    │  │
│ └──────────────────────────────────────────────┘  │
│ Model: [session默认 ▾]                             │
│            [ ] 生成最终报告   [清空] [Send ↵]      │
└────────────────────────────────────────────────────┘
```

**Model 选择器**：默认使用 session.model；下拉可切换到 `/api/models` 返回的其它 model；若与 session.model 不同，本次 `appendTurn` 请求带 `model` 字段让后端本轮覆盖。切换模型不影响 session.model。

#### 实现内容
1. `附加` 区域默认折叠；有值时右侧显示计数。
2. `Focus finding IDs`：从 `FindingsList` 选中 → 传入 composer props；composer 提供「清空选择」。
3. 快捷键：`Enter` 提交、`Shift+Enter` 换行、`Cmd/Ctrl+Enter` 也提交。
4. 提交逻辑：
   - 校验 instruction 非空（除非 `finalize=true`）；
   - `setSending(true)` 锁 UI；
   - 调 `appendTurn()`；
   - 成功：用返回值 replace 页面 state、清空 composer、`scrollTo` 新 assistant 消息；
   - 失败：toast + 保留输入内容。
5. 加载态：Send 按钮变转圈；MessageTimeline 顶部显示一条临时 user 消息 + 「模型正在思考...」占位。

#### 验收标准
- Enter 发送，Shift+Enter 换行；
- finalize 勾选后即使 instruction 为空也能提交；
- 提交时不允许二次提交；
- 失败时输入不丢。

#### 风险
- 后端同步接口可能 30s+，Composer 要允许取消（`AbortController`），避免用户卡死。

---

### Task F11：Finding 选择联动

#### 目标
`FindingsList` 与 `ContinueTurnComposer` 选择打通。

#### 实现内容
- SessionDetailPage 维护 `selectedFindingIds: Set<string>`；
- FindingsList 每条卡片加 checkbox；
- Composer 的 `附加` 区显示当前 focus 数量，点击可清空；
- 发送追问时把这些 id 传给 `appendTurn.focus_finding_ids`。

#### 验收标准
- 选 3 个 findings 后发送追问，后端 turn.focus_finding_ids 正确；
- 切换会话后选择自动清空。

---

## 9. Phase 6 — 联调与打磨

### Task F12：端到端手动冒烟脚本

#### 目标
有一条可重复跑的全链路验证。

#### 实现内容
写 `docs/e2e-chat-details.md`，记录：
1. 启动后端 `cargo run -- serve`；
2. 启动前端 `npm run dev`；
3. 在 `/` 提交一个新会话（或者在 `/sessions` 新建）；
4. 打开详情页，确认 timeline/findings/context 都有内容；
5. 追问一轮，确认 turn 2 出现；
6. 标记一个 finding 为 confirmed；
7. 勾选 finalize 提交，确认生成 final_report；
8. 在 `/sessions` 里删除该会话。

每一步写明预期。

#### 验收标准
- 任何新成员按脚本走一遍都能跑通。

---

### Task F13：空态、错态、加载态统一

#### 目标
避免每个页面自己造轮子。

#### 实现内容
- `components/EmptyState.tsx`
- `components/ErrorPanel.tsx`（区分 admission/quota/network/unknown 四类，复用 `ApiClientError.kind`）
- `components/LoadingOverlay.tsx`

把三个页面的 loading / error / empty 替换成这三个通用组件。

#### 验收标准
- 主要页面的三态视觉一致；
- 错态文案保持 `lib/api.ts` 已有分类的中文文案不变。

---

### Task F14：截图 + 文档

#### 目标
README 更新。

#### 实现内容
- `README.md` 加一节 `Multi-turn Chat Sessions`，带 2 张截图（列表 + 详情）；
- 指向 `docs/e2e-chat-details.md`；
- `code-review` 侧 README 同步新增 `sessions` CLI 子命令说明。

#### 验收标准
- README 能让新人五分钟内跑通。

---

## 10. 推荐开发顺序

### 第 1 批（并行）
- B1 list 接口
- B2 delete 接口
- F1 前端路由重构
- F2 共享类型

### 第 2 批
- B3 finding 状态接口
- B4 orchestrator admission + artifact 修复
- F3 API client 重构

### 第 3 批
- B5 continue_session 上下文修复
- F4 / F5 会话列表 + 新建

### 第 4 批
- B6 CLI
- B7 openapi/http-api 文档同步
- B8 错误映射收紧
- F6 详情页骨架

### 第 5 批
- F7 MessageTimeline
- F8 FindingsList + 状态切换
- F9 TurnsIndex / ContextPanel

### 第 6 批
- F10 Composer
- F11 Finding 选择联动

### 第 7 批（收口）
- F12 e2e 脚本
- F13 空/错/加载态统一
- F14 文档截图

---

## 11. 里程碑与交付物

### Milestone A：后端 session 接口闭环
交付：B1 + B2 + B3 + B4 + B7 + B8
完成标志：
- openapi 校验通过；
- 本地 curl 能跑「创建 → 列表 → 详情 → 追问 → 标记 finding → 删除」全流程；
- orchestrator 在新建会话时正确落 admission 与 artifacts。

### Milestone B：前端骨架可点
交付：F1 + F2 + F3 + F4 + F5
完成标志：
- `/sessions` 能看到列表、能新建、能删除；
- 共享类型全链路生效；
- `npm run build` 无报错。

### Milestone C：详情页可读
交付：F6 + F7 + F8 + F9
完成标志：
- 详情页三栏渲染正常；
- Findings 状态可切换并回写；
- turn 跳转可用。

### Milestone D：追问可写
交付：F10 + F11 + B5 + B6
完成标志：
- 聊天式追问可用；
- focus findings 联动正确；
- CLI `sessions continue` 能得到同样结果。

### Milestone E：收口
交付：F12 + F13 + F14
完成标志：
- e2e 文档存在；
- 三态 UI 统一；
- README 更新并自带截图。

---

## 12. 风险与应对

### 风险 1：后端接口契约一边改一边跑前端
应对：Milestone A 完成后才开 Milestone C/D 的前端联调；B1-B3 一定要先锁 openapi。

### 风险 2：追问轮次上下文膨胀导致 provider 超时
应对：
- 走 `src/context.rs` 已有预算；
- 超出部分截断并在 prompt 里说明；
- 给 Composer 加 AbortController + 30s 提示。

### 风险 3：文件存储在大量会话下列表慢
应对：
- 加 `limit` 默认 20；
- 列表扫描失败的目录跳过（防止单个坏文件拖死整个页面）；
- 未来切 SQLite 的契约已在 `docs/review_sessions.sql` 定义，迁移成本可控。

### 风险 4：删除操作误删
应对：
- 前端二次确认模态框；
- 后端 path 校验 + 仅允许 `rs-*` 前缀 id。

### 风险 5：Markdown XSS
应对：
- 用 `react-markdown` + 默认关闭 rawHtml；
- 不额外引 `rehype-raw`。

### 风险 6：react-router v7 API 变动
应对：锁定具体版本，文档记录；若团队熟悉 v6 可先退到 v6.26+。

---

## 13. 一句话收口

把「多轮会话详情」做完，不是加一个新页面，而是：

> **后端把 finding 状态和 artifact 落盘补齐、前端把 router / 列表 / 详情 / 追问四件套搭起来、联调打磨三态**。

这三件之后，`ReviewSession` 就从「跑完即丢的 JSON」升级成「研发能坐下来查、能追问、能收敛」的工作面。后续 Web UI、历史趋势、PR 集成都会顺很多。
