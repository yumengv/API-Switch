# Web Admin Phase 2 & Phase 3 — Detailed Execution Plan

## Context

Phase 1 is 92% complete. Display consistency and core operation chains verified. Next: RuntimeMode governance (Phase 2) and business module Web migration (Phase 3).

DEV ports: 9098 = Proxy/API, 9099 = Web Admin.

## Architecture Summary

- **Services layer**: Only `channel_service.rs` exists — reference pattern for new services.
- **Commands layer**: `pool.rs`, `token.rs`, `usage.rs` contain business logic directly — need service extraction.
- **Admin layer**: HTTP endpoints in `admin/`; `AdminState` with optional `runtime` and `app_handle`.
- **Frontend adapter**: `ApiAdapter` interface with `channels` only — needs `pool`, `tokens`, `usage` namespaces.

## Dependency Graph

```
Wave 0 (Plan) ─→ Wave 1 (RuntimeMode + Service Extraction + Adapter Interface)
                        ↓
                 Wave 2 (Startup Refactor + HTTP Endpoints + Web Adapter)
                        ↓
                 Wave 3 (Shared Feature Components + Mode Badge)
                        ↓
                 Wave 4 (Route Wiring + Build Gates)
                        ↓
                 Wave 5 (Integration Verification)
```

---

## Wave 0 — Foundation

### T0.1: Plan File & Task Registration
- **What to do**: This plan file. Register tasks in tracking.
- **Files**: `.sisyphus/plans/web-admin-phase2-phase3-detailed.md`
- **Deps**: None
- **Agent**: `quick`
- **Effort**: S

---

## Wave 1 — RuntimeMode + Service Extraction + Adapter (6 parallel tasks)

### T1.1: Define `RuntimeMode` enum + `detect_runtime_mode()`
- **What to do**:
  1. Create `src-tauri/src/runtime_mode.rs` with `RuntimeMode` enum (Combined/Standalone) and `ModeSource` (Cli/Env/Auto).
  2. Implement `detect_runtime_mode()`: CLI args > env vars > auto-detect.
  3. Log mode at startup.
  4. Add `pub mod runtime_mode;` to `lib.rs`.
- **Files**: `src-tauri/src/runtime_mode.rs`, `src-tauri/src/lib.rs`
- **Deps**: T0.1
- **Agent**: `deep`
- **QA**: `cargo check` passes.
- **Effort**: M

### T1.2: Extract `pool_service.rs` from `commands/pool.rs`
- **What to do**:
  1. Create `src-tauri/src/services/pool_service.rs` with pure business logic functions.
  2. Move: `list_entries`, `toggle_entry`, `reorder_entries`, `delete_entry`, `create_entry`, `backfill_entry_catalog_meta`, `test_entry_latency`, `update_entry_response_ms`.
  3. Refactor `commands/pool.rs` to call service + add tray refresh.
  4. Add `pub mod pool_service;` to `services/mod.rs`.
- **Files**: `src-tauri/src/services/pool_service.rs`, `src-tauri/src/commands/pool.rs`, `src-tauri/src/services/mod.rs`
- **Deps**: T0.1
- **Agent**: `deep`
- **QA**: `cargo check` passes. Tauri pool commands unchanged.
- **Effort**: L

### T1.3: Extract `token_service.rs` from `commands/token.rs`
- **What to do**: Create `src-tauri/src/services/token_service.rs` with `list_access_keys`, `create_access_key`, `delete_access_key`, `toggle_access_key`. Refactor `commands/token.rs`.
- **Files**: `src-tauri/src/services/token_service.rs`, `src-tauri/src/commands/token.rs`, `src-tauri/src/services/mod.rs`
- **Deps**: T0.1
- **Agent**: `deep`
- **QA**: `cargo check` passes.
- **Effort**: S

### T1.4: Extract `usage_service.rs` from `commands/usage.rs`
- **What to do**: Create `src-tauri/src/services/usage_service.rs` with `get_usage_logs`, `get_dashboard_stats`, `get_model_consumption`, `get_call_trend`, `get_model_distribution`, `get_model_ranking`, `get_user_ranking`, `get_user_trend`. Service accepts plain args, not Tauri-specific types.
- **Files**: `src-tauri/src/services/usage_service.rs`, `src-tauri/src/commands/usage.rs`, `src-tauri/src/services/mod.rs`
- **Deps**: T0.1
- **Agent**: `deep`
- **QA**: `cargo check` passes.
- **Effort**: S

**⚠️ CONFLICT**: T1.2/T1.3/T1.4 all modify `services/mod.rs`. Run T1.2 first, then T1.3/T1.4 append `pub mod` lines.

### T1.5: Extend `ApiAdapter` interface for Pool/Token/Usage
- **What to do**:
  1. Add `pool`, `tokens`, `usage` namespaces to `src/lib/apiAdapter.ts`.
  2. Add `invoke()` implementations to `src/lib/tauriApiAdapter.ts`.
  3. `webAdminApiAdapter` HTTP implementations come in T2.5.
- **Files**: `src/lib/apiAdapter.ts`, `src/lib/tauriApiAdapter.ts`
- **Deps**: T0.1
- **Agent**: `visual-engineering`
- **QA**: `pnpm typecheck` passes.
- **Effort**: M

### T1.6: Tray no-op guard + mode in status response
- **What to do**:
  1. Make `refresh_tray_if_enabled()` skip gracefully in Standalone mode.
  2. Add `runtime_mode` field to `AdminStatus` in `admin/handlers.rs`.
  3. Update `status()` handler to include mode.
- **Files**: `src-tauri/src/lib.rs`, `src-tauri/src/admin/handlers.rs`
- **Deps**: T1.1
- **Agent**: `deep`
- **QA**: `cargo check` passes. `GET /admin/status` returns `runtime_mode`.
- **Effort**: S

---

## Wave 2 — Startup Refactor + HTTP Endpoints + Web Adapter (6 parallel tasks)

### T2.1: Refactor `lib.rs` startup to branch on RuntimeMode
- **What to do**:
  1. Call `detect_runtime_mode()` at top of `run()`.
  2. Wrap tray/window creation in `if mode == Combined`.
  3. Standalone: init DB, AppState, proxy, admin — no window/tray.
  4. Catch tray build errors → fallback to Standalone.
  5. Store `RuntimeMode` in `AppState`.
- **Files**: `src-tauri/src/lib.rs`
- **Deps**: T1.1
- **Agent**: `deep`
- **QA**: `cargo check` passes. `API_SWITCH_HEADLESS=1` starts without tray.
- **Effort**: L

### T2.2: Add `/admin/pool` HTTP endpoints
- **What to do**:
  1. Create `src-tauri/src/admin/pool_handlers.rs` calling `pool_service::*`.
  2. Routes: `GET/POST /admin/pool`, `PUT/DELETE /admin/pool/:id`, `POST /admin/pool/reorder`, `POST /admin/pool/:id/test-latency`, `POST /admin/pool/backfill-catalog-meta`.
  3. Register in `admin/router.rs`.
- **Files**: `src-tauri/src/admin/pool_handlers.rs`, `src-tauri/src/admin/router.rs`, `src-tauri/src/admin/mod.rs`
- **Deps**: T1.2
- **Agent**: `deep`
- **QA**: `cargo check` passes. `curl GET /admin/pool` returns entries.
- **Effort**: M

### T2.3: Add `/admin/tokens` HTTP endpoints
- **What to do**: Create `src-tauri/src/admin/token_handlers.rs`. Routes: `GET/POST /admin/tokens`, `DELETE /admin/tokens/:id`, `PUT /admin/tokens/:id/toggle`.
- **Files**: `src-tauri/src/admin/token_handlers.rs`, `src-tauri/src/admin/router.rs`, `src-tauri/src/admin/mod.rs`
- **Deps**: T1.3
- **Agent**: `deep`
- **QA**: `cargo check` passes.
- **Effort**: S

### T2.4: Add `/admin/logs` + `/admin/dashboard` HTTP endpoints
- **What to do**: Create `src-tauri/src/admin/usage_handlers.rs`. Routes: `GET /admin/logs`, `GET /admin/dashboard/stats`, `GET /admin/dashboard/model-consumption`, `GET /admin/dashboard/call-trend`, etc.
- **Files**: `src-tauri/src/admin/usage_handlers.rs`, `src-tauri/src/admin/router.rs`, `src-tauri/src/admin/mod.rs`
- **Deps**: T1.4
- **Agent**: `deep`
- **QA**: `cargo check` passes.
- **Effort**: M

**⚠️ CONFLICT**: T2.2/T2.3/T2.4 all modify `admin/router.rs` and `admin/mod.rs`. Append-only, no real conflict.

### T2.5: Implement `webAdminApiAdapter` for Pool/Token/Usage
- **What to do**: Extend `src/lib/webAdminApiAdapter.ts` with `pool`, `tokens`, `usage` namespaces calling `/admin/*` endpoints.
- **Files**: `src/lib/webAdminApiAdapter.ts`
- **Deps**: T1.5, T2.2, T2.3, T2.4
- **Agent**: `visual-engineering`
- **QA**: `pnpm typecheck` passes.
- **Effort**: M

### T2.6: Update `WEB_ADMIN_PLAN.md` Phase 2 markers
- **What to do**: Mark Phase 2 items complete in plan file.
- **Files**: `WEB_ADMIN_PLAN.md`
- **Deps**: T2.1
- **Agent**: `quick`
- **Effort**: S

---

## Wave 3 — Shared Feature Components + Mode Badge (5 parallel tasks)

### T3.1: Create `src/features/pool/PoolManager.tsx`
- **What to do**: Extract `ApiPoolPage` business UI (988 lines) into shared component. Replace `listen("tray-priority-changed")` with React Query polling. Use `useApiAdapter().pool.*`.
- **Files**: `src/features/pool/PoolManager.tsx`, `src/pages/ApiPoolPage.tsx`
- **Deps**: T1.5, T2.5
- **Agent**: `visual-engineering`
- **QA**: `pnpm typecheck` passes. Desktop Pool page works.
- **Effort**: L

### T3.2: Create `src/features/tokens/TokenManager.tsx`
- **What to do**: Extract `TokenPage` (207 lines) into shared component. Use `useApiAdapter().tokens.*`.
- **Files**: `src/features/tokens/TokenManager.tsx`, `src/pages/TokenPage.tsx`
- **Deps**: T1.5, T2.5
- **Agent**: `visual-engineering`
- **QA**: `pnpm typecheck` passes.
- **Effort**: S

### T3.3: Create `src/features/logs/LogViewer.tsx`
- **What to do**: Extract `LogPage` (253 lines). Replace `listen("new-usage-log")` with polling.
- **Files**: `src/features/logs/LogViewer.tsx`, `src/pages/LogPage.tsx`
- **Deps**: T1.5, T2.5
- **Agent**: `visual-engineering`
- **QA**: `pnpm typecheck` passes.
- **Effort**: S

### T3.4: Create `src/features/dashboard/DashboardView.tsx`
- **What to do**: Extract `DashboardPage` (343 lines). Use `useApiAdapter().usage.*`.
- **Files**: `src/features/dashboard/DashboardView.tsx`, `src/pages/DashboardPage.tsx`
- **Deps**: T1.5, T2.5
- **Agent**: `visual-engineering`
- **QA**: `pnpm typecheck` passes.
- **Effort**: S

### T3.5: Display RuntimeMode in Web Admin header
- **What to do**: Read `runtime_mode` from `getStatus()`, show badge in header.
- **Files**: `src/web-admin/src/WebAdminApp.tsx`, `src/web-admin/src/api.ts`
- **Deps**: T1.6
- **Agent**: `visual-engineering`
- **QA**: Badge visible in browser.
- **Effort**: S

---

## Wave 4 — Route Wiring + Build Gates (3 parallel tasks)

### T4.1: Wire features into WebAdminApp router
- **What to do**: Replace `ComingSoonPlaceholder` with real `<PoolManager>`, `<TokenManager>`, `<LogViewer>`, `<DashboardView>`.
- **Files**: `src/web-admin/src/WebAdminApp.tsx`
- **Deps**: T3.1–T3.4
- **Agent**: `visual-engineering`
- **QA**: `pnpm typecheck` passes.
- **Effort**: S

### T4.2: Full Rust build verification
- **What to do**: `cargo check`. Fix any errors in new modules.
- **Deps**: T1.1–T1.6, T2.1–T2.6
- **Agent**: `deep`
- **QA**: `cargo check` exit 0.
- **Effort**: M

### T4.3: Full frontend build verification
- **What to do**: `pnpm typecheck` + `pnpm build:web-admin` + `pnpm build:renderer`.
- **Deps**: T4.1, T1.5, T2.5
- **Agent**: `visual-engineering`
- **QA**: All three commands exit 0.
- **Effort**: M

---

## Wave 5 — Integration Verification (3 parallel tasks)

### T5.1: Standalone mode verification
- **What to do**: `API_SWITCH_HEADLESS=1 cargo run`. Verify proxy + admin start, no tray. `GET /admin/status` shows Standalone. Browser loads all pages.
- **Deps**: T4.2, T4.3
- **Agent**: `deep` + `playwright`
- **QA**: All endpoints respond. Web UI loads all pages.
- **Effort**: M

### T5.2: Combined mode desktop regression
- **What to do**: Launch without env override. Verify tray, window, proxy, settings, channels, pool all work.
- **Deps**: T4.2, T4.3
- **Agent**: `deep`
- **QA**: Phase 1 smoke checklist passes.
- **Effort**: M

### T5.3: Web Admin end-to-end — all pages in browser
- **What to do**: Login, navigate all 6 pages, perform one CRUD per page. Verify cross-platform data sync.
- **Deps**: T4.1, T4.2, T4.3
- **Agent**: `playwright`
- **QA**: All pages load. One CRUD per page succeeds.
- **Effort**: M

---

## Parallelism Summary

| Wave | Tasks | Parallel Tracks | Est. Duration |
|------|-------|-----------------|---------------|
| Wave 0 | T0.1 | 1 | 10 min |
| Wave 1 | T1.1–T1.6 | **6 tracks** | ~2 hrs |
| Wave 2 | T2.1–T2.6 | **6 tracks** | ~2.5 hrs |
| Wave 3 | T3.1–T3.5 | **5 tracks** | ~2 hrs |
| Wave 4 | T4.1–T4.3 | **3 tracks** | ~1 hr |
| Wave 5 | T5.1–T5.3 | **3 tracks** | ~1.5 hrs |

**Critical path**: T0.1 → T1.1 → T2.1 → T4.2 (~3.5 hrs)
**Parallelization rate**: ~80%

---

## Exit Criteria

### Phase 2 DoD:
- [ ] `RuntimeMode` enum defined and logged at startup
- [ ] `detect_runtime_mode()` respects CLI > env > auto
- [ ] Standalone mode starts proxy + admin without GUI APIs
- [ ] `GET /admin/status` includes `runtime_mode`
- [ ] Combined mode behaves identically to pre-Phase 2

### Phase 3 DoD:
- [ ] `pool_service.rs`, `token_service.rs`, `usage_service.rs` exist
- [ ] HTTP endpoints work for Pool/Tokens/Logs/Dashboard
- [ ] `src/features/pool/`, `tokens/`, `logs/`, `dashboard/` exist
- [ ] `webAdminApiAdapter` implements all namespaces
- [ ] WebAdminApp uses real components (no `ComingSoonPlaceholder`)
- [ ] `cargo check` → exit 0
- [ ] `pnpm typecheck` → exit 0
- [ ] `pnpm build:web-admin` → exit 0
- [ ] `pnpm build:renderer` → exit 0
- [ ] Desktop + Web both render all pages
- [ ] One CRUD per Web Admin page verified
- [ ] Standalone mode launches and serves all endpoints
