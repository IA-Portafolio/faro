# A/B Testing Stats Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add automatic A/B experiment analysis for feature-flag exposures and conversion events.

**Architecture:** SDKs emit `$feature_exposure` into existing product events. The backend adds an authenticated analysis endpoint that joins exposures to conversions and computes frequentist two-proportion stats. The frontend adds an Experiments page that renders the lift, p-value, CI, and variant rows.

**Tech Stack:** Rust/Axum/ClickHouse, TypeScript SDKs, SvelteKit frontend.

---

### Task 1: SDK Feature Exposure Events

**Files:**
- Modify: `sdks/node/src/index.ts`
- Modify: `sdks/node/test/client.test.mjs`
- Modify: `sdks/nextjs/src/browser-core.ts`
- Modify: `sdks/nextjs/test/browser.test.mjs`

- [x] Write failing tests proving `isFeatureEnabled()` emits one `$feature_exposure` per `(flag, distinct_id, variant)`.
- [x] Implement exposure de-duplication and enqueue product events with `flag_key`, `variant`, and `enabled`.
- [x] Run Node and Next SDK tests.

### Task 2: Backend Experiment Analysis

**Files:**
- Create: `backend/src/api/experiments.rs`
- Modify: `backend/src/api/mod.rs`
- Test: `backend/src/api/experiments.rs` unit tests

- [x] Write failing unit tests for lift, p-value, and 95% CI math.
- [x] Implement stats helpers.
- [x] Implement `POST /api/v1/experiments/analyze`.
- [x] Wire the router into `/api/v1`.

### Task 3: Frontend Experiments UI

**Files:**
- Modify: `frontend/src/lib/api.ts`
- Create: `frontend/src/routes/experiments/+page.svelte`
- Modify: `frontend/src/lib/components/Sidebar.svelte`

- [x] Add API types and `analyzeExperiment`.
- [x] Build `/experiments` page with inputs and result summary.
- [x] Add sidebar navigation.
- [x] Run frontend checks/build.

Verification notes:
- `sdks/node`: `npm test` passed 19/19.
- `sdks/nextjs`: `npm test` passed 22/22.
- `frontend`: `npm run build` passed; `npm run check` is blocked by existing type/config errors outside the experiments page.
- `backend`: Rust tests could not run in this shell because `cargo` is not installed/on PATH.
