# Implementation Plan

> Actionable task breakdown for evolving code-review v0.3 → v1.0.
> Derived from `newdesign.md`. Each task has: what to change, which files, acceptance criteria, estimated size, and dependencies.

---

## Codebase Baseline (v0.3)

```
Total: ~5,050 lines across 22 source files
Key files by size:
  review_service.rs  608 LOC   (orchestration)
  expand.rs          507 LOC   (dependency expansion)
  jira.rs            454 LOC   (Jira enrichment)
  prompt.rs          343 LOC   (prompt assembly)
  copilot.rs         340 LOC   (LLM calls — to be abstracted)
  cli.rs             334 LOC   (arg definitions)
  admission.rs       291 LOC   (gate logic)
  review_parser.rs   252 LOC   (text parser — to become fallback)
```

---

## Phase A: Structured Output + Multi-Provider

**Goal**: Eliminate fragile text parsing. Support Claude / OpenAI / Ollama alongside copilot.

### A-1. Define LlmProvider trait and response types

**Create** `src/llm.rs`

```rust
pub trait LlmProvider: Send + Sync {
    fn name(&self) -> &str;
    fn supports_json_schema(&self) -> bool;
    fn review(
        &self,
        prompt: &str,
        json_schema: Option<&serde_json::Value>,
        model: Option<&str>,
    ) -> Result<LlmResponse>;
}

pub struct LlmResponse {
    pub raw_text: String,
    pub structured: Option<serde_json::Value>,
    pub provider: String,
    pub model: String,
    pub token_usage: Option<TokenUsage>,
}

pub struct TokenUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
}

pub fn create_provider(cfg: &AppConfig) -> Result<Box<dyn LlmProvider>>;
```

**Files**: new `src/llm.rs`, edit `src/lib.rs` (add `pub mod llm`)

**Size**: ~80 LOC

**Acceptance**:
- [ ] Trait compiles and is object-safe
- [ ] `create_provider()` reads `config.llm.provider` and returns the right impl
- [ ] Unit test: trait can be mocked

**Depends on**: nothing

---

### A-2. Move copilot logic into CopilotCliProvider

**Refactor** `src/copilot.rs`

Extract `run_review()` into a struct that implements `LlmProvider`:

```rust
pub struct CopilotCliProvider {
    store: SessionStore,
}

impl LlmProvider for CopilotCliProvider {
    fn name(&self) -> &str { "copilot" }
    fn supports_json_schema(&self) -> bool { false }
    fn review(&self, prompt, schema, model) -> Result<LlmResponse> {
        // existing run_review() logic
        // returns LlmResponse { raw_text, structured: None, ... }
    }
}
```

Keep auth functions (`login`, `status`, `refresh`, `whoami`, `logout`) as free functions — they are copilot-specific and not part of the provider trait.

**Files**: edit `src/copilot.rs`, edit `src/llm.rs`

**Size**: ~60 LOC net change (mostly moving code)

**Acceptance**:
- [ ] `CopilotCliProvider` implements `LlmProvider`
- [ ] Existing `review` / `deep-review` commands still work with `provider = "copilot"`
- [ ] All 40 existing tests still pass

**Depends on**: A-1

---

### A-3. Define ReviewResult JSON schema for model output

**Create** `src/review_json_schema.rs`

Build the JSON schema that will be sent to structured-output-capable providers:

```rust
pub fn review_result_schema(mode: ReviewMode) -> serde_json::Value {
    // Returns JSON Schema for ReviewResultRaw
    // Critical mode schema includes impact_scope + release_checks as required
}

#[derive(Deserialize)]
pub struct ReviewResultRaw {
    pub high_risk: Vec<ReviewIssueRaw>,
    pub medium_risk: Vec<ReviewIssueRaw>,
    pub low_risk: Vec<ReviewIssueRaw>,
    pub missing_tests: Vec<MissingTestRaw>,
    pub summary: String,
    pub impact_scope: Option<Vec<String>>,
    pub release_checks: Option<Vec<String>>,
}

#[derive(Deserialize)]
pub struct ReviewIssueRaw {
    pub title: String,
    pub file: Option<String>,
    pub location: Option<String>,
    pub reason: String,
    pub trigger: Option<String>,
    pub impact: Option<String>,
    pub suggestion: Option<String>,
    pub layer: Option<String>,  // "basic"|"engineering"|"business"|"risk"
}

pub fn convert_raw_to_result(raw: ReviewResultRaw, mode: ReviewMode, used_rules: Vec<String>) -> ReviewResult;
```

**Files**: new `src/review_json_schema.rs`, edit `src/lib.rs`

**Size**: ~150 LOC

**Acceptance**:
- [ ] Schema passes JSON Schema Draft-07 validation
- [ ] `convert_raw_to_result` produces a valid `ReviewResult`
- [ ] Critical mode schema includes `impact_scope` and `release_checks` as required fields
- [ ] Each issue has `layer` field

**Depends on**: nothing (can parallel with A-1, A-2)

---

### A-4. Implement ClaudeApiProvider

**Create** `src/llm_claude.rs`

```rust
pub struct ClaudeApiProvider {
    api_key: String,
    api_base: String,
}

impl LlmProvider for ClaudeApiProvider {
    fn name(&self) -> &str { "claude" }
    fn supports_json_schema(&self) -> bool { true }
    fn review(&self, prompt, schema, model) -> Result<LlmResponse> {
        // POST https://api.anthropic.com/v1/messages
        // Use tool_use with schema for structured output
        // Parse tool_use response → LlmResponse { structured: Some(...) }
    }
}
```

API key read from env var specified in config (`api_key_env = "ANTHROPIC_API_KEY"`).

**Files**: new `src/llm_claude.rs`, edit `src/llm.rs` (register provider), edit `src/lib.rs`

**Size**: ~180 LOC

**Acceptance**:
- [ ] Can call Claude API with a review prompt
- [ ] Structured output via tool_use returns valid JSON matching schema
- [ ] Falls back gracefully if API key missing (clear error message)
- [ ] Token usage reported in response

**Depends on**: A-1, A-3

---

### A-5. Implement OpenAiApiProvider

**Create** `src/llm_openai.rs`

Same pattern as Claude but using OpenAI's `response_format: { type: "json_schema", ... }`.

**Files**: new `src/llm_openai.rs`, edit `src/llm.rs`, edit `src/lib.rs`

**Size**: ~150 LOC

**Acceptance**:
- [ ] Can call OpenAI API with structured JSON output
- [ ] Works with `gpt-4o`, `gpt-5`, `o3` etc.

**Depends on**: A-1, A-3

---

### A-6. Implement OllamaProvider

**Create** `src/llm_ollama.rs`

For local / air-gapped use. Calls Ollama's `/api/generate` endpoint. Text-only output (no JSON schema support), so uses the text parser fallback.

**Files**: new `src/llm_ollama.rs`, edit `src/llm.rs`, edit `src/lib.rs`

**Size**: ~100 LOC

**Acceptance**:
- [ ] Can call local Ollama with a review prompt
- [ ] Handles connection refused gracefully
- [ ] `supports_json_schema()` returns false → uses text parser

**Depends on**: A-1

---

### A-7. Wire provider into service layer

**Refactor** `src/services/review_service.rs`

Replace all `copilot::run_review(store, &prompt, model)` calls with:

```rust
let provider = llm::create_provider(&cfg)?;
let response = provider.review(&prompt, json_schema, model)?;
let result = if let Some(structured) = response.structured {
    convert_raw_to_result(serde_json::from_value(structured)?, mode, used_rules)
} else {
    parse_review_text(mode, &response.raw_text, used_rules)  // fallback
};
```

**Files**: edit `src/services/review_service.rs`, edit `src/lib.rs` (pass config/provider through)

**Size**: ~100 LOC net change

**Acceptance**:
- [ ] `execute_review` uses provider trait (not copilot directly)
- [ ] `execute_deep_review` uses provider trait for both stages
- [ ] JSON-capable providers skip text parser entirely
- [ ] Text-only providers still use `review_parser.rs` as before
- [ ] Repair flow only triggers for text-parse failures (not for JSON providers)

**Depends on**: A-1, A-2, A-3, A-4 (at least one real provider)

---

### A-8. Add `layer` field to ReviewIssue

**Edit** `src/review_schema.rs`

```rust
pub struct ReviewIssue {
    // ... existing fields ...
    pub layer: Option<String>,
}
```

**Edit** `src/review_parser.rs` — infer layer from section context when parsing text:
- Issues found in "高风险" section with concurrency/null/bounds keywords → `basic`
- Issues about architecture/dependency/performance → `engineering`
- Issues referencing rules/auth/state/idempotency → `business`
- Issues about migration/contract/compat → `risk`

**Edit** `src/review_render.rs` — show layer in text output.

**Files**: edit `review_schema.rs`, `review_parser.rs`, `review_render.rs`, `review_validate.rs`

**Size**: ~60 LOC

**Acceptance**:
- [ ] JSON output includes `layer` per issue
- [ ] Text output shows layer tag
- [ ] JSON providers fill layer directly; text parser infers it

**Depends on**: A-3

---

### A-9. Update config and CLI for provider selection

**Edit** `src/config.rs`

```rust
pub struct LlmConfig {
    pub provider: Option<String>,      // "copilot" | "claude" | "openai" | "ollama"
    pub model: Option<String>,
    pub api_key_env: Option<String>,   // NEW: env var name for API key
    pub api_base: Option<String>,      // NEW: custom API base URL
    pub fallback_provider: Option<String>, // NEW: fallback if primary fails
}
```

**Edit** `src/cli.rs` — add `--provider` flag to `ReviewArgs` and `AnalyzeArgs`.

**Files**: edit `config.rs`, `cli.rs`

**Size**: ~40 LOC

**Acceptance**:
- [ ] `config.toml` with `provider = "claude"` uses Claude API
- [ ] `--provider claude` CLI flag overrides config
- [ ] Missing API key gives clear error: "set ANTHROPIC_API_KEY env var"
- [ ] Default remains "copilot" for backward compat

**Depends on**: A-1

---

## Phase B: Async Tasks + Review History

**Goal**: Non-blocking API. Persistent review records.

### B-1. Implement TaskManager

**Create** `src/task.rs`

```rust
pub struct TaskManager { ... }

impl TaskManager {
    pub fn new() -> Self;
    pub fn submit<F>(&self, kind: &str, work: F) -> String
    where F: FnOnce(ProgressSender) -> Result<serde_json::Value> + Send + 'static;
    pub fn get(&self, id: &str) -> Option<TaskStatus>;
    pub fn list(&self) -> Vec<TaskSummary>;
    pub fn cleanup_older_than(&self, seconds: u64);
}

pub enum TaskStatus {
    Pending,
    Running { progress: String, started_at: String },
    Done { result: serde_json::Value, exit_code: i32, finished_at: String },
    Failed { error: String, finished_at: String },
}
```

Uses `DashMap<String, TaskEntry>` for concurrent access. Background work runs on `tokio::task::spawn_blocking`.

**Files**: new `src/task.rs`, edit `src/lib.rs`

**New deps**: `dashmap`, `uuid`

**Size**: ~200 LOC

**Acceptance**:
- [ ] Submit returns task ID immediately
- [ ] Get returns current status
- [ ] Status transitions: Pending → Running → Done/Failed
- [ ] Cleanup removes old completed tasks
- [ ] Thread-safe under concurrent access

**Depends on**: nothing

---

### B-2. Add async API routes

**Edit** `src/api.rs`

New routes:

```
POST /api/analyze      → { "task_id": "..." }   (async, returns immediately)
POST /api/review       → { "task_id": "..." }   (async)
POST /api/deep-review  → { "task_id": "..." }   (async)
GET  /api/task/:id     → TaskStatus
GET  /api/tasks        → Vec<TaskSummary>
```

Keep `?sync=true` query parameter for backward compatibility — when set, blocks and returns result directly (current behavior).

Add `TaskManager` to `ApiState`.

**Files**: edit `src/api.rs`

**Size**: ~120 LOC

**Acceptance**:
- [ ] `POST /api/analyze` returns `202 Accepted` with task_id
- [ ] `GET /api/task/:id` returns current status
- [ ] `?sync=true` preserves old blocking behavior
- [ ] Task list endpoint works

**Depends on**: B-1

---

### B-3. Implement review history storage

**Create** `src/history.rs`

```rust
pub struct HistoryStore { ... }

impl HistoryStore {
    pub fn open_default() -> Result<Self>;        // ~/.config/code-review/history.db
    pub fn save(&self, entry: &HistoryEntry) -> Result<String>;  // returns ID
    pub fn get(&self, id: &str) -> Result<Option<HistoryEntry>>;
    pub fn list(&self, filter: HistoryFilter) -> Result<Vec<HistorySummary>>;
    pub fn trend(&self, repo: &str, days: u32) -> Result<Vec<TrendPoint>>;
}

pub struct HistoryEntry {
    pub id: String,
    pub created_at: String,
    pub repo: String,
    pub git_range: Option<String>,
    pub branch: Option<String>,
    pub mode: String,
    pub strategy: Option<String>,
    pub exit_code: i32,
    pub high_risk_count: u32,
    pub medium_risk_count: u32,
    pub low_risk_count: u32,
    pub score: u8,
    pub confidence: String,
    pub result_json: String,
}

pub struct TrendPoint {
    pub date: String,
    pub review_count: u32,
    pub avg_high_risk: f32,
    pub avg_score: f32,
}
```

**Files**: new `src/history.rs`, edit `src/lib.rs`

**New deps**: `rusqlite`

**Size**: ~250 LOC

**Acceptance**:
- [ ] SQLite DB auto-created on first use
- [ ] Save + get round-trip works
- [ ] List with repo/limit filter works
- [ ] Trend aggregation returns daily averages

**Depends on**: nothing

---

### B-4. Auto-save reviews to history

**Edit** `src/services/review_service.rs`

After every `execute_review` / `execute_deep_review` / `execute_analyze`, save result to history:

```rust
if let Ok(store) = HistoryStore::open_default() {
    let _ = store.save(&HistoryEntry::from_review_result(&result, repo, git_range));
}
```

Non-blocking, non-fatal — history save failure should not break the review.

**Files**: edit `review_service.rs`

**Size**: ~40 LOC

**Acceptance**:
- [ ] Every review execution creates a history record
- [ ] History save failure does not affect review output
- [ ] History entry contains full ReviewResult JSON

**Depends on**: B-3

---

### B-5. Add `history` CLI commands

**Edit** `src/cli.rs`, `src/lib.rs`

```
code-review history                           # list recent 20
code-review history --repo . --limit 10       # filter
code-review history show <id>                 # full result
code-review history trend --repo . --days 30  # trend
```

**Files**: edit `cli.rs`, `lib.rs`

**Size**: ~100 LOC

**Acceptance**:
- [ ] `history` lists recent reviews with summary (id, repo, mode, exit_code, date)
- [ ] `history show <id>` prints full ReviewResult (text or JSON via `--format`)
- [ ] `history trend` shows daily risk trend

**Depends on**: B-3

---

### B-6. Add history API endpoints

**Edit** `src/api.rs`

```
GET /api/history?repo=.&limit=10
GET /api/history/:id
GET /api/history/trend?repo=.&days=30
```

**Files**: edit `api.rs`

**Size**: ~80 LOC

**Acceptance**:
- [ ] History list returns JSON array with summary fields
- [ ] History detail returns full ReviewResult
- [ ] Trend returns array of TrendPoint

**Depends on**: B-3

---

## Phase C: CI/PR Integration

**Goal**: Post review results to GitHub PRs. Provide CI workflow templates.

### C-1. GitHub API client

**Create** `src/github.rs`

```rust
pub struct GitHubClient { ... }

impl GitHubClient {
    pub fn from_env() -> Result<Self>;  // reads GITHUB_TOKEN
    pub fn get_pr_info(&self, owner: &str, repo: &str, pr: u64) -> Result<PrInfo>;
    pub fn get_pr_diff(&self, owner: &str, repo: &str, pr: u64) -> Result<String>;
    pub fn post_comment(&self, owner: &str, repo: &str, pr: u64, body: &str) -> Result<()>;
    pub fn update_comment(&self, owner: &str, repo: &str, comment_id: u64, body: &str) -> Result<()>;
    pub fn find_bot_comment(&self, owner: &str, repo: &str, pr: u64) -> Result<Option<u64>>;
}
```

Uses `reqwest` (already a dependency) to call GitHub REST API.
`find_bot_comment` looks for an existing comment with a marker string to enable update-in-place instead of duplicate comments.

**Files**: new `src/github.rs`, edit `src/lib.rs`

**Size**: ~200 LOC

**Acceptance**:
- [ ] Can fetch PR title, body, diff
- [ ] Can post Markdown comment to a PR
- [ ] Can find and update existing bot comment
- [ ] Clear error if GITHUB_TOKEN not set

**Depends on**: nothing

---

### C-2. Markdown renderer for PR comments

**Create** `src/review_render_md.rs`

Converts `ReviewResult` into GitHub-flavored Markdown:

```rust
pub fn render_review_result_markdown(result: &ReviewResult) -> String;
```

Output structure:
```
## AI Code Review — {mode} Mode
Score: X | Confidence: Y | Human review: Z

### High Risk (N)
table of issues with file, title, layer, impact

### Medium Risk (N)
...

### Missing Tests
bullet list

### Summary
text

---
footer with tool version
```

**Files**: new `src/review_render_md.rs`, edit `src/lib.rs`

**Size**: ~120 LOC

**Acceptance**:
- [ ] Renders clean GFM that GitHub displays correctly
- [ ] Tables for risk issues with clickable file paths
- [ ] Layer column shown when populated
- [ ] Critical mode includes impact scope + release checks sections

**Depends on**: nothing

---

### C-3. `pr-review` CLI command

**Edit** `src/cli.rs`, `src/lib.rs`

```
code-review pr-review --repo owner/repo --pr 123 [--mode standard] [--dry-run]
```

Flow:
1. Fetch PR diff + description via GitHub API
2. Use PR title as `goal`, PR body as `issue`
3. Clone repo (or use local if `--local-repo` given)
4. Run `execute_analyze` pipeline
5. Render result as Markdown
6. Post as PR comment (or print if `--dry-run`)
7. If existing bot comment found, update in place

**Files**: edit `cli.rs`, `lib.rs`, new `src/services/pr_service.rs`

**Size**: ~200 LOC

**Acceptance**:
- [ ] Can review a public PR by number
- [ ] Posts formatted Markdown comment
- [ ] `--dry-run` prints but doesn't post
- [ ] Updates existing comment on re-run (no duplicate comments)
- [ ] Exit code reflects review result (0/2/3/4)

**Depends on**: C-1, C-2, Phase A (provider)

---

### C-4. GitHub Actions workflow template

**Create** `templates/github-actions.yml`

Reusable workflow for teams to drop into `.github/workflows/`.

**Files**: new `templates/github-actions.yml`

**Size**: ~50 LOC YAML

**Acceptance**:
- [ ] Works with `actions/checkout@v4`
- [ ] Runs `analyze` + `pr-review` in sequence
- [ ] Passes exit code to GitHub check status
- [ ] Documents required secrets

**Depends on**: C-3

---

## Phase D: Web UI

**Goal**: Single-binary web dashboard for review results.

### D-1. Embed static assets

**Create** `src/web.rs`

Use `rust-embed` to embed frontend files into the binary:

```rust
#[derive(rust_embed::Embed)]
#[folder = "frontend/dist"]
struct FrontendAssets;

pub fn frontend_routes() -> Router { ... }
```

**Files**: new `src/web.rs`, edit `src/api.rs` (mount frontend routes)

**New deps**: `rust-embed`, `mime_guess`

**Size**: ~60 LOC (Rust side)

**Acceptance**:
- [ ] `code-review serve` serves frontend at `/`
- [ ] API still available at `/api/*`
- [ ] Single binary, no separate file server needed

**Depends on**: nothing

---

### D-2. Frontend: dashboard page

**Create** `frontend/` directory with minimal React + Tailwind app (or htmx for simplicity).

Pages:
- `/` — dashboard: recent reviews table + risk trend mini-chart
- `/review/:id` — review detail with collapsible issue cards
- `/review/new` — form to submit new review

**Files**: new `frontend/` directory

**Size**: ~800 LOC (TypeScript/JSX)

**Acceptance**:
- [ ] Dashboard shows recent reviews from `/api/history`
- [ ] Click through to review detail
- [ ] Review detail shows structured issues with expand/collapse
- [ ] New review form submits to `/api/analyze` and polls task status

**Depends on**: D-1, B-2, B-6

---

## Phase E: Smart Analysis (parallel track)

### E-1. Tree-sitter integration

**Create** `src/treesitter.rs`

```rust
pub fn extract_imports(path: &str, content: &str, lang: Language) -> Vec<ImportRef>;
pub fn extract_definitions(path: &str, content: &str, lang: Language) -> Vec<SymbolDef>;
pub fn detect_language(path: &str) -> Option<Language>;
```

Replace heuristic import extraction in `expand.rs` with tree-sitter when available. Fall back to regex for unsupported languages.

**Files**: new `src/treesitter.rs`, edit `src/expand.rs`

**New deps**: `tree-sitter`, `tree-sitter-rust`, `tree-sitter-typescript`, `tree-sitter-java`, `tree-sitter-go`, `tree-sitter-python`

**Size**: ~300 LOC

**Acceptance**:
- [ ] Exact import extraction for Rust, TypeScript, Java
- [ ] Exact symbol definition extraction
- [ ] Graceful fallback for unknown languages
- [ ] Measurably fewer false positives in expanded context

**Depends on**: nothing

---

### E-2. Project-level rule engine

**Create** `src/rules.rs`

Load `.code-review-rules.toml` from project root:

```rust
pub struct Rule {
    pub name: String,
    pub description: String,
    pub pattern: RulePattern,
    pub severity: String,
    pub layer: String,
}

pub fn load_project_rules(repo: &Path) -> Result<Vec<Rule>>;
pub fn evaluate_rules(rules: &[Rule], changed_files: &[String], file_contents: &[(String, String)]) -> Vec<RuleFinding>;
```

Findings get injected into the prompt as "confirmed violations" and into the final ReviewResult.

**Files**: new `src/rules.rs`, edit `prompt.rs`, edit `review_service.rs`

**Size**: ~200 LOC

**Acceptance**:
- [ ] Loads rules from `.code-review-rules.toml`
- [ ] Evaluates file path patterns and content patterns
- [ ] Injects findings into prompt
- [ ] Findings appear in ReviewResult

**Depends on**: nothing

---

### E-3. Context metadata cache

**Create** `src/cache.rs`

```rust
pub struct ContextCache { ... }

impl ContextCache {
    pub fn open_default() -> Result<Self>;  // ~/.config/code-review/cache.db
    pub fn get_file_meta(&self, path: &str, mtime: u64, size: u64) -> Option<FileMeta>;
    pub fn set_file_meta(&self, path: &str, mtime: u64, size: u64, meta: &FileMeta);
    pub fn invalidate_repo(&self, repo: &str);
}

pub struct FileMeta {
    pub symbols: Vec<String>,
    pub imports: Vec<String>,
    pub definitions: Vec<String>,
}
```

**Files**: new `src/cache.rs`, edit `expand.rs`

**Size**: ~150 LOC

**Acceptance**:
- [ ] Cache hit skips re-extraction
- [ ] Modified file (different mtime/size) triggers re-extraction
- [ ] Second run of same repo is measurably faster

**Depends on**: nothing (benefits from E-1 but works without it)

---

## Execution Timeline

```
Week  1  2  3  4  5  6  7  8  9  10  11  12
      ─────────────────────────────────────────
A-1   ██                                        LlmProvider trait
A-2   ██                                        CopilotCliProvider
A-3   ████                                      JSON schema + raw types
A-9   ██                                        Config + CLI flags
A-4      ████                                   ClaudeApiProvider
A-5      ████                                   OpenAiApiProvider
A-6         ██                                  OllamaProvider
A-7         ████                                Wire into service layer
A-8            ██                                Layer tagging
      ─────────────────────────────────────────
B-1               ████                          TaskManager
B-3               ████                          History SQLite
B-2                  ████                       Async API routes
B-4                  ██                         Auto-save to history
B-5                     ██                      History CLI
B-6                     ██                      History API
      ─────────────────────────────────────────
C-1                        ████                 GitHub client
C-2                        ██                   Markdown renderer
C-3                           ████              pr-review command
C-4                              ██             Actions template
      ─────────────────────────────────────────
D-1                                 ██          Embed static assets
D-2                                 ██████      Frontend pages
      ─────────────────────────────────────────
E-1         ░░░░░░░░░░░░░░░░░░░░░░░░░░         Tree-sitter (parallel)
E-2                  ░░░░░░░░░░░░░░░░░░         Rule engine (parallel)
E-3                        ░░░░░░░░░░░░         Cache (parallel)

██ = main track (blocking)
░░ = parallel track (independent)
```

---

## Definition of Done per Phase

### Phase A Complete
- [ ] `code-review analyze` works with `--provider claude` using structured JSON output
- [ ] `code-review analyze` still works with `--provider copilot` using text parser fallback
- [ ] ReviewResult JSON includes `layer` field per issue
- [ ] Config `provider = "claude"` / `"openai"` / `"ollama"` / `"copilot"` all functional
- [ ] All existing tests pass + new provider tests added

### Phase B Complete
- [ ] `POST /api/analyze` returns task_id; `GET /api/task/:id` returns status
- [ ] `?sync=true` preserves blocking behavior
- [ ] `code-review history` shows past reviews
- [ ] `code-review history trend` shows risk trend
- [ ] Every review auto-saved to SQLite

### Phase C Complete
- [ ] `code-review pr-review --repo owner/repo --pr 123` posts Markdown to PR
- [ ] `--dry-run` prints without posting
- [ ] Re-run updates existing comment (no duplicates)
- [ ] GitHub Actions template tested on a real PR
- [ ] Exit code propagates to CI check status

### Phase D Complete
- [ ] `code-review serve` shows web dashboard at `/`
- [ ] Dashboard lists recent reviews with risk trend chart
- [ ] Review detail page shows structured results
- [ ] New review form submits and polls async task

### Phase E Complete
- [ ] Tree-sitter extracts imports for Rust + TypeScript + Java
- [ ] `.code-review-rules.toml` rules detected and injected into prompt + result
- [ ] Context cache makes second run measurably faster

---

## Risk Mitigation

| Risk | Mitigation |
|------|------------|
| Claude/OpenAI API changes | Pin SDK version; abstract behind trait so swap is one file |
| Structured output schema drift | Schema is versioned; old results remain readable |
| Tree-sitter grammars add binary size | Feature-gate behind `treesitter` cargo feature |
| SQLite on shared filesystem | Use WAL mode; file lock already proven in session.rs |
| Frontend bundling complexity | Start with htmx (zero build step) if React is too heavy |
| Breaking CLI changes | Default all new fields to backward-compatible values |
