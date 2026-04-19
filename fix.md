# Design Fix Plan

Ordered by risk/value ratio. Phase 1 is safe, local refactoring that can be verified with `cargo build` + existing tests. Phase 2 changes touch many files and will be done behind a clear plan but in separate commits.

## Phase 1 — low-risk, high-value (do now)

### F1. Remove magic-number config sentinels
**Problem:** `lib.rs:48,51` detects "user didn't set this" via `== 48_000` / `== 12_000`. Any user who explicitly passes the same value gets their flag silently overridden.

**Fix:**
- Change `RunArgs.context_budget_bytes` / `context_file_max_bytes` and the same fields on `AnalyzeArgs` / `DeepReviewArgs` from `usize` with `default_value_t = ...` to `Option<usize>` (no clap default).
- Introduce const defaults `DEFAULT_CONTEXT_BUDGET_BYTES = 48_000`, `DEFAULT_CONTEXT_FILE_MAX_BYTES = 12_000` in `config`.
- `apply_context_config` becomes: if CLI `None`, use config; if config `None`, use const default. Caller downstream reads resolved `usize`.

**Files:** `src/cli.rs`, `src/lib.rs`, `src/config.rs`, `src/services/review_service.rs` (sites that read these fields).

### F2. Unique IDs
**Problem:** `orchestrator.rs:514` uses nanosecond timestamps as IDs. In a single call stack, `build_findings_from_result` emits many findings — collisions possible; also not globally unique.

**Fix:** Add `uuid = { version = "1", features = ["v4"] }` to Cargo.toml. Replace `new_id(prefix)` with `format!("{}-{}", prefix, Uuid::new_v4().simple())`.

**Files:** `Cargo.toml`, `src/orchestrator.rs`.

### F3. Deduplicate repair retry logic
**Problem:** `execute_review` (review_service.rs:257-283) and `execute_deep_review` (review_service.rs:374-390) share near-identical repair-retry logic.

**Fix:** Extract `run_with_repair(store, prompt, mode, model, prompt_args, changed_files, diff) -> ReviewResult` inside `review_service`. Both paths call it.

**Files:** `src/services/review_service.rs`.

### F4. CORS gated behind config
**Problem:** `api.rs:63` applies `CorsLayer::permissive()` unconditionally.

**Fix:** Add `api.cors_permissive: Option<bool>` to `AppConfig` (default `false`). Only attach `CorsLayer::permissive()` when enabled; otherwise no CORS layer.

**Files:** `src/config.rs`, `src/api.rs`.

## Phase 2 — larger refactors (separate PRs, deferred)

### F5. Unify review pipeline
Currently `services/review_service` and `orchestrator` each build their own review flow. Extract a `ReviewPipeline` that owns: prompt build → LLM call → parse → risk analyze → validate → repair. Both the one-shot CLI and the multi-turn session should drive it. Deferred — mechanical but large; do after Phase 1 lands.

### F6. Async provider trait
`LlmProvider::chat` should be `async fn` (via `async-trait` or native AFIT). The `api.rs` `spawn_blocking` wrappers exist only because the core is sync. Invert: core is async, sync CLI callers use `Runtime::block_on`. Deferred — large; touches all commands.

### F7. Use `LlmProvider` throughout
`review_service` currently calls `copilot::run_review` directly, bypassing the trait. After F6, take `&dyn LlmProvider` in `execute_review` / `execute_deep_review`. Deferred with F6.

### F8. Sum type for `AnalyzeExecution`
Replace `{review: Option, stage1: Option, stage2: Option}` with `enum AnalyzeOutcome { Standard(ReviewResult), Deep{stage1, stage2} }`. Low-risk but touches serialization — defer.

### F9. Better stage-2 focus extraction
Replace brittle regex (`review_service.rs:585`) with structured output: ask the LLM for a JSON block listing focus files/symbols, fall back to regex. Deferred — requires prompt tweaks.

### F10. Finding identity
`orchestrator.rs:406` uses title+file equality. Replace with a stable content hash (normalized title + file + line_range) or an LLM-assigned `code`. Deferred.

## Verification

After each Phase 1 item:
1. `cargo build` clean.
2. `cargo test` passes (existing tests in `lib.rs`, `config.rs`).
3. Grep for old field usages to confirm no stale references.
