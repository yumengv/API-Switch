//! Channel facade：channel CRUD、模型获取、连接测试等。
//!
//! 调用 `services::channel_service` 中的函数，底层使用 `&Database` 和
//! `Option<&AppEventHandle>`，在所有构建模式下均可用。

use crate::database::Channel;
use crate::database::dao::PaginatedResult;
use crate::error::AppError;
use crate::services::channel_service::{
    self, CreateChannelParams, SaveChannelWithModelsParams, SaveChannelWithModelsResult,
    UpdateChannelParams, UpdateResponseMsParams,
};

use super::ServerApi;

impl ServerApi {
    /// 获取所有 channel 列表。
    pub fn list_channels(&self) -> Result<Vec<Channel>, AppError> {
        channel_service::list_channels(&self.state().db)
    }

    /// 分页获取 channel 列表。
    pub fn list_channels_paginated(
        &self,
        page: i32,
        page_size: i32,
    ) -> Result<PaginatedResult<Channel>, AppError> {
        channel_service::list_channels_paginated(&self.state().db, page, page_size)
    }

    /// 创建新 channel。
    pub fn create_channel(&self, params: CreateChannelParams) -> Result<Channel, AppError> {
        channel_service::create_channel(&self.state().db, params)
    }

    /// 更新 channel，同时触发前端事件。
    pub fn update_channel(&self, params: UpdateChannelParams) -> Result<Channel, AppError> {
        channel_service::update_channel(
            &self.state().db,
            Some(self.app()),
            params,
        )
    }

    /// 删除 channel，同时触发前端事件。
    pub fn delete_channel(&self, id: String) -> Result<(), AppError> {
        channel_service::delete_channel(&self.state().db, Some(self.app()), id)
    }

    /// 更新 channel 的响应时间。
    pub fn update_channel_response_ms(
        &self,
        channel_id: &str,
        response_ms: &str,
    ) -> Result<(), AppError> {
        channel_service::update_channel_response_ms(
            &self.state().db,
            UpdateResponseMsParams {
                channel_id: channel_id.to_string(),
                response_ms: response_ms.to_string(),
            },
        )
    }

    /// 一步保存 channel 并同步模型，同时触发前端事件。
    pub fn save_channel_with_models(
        &self,
        params: SaveChannelWithModelsParams,
    ) -> Result<SaveChannelWithModelsResult, AppError> {
        channel_service::save_channel_with_models(
            &self.state().db,
            Some(self.app()),
            params,
        )
    }
}
