# 10.B SDK Tracking API Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close the Segment/PostHog-style tracking API contract across the Faro SDKs with focused tests and minimal fixes.

**Architecture:** Keep each SDK's existing single-file/client pattern. Product events use the dedicated events queue and `POST /api/v1/ingest/events`; logs keep their current queue and endpoint. Tests validate public methods by capturing real HTTP payloads.

**Tech Stack:** TypeScript SDKs with `node --test` and local HTTP servers; Dart/Flutter with `flutter_test`; Kotlin/JUnit with JDK `HttpServer`; Go/Python suites are already covered unless inspection finds gaps.

---

### Task 1: Next.js Browser Tracking Tests

**Files:**

- Modify: `sdks/nextjs/test/browser.test.mjs`
- Verify: `sdks/nextjs/src/browser-core.ts`

- [ ] **Step 1: Write failing tests for `track`, `identify`, `page`, and `alias`**

Add tests that initialize `initFaroClient(commonOpts(port))`, call the public tracking methods, `await c.flush()`, and assert captured `/api/v1/ingest/events` payloads.

- [ ] **Step 2: Run the focused suite to verify failures or existing pass**

Run: `cd sdks/nextjs; npm test`

Expected before fixes: tests either fail on missing contract details or pass if implementation is already complete.

- [ ] **Step 3: Implement minimal fixes if any test fails**

Keep changes inside `sdks/nextjs/src/browser-core.ts` or `sdks/nextjs/src/client.ts`. Do not add `screen` to Next.js.

- [ ] **Step 4: Re-run `npm test`**

Expected: all Next.js SDK tests pass.

### Task 2: Expo Mobile Tracking Tests

**Files:**

- Modify: `sdks/expo/test/client.test.mjs`
- Verify: `sdks/expo/src/index.ts`

- [ ] **Step 1: Write failing tests for `track`, `identify`, `screen`, and `alias`**

Use the existing local HTTP server. Assert events endpoint payloads have source `mobile`, `screen` emits `type: "screen"`, and `identify`/`alias` update `distinct_id`.

- [ ] **Step 2: Run the focused suite**

Run: `cd sdks/expo; npm test`

Expected before fixes: tests either fail on missing contract details or pass if implementation is already complete.

- [ ] **Step 3: Implement minimal fixes if any test fails**

Keep changes inside `sdks/expo/src/index.ts`. Do not add `page` to Expo.

- [ ] **Step 4: Re-run `npm test`**

Expected: all Expo SDK tests pass.

### Task 3: Flutter And Kotlin Coverage Check

**Files:**

- Modify if needed: `sdks/flutter/test/faro_test.dart`
- Modify if needed: `sdks/kotlin/src/test/kotlin/com/iaportafolio/faro/FaroTest.kt`
- Verify: `sdks/flutter/lib/faro_sdk.dart`
- Verify: `sdks/kotlin/src/main/kotlin/com/iaportafolio/faro/Faro.kt`

- [ ] **Step 1: Inspect existing product-event tests**

Confirm whether `track`, `identify`, `page`/`screen`, and `alias` already have payload assertions.

- [ ] **Step 2: Add missing tests before production fixes**

For Flutter, assert both `page` and `screen`. For Kotlin, assert `screen`. Both should assert `track`, `identify`, and `alias` when missing.

- [ ] **Step 3: Run SDK tests when local toolchains are available**

Run: `cd sdks/flutter; flutter test`

Run: `cd sdks/kotlin; ./gradlew test`

Expected: all tests pass or toolchain absence is reported explicitly.

### Task 4: Final Contract Verification

**Files:**

- Read: `sdks/README.md`
- Read: SDK README files if touched

- [ ] **Step 1: Compare docs against implemented methods**

Confirm availability table matches public exports.

- [ ] **Step 2: Run touched SDK suites**

Run every suite for SDKs modified in this pass.

- [ ] **Step 3: Summarize verification**

Report exact commands and whether they passed, failed, or were blocked by missing toolchains.
