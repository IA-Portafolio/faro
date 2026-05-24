# Product Sessions Replay Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build `/sessions` with recent product sessions, replay availability, pageviews, duration, error count, and click-through to rrweb replay.

**Architecture:** Add one backend endpoint over `product_sessions`, enriched with grouped replay chunks and error sessions. Add frontend API types, pure helper functions, a Svelte route, and navigation entries.

**Tech Stack:** Rust/Axum/ClickHouse backend, Svelte 5 frontend, Vitest for helper tests.

---

## Tasks

### Task 1: Frontend Helpers

- [ ] Write `frontend/src/lib/sessions.test.ts` first for duration formatting, hrefs, and session health.
- [ ] Run `cd frontend; npm test -- src/lib/sessions.test.ts` and verify it fails because `sessions.ts` does not exist.
- [ ] Add `frontend/src/lib/sessions.ts`.
- [ ] Re-run helper tests and commit.

### Task 2: Backend Endpoint

- [ ] Create `backend/src/api/sessions.rs` with unit tests for bool parsing.
- [ ] Try `cd backend; cargo test api::sessions`; document if `cargo` is unavailable.
- [ ] Implement `GET /api/v1/sessions` with parameterized SQL.
- [ ] Register it in `backend/src/api/mod.rs`.
- [ ] Commit.

### Task 3: Frontend API

- [ ] Add `ProductSessionSummary`, `ProductSessionFilters`, and `fetchProductSessions` to `frontend/src/lib/api.ts`.
- [ ] Run frontend tests and commit.

### Task 4: `/sessions` Route

- [ ] Create `frontend/src/routes/sessions/+page.svelte`.
- [ ] Render cards, filters, table rows, replay links, user links, and events links.
- [ ] Run `cd frontend; npm run build` and commit.

### Task 5: Navigation

- [ ] Add `nav.sessions` failing assertion to `frontend/src/lib/palette.test.ts`.
- [ ] Run the palette test and verify failure.
- [ ] Add Sidebar and command palette entries.
- [ ] Run the palette test and commit.

### Task 6: Verification

- [ ] Run `cd frontend; npm test`.
- [ ] Run `cd frontend; npm run build`.
- [ ] Run `cd frontend; npm run check`.
- [ ] Try `cd backend; cargo test api::sessions`.
- [ ] Report pre-existing or toolchain failures clearly.
