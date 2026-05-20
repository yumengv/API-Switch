use crate::database::dao::PaginatedResult;
use crate::database::{AccessKey, Database};
use crate::error::AppError;

/// List all access keys
pub fn list_access_keys(db: &Database) -> Result<Vec<AccessKey>, AppError> {
    db.list_access_keys()
}

pub fn list_access_keys_paginated(
    db: &Database,
    page: i32,
    page_size: i32,
) -> Result<PaginatedResult<AccessKey>, AppError> {
    db.list_access_keys_paginated(page, page_size)
}

/// Create a new access key
pub fn create_access_key(db: &Database, name: &str) -> Result<AccessKey, AppError> {
    let key = db.create_access_key(name)?;
    crate::state_version::bump("token");
    Ok(key)
}

/// Delete an access key by ID
pub fn delete_access_key(
    db: &Database,
    id: &str,
    app: Option<&tauri::AppHandle>,
) -> Result<(), AppError> {
    db.delete_access_key(id)?;
    if let Some(app) = app {
        crate::refresh_tray_if_enabled(app);
    }
    crate::state_version::bump("token");
    Ok(())
}

/// Toggle access key enabled state
pub fn toggle_access_key(
    db: &Database,
    id: &str,
    enabled: bool,
    app: Option<&tauri::AppHandle>,
) -> Result<(), AppError> {
    db.toggle_access_key(id, enabled)?;
    if let Some(app) = app {
        crate::refresh_tray_if_enabled(app);
    }
    crate::state_version::bump("token");
    Ok(())
}
