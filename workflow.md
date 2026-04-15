# code-review Architecture & Workflow

## Architecture Overview

```
┌─────────────────────────────────────────────────────────────┐
│                         CLI / HTTP API                       │
│  main.rs → lib.rs (CLI dispatch)    api.rs (axum routes)    │
└──────────────┬──────────────────────────┬───────────────────┘
               │                          │
               ▼                          ▼
┌─────────────────────────────────────────────────────────────┐
│                    Service Layer                              │
│              services/review_service.rs                       │
│  execute_prompt / execute_run / execute_review /             │
│  execute_deep_review / execute_analyze / execute_validate    │
└──┬──────┬──────┬──────┬──────┬──────┬──────┬───────────────┘
   │      │      │      │      │      │      │
   ▼      ▼      ▼      ▼      ▼      ▼      ▼
┌──────┐┌──────┐┌──────┐┌──────┐┌──────┐┌──────┐┌──────────┐
│admit ││prompt││parser││valid ││risk  ││layers││copilot   │
│.rs   ││.rs   ││.rs   ││.rs   ││.rs   ││.rs   ││.rs       │
└──────┘└──────┘└──────┘└──────┘└──────┘└──────┘└──────────┘
   ▲      ▲                                        │
   │      │                                        ▼
┌──────┐┌──────┐┌──────┐┌──────┐          ┌──────────────┐
│config││jira  ││contxt││expand│          │ copilot CLI  │
│.rs   ││.rs   ││.rs   ││.rs   │          │ (external)   │
└──────┘└──────┘└──────┘└──────┘          └──────────────┘
```

## Core Flow: `analyze` (recommended entry point)

```
1. CLI parses args + loads config
2. execute_run():
   ├── git diff → get diff text + changed file list
   ├── git ls-files → get all repo files
   ├── enrich from Jira (if --jira provided)
   ├── expand_related_files() → find tests, DTOs, schemas nearby
   ├── expand_dependency_files() → trace imports, symbols, backend/frontend chains
   ├── read context files within budget (skip binary/non-utf8/oversized)
   ├── check_admission() → P0/P1/P2 gate (BLOCKS if fails)
   └── build_prompt_from_sources() → assemble structured prompt
3. Strategy routing:
   ├── Standard → execute_review() → single copilot call
   └── Deep (forced for Critical) → execute_deep_review()
       ├── Stage 1: full review with expanded context
       ├── Extract high-risk files + uncertain points from stage 1
       ├── Stage 2: targeted review with extra focus files
       └── Repair loop if output validation fails
4. Output: structured ReviewResult (JSON or text)
```

## Key Modules

| Module | Role |
|--------|------|
| **admission.rs** | Gate keeper. Blocks review if P0 missing (standard/critical), requires P2 for critical. Returns Pass/Warn/Block + confidence. |
| **review_layers.rs** | Defines 4-layer checklist (Basic/Engineering/Business/Risk). Dynamic per mode and change_type. Injected into prompt. |
| **prompt.rs** | Assembles the final prompt from: role instruction + context + layers + risk hints + output constraints + diff + files. |
| **copilot.rs** | Calls local `copilot --no-ask-user -p "..."`. Handles auth probe, timeout (90s), large prompts via @file. |
| **review_parser.rs** | Parses model text output into structured `ReviewResult`. Section detection → issue field extraction → critical backfill. |
| **review_validate.rs** | Validates parsed result: summary not empty, issues have required fields, critical has impact_scope + release_checks. Auto-repairs missing fields. |
| **risk.rs** | Program-level risk analysis from file paths + diff keywords. Detects DB migration, API contract, auth, cross-layer risks. |
| **expand.rs** | Dependency expansion: related files (tests/DTOs/schemas), symbol definitions, import chains, backend/frontend architecture chains. Prioritized by signal quality. |
| **context.rs** | Reads files within budget. Skips binary/non-utf8. Truncates at UTF-8 boundaries. |
| **jira.rs** | Enriches prompt args from Jira: fills goal, rules, expected behavior, change_type, risks, focus. Native API or external command. |
| **config.rs** | TOML config at `~/.config/code-review/config.toml`. Default model, Jira settings, review mode, context budget. |
| **session.rs** | Session storage at `~/.config/code-review/session.json`. File-locked, atomic writes, token sanitization. |
| **api.rs** | Axum HTTP server. All endpoints delegate to service layer. Differentiated error codes (401/409/422/404/500). |

## Three Modes

| | Lite | Standard | Critical |
|---|------|----------|----------|
| **P0 required** | diff only | diff + goal + rules | diff + goal + rules |
| **P1 required** | no | no (warns if >2 missing) | at least 2 |
| **P2 required** | no | no | yes (baseline/incident/focus/jira) |
| **Review stages** | 1 | 1 | 2 (forced) |
| **Output extras** | - | - | impact_scope + release_checks |
| **Confidence** | low if P0 missing | medium if P1 thin | high only if all present |

## Four Review Layers

Each review prompt includes a structured checklist generated from `review_layers.rs`:

1. **Basic Layer** — Null/unwrap/bounds, exception handling, resource leaks, concurrency races
2. **Engineering Layer** — Architecture violations, cross-layer calls, maintainability, perf hazards
3. **Business Layer** — Requirement drift, rule coverage, auth/state/idempotency checks (uses provided rules as reference)
4. **Risk Layer** — Impact scope, API contract, DB migration, compatibility, rollback (dynamic per `--type`)

Change type (`--type server|db|frontend|infra|contract|api`) adds targeted checks to the Risk layer.

## Admission Gate (P0/P1/P2)

```
P0 (must provide):
  - diff (from git or --diff-file)
  - goal (--goal or auto from Jira summary)
  - rules (--rule or auto from Jira acceptance)

P1 (strongly recommended):
  - expected behavior (--expected-normal / --expected-error / --expected-edge)
  - stack (--stack)
  - context files (auto-expanded or --context-file)
  - issue description (--issue)
  - test results (--test-result)

P2 (required for critical):
  - baseline files (--baseline-file)
  - incident files (--incident-file)
  - focus points (--focus)
  - jira enrichment (--jira)
```

Enforcement:
- **Lite**: only diff required; missing P0 → low confidence warning
- **Standard**: all P0 required or BLOCK; >2 P1 missing → warn
- **Critical**: all P0 + ≥2 P1 + ≥1 P2 required or BLOCK

## Dependency Expansion

When `--include-context` is enabled (default for analyze/deep-review), the system automatically expands context beyond the raw diff:

1. **Related files** (`expand_related_files`): tests, DTOs, schemas, contracts, high-value neighbors in same directory
2. **Dependency files** (`expand_dependency_files`): prioritized by signal quality:
   - `import-chain` — files imported/required by changed files
   - `backend-chain` — controller↔service↔repository↔entity↔dto
   - `frontend-chain` — component↔store↔hook↔composable↔api
   - `route-chain` — handler↔service↔repo↔dto
   - `reference` — files that call symbols defined in changed files
   - `symbol` — files that define symbols used in changed files

All expansion is budget-controlled (default 48KB total, 12KB per file).

## Structured Output Pipeline

```
Model raw text
  → review_parser.rs: section detection + field extraction
  → review_schema.rs: ReviewResult struct (high/med/low risk, tests, summary, ...)
  → risk.rs: program-level risk hints injected
  → review_validate.rs: field completeness check + auto-repair
  → If validation fails: one repair attempt via copilot
  → review_render.rs: text output  /  serde_json: JSON output
```

ReviewResult always includes:
- `mode`, `input_ok`, `input_score`, `confidence`
- `high_risk`, `medium_risk`, `low_risk` (each with file/location/reason/trigger/impact/suggestion)
- `missing_tests`, `summary`, `needs_human_review`, `used_rules`
- `impact_scope`, `release_checks`, `risk_hints` (critical mode / risk analyzer)
- `validation_report`, `repair_attempted`, `repair_succeeded`
- `raw_text` (original model output preserved)

## Exit Codes

| Code | Meaning |
|------|---------|
| 0 | Success, no high-risk issues |
| 2 | Success, but high-risk or needs human review |
| 3 | Admission blocked (missing required context) |
| 4 | Output validation failed after repair attempt |
| 5 | Runtime error |

## HTTP API

All endpoints delegate to the same service layer as CLI:

| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/health` | Health check |
| GET | `/api/models` | List available models |
| POST | `/api/validate` | Admission check only |
| POST | `/api/prompt` | Generate review prompt |
| POST | `/api/assemble` | Preview Jira enrichment |
| POST | `/api/run` | Generate prompt from git diff |
| POST | `/api/analyze` | Full pipeline: admission → prompt → review → validate |
| POST | `/api/review` | Single-stage review (blocks critical mode) |
| POST | `/api/deep-review` | Two-stage review |

Error status codes: 400 bad input, 401 auth required, 409 mode conflict, 422 admission blocked, 404 not found, 500 internal.

## Config

`~/.config/code-review/config.toml`:

```toml
[llm]
provider = "copilot"
model = "gpt-5.4"

[jira]
provider = "native"
base_url = "https://your-company.atlassian.net"

[review]
mode = "standard"
include_context = true
context_budget_bytes = 48000
context_file_max_bytes = 12000
stack = "Rust + Axum + PostgreSQL"
output_format = "text"
```

## Auth Flow

Authentication is entirely delegated to the local `copilot` CLI:

- Login: `copilot login`
- Probe: `copilot --no-ask-user -p "reply with exactly OK"` (20s timeout)
- Review: `copilot --no-ask-user -p "..."` (90s timeout, @file for large prompts)
- Session metadata stored at `~/.config/code-review/session.json` (tokens are never stored in plaintext)
