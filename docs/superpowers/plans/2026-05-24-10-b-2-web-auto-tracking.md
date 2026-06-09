# 10.B.2 Web Auto Tracking Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add opt-in product-event auto tracking to the browser SDK.

**Architecture:** Extend `FaroBrowserOptions` with `autoCapture` and install browser listeners only when each option is enabled. Keep legacy breadcrumb capture flags unchanged. Emit product events through the existing `track()` and `page()` methods so identity, session, context, batching, and flush behavior stay shared.

**Tech Stack:** TypeScript browser core in `sdks/nextjs/src/browser-core.ts`; `node --test` browser-core tests with minimal DOM stubs in `sdks/nextjs/test/browser.test.mjs`.

---

### Task 1: Auto Capture Tests

**Files:**

- Modify: `sdks/nextjs/test/browser.test.mjs`

- [ ] Add DOM listener stubs for `window` and `document`.
- [ ] Add tests for opt-in page views, clicks, form submissions, rage clicks, and dead clicks.
- [ ] Run `cd sdks/nextjs; npm test` and verify the new tests fail before implementation.

### Task 2: Browser Core Implementation

**Files:**

- Modify: `sdks/nextjs/src/browser-core.ts`

- [ ] Add `autoCapture` option type.
- [ ] Resolve defaults to all false.
- [ ] Install page, click, submit, rage-click, and dead-click listeners only for enabled options.
- [ ] Use existing `page()` and `track()` emitters.
- [ ] Preserve legacy `captureClicks` and `captureNavigation` breadcrumb behavior.

### Task 3: Documentation And Verification

**Files:**

- Modify: `sdks/nextjs/README.md`
- Modify: `sdks/README.md`

- [ ] Document `autoCapture`.
- [ ] Run `cd sdks/nextjs; npm test`.
- [ ] Report exact verification outcome.
