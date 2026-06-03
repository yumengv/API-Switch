use serde::Serialize;

/// 平台壳层类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum RuntimeShell {
    /// 桌面壳层（Windows/macOS/Linux GUI）
    Desktop,
    /// 移动壳层（Android/iOS）
    Mobile,
    /// 无壳层（headless / CLI 模式）
    None,
}

/// 运行时启动计划，显式声明各层是否启动
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct RuntimePlan {
    pub start_core: bool,
    pub start_server_api: bool,
    pub start_web_service: bool,
    pub shell: RuntimeShell,
}

impl RuntimePlan {
    /// 桌面壳层计划：启动全部基础运行层 + 桌面 GUI
    pub fn desktop_shell() -> Self {
        Self {
            start_core: true,
            start_server_api: true,
            start_web_service: true,
            shell: RuntimeShell::Desktop,
        }
    }

    /// 移动壳层计划：启动全部基础运行层 + 移动 GUI
    pub fn mobile_shell() -> Self {
        Self {
            start_core: true,
            start_server_api: true,
            start_web_service: true,
            shell: RuntimeShell::Mobile,
        }
    }

    /// 无壳层计划：启动全部基础运行层，无 GUI
    pub fn no_shell() -> Self {
        Self {
            start_core: true,
            start_server_api: true,
            start_web_service: true,
            shell: RuntimeShell::None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{RuntimePlan, RuntimeShell};

    #[test]
    fn mobile_shell_still_starts_base_runtime() {
        let plan = RuntimePlan::mobile_shell();
        assert!(plan.start_core);
        assert!(plan.start_server_api);
        assert!(plan.start_web_service);
        assert_eq!(plan.shell, RuntimeShell::Mobile);
    }

    #[test]
    fn no_shell_still_keeps_web_service_management_entry() {
        let plan = RuntimePlan::no_shell();
        assert!(plan.start_core);
        assert!(plan.start_server_api);
        assert!(plan.start_web_service);
        assert_eq!(plan.shell, RuntimeShell::None);
    }

    #[test]
    fn desktop_shell_starts_all_layers() {
        let plan = RuntimePlan::desktop_shell();
        assert!(plan.start_core);
        assert!(plan.start_server_api);
        assert!(plan.start_web_service);
        assert_eq!(plan.shell, RuntimeShell::Desktop);
    }

    #[test]
    fn all_plans_share_same_base_layers() {
        let desktop = RuntimePlan::desktop_shell();
        let mobile = RuntimePlan::mobile_shell();
        let no_shell = RuntimePlan::no_shell();

        assert_eq!(desktop.start_core, mobile.start_core);
        assert_eq!(desktop.start_core, no_shell.start_core);
        assert_eq!(desktop.start_server_api, mobile.start_server_api);
        assert_eq!(desktop.start_server_api, no_shell.start_server_api);
        assert_eq!(desktop.start_web_service, mobile.start_web_service);
        assert_eq!(desktop.start_web_service, no_shell.start_web_service);
    }
}
