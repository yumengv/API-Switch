//! Pool facade：API 入口的 CRUD、批量操作、延迟测试等。
//!
//! 调用 `services::pool_service` 中的函数，底层使用 `&Database`，
//! 在所有构建模式下均可用。

use crate::database::ApiEntry;
use crate::database::dao::PaginatedResult;
use crate::error::AppError;
use crate::services::pool_service::{self, CatalogMetaUpdate, CreateEntryParams, TestLatencyResult};

use super::ServerApi;

impl ServerApi {
    /// 获取所有 API 入口。
    pub fn list_entries(&self) -> Result<Vec<ApiEntry>, AppError> {
        pool_service::list_entries(&self.state().db)
    }

    /// 分页获取 API 入口。
    pub fn list_entries_paginated(
        &self,
        page: i32,
        page_size: i32,
        group_name: Option<&str>,
        search: Option<&str>,
        channel_id: Option<&str>,
    ) -> Result<PaginatedResult<ApiEntry>, AppError> {
        pool_service::list_entries_paginated(
            &self.state().db,
            page,
            page_size,
            group_name,
            search,
            channel_id,
        )
    }

    /// 切换单个入口的启用状态。
    pub fn toggle_entry(&self, id: &str, enabled: bool, pin_to_top: bool) -> Result<(), AppError> {
        pool_service::toggle_entry(
            &self.state().db,
            &self.state().failure_counts,
            id,
            enabled,
            pin_to_top,
        )
    }

    /// 批量切换入口的启用状态（单次 IPC 调用）。
    pub fn batch_toggle_entries(&self, ids: &[String], enabled: bool) -> Result<(), AppError> {
        pool_service::batch_toggle_entries(
            &self.state().db,
            &self.state().failure_counts,
            ids,
            enabled,
        )
    }

    /// 按给定顺序重新排列入口。
    pub fn reorder_entries(&self, ordered_ids: &[String]) -> Result<(), AppError> {
        pool_service::reorder_entries(&self.state().db, ordered_ids)
    }

    /// 删除指定入口。
    pub fn delete_entry(&self, id: &str) -> Result<(), AppError> {
        pool_service::delete_entry(&self.state().db, id)
    }

    /// 创建新 API 入口。
    pub fn create_entry(&self, params: CreateEntryParams) -> Result<ApiEntry, AppError> {
        pool_service::create_entry(&self.state().db, params)
    }

    /// 批量回填目录元数据。
    pub fn backfill_entry_catalog_meta(
        &self,
        items: Vec<CatalogMetaUpdate>,
    ) -> Result<(), AppError> {
        pool_service::backfill_entry_catalog_meta(&self.state().db, items)
    }

    /// 测试指定入口的延迟。
    pub async fn test_entry_latency(
        &self,
        entry_id: &str,
        model_score: f64,
    ) -> Result<TestLatencyResult, AppError> {
        pool_service::test_entry_latency(&self.state().db, entry_id, model_score).await
    }

    /// 更新指定入口的响应时间。
    pub fn update_entry_response_ms(
        &self,
        entry_id: &str,
        response_ms: &str,
    ) -> Result<(), AppError> {
        pool_service::update_entry_response_ms(&self.state().db, entry_id, response_ms)
    }

    /// 获取所有分组名称。
    pub fn get_all_groups(&self) -> Result<Vec<String>, AppError> {
        pool_service::get_all_groups(&self.state().db)
    }

    /// 更新指定入口的显示名称。
    pub fn update_entry_display_name(
        &self,
        id: &str,
        display_name: &str,
    ) -> Result<(), AppError> {
        pool_service::update_entry_display_name(&self.state().db, id, display_name)
    }

    /// 更新指定入口的分组。
    pub fn update_entry_group(&self, id: &str, group_name: &str) -> Result<(), AppError> {
        pool_service::update_entry_group(&self.state().db, id, group_name)
    }
}
