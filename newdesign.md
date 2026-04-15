# code-review Next-Gen Design

> Based on a full audit of the current codebase (v0.3), require.md, require2.md, and implementation-plan.md.
> This document proposes the next evolution: what to build, why, and in what order.

---

## 0. High-Level Architecture Graphs

### 0.1 Current System (v0.3)

```
                           ┌───────────────┐
                           │    User        │
                           └──────┬────────┘
                                  │
                    ┌─────────────┴─────────────┐
                    │                           │
               CLI (clap)               HTTP API (axum)
                    │                           │
                    └─────────────┬─────────────┘
                                  │
                    ┌─────────────▼─────────────┐
                    │    Service Layer           │
                    │  review_service.rs         │
                    │                           │
                    │  execute_prompt()          │
                    │  execute_run()             │
                    │  execute_review()          │
                    │  execute_deep_review()     │
                    │  execute_analyze()         │
                    │  execute_validate()        │
                    └──┬────┬────┬────┬────┬────┘
                       │    │    │    │    │
          ┌────────────┘    │    │    │    └────────────┐
          ▼                 ▼    │    ▼                 ▼
    ┌───────────┐   ┌──────────┐│┌──────────┐   ┌───────────┐
    │ admission │   │ prompt   │││ risk     │   │ copilot   │
    │  .rs      │   │  .rs     │││  .rs     │   │  .rs      │
    │           │   │          │││          │   │           │
    │ P0/P1/P2  │   │ assemble │││ file/diff│   │ CLI call  │
    │ gate      │   │ layers   │││ analysis │   │ timeout   │
    │ block/    │   │ context  │││ hints    │   │ @file     │
    │ warn/pass │   │ output   │││          │   │           │
    └───────────┘   └──────────┘│└──────────┘   └─────┬─────┘
                                │                     │
                    ┌───────────▼──────────┐          │
                    │                      │          ▼
              ┌─────┴──────┐  ┌────────────┴┐  ┌──────────┐
              │ parser     │  │ validate    │  │ copilot  │
              │  .rs       │  │  .rs        │  │ CLI      │
              │            │  │             │  │(external)│
              │ regex      │  │ field check │  └──────────┘
              │ section    │  │ auto-repair │
              │ extraction │  │ summary fix │
              └────────────┘  └─────────────┘
                                      │
          ┌───────────────────────────┘
          ▼
    ┌──────────┐   ┌──────────┐   ┌──────────┐   ┌──────────┐
    │ context  │   │ expand   │   │ jira     │   │ config   │
    │  .rs     │   │  .rs     │   │  .rs     │   │  .rs     │
    │          │   │          │   │          │   │          │
    │ budget   │   │ import   │   │ enrich   │   │ TOML     │
    │ binary   │   │ symbol   │   │ accept   │   │ defaults │
    │ truncate │   │ backend  │   │ risks    │   │ model    │
    │ utf-8    │   │ frontend │   │ infer    │   │          │
    └──────────┘   └──────────┘   └──────────┘   └──────────┘
```

### 0.2 Target System (v1.0)

```
                           ┌───────────────┐
                           │    User        │
                           └──────┬────────┘
                                  │
               ┌──────────────────┼──────────────────┐
               │                  │                  │
          CLI (clap)       HTTP API (axum)      Web UI
               │                  │              (embedded)
               │                  │                  │
               └──────────┬───────┴──────────────────┘
                          │
               ┌──────────▼──────────┐
               │   Service Layer     │
               │                     │
               │  + TaskManager      │──────┐
               │  + HistoryStore     │      │
               └──────────┬──────────┘      │
                          │                 │
          ┌───────────────┼──────────┐      │
          │               │          │      │
          ▼               ▼          ▼      ▼
   ┌────────────┐  ┌───────────┐  ┌──────────────┐
   │ Admission  │  │  Prompt   │  │  LLM Layer   │
   │ Gate       │  │  Builder  │  │  (NEW)       │
   │            │  │           │  │              │
   │ P0/P1/P2   │  │ layers    │  │ ┌──────────┐│
   │ mode rules │  │ risk hint │  │ │ Provider ││
   │ incident   │  │ schema    │  │ │ Trait    ││
   └────────────┘  └───────────┘  │ └────┬─────┘│
                                  │      │      │
                                  │  ┌───┴────┐ │
                                  │  │        │ │
                        ┌─────────┤  ▼  ▼  ▼  ▼│
                        │         │ Cop Cla OAI O│
                        │         │ ilot ude pen ll│
                        │         │      API AI ama│
                        │         └──────────────┘
                        │               │
                        ▼               ▼
                 ┌────────────┐  ┌────────────┐
                 │ Structured │  │ Text Parse │
                 │ JSON Path  │  │ Fallback   │
                 │ (NEW)      │  │ (existing) │
                 │            │  │            │
                 │ tool_use / │  │ regex      │
                 │ json_mode  │  │ section    │
                 │ direct     │  │ extract    │
                 │ deserialize│  │ repair     │
                 └──────┬─────┘  └──────┬─────┘
                        │               │
                        └───────┬───────┘
                                ▼
                 ┌────────────────────────┐
                 │  Validate + Risk       │
                 │  + Layer Tag (NEW)     │
                 └───────────┬────────────┘
                             │
              ┌──────────────┼──────────────┐
              │              │              │
              ▼              ▼              ▼
       ┌────────────┐ ┌───────────┐ ┌────────────┐
       │  History   │ │  GitHub   │ │  Render    │
       │  (NEW)     │ │  PR Post  │ │            │
       │            │ │  (NEW)    │ │  Text      │
       │  SQLite    │ │           │ │  JSON      │
       │  trend     │ │  comment  │ │  Markdown  │
       │  compare   │ │  CI gate  │ │  (NEW)     │
       └────────────┘ └───────────┘ └────────────┘
```

### 0.3 End-to-End Review Workflow (Target)

```
┌──────────────────────────────────────────────────────────────────────────┐
│                         TRIGGER                                         │
│   CLI: code-review analyze --git HEAD~1..HEAD                           │
│   API: POST /api/analyze { git, repo, mode, ... }                      │
│   CI:  GitHub Action on pull_request                                    │
│   Web: Submit form on /review/new                                       │
└──────────────────────────────┬───────────────────────────────────────────┘
                               │
                               ▼
┌──────────────────────────────────────────────────────────────────────────┐
│  PHASE 1: COLLECT                                                       │
│                                                                         │
│  ┌─────────┐  ┌──────────┐  ┌──────────┐  ┌───────────┐  ┌──────────┐ │
│  │git diff │  │git       │  │Jira /    │  │config    │  │incident │ │
│  │         │  │ls-files  │  │Issue     │  │.toml     │  │baseline │ │
│  │diff text│  │all files │  │enrich    │  │defaults  │  │files    │ │
│  └────┬────┘  └────┬─────┘  └────┬─────┘  └────┬─────┘  └────┬────┘ │
│       │            │             │              │              │      │
│       └────────────┴──────┬──────┴──────────────┴──────────────┘      │
│                           ▼                                           │
│                 ┌───────────────────┐                                  │
│                 │ Merged PromptArgs │                                  │
│                 └─────────┬─────────┘                                  │
└───────────────────────────┼──────────────────────────────────────────────┘
                            │
                            ▼
┌──────────────────────────────────────────────────────────────────────────┐
│  PHASE 2: EXPAND                                                        │
│                                                                         │
│  Changed files ──┬──► expand_related_files()                            │
│                  │      tests, DTOs, schemas, contracts                  │
│                  │                                                       │
│                  ├──► expand_dependency_files()                          │
│                  │      import chain ──► highest priority                │
│                  │      backend chain ──► controller/service/repo/dto    │
│                  │      frontend chain ──► component/store/hook/api      │
│                  │      route chain                                      │
│                  │      reference / symbol ──► lowest priority           │
│                  │                                                       │
│                  └──► read_repo_context_with_budget()                    │
│                        skip binary, non-utf8, oversized                  │
│                        truncate at utf-8 boundary                        │
│                        total budget: 48KB (configurable)                 │
│                                                                         │
│  Future: tree-sitter for exact import/call resolution                   │
└──────────────────────────┬───────────────────────────────────────────────┘
                           │
                           ▼
┌──────────────────────────────────────────────────────────────────────────┐
│  PHASE 3: GATE                                                          │
│                                                                         │
│  check_admission(prompt_args, has_diff, has_context)                    │
│                                                                         │
│  ┌────────────┬──────────────────┬──────────────────────────────────┐   │
│  │   Lite     │   Standard       │   Critical                      │   │
│  ├────────────┼──────────────────┼──────────────────────────────────┤   │
│  │ diff only  │ diff+goal+rules  │ diff+goal+rules                 │   │
│  │ warn if    │ BLOCK if P0 miss │ BLOCK if P0 miss                │   │
│  │ P0 missing │ warn if >2 P1    │ BLOCK if <2 P1                  │   │
│  │            │ missing          │ BLOCK if no P2                   │   │
│  │ confidence │                  │ (baseline/incident/focus/jira)   │   │
│  │ = low      │ confidence       │                                  │   │
│  │            │ = medium/high    │ confidence = high only if all    │   │
│  └────────────┴──────────────────┴──────────────────────────────────┘   │
│                                                                         │
│  Result: Pass / Warn / Block  +  score  +  confidence                   │
│  If Block → exit code 3, stop here                                      │
└──────────────────────────┬───────────────────────────────────────────────┘
                           │ Pass or Warn
                           ▼
┌──────────────────────────────────────────────────────────────────────────┐
│  PHASE 4: BUILD PROMPT                                                  │
│                                                                         │
│  ┌────────────────────────────────────────────────────────────────────┐ │
│  │ Role instruction                                                   │ │
│  │ "你是一个资深代码审查工程师..."                                     │ │
│  ├────────────────────────────────────────────────────────────────────┤ │
│  │ Context: stack / goal / why / issue / rules / risks / expected    │ │
│  ├────────────────────────────────────────────────────────────────────┤ │
│  │ Four-layer checklist (from review_layers.rs)                      │ │
│  │   Basic:       null/bounds/exception/concurrency                  │ │
│  │   Engineering: architecture/maintainability/perf                   │ │
│  │   Business:    rules/auth/state/idempotency                       │ │
│  │   Risk:        impact/contract/migration/compat (per change_type) │ │
│  ├────────────────────────────────────────────────────────────────────┤ │
│  │ Program-level risk hints (from risk.rs)                           │ │
│  │   DB migration / API contract / auth / cross-layer / incident     │ │
│  ├────────────────────────────────────────────────────────────────────┤ │
│  │ Output constraints (text sections or JSON schema)                 │ │
│  ├────────────────────────────────────────────────────────────────────┤ │
│  │ ## Diff                                                           │ │
│  │ ```diff ... ```                                                   │ │
│  ├────────────────────────────────────────────────────────────────────┤ │
│  │ ## Context Files (budget-controlled)                              │ │
│  │ ### src/order/service.rs                                          │ │
│  │ ### src/order/dto.rs (dependency-context:backend-chain)           │ │
│  │ ### tests/order_test.rs (dependency-context:reference)            │ │
│  └────────────────────────────────────────────────────────────────────┘ │
└──────────────────────────┬───────────────────────────────────────────────┘
                           │
                           ▼
┌──────────────────────────────────────────────────────────────────────────┐
│  PHASE 5: LLM CALL                                                      │
│                                                                         │
│  ┌───────────────────────────────────────────┐                          │
│  │ Provider Router (NEW in v1.0)             │                          │
│  │                                           │                          │
│  │  config.provider ─┬─► CopilotCliProvider  │──► text output           │
│  │                   │     copilot -p "..."   │    (parse needed)        │
│  │                   │                       │                          │
│  │                   ├─► ClaudeApiProvider    │──► JSON via tool_use     │
│  │                   │     POST /messages     │    (direct deserialize)  │
│  │                   │     + tool schema      │                          │
│  │                   │                       │                          │
│  │                   ├─► OpenAiApiProvider    │──► JSON via json_mode    │
│  │                   │     POST /completions  │    (direct deserialize)  │
│  │                   │                       │                          │
│  │                   └─► OllamaProvider      │──► text or JSON          │
│  │                         local model        │                          │
│  └───────────────────────────────────────────┘                          │
│                                                                         │
│  Strategy:                                                               │
│  ┌──────────────────┐    ┌─────────────────────────────────────────┐    │
│  │ Standard / Lite  │    │ Deep (forced for Critical)              │    │
│  │                  │    │                                         │    │
│  │  Single LLM call │    │  Stage 1: full review                  │    │
│  │  ───► result     │    │  ───► extract high-risk files + hints  │    │
│  │                  │    │  Stage 2: targeted deep dive            │    │
│  │                  │    │  ───► focused on stage 1 findings      │    │
│  └──────────────────┘    └─────────────────────────────────────────┘    │
└──────────────────────────┬───────────────────────────────────────────────┘
                           │
                           ▼
┌──────────────────────────────────────────────────────────────────────────┐
│  PHASE 6: PARSE + VALIDATE                                              │
│                                                                         │
│  ┌──────────────────────────────────┐                                   │
│  │ JSON provider?                   │                                   │
│  │  YES ──► direct serde deserialize│                                   │
│  │  NO  ──► review_parser.rs        │                                   │
│  │           section detect          │                                   │
│  │           field extract           │                                   │
│  │           critical backfill       │                                   │
│  └──────────────┬───────────────────┘                                   │
│                 │                                                        │
│                 ▼                                                        │
│  ┌──────────────────────────────────┐                                   │
│  │ review_validate.rs               │                                   │
│  │                                  │                                   │
│  │  summary not empty?              │──► auto-repair if missing         │
│  │  issues have file/reason?        │──► fill defaults if missing       │
│  │  critical has impact_scope?      │──► ERROR if missing in critical   │
│  │  critical has release_checks?    │──► ERROR if missing in critical   │
│  └──────────────┬───────────────────┘                                   │
│                 │                                                        │
│            ok?  ├──YES──► finalize                                       │
│                 │                                                        │
│                 └──NO───► repair prompt ──► second LLM call              │
│                           re-parse ──► re-validate                       │
│                           still fail? ──► exit code 4                    │
└──────────────────────────┬───────────────────────────────────────────────┘
                           │
                           ▼
┌──────────────────────────────────────────────────────────────────────────┐
│  PHASE 7: OUTPUT                                                        │
│                                                                         │
│  ReviewResult                                                            │
│  ┌────────────────────────────────────────────────────────────────────┐ │
│  │  mode / input_ok / input_score / confidence                       │ │
│  │  high_risk[] / medium_risk[] / low_risk[]                         │ │
│  │    each: title, file, location, reason, trigger, impact,          │ │
│  │          suggestion, layer (NEW)                                   │ │
│  │  missing_tests[] / summary / needs_human_review                   │ │
│  │  used_rules[] / impact_scope[] / release_checks[]                 │ │
│  │  risk_hints[] / validation_report / raw_text                      │ │
│  └────────────────────────────────────────────────────────────────────┘ │
│                                                                         │
│  Render targets:                                                         │
│  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌───────────┐              │
│  │ Terminal │  │ JSON     │  │ Markdown │  │ Web UI    │              │
│  │ text     │  │ (CI /    │  │ (PR      │  │ (dashboard│              │
│  │          │  │  script) │  │  comment)│  │  detail)  │              │
│  └──────────┘  └──────────┘  └──────────┘  └───────────┘              │
│                                                                         │
│  Side effects:                                                           │
│  ┌──────────────┐  ┌───────────────┐  ┌────────────────┐               │
│  │ Save to      │  │ Post to       │  │ Exit code      │               │
│  │ history DB   │  │ GitHub PR     │  │ 0/2/3/4/5      │               │
│  │ (NEW)        │  │ (NEW)         │  │ (CI gate)      │               │
│  └──────────────┘  └───────────────┘  └────────────────┘               │
└──────────────────────────────────────────────────────────────────────────┘
```

### 0.4 Async Task Workflow (API)

```
Client                          API Server                    Background
  │                                │                              │
  │  POST /api/analyze             │                              │
  │  { git, repo, mode, ... }      │                              │
  │ ──────────────────────────────►│                              │
  │                                │  TaskManager.submit()        │
  │                                │─────────────────────────────►│
  │  { "task_id": "abc-123" }      │                              │  execute_analyze()
  │ ◄──────────────────────────────│                              │  running...
  │                                │                              │
  │  GET /api/task/abc-123         │                              │
  │ ──────────────────────────────►│                              │
  │  { "status": "running",        │                              │
  │    "progress": "stage 1..." }  │                              │
  │ ◄──────────────────────────────│                              │
  │                                │                              │  stage 1 done
  │         ... poll ...           │                              │  stage 2 running
  │                                │                              │
  │  GET /api/task/abc-123         │                              │  done!
  │ ──────────────────────────────►│                              │
  │  { "status": "done",           │  ◄──────────────────────────│
  │    "exit_code": 2,             │                              │
  │    "result": { ReviewResult }} │                              │
  │ ◄──────────────────────────────│                              │
  │                                │                              │
  │  ── OR via SSE ──              │                              │
  │  GET /api/task/abc-123/stream  │                              │
  │ ──────────────────────────────►│                              │
  │  data: {"progress":"stage 1"}  │                              │
  │  data: {"progress":"stage 2"}  │                              │
  │  data: {"status":"done",...}   │                              │
  │ ◄─────────── stream ──────────│                              │
```

### 0.5 CI/PR Integration Workflow

```
┌──────────────────────────────────────────────────────────────────────┐
│  Developer pushes code / opens PR                                    │
└──────────────────────┬───────────────────────────────────────────────┘
                       │
                       ▼
┌──────────────────────────────────────────────────────────────────────┐
│  GitHub Actions trigger: on pull_request                             │
│                                                                      │
│  ┌────────────────────────────────────────────────────────────────┐  │
│  │  Step 1: checkout + fetch full history                        │  │
│  │  Step 2: install code-review                                  │  │
│  │  Step 3: code-review analyze                                  │  │
│  │            --repo .                                           │  │
│  │            --git origin/main...HEAD                           │  │
│  │            --mode standard                                    │  │
│  │            --format json > review.json                        │  │
│  │                                                               │  │
│  │          Captures exit_code:                                  │  │
│  │            0 = clean     ──► green check                      │  │
│  │            2 = high risk ──► yellow warning                   │  │
│  │            3 = blocked   ──► red X (missing context)          │  │
│  │            4 = parse fail──► orange warning                   │  │
│  │                                                               │  │
│  │  Step 4: code-review pr-review                                │  │
│  │            --repo owner/repo                                  │  │
│  │            --pr $PR_NUMBER                                    │  │
│  │                                                               │  │
│  │          Posts structured Markdown comment to PR:              │  │
│  │  ┌──────────────────────────────────────────────────────┐     │  │
│  │  │  ## AI Code Review - Standard Mode                   │     │  │
│  │  │  Score: 75 | Confidence: medium | Human review: yes  │     │  │
│  │  │                                                      │     │  │
│  │  │  ### High Risk (1)                                   │     │  │
│  │  │  | File | Issue | Layer | Impact |                   │     │  │
│  │  │  | service.rs:create_order | No idempotency |        │     │  │
│  │  │  |   business | Duplicate orders |                   │     │  │
│  │  │                                                      │     │  │
│  │  │  ### Missing Tests                                   │     │  │
│  │  │  - Concurrent retry scenario                         │     │  │
│  │  │                                                      │     │  │
│  │  │  ### Summary                                         │     │  │
│  │  │  1 high-risk issue found. Review before merge.       │     │  │
│  │  └──────────────────────────────────────────────────────┘     │  │
│  └────────────────────────────────────────────────────────────────┘  │
└──────────────────────────────────────────────────────────────────────┘
                       │
                       ▼
┌──────────────────────────────────────────────────────────────────────┐
│  Human reviewer sees:                                                │
│                                                                      │
│  ┌──────────────────────────────────────────────────────────────┐    │
│  │  PR #42: Fix order payment                                  │    │
│  │                                                              │    │
│  │  Checks:                                                     │    │
│  │    ✅ Build          passed                                  │    │
│  │    ✅ Unit tests     passed                                  │    │
│  │    ⚠️  AI Review     high risk found (exit 2)                │    │
│  │                                                              │    │
│  │  Comments:                                                   │    │
│  │    🤖 AI Code Review  [see structured review above]          │    │
│  │    👤 Senior Dev      "confirmed, adding idempotency key"    │    │
│  └──────────────────────────────────────────────────────────────┘    │
└──────────────────────────────────────────────────────────────────────┘
```

### 0.6 Module Dependency Graph (Target v1.0)

```
                        main.rs
                          │
                        lib.rs ◄─── config.rs
                          │
              ┌───────────┼───────────────────────┐
              │           │                       │
          api.rs    cli dispatch             web/ (NEW)
              │           │                       │
              └─────┬─────┘                       │
                    │                             │
            services/review_service.rs ◄──────────┘
              │    │    │    │    │
              │    │    │    │    └──────────────────────┐
              │    │    │    │                           │
              │    │    │    └──► task.rs (NEW)          │
              │    │    │         async submit/poll      │
              │    │    │                                │
              │    │    └──────► history.rs (NEW)        │
              │    │              SQLite store           │
              │    │              trend query            │
              │    │                                     │
              │    └──────────► llm.rs (NEW)             │
              │                  │                       │
              │          ┌───────┼───────┬──────┐       │
              │          │       │       │      │       │
              │       copilot claude  openai  ollama    │
              │       .rs    ApiProv ApiProv  Prov      │
              │       (exist) (NEW)  (NEW)   (NEW)     │
              │                                         │
              ├──► admission.rs                         │
              ├──► prompt.rs ◄── review_layers.rs       │
              ├──► risk.rs                              │
              ├──► review_parser.rs (fallback)          │
              ├──► review_validate.rs                   │
              ├──► review_render.rs                     │
              ├──► review_render_md.rs (NEW)            │
              ├──► github.rs (NEW)                      │
              │      PR diff/post comment               │
              │                                         │
              ├──► expand.rs ◄── treesitter.rs (NEW)    │
              ├──► context.rs ◄── cache.rs (NEW)        │
              ├──► jira.rs                              │
              ├──► gitops.rs                            │
              ├──► session.rs                           │
              ├──► models.rs                            │
              └──► rules.rs (NEW)                       │
                    project-level rule engine            │
                                                        │
                                           ┌────────────┘
                                           ▼
                                    review_schema.rs
                                    (shared data types)
```

### 0.7 Data Flow Summary

```
INPUT                    PROCESS                  OUTPUT
─────                    ───────                  ──────

git diff ──────┐
git ls-files ──┤
Jira/Issue ────┤    ┌─────────┐
config.toml ───┤───►│ Collect │
baseline ──────┤    └────┬────┘
incident ──────┘         │
                         ▼
                    ┌─────────┐
                    │ Expand  │ ◄── expand.rs + treesitter (future)
                    └────┬────┘
                         │
                         ▼
                    ┌─────────┐
                    │  Gate   │ ──► BLOCK (exit 3) if P0/P1/P2 fail
                    └────┬────┘
                         │ Pass
                         ▼
                    ┌─────────┐
                    │  Build  │ ──► layers + risk hints + constraints
                    │ Prompt  │
                    └────┬────┘
                         │
                         ▼
                    ┌─────────┐     ┌──────────────────┐
                    │   LLM   │ ──► │ JSON (preferred) │
                    │  Call   │     │ Text (fallback)  │
                    └────┬────┘     └────────┬─────────┘
                         │                   │
                         ▼                   ▼
                    ┌─────────┐        ┌──────────┐
                    │Validate │ ◄──────│  Parse   │
                    │+ Repair │        │(if text) │
                    └────┬────┘        └──────────┘
                         │
            ┌────────────┼────────────┬──────────────┐
            ▼            ▼            ▼              ▼
      ┌──────────┐ ┌──────────┐ ┌──────────┐ ┌───────────┐
      │ Terminal │ │   JSON   │ │ Markdown │ │  History  │
      │ (human)  │ │ (CI/API) │ │ (PR post)│ │ (SQLite)  │
      └──────────┘ └──────────┘ └──────────┘ └───────────┘

      exit code: 0 (clean) / 2 (risk) / 3 (blocked) / 4 (parse fail) / 5 (error)
```

---

## 1. Where We Are Now

### Done Well
- Admission gate with P0/P1/P2 enforcement across all commands
- Four-layer review (Basic/Engineering/Business/Risk) in prompt
- Structured output schema with parser + validator + auto-repair
- Program-level risk analyzer (DB/API/auth/cross-layer)
- Cross-language dependency expansion (import chain, backend chain, frontend chain)
- Context budget protection (binary skip, truncation, UTF-8 boundary)
- HTTP API with shared service layer
- Exit codes for CI consumption
- Incident file and baseline file support
- Three-mode differentiation (lite/standard/critical)

### Key Weaknesses

| Area | Problem |
|------|---------|
| **LLM coupling** | Locked to local `copilot` CLI. No direct API, no model choice beyond copilot's offerings |
| **Output parsing** | Regex-based text parser is fragile. Model output drifts → parse failures → repair round-trip |
| **Layer tagging** | Issues in ReviewResult are not tagged with which layer (basic/engineering/business/risk) they belong to |
| **No async** | API blocks on review calls. Deep-review can take 3+ minutes |
| **No history** | Every run is ephemeral. No way to compare reviews across commits or track trends |
| **No PR integration** | Can read git diff but can't post results back to GitHub/GitLab PR |
| **No Web UI** | API exists but no frontend |
| **Single-machine** | Repo must be local. Can't review remote repos or work in a shared server model |
| **Engineering checks** | Only prompt hints, no actual static analysis (even lightweight) |
| **Prompt caching** | Every review builds prompt from scratch. No reuse of unchanged context |

---

## 2. Design Principles for Next Phase

1. **Eliminate the fragile parser** — get structured output from the model directly, not by parsing free text
2. **Decouple from copilot CLI** — support direct LLM API calls (Claude, OpenAI, etc.) as first-class providers
3. **Make results actionable** — post to PRs, integrate with CI, track over time
4. **Keep CLI first** — every new capability must work from CLI before it gets an API or UI
5. **Incremental delivery** — each phase must be independently shippable and useful

---

## 3. Proposed Phases

```
Phase A: Structured Output + Multi-Provider    (foundation rewrite)
Phase B: Async Tasks + Review History           (platform basics)
Phase C: CI/PR Integration                      (team adoption)
Phase D: Web UI + Dashboard                     (visibility)
Phase E: Smart Analysis Enhancements            (precision)
```

---

## 4. Phase A: Structured Output + Multi-Provider

### A1. JSON-Mode Structured Output

**Problem**: The current flow is: prompt asks for text → model outputs free text → regex parser tries to extract structure → often fails → triggers a repair round-trip. This is the single biggest reliability issue.

**Solution**: Use the model's native structured output capability.

```
Current:  prompt("output as text sections") → free text → regex parse → repair
Proposed: prompt("output as JSON") + JSON schema constraint → valid JSON → direct deserialize
```

**Design**:

New file: `src/llm.rs`

```rust
pub enum StructuredOutputMode {
    /// Model supports native JSON mode / tool_use (Claude, OpenAI)
    JsonSchema,
    /// Model only supports text (copilot CLI, older models)
    TextWithParse,
}

pub struct LlmResponse {
    pub raw_text: String,
    pub structured: Option<ReviewResultRaw>,  // present if JSON mode worked
    pub token_usage: Option<TokenUsage>,
}
```

When the provider supports JSON schema (Claude tool_use, OpenAI json_mode):
- Send the ReviewResult schema as the output format
- Model returns valid JSON directly
- No parsing or repair needed
- `review_parser.rs` becomes the fallback for text-only providers

**Schema to send to the model**:

```json
{
  "type": "object",
  "properties": {
    "high_risk": {
      "type": "array",
      "items": {
        "type": "object",
        "required": ["title", "file", "reason"],
        "properties": {
          "title": { "type": "string" },
          "file": { "type": "string" },
          "location": { "type": "string" },
          "reason": { "type": "string" },
          "trigger": { "type": "string" },
          "impact": { "type": "string" },
          "suggestion": { "type": "string" },
          "layer": { "type": "string", "enum": ["basic", "engineering", "business", "risk"] }
        }
      }
    },
    ...
  }
}
```

This also solves the **layer tagging** problem — the model directly tags which layer each issue belongs to.

### A2. Multi-Provider LLM Abstraction

**Problem**: Currently hardcoded to `copilot` CLI. Users with Claude API keys, OpenAI keys, or local models can't use them.

**Design**:

New file: `src/llm.rs` (provider abstraction)

```rust
pub trait LlmProvider: Send + Sync {
    fn name(&self) -> &str;
    fn supports_json_schema(&self) -> bool;
    fn review(&self, prompt: &str, schema: Option<&serde_json::Value>, model: Option<&str>) -> Result<LlmResponse>;
}

pub struct CopilotCliProvider { ... }      // current behavior, text-only
pub struct ClaudeApiProvider { ... }       // direct Anthropic API, JSON via tool_use
pub struct OpenAiApiProvider { ... }       // direct OpenAI API, JSON mode
pub struct OllamaProvider { ... }          // local models via Ollama
```

Config:

```toml
[llm]
provider = "claude"           # copilot | claude | openai | ollama
model = "claude-sonnet-4-6"
api_key_env = "ANTHROPIC_API_KEY"  # env var name, never stored in config

# Optional: fallback provider if primary fails
fallback_provider = "copilot"
```

**Implementation order**:
1. Define `LlmProvider` trait
2. Move current copilot logic into `CopilotCliProvider`
3. Add `ClaudeApiProvider` (highest value — supports tool_use for structured output)
4. Add `OpenAiApiProvider`
5. Add `OllamaProvider` (for air-gapped / local use)

### A3. Layer-Tagged Issues in ReviewResult

**Change**: Add `layer` field to `ReviewIssue`:

```rust
pub struct ReviewIssue {
    pub title: String,
    pub file: Option<String>,
    pub location: Option<String>,
    pub reason: Option<String>,
    pub trigger: Option<String>,
    pub impact: Option<String>,
    pub suggestion: Option<String>,
    pub layer: Option<String>,  // "basic" | "engineering" | "business" | "risk"
}
```

With JSON-mode providers, the model fills this directly.
With text-mode providers, the parser infers layer from keywords (existing section context + heuristics).

### A4. Deliverables

- [ ] `src/llm.rs` — provider trait + CopilotCliProvider + ClaudeApiProvider
- [ ] JSON schema definition for structured output
- [ ] `review_parser.rs` demoted to fallback for text-only providers
- [ ] `layer` field on ReviewIssue
- [ ] Config: `[llm] provider = "claude" | "copilot" | "openai" | "ollama"`
- [ ] New dependency: `anthropic-sdk` or raw `reqwest` for Claude API

---

## 5. Phase B: Async Tasks + Review History

### B1. Async Task Model

**Problem**: `POST /api/deep-review` blocks for 2-5 minutes. HTTP clients time out. No progress visibility.

**Design**:

New file: `src/task.rs`

```rust
pub enum TaskStatus {
    Pending,
    Running { progress: String },
    Done { result: serde_json::Value, exit_code: i32 },
    Failed { error: String },
}

pub struct TaskManager {
    tasks: DashMap<String, TaskEntry>,
}

impl TaskManager {
    pub fn submit(&self, kind: &str, work: impl FnOnce() -> Result<serde_json::Value> + Send + 'static) -> String;
    pub fn get(&self, task_id: &str) -> Option<TaskStatus>;
    pub fn list(&self) -> Vec<TaskSummary>;
}
```

New API routes:

```
POST /api/analyze    → { "task_id": "abc123" }      (returns immediately)
GET  /api/task/:id   → { "status": "running", "progress": "stage 1 complete" }
GET  /api/task/:id   → { "status": "done", "result": {...}, "exit_code": 0 }
GET  /api/tasks      → [{ "id": "abc123", "kind": "analyze", "status": "done", ... }]
```

**SSE streaming** (optional enhancement):

```
GET /api/task/:id/stream → Server-Sent Events with progress updates
```

**CLI behavior**: CLI commands remain synchronous (block and print). Only the API gets async mode.

### B2. Review History

**Problem**: Every review is fire-and-forget. Teams can't track: "did the risk go down since last review?" or "what issues were flagged across the last 10 PRs?"

**Design**:

New file: `src/history.rs`

Storage: SQLite at `~/.config/code-review/history.db`

```sql
CREATE TABLE reviews (
    id TEXT PRIMARY KEY,
    created_at TEXT NOT NULL,
    repo TEXT NOT NULL,
    git_range TEXT,
    branch TEXT,
    mode TEXT NOT NULL,
    strategy TEXT,
    exit_code INTEGER NOT NULL,
    high_risk_count INTEGER,
    medium_risk_count INTEGER,
    low_risk_count INTEGER,
    score INTEGER,
    confidence TEXT,
    result_json TEXT NOT NULL   -- full ReviewResult
);

CREATE INDEX idx_repo_created ON reviews(repo, created_at);
```

New CLI commands:

```bash
code-review history                          # list recent reviews
code-review history --repo . --limit 10      # filter by repo
code-review history show <id>                # show full result
code-review history diff <id1> <id2>         # compare two reviews
code-review history trend --repo . --days 30 # risk trend over time
```

New API:

```
GET /api/history?repo=.&limit=10
GET /api/history/:id
GET /api/history/trend?repo=.&days=30
```

**Auto-save**: Every `analyze` / `review` / `deep-review` execution automatically saves to history.

### B3. Deliverables

- [ ] `src/task.rs` — async task manager with DashMap
- [ ] Async API routes (`POST` returns task_id, `GET /task/:id` polls)
- [ ] `src/history.rs` — SQLite storage for review results
- [ ] `history` CLI command family
- [ ] History API endpoints
- [ ] New dependency: `rusqlite`, `dashmap`, `uuid`

---

## 6. Phase C: CI/PR Integration

### C1. GitHub PR Integration

**Problem**: Users must manually copy review results into PR comments. No automated workflow.

**Design**:

New file: `src/github.rs`

```rust
pub struct GitHubClient {
    token: String,  // from GITHUB_TOKEN env
    api_base: String,
}

impl GitHubClient {
    pub fn post_review_comment(&self, owner: &str, repo: &str, pr: u64, body: &str) -> Result<()>;
    pub fn get_pr_info(&self, owner: &str, repo: &str, pr: u64) -> Result<PrInfo>;
    pub fn get_pr_diff(&self, owner: &str, repo: &str, pr: u64) -> Result<String>;
}
```

New CLI command:

```bash
# Review a PR and post results as comment
code-review pr-review --repo owner/repo --pr 123

# Review a PR but only print locally (dry run)
code-review pr-review --repo owner/repo --pr 123 --dry-run
```

The command:
1. Fetches PR diff + description from GitHub API
2. Uses PR title as `goal`, PR body as `issue`
3. Runs full analyze pipeline
4. Formats ReviewResult as a Markdown comment
5. Posts to the PR (or prints if `--dry-run`)

### C2. GitHub Actions Integration

Provide a reusable workflow:

```yaml
# .github/workflows/code-review.yml
name: AI Code Review
on: [pull_request]
jobs:
  review:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
        with:
          fetch-depth: 0
      - name: Install code-review
        run: cargo install --path .
      - name: Run review
        run: |
          code-review analyze \
            --repo . \
            --git origin/main...HEAD \
            --mode standard \
            --format json > review.json
          exit_code=$?
          echo "exit_code=$exit_code" >> $GITHUB_OUTPUT
        env:
          COPILOT_GITHUB_TOKEN: ${{ secrets.COPILOT_TOKEN }}
      - name: Post results
        if: always()
        run: code-review pr-review --repo ${{ github.repository }} --pr ${{ github.event.pull_request.number }}
        env:
          GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
```

### C3. Review Result Markdown Renderer

New file: `src/review_render_md.rs`

Renders ReviewResult as GitHub-flavored Markdown suitable for PR comments:

```markdown
## AI Code Review — Standard Mode

**Score**: 75/100 | **Confidence**: medium | **Needs human review**: yes

### High Risk (1)
| File | Issue | Impact |
|------|-------|--------|
| `src/order/service.rs:create_order` | Missing idempotency check | Duplicate orders on retry |

> **Trigger**: Concurrent retry  
> **Suggestion**: Add unique constraint on order_key

### Medium Risk (0)
None

### Missing Tests
- Concurrent retry scenario
- Duplicate submission edge case

### Summary
Found 1 high-risk issue. Manual review recommended before merge.

---
*Generated by [code-review](https://github.com/user/code-review) v0.4*
```

### C4. Deliverables

- [ ] `src/github.rs` — GitHub API client (PR info, diff, post comment)
- [ ] `pr-review` CLI command
- [ ] `src/review_render_md.rs` — Markdown renderer for PR comments
- [ ] GitHub Actions workflow template
- [ ] GitLab CI template (follow-up)

---

## 7. Phase D: Web UI + Dashboard

### D1. Tech Stack

```
Frontend:  React + Tailwind + shadcn/ui (or plain HTML + htmx for simplicity)
Backend:   Existing axum API + new page routes
Bundling:  Embed static assets in the binary via rust-embed
```

The goal is a **single binary** — `code-review serve` starts both API and Web UI.

### D2. Pages

```
/                     → Dashboard: recent reviews, risk trends, quick actions
/review/new           → Submit new review (form: repo path, git range, mode, options)
/review/:id           → View review result (structured, with expand/collapse per issue)
/review/:id/raw       → Raw model output
/history              → Review history list with filters
/history/trend        → Risk trend chart over time
/settings             → Config editor (model, Jira, defaults)
```

### D3. Dashboard Metrics

```
┌────────────────────────────────────────────────────┐
│  Recent Reviews          Risk Trend (30d)          │
│  ┌──────────────────┐    ┌──────────────────────┐  │
│  │ #12 main~1 ✅ 0  │    │  ███                 │  │
│  │ #11 main~2 ⚠️ 2  │    │  ██████              │  │
│  │ #10 feat-x ❌ 5  │    │  ████                 │  │
│  │ #9  main~3 ✅ 0  │    │  ██                   │  │
│  └──────────────────┘    └──────────────────────┘  │
│                                                    │
│  Quick Actions                                     │
│  [Review HEAD~1..HEAD]  [Review PR #...]           │
└────────────────────────────────────────────────────┘
```

### D4. Deliverables

- [ ] `src/web/` — axum routes serving embedded static files
- [ ] Frontend app (React or htmx)
- [ ] Dashboard page with recent reviews + trend chart
- [ ] Review detail page with structured result view
- [ ] New review submission form
- [ ] Settings page
- [ ] New dependency: `rust-embed` (or `include_dir`)

---

## 8. Phase E: Smart Analysis Enhancements

### E1. Tree-sitter Based Expansion

**Problem**: Current dependency expansion is heuristic (file name patterns, regex import extraction). It misses indirect dependencies and produces false positives.

**Solution**: Use tree-sitter for language-aware parsing.

```rust
// New file: src/treesitter.rs
pub fn extract_imports(path: &str, content: &str) -> Vec<ImportRef>;
pub fn extract_definitions(path: &str, content: &str) -> Vec<SymbolDef>;
pub fn extract_call_sites(path: &str, content: &str) -> Vec<CallSite>;
```

Benefits:
- Exact import resolution (not regex guessing)
- Actual call graph (not string matching)
- Correct symbol definitions (not "file has fn and mentions name")
- Works across languages via grammar swapping

**Language grammars to support first**: Rust, TypeScript, Java, Go, Python.

### E2. Lightweight Static Checks

**Problem**: Engineering layer is pure prompt. No program-level detection of architecture violations.

**Solution**: Add configurable rule engine for the engineering layer.

New file: `src/rules.rs`

```toml
# .code-review-rules.toml (project root)

[[rules]]
name = "no-handler-to-repo"
description = "Handler should not directly call repository"
pattern = "handler/*.rs imports repo/*.rs"
severity = "medium"
layer = "engineering"

[[rules]]
name = "no-raw-sql-in-handler"
description = "Raw SQL should not appear in handler layer"
pattern = "handler/**/*.rs contains 'sqlx::query'"
severity = "high"
layer = "engineering"
```

These rules produce findings BEFORE the LLM call, and get injected into the prompt as "confirmed violations" for the model to validate and explain.

### E3. Context Caching

**Problem**: Every review re-reads all context files from disk, re-extracts symbols, re-resolves imports.

**Solution**: Cache extracted metadata per file (keyed by path + mtime + size).

```rust
// New file: src/cache.rs
pub struct ContextCache {
    db: sled::Db,  // or simple JSON file
}

impl ContextCache {
    pub fn get_symbols(&self, path: &str, mtime: u64) -> Option<Vec<String>>;
    pub fn set_symbols(&self, path: &str, mtime: u64, symbols: Vec<String>);
    pub fn get_imports(&self, path: &str, mtime: u64) -> Option<Vec<String>>;
    // ...
}
```

This makes repeated reviews of the same repo much faster — only changed files need re-analysis.

### E4. Deliverables

- [ ] `src/treesitter.rs` — tree-sitter based import/definition/call extraction
- [ ] `src/rules.rs` — configurable engineering rule engine
- [ ] `.code-review-rules.toml` project-level config
- [ ] `src/cache.rs` — context metadata cache
- [ ] New dependencies: `tree-sitter`, `tree-sitter-rust`, `tree-sitter-typescript`, etc.

---

## 9. Priority & Dependency Map

```
Phase A ──────────────────────────────────────→ MUST DO FIRST
  A1 JSON structured output                    (biggest reliability win)
  A2 Multi-provider LLM                        (biggest adoption win)
  A3 Layer-tagged issues                       (free once A1 is done)
         │
         ▼
Phase B ──────────────────────────────────────→ PLATFORM BASICS
  B1 Async tasks                               (unblocks web usage)
  B2 Review history                            (unblocks trends/CI value)
         │
         ▼
Phase C ──────────────────────────────────────→ TEAM ADOPTION
  C1 GitHub PR integration                     (biggest team value)
  C2 GitHub Actions template                   (CI gate)
  C3 Markdown renderer                         (dependency of C1)
         │
         ▼
Phase D ──────────────────────────────────────→ VISIBILITY
  D1 Web UI with embedded frontend
  D2 Dashboard + trend charts
         │
         ▼
Phase E ──────────────────────────────────────→ PRECISION
  E1 Tree-sitter expansion                     (can be done in parallel)
  E2 Rule engine                               (can be done in parallel)
  E3 Context caching                           (can be done in parallel)
```

Phase E items are independent and can be done in parallel with B/C/D.

---

## 10. Suggested Sprint Plan

### Sprint 1 (2 weeks): A1 + A2-partial
- Define `LlmProvider` trait
- Move copilot logic into `CopilotCliProvider`
- Add `ClaudeApiProvider` with tool_use structured output
- Define JSON schema for ReviewResult
- Wire structured output through service layer
- Keep `review_parser.rs` as fallback for text providers

### Sprint 2 (1 week): A2-complete + A3
- Add `OpenAiApiProvider`
- Add `layer` field to ReviewIssue
- Update review_render and review_validate
- Update config.toml documentation

### Sprint 3 (2 weeks): B1 + B2
- Implement TaskManager with DashMap
- Add async API routes
- Add SQLite history storage
- Add `history` CLI commands
- Auto-save reviews to history

### Sprint 4 (2 weeks): C1 + C2 + C3
- Implement GitHub API client
- Add `pr-review` command
- Add Markdown renderer
- Write GitHub Actions workflow template
- Test end-to-end with a real PR

### Sprint 5 (2-3 weeks): D1 + D2
- Choose frontend approach (React or htmx)
- Build dashboard page
- Build review detail page
- Embed static assets in binary
- Test `code-review serve` with full UI

### Ongoing: E1-E3
- Tree-sitter integration (start with Rust + TypeScript)
- Rule engine MVP
- Context caching

---

## 11. Breaking Changes to Plan For

| Change | Migration |
|--------|-----------|
| `[llm] provider` config field | Default to "copilot" for backward compat |
| ReviewIssue gains `layer` field | Optional field, old JSON still valid |
| API returns `task_id` instead of direct result | Add `?sync=true` query param for backward compat |
| History DB created on first run | Auto-migration, no user action needed |
| `review` command with critical mode | Already blocked in current code, no change |

---

## 12. New Dependencies Summary

| Phase | Crate | Purpose |
|-------|-------|---------|
| A | `reqwest` (already have) | Claude/OpenAI API calls |
| B | `rusqlite` | Review history storage |
| B | `dashmap` | Concurrent task map |
| B | `uuid` | Task IDs |
| D | `rust-embed` | Embed frontend assets |
| E | `tree-sitter` + grammars | Language-aware parsing |

---

## 13. One-Line Summary

The next phase transforms `code-review` from a **local prompt-building CLI** into a **multi-provider, async, CI-integrated review platform** — with structured model output as the foundation, PR integration as the adoption driver, and a web dashboard for team visibility.
