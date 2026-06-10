//! Pool facade：API 入口的 CRUD、批量操作、延迟测试等。
//!
//! 调用 `services::pool_service` 中的函数，底层使用 `&Database`，
//! 在所有构建模式下均可用。

use crate::database::dao::PaginatedResult;
use crate::database::{ApiEntry, ModelGroupConfig};
use crate::error::AppError;
use crate::services::pool_service::{
    self, CatalogMetaUpdate, CreateEntryParams, ReplaceModelGroupEntriesParams, TestLatencyResult,
    SortIndexUpdate, UpsertModelGroupParams,
};

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
        let result = pool_service::toggle_entry(
            &self.state().db,
            &self.state().failure_counts,
            id,
            enabled,
            pin_to_top,
        );
        if result.is_ok() {
            crate::event::emit(self.app(), "entries-changed");
        }
        result
    }

    /// 批量切换入口的启用状态（单次 IPC 调用）。
    pub fn batch_toggle_entries(&self, ids: &[String], enabled: bool) -> Result<(), AppError> {
        let result = pool_service::batch_toggle_entries(
            &self.state().db,
            &self.state().failure_counts,
            ids,
            enabled,
        );
        if result.is_ok() {
            crate::event::emit(self.app(), "entries-changed");
        }
        result
    }

    /// 按给定顺序重新排列入口。
    pub fn reorder_entries(&self, ordered_ids: &[String]) -> Result<(), AppError> {
        let result = pool_service::reorder_entries(&self.state().db, ordered_ids);
        if result.is_ok() {
            crate::event::emit(self.app(), "entries-changed");
        }
        result
    }

    /// 更新单个入口的自定义排序值。
    pub fn update_entry_sort_index(&self, id: &str, sort_index: i32) -> Result<(), AppError> {
        let result = pool_service::update_entry_sort_index(&self.state().db, id, sort_index);
        if result.is_ok() {
            crate::event::emit(self.app(), "entries-changed");
        }
        result
    }

    /// 批量更新入口排序值。
    pub fn batch_update_entry_sort_indexes(
        &self,
        items: &[SortIndexUpdate],
    ) -> Result<(), AppError> {
        let result = pool_service::batch_update_entry_sort_indexes(&self.state().db, items);
        if result.is_ok() {
            crate::event::emit(self.app(), "entries-changed");
        }
        result
    }

    /// 删除指定入口。
    pub fn delete_entry(&self, id: &str) -> Result<(), AppError> {
        let result = pool_service::delete_entry(&self.state().db, id);
        if result.is_ok() {
            crate::event::emit(self.app(), "entries-changed");
        }
        result
    }

    /// 创建新 API 入口。
    pub fn create_entry(&self, params: CreateEntryParams) -> Result<ApiEntry, AppError> {
        let entry = pool_service::create_entry(&self.state().db, params)?;
        crate::event::emit(self.app(), "entries-changed");
        Ok(entry)
    }

    /// 批量回填目录元数据。
    pub fn backfill_entry_catalog_meta(
        &self,
        items: Vec<CatalogMetaUpdate>,
    ) -> Result<(), AppError> {
        let result = pool_service::backfill_entry_catalog_meta(&self.state().db, items);
        if result.is_ok() {
            crate::event::emit(self.app(), "entries-changed");
        }
        result
    }

    /// 测试指定入口的延迟。
    pub async fn test_entry_latency(
        &self,
        entry_id: &str,
        model_score: f64,
    ) -> Result<TestLatencyResult, AppError> {
        let result =
            pool_service::test_entry_latency(&self.state().db, entry_id, model_score).await;
        if result.is_ok() {
            crate::event::emit(self.app(), "entries-changed");
        }
        result
    }

    /// 更新指定入口的响应时间。
    pub fn update_entry_response_ms(
        &self,
        entry_id: &str,
        response_ms: &str,
    ) -> Result<(), AppError> {
        let result =
            pool_service::update_entry_response_ms(&self.state().db, entry_id, response_ms);
        if result.is_ok() {
            crate::event::emit(self.app(), "entries-changed");
        }
        result
    }

    /// 获取所有分组名称。
    pub fn get_all_groups(&self) -> Result<Vec<String>, AppError> {
        pool_service::get_all_groups(&self.state().db)
    }

    /// 获取模型分组配置。
    pub fn list_model_groups(&self) -> Result<Vec<ModelGroupConfig>, AppError> {
        pool_service::list_model_groups(&self.state().db)
    }

    /// 获取指定模型分组的成员条目 ID。
    pub fn list_model_group_entry_ids(&self, name: &str) -> Result<Vec<String>, AppError> {
        pool_service::list_model_group_entry_ids(&self.state().db, name)
    }

    /// 新增或更新模型分组配置。
    pub fn upsert_model_group(
        &self,
        params: UpsertModelGroupParams,
    ) -> Result<ModelGroupConfig, AppError> {
        let result = pool_service::upsert_model_group(&self.state().db, params);
        if result.is_ok() {
            crate::event::emit(self.app(), "entries-changed");
        }
        result
    }

    /// 启用或停用模型分组。
    pub fn update_model_group_enabled(&self, name: &str, enabled: bool) -> Result<(), AppError> {
        let result = pool_service::update_model_group_enabled(&self.state().db, name, enabled);
        if result.is_ok() {
            crate::event::emit(self.app(), "entries-changed");
        }
        result
    }

    /// 删除模型分组并将成员移回 auto。
    pub fn delete_model_group(&self, name: &str) -> Result<(), AppError> {
        let result = pool_service::delete_model_group(&self.state().db, name);
        if result.is_ok() {
            crate::event::emit(self.app(), "entries-changed");
        }
        result
    }

    /// 替换某个模型分组的成员。
    pub fn replace_model_group_entries(
        &self,
        params: ReplaceModelGroupEntriesParams,
    ) -> Result<(), AppError> {
        let result = pool_service::replace_model_group_entries(&self.state().db, params);
        if result.is_ok() {
            crate::event::emit(self.app(), "entries-changed");
        }
        result
    }

    /// 更新指定入口的显示名称。
    pub fn update_entry_display_name(&self, id: &str, display_name: &str) -> Result<(), AppError> {
        let result = pool_service::update_entry_display_name(&self.state().db, id, display_name);
        if result.is_ok() {
            crate::event::emit(self.app(), "entries-changed");
        }
        result
    }

    /// 更新指定入口的分组。
    pub fn update_entry_group(&self, id: &str, group_name: &str) -> Result<(), AppError> {
        let result = pool_service::update_entry_group(&self.state().db, id, group_name);
        if result.is_ok() {
            crate::event::emit(self.app(), "entries-changed");
        }
        result
    }
}
