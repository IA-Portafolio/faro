# 10.B.3 Events Ingest Endpoint Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make `POST /api/v1/ingest/events` accept the new `{ batch: [...] }` contract, validate events, redact payload JSON, and preserve legacy SDK compatibility.

**Architecture:** Keep `backend/src/ingest/events.rs` as the canonical handler. Add parsing helpers for `batch/events`, validation helpers for event names and JSON size, and focused unit tests around normalization/redaction so the behavior is fast to verify without ClickHouse.

**Tech Stack:** Rust, Axum, serde, serde_json, existing Faro `ApiError`, existing `ProductEventRow`.

---

### Task 1: Failing Helper Tests

**Files:**

- Modify: `backend/src/ingest/events.rs`

- [ ] Add unit tests for new `batch` envelope, legacy `events` envelope, event-name validation, 16KB properties limit, and redaction scope.
- [ ] Run `cargo test ingest::events --lib`.
- [ ] Confirm tests fail because helpers/behavior do not exist yet.

### Task 2: Implement Parsing And Validation

**Files:**

- Modify: `backend/src/ingest/events.rs`

- [ ] Change `IngestPayload` to accept `batch` and `events`.
- [ ] Add `events_batch()` that returns an error when both are absent.
- [ ] Add `event` field as canonical name and keep `name` as legacy alias.
- [ ] Add slug-like validation and 16KB `properties` JSON limit.
- [ ] Use validation from the ingest loop before enqueuing rows.

### Task 3: Redaction Scope And Verification

**Files:**

- Modify: `backend/src/ingest/events.rs`
- Optional docs: `sdks/README.md`

- [ ] Restrict event redaction to `properties`, `user_properties`, and `context`.
- [ ] Run `cargo test ingest::events --lib`.
- [ ] Run broader backend tests only if local services/tooling are available.
