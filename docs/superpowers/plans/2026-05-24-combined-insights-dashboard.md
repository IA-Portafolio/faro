# Combined Insights Dashboard Implementation Plan

## Objective

Add `/insights`, a combined observability/product analytics dashboard that answers:
"Which backend errors and latency are breaking this product journey?"

## Steps

1. Add backend endpoint `GET /api/v1/insights/service-dashboard`.
2. Add frontend API types/client.
3. Add pure frontend helpers plus tests for narrative, rate formatting, and links.
4. Build `/insights` route with controls and a compact dashboard.
5. Add sidebar and command palette navigation.
6. Verify with frontend tests/build/check and backend test attempt.

## Files

- Modify: `backend/src/api/insights.rs`
- Modify: `frontend/src/lib/api.ts`
- Create: `frontend/src/lib/insights.ts`
- Create: `frontend/src/lib/insights.test.ts`
- Create: `frontend/src/routes/insights/+page.svelte`
- Modify: `frontend/src/lib/components/Sidebar.svelte`
- Modify: `frontend/src/lib/palette.ts`
- Modify: `frontend/src/lib/palette.test.ts`

## Verification

- `npm test`
- `npm run build`
- `npm run check`
- `cargo test api::insights`

Expected workspace caveat: local Windows PATH may not include `cargo`; `svelte-check`
currently has known baseline errors unrelated to `/insights`.
