# Feature Flag Error Rollback Implementation Plan

**Goal:** Add automatic rollback recommendation alerts when feature flag treatment users see a 5x error-rate increase.

**Architecture:** A backend worker periodically scans recent feature exposures, joins treatment/control cohorts to linked backend errors through `trace_id`, and persists `feature-rollback:*` incidents in `faro.alert_incidents`.

## Task 1: Detector Worker

Files:
- Create: `backend/src/workers/feature_rollback_detector.rs`
- Modify: `backend/src/workers/mod.rs`

- [x] Build a ClickHouse query that returns per-project/per-flag A/B samples, errors, ratio, and top error service.
- [x] Add stable UUIDv5 rule ids for feature rollback incidents.
- [x] Add fire/resolve logic using active incident memory and startup recovery.
- [x] Add unit tests for ratio math, rule id determinism, and query shape.

## Task 2: Config and Startup

Files:
- Modify: `backend/src/config.rs`
- Modify: `backend/src/main.rs`

- [x] Add env config for enabled, interval, window, ratio, resolve ratio, minimum samples, and minimum treatment errors.
- [x] Start the worker with other background detectors.

## Task 3: Verification

- [x] Run Rust tests if toolchain is available.
- [x] Run frontend build/check only if frontend changed.
- [x] Document environment limitations.

Verification notes:
- Rust tests could not run in this shell because `cargo` is not installed/on PATH.
- `rustfmt` is also unavailable/on PATH.
- The env reference generator could not run because `/bin/bash` is missing; `docs/reference/environment.md` was updated manually for the new variables.
- No frontend files changed for this goal, so no frontend build was needed.
