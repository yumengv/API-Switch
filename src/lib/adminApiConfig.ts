/**
 * Single source of truth for the admin API base path prefix.
 *
 * CURRENT STATE (2026-05):
 *   The Rust backend registers all admin routes under `/admin/*`
 *   (see src-tauri/src/admin/router.rs). Static file serving also
 *   depends on the `/admin/` prefix (see static_files.rs admin_asset).
 *
 * FUTURE MIGRATION:
 *   When the backend migrates routes to root-level (e.g. `/channels`
 *   instead of `/admin/channels`), update this single constant and
 *   rebuild both the desktop web-admin and standalone web-admin.
 *   The vite.config.ts `base` and index.html `<base>` must also change
 *   in coordination with the Rust router and static_files handler.
 *
 * IMPORTANT: Do NOT hardcode `/admin` in new code. Import this constant.
 */
export const ADMIN_API_PREFIX = '/admin';
