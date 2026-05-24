# Feature Flags Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add project-scoped feature flags with local SDK evaluation and 30-second refresh.

**Architecture:** ClickHouse stores flag definitions. Rust backend caches active flags and serves them through a project-token endpoint. The JS SDK periodically fetches active flags and evaluates conditions plus sticky percentage rollout locally.

**Tech Stack:** Rust/Axum/ClickHouse, TypeScript SDK, Node built-in test runner.

---

### Task 1: Backend Feature Flags Storage And Endpoint

**Files:**
- Create: `backend/src/feature_flags.rs`
- Create: `backend/src/api/feature_flags.rs`
- Create: `clickhouse/init/88-feature-flags.sql`
- Create: `clickhouse/migrations/017-feature-flags.sql`
- Modify: `backend/src/lib.rs`
- Modify: `backend/src/main.rs`
- Modify: `backend/src/state.rs`
- Modify: `backend/src/api/mod.rs`
- Modify: `backend/src/storage/models.rs`
- Test: `backend/tests/feature_flags.rs`

- [ ] Write failing backend integration test for `GET /api/v1/ingest/feature-flags`.
- [ ] Add ClickHouse schema and storage row.
- [ ] Add `FeatureFlagsCache` with `reload`, `flags_for_project`, and 30-second refresh.
- [ ] Add ingest-authenticated endpoint returning active flags for the resolved project.
- [ ] Wire cache into `AppState`, boot reload, and periodic refresh.
- [ ] Run the backend feature flag test and relevant compile checks.

### Task 2: JS SDK Local Evaluation

**Files:**
- Modify: `sdks/node/src/index.ts`
- Modify: `sdks/node/test/client.test.mjs`

- [ ] Write failing SDK tests for `isFeatureEnabled`, property conditions, sticky rollout, and fetch refresh.
- [ ] Add feature flag types and SDK options for refresh interval.
- [ ] Add automatic flag refresh and explicit `refreshFeatureFlags`.
- [ ] Add synchronous local evaluator using cached flags.
- [ ] Run SDK build and tests.

### Task 3: Documentation Touches

**Files:**
- Modify: `sdks/node/README.md`

- [ ] Document the `isFeatureEnabled` API with a short example.
- [ ] Document that browser-visible flag rules are not secrets.
- [ ] Run docs-adjacent lint only if the repo already exposes a targeted command.
