use std::sync::atomic::{AtomicBool, Ordering};

/// 各业务模块的 dirty 标记
pub struct DirtyFlags {
    pub log: AtomicBool,
    pub pool: AtomicBool,
    pub channel: AtomicBool,
    pub token: AtomicBool,
}

impl DirtyFlags {
    pub fn new() -> Self {
        Self {
            log: AtomicBool::new(false),
            pool: AtomicBool::new(false),
            channel: AtomicBool::new(false),
            token: AtomicBool::new(false),
        }
    }

    /// 标记对应模块为 dirty
    #[inline]
    pub fn mark_log(&self) {
        self.log.store(true, Ordering::Release);
    }
    #[inline]
    pub fn mark_pool(&self) {
        self.pool.store(true, Ordering::Release);
    }
    #[inline]
    pub fn mark_channel(&self) {
        self.channel.store(true, Ordering::Release);
    }
    #[inline]
    pub fn mark_token(&self) {
        self.token.store(true, Ordering::Release);
    }

    /// 取出 dirty 状态并自动清零（原子 swap）
    #[inline]
    pub fn take_log(&self) -> bool {
        self.log.swap(false, Ordering::AcqRel)
    }
    #[inline]
    pub fn take_pool(&self) -> bool {
        self.pool.swap(false, Ordering::AcqRel)
    }
    #[inline]
    pub fn take_channel(&self) -> bool {
        self.channel.swap(false, Ordering::AcqRel)
    }
    #[inline]
    pub fn take_token(&self) -> bool {
        self.token.swap(false, Ordering::AcqRel)
    }
}
