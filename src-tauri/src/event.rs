//! Event sink abstraction so non-GUI builds compile without the `tauri` crate.
//!
//! GUI builds alias `AppEventHandle` to `tauri::AppHandle` and emit real
//! frontend events. Headless builds use a zero-sized stand-in and no-op emits.
//! Handles are always wrapped in `Option` at call sites, so the headless side
//! simply never constructs one.

#[cfg(feature = "gui")]
pub type AppEventHandle = tauri::AppHandle;

#[cfg(not(feature = "gui"))]
#[derive(Clone)]
pub struct AppEventHandle;

/// Emit a frontend event. No-op when the handle is absent or in headless builds.
#[cfg(feature = "gui")]
pub fn emit(handle: &AppEventHandle, event: &str) {
    use tauri::Emitter;
    let _ = handle.emit(event, ());
}

#[cfg(not(feature = "gui"))]
pub fn emit(_handle: &AppEventHandle, _event: &str) {}

/// Rebuild the tray menu after pool/channel changes. No-op in headless builds.
#[cfg(not(feature = "gui"))]
pub fn refresh_tray_if_enabled(_handle: &AppEventHandle) {}
