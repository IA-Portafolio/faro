# Product Retention Heatmap Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build `/retention` with D1/D7/D30 product-user cohort retention.

**Architecture:** Add one backend query endpoint over `faro.product_events`, one frontend API client section, a pure helper module for heatmap math, and a Svelte route. Navigation is exposed through Sidebar and command palette.

**Tech Stack:** Rust/Axum/ClickHouse backend, Svelte 5 frontend, Vitest for frontend helpers, Rust unit tests for backend pure helpers.

---

## File Structure

- Create `backend/src/api/retention.rs`: endpoint, query params, response structs, SQL construction, pure helper validation.
- Modify `backend/src/api/mod.rs`: register module and router.
- Modify `frontend/src/lib/api.ts`: retention types and `fetchRetention`.
- Create `frontend/src/lib/retention.ts`: pure UI math helpers.
- Create `frontend/src/lib/retention.test.ts`: helper tests.
- Create `frontend/src/routes/retention/+page.svelte`: heatmap UI.
- Modify `frontend/src/lib/components/Sidebar.svelte`: sidebar link.
- Modify `frontend/src/lib/palette.ts`: command palette navigation.
- Modify `frontend/src/lib/palette.test.ts`: assert `nav.retention`.

## Tasks

### Task 1: Frontend Retention Helpers

- [ ] Write failing tests in `frontend/src/lib/retention.test.ts` for rate, maturity, weighted retention, and color.
- [ ] Run `cd frontend; npm test -- src/lib/retention.test.ts` and verify failure because helpers do not exist.
- [ ] Create `frontend/src/lib/retention.ts` with the tested helpers.
- [ ] Re-run the helper test and commit.

### Task 2: Backend Retention Endpoint

- [ ] Write failing Rust unit tests in `backend/src/api/retention.rs` for supported interval validation and row percentage helper.
- [ ] Run `cd backend; cargo test api::retention` and verify failure before registering production code.
- [ ] Implement `GET /api/v1/retention` with parameterized ClickHouse SQL and response structs.
- [ ] Register module in `backend/src/api/mod.rs`.
- [ ] Run `cd backend; cargo test api::retention` and commit.

### Task 3: API Client

- [ ] Add retention response types and `fetchRetention` to `frontend/src/lib/api.ts`.
- [ ] Run `cd frontend; npm test` and commit.

### Task 4: Route UI

- [ ] Create `frontend/src/routes/retention/+page.svelte`.
- [ ] Use `fetchFunnelEvents` for event options and `fetchRetention` for data.
- [ ] Render metric cards and heatmap table with mature/unavailable cell states.
- [ ] Run `cd frontend; npm run build` and commit.

### Task 5: Navigation

- [ ] Add sidebar link to `/retention`.
- [ ] Add `nav.retention` command.
- [ ] Update palette tests first, watch them fail, implement, then pass.
- [ ] Commit.

### Task 6: Verification

- [ ] Run `cd frontend; npm test`.
- [ ] Run `cd frontend; npm run build`.
- [ ] Run `cd frontend; npm run check`.
- [ ] Run `cd backend; cargo test api::retention`.
- [ ] Document any pre-existing unrelated failures.
