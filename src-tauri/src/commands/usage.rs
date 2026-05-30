#[cfg(feature = "gui")]
use crate::database::*;
#[cfg(feature = "gui")]
use crate::error::AppError;
#[cfg(feature = "gui")]
use crate::services::log_service;
#[cfg(feature = "gui")]
use crate::AppState;
use serde::Deserialize;
#[cfg(feature = "gui")]
use tauri::State;

#[cfg(feature = "gui")]
#[tauri::command]
pub fn get_usage_logs(
    state: State<'_, AppState>,
    filter: UsageLogFilter,
) -> Result<PaginatedResult<UsageLog>, AppError> {
    log_service::get_usage_logs(&state.db, &filter)
}

#[cfg(feature = "gui")]
#[tauri::command]
pub fn get_dashboard_stats(
    state: State<'_, AppState>,
    filter: Option<DashboardFilterParams>,
) -> Result<DashboardStats, AppError> {
    let (start, end, _) = parse_filter(filter);
    log_service::get_dashboard_stats(&state.db, start, end)
}

#[cfg(feature = "gui")]
#[tauri::command]
pub fn get_model_consumption(
    state: State<'_, AppState>,
    filter: Option<DashboardFilterParams>,
) -> Result<Vec<ChartDataPoint>, AppError> {
    let (start, end, granularity) = parse_filter(filter);
    log_service::get_model_consumption(&state.db, start, end, granularity.as_deref())
}

#[cfg(feature = "gui")]
#[tauri::command]
pub fn get_call_trend(
    state: State<'_, AppState>,
    filter: Option<DashboardFilterParams>,
) -> Result<Vec<ChartDataPoint>, AppError> {
    let (start, end, granularity) = parse_filter(filter);
    log_service::get_call_trend(&state.db, start, end, granularity.as_deref())
}

#[cfg(feature = "gui")]
#[tauri::command]
pub fn get_model_distribution(
    state: State<'_, AppState>,
    filter: Option<DashboardFilterParams>,
) -> Result<Vec<ModelRanking>, AppError> {
    let (start, end, _) = parse_filter(filter);
    log_service::get_model_distribution(&state.db, start, end)
}

#[cfg(feature = "gui")]
#[tauri::command]
pub fn get_model_ranking(
    state: State<'_, AppState>,
    filter: Option<DashboardFilterParams>,
) -> Result<Vec<ModelRanking>, AppError> {
    let (start, end, _) = parse_filter(filter);
    log_service::get_model_ranking(&state.db, start, end)
}

#[cfg(feature = "gui")]
#[tauri::command]
pub fn get_user_ranking(
    state: State<'_, AppState>,
    filter: Option<DashboardFilterParams>,
) -> Result<Vec<UserRanking>, AppError> {
    let (start, end, _) = parse_filter(filter);
    log_service::get_user_ranking(&state.db, start, end)
}

#[cfg(feature = "gui")]
#[tauri::command]
pub fn get_user_trend(
    state: State<'_, AppState>,
    filter: Option<DashboardFilterParams>,
) -> Result<Vec<ChartDataPoint>, AppError> {
    let (start, end, granularity) = parse_filter(filter);
    log_service::get_user_trend(&state.db, start, end, granularity.as_deref())
}

#[derive(Deserialize)]
pub struct DashboardFilterParams {
    pub start_time: Option<i64>,
    pub end_time: Option<i64>,
    pub granularity: Option<String>,
}

#[cfg(feature = "gui")]
fn parse_filter(
    filter: Option<DashboardFilterParams>,
) -> (Option<i64>, Option<i64>, Option<String>) {
    match filter {
        Some(f) => (f.start_time, f.end_time, f.granularity),
        None => (None, None, None),
    }
}

#[cfg(feature = "gui")]
#[tauri::command]
pub fn clear_log_details(state: State<'_, AppState>) -> Result<u64, AppError> {
    log_service::clear_log_details(&state.db)
}
