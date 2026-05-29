# TODO

> 执行级待办清单，按优先级排列。状态标记：⏳ 待开始 / 🔄 进行中 / ❌ 阻塞

---

## P0

### ⏳ 依赖膨胀分析落地

**来源：** `docs/dependency_analysis.md` 分析显示，在 Termux 上运行 `api-switch` 时，需搬运 ~678 个 `.so`（约 708MB）的 GUI 依赖库（GTK、WebKit2GTK、ICU 等）。即使只用 headless 模式，这些库也会因 ELF NEEDED 被强制载入。

**目标：** 编译 headless-only 版本（剔除 GTK/WebKit 依赖），使 Termux 部署彻底轻量化。

**方案：**
- 编译时引入条件编译（`ENABLE_GUI` 开关），GUI 与 core 解耦
- 分发双版本：`api-switch-server`（headless，~10-20MB）+ `api-switch-desktop`（全量）
- 编译时使用 `-Wl,--as-needed` 等链接选项减少不必要的 NEEDED

**关联文件：**
- `docs/dependency_analysis.md`
- `src-tauri/Cargo.toml` — 依赖管理
- `src-tauri/src/lib.rs` — 入口
- `.github/workflows/release.yml` — 构建分发

### ⏳ 手机浏览器适配 — 当前 UI 框架对移动端支持极差

**来源：** Web Admin 目前使用 Radix UI + Tailwind 的桌面优先布局，在手机浏览器上几乎不可用。参考 GitHub 使用 Primer React（CSS Modules + 原生 HTML 元素）的做法，Radix UI 的 Dialog/Popover/Select 等组件在触屏交互、小屏布局上体验很差，且组件语义化过重难以按需定制。

**现有问题：**
- 固定宽度侧边栏（`w-56`）在手机上占满整个屏幕
- 表格/列表（PoolManager、ChannelManager）在手机上无法正常浏览
- Radix UI Dialog 在手机上全屏覆盖体验差
- 没有移动端底部导航栏替代侧边栏
- Select/DropdownMenu 等下拉组件在触屏上交互违和
- 可拖拽排序（`@dnd-kit`）在触屏上基本不可用

**建议方案：**
- 参考 GitHub Primer React 的轻量风格，替换或跳过部分 Radix UI 重型组件
- 移动端用底部导航栏（Bottom Tab Bar）替代固定侧边栏
- 关键页面（渠道管理、API 管理、令牌）提供简化的移动端视图
- 使用 Tailwind 响应式断点（`sm:`/`md:`/`lg:`）重构布局
- 表格/列表改为卡片式布局，适配小屏
- 拖拽排序提供"长按拖动手柄"模式，或回退到列表排序按钮

**关联文件：**
- `src/features/shell/MainShell.tsx` — 侧边栏 + 主内容布局
- `src/features/pool/PoolManager.tsx` — 拖拽排序 + 复杂列表
- `src/features/channels/ChannelManager.tsx` — 表格列表
- `package.json` — Radix UI 组件库依赖
- `src/components/ui/` — 基础 UI 组件

### ⏳ 对话中显示的模型名来源不明 — model info 注入数据追踪

**来源：** 用户在对话（OpenCode/CLI）中看到的模型标签如 `deepseek-v4-singapore`、`deepseek-v4-flash`、`gpt-5.5` 等，部分名称在当前 AUTO 分组的 api_entries 中并不存在。这些显示名疑似来自：

1. **流式响应尾部注入**（`forwarder.rs` 的 `model_info_delta()`）：路由选中的 `entry.model` 被写为 SSE chunk 注入到正文，客户端将其解析为对话内容显示。
2. **上游透传**：上游返回的响应 `model` 字段（如 OpenAI 原始模型名）未被过滤，直接穿透到客户端。

**问题：**
- `model_info_delta()` 注入的 `"\n\nmodel: {name}"` 被客户端当作正文内容渲染，而不是独立的元数据
- 如果路由选中的模型名与上游响应中的 `model` 字段不同，两者都会被显示，造成混淆
- 部分模型名（`deepseek-v4-singapore`）既不在 api_entries 也不在任何渠道中，可能来自上游响应的 `model` 字段

**关联文件：**
- `src-tauri/src/proxy/forwarder.rs` — `model_info_delta()` 注入逻辑（L1605-L1624）、`stream_chunk_has_model_info_delta()` 检测上游已有模型信息（L1534-L1550）
- `src-tauri/src/proxy/forwarder.rs` — 非流式响应 `resolved_model` 日志字段（L243、L2113）

---

## P1

### ⏳ 消息角色兼容性

**来源：** 部分上游拒绝 `messages[]` 中非常规 role 的消息，返回 400 error。

**已观测到的错误：**
1. `role: "system"` 被部分上游（如 M CODIN PLAN 等）拒绝
2. `role: "developer"`（OpenAI Responses API 的 role）被 OpenInference 等不支持的上游拒绝：
   ```
   messages[0].role: unknown variant `developer`,
   expected one of `system`, `user`, `assistant`, `tool`
   ```
   注：顶层白名单已拦截非常规字段，但 `messages` 数组内部每个消息的 `role` 字段未做过滤。

**方案：** 在 forwarder 或 protocol adapter 的 `transform_request` 中，对 messages 内部 role 做归一化（`developer` → `system`，部分上游 `system` → `user`）。按渠道（api_type）差异处理。

**关联：** `src-tauri/src/proxy/forwarder.rs`、`src-tauri/src/proxy/protocol/*.rs` 各 adapter 的 `transform_request`

---

## P2

### ⏳ Gemini 原生协议端点补全

- **countTokens / embedContent / batchEmbedContents**：直接转发 Gemini 上游的专用端点

### ⏳ 渠道启用状态接入路由

**来源：** 当前渠道禁用/启用不影响路由判断，禁用渠道的模型仍可被访问

**方案：** 渠道状态影响路由模型筛选。需要评估影响面——渠道涉及较多关联点，建议引入 L1 缓存

---

## P3

### ⏳ 全局错误语义统一

- 统一 IPC 与 HTTP 错误结构、UI 反馈一致性

---

## 📋 策划文档摘要

### docs/GENERIC_AGENT_FLOATING_AGENT_PLAN.md — Agent 集成方案

**目标**：右下角独立机器人入口，点击后自动连接 GenericAgent 并打开独立对话窗口

**当前状态**：方案设计阶段

**核心设计**：
- 独立透明小窗口 `agent-launcher`（always_on_top）
- 独立对话窗口 `agent-chat`（不依赖主窗口）
- 完整 pipeline：检查配置 → 启动 proxy → 启动 GA adapter → 等待 ready → 打开窗口

**待办**：
- 实现 `agent-launcher` 窗口
- 实现 `ensure_runtime_ready` 流程
- 实现 `agent-chat` 窗口及连接逻辑

**优先级建议**：P2（核心功能稳定后）

---

### docs/security-audit.md — 公网安全审核

**审核日期**：2026-05-15（v0.6.12）

**风险统计**：6 个高风险、4 个中风险、3 个低风险

**高风险问题**：
1. **RISK-01**: CORS 完全开放（允许任意源）
2. **RISK-02**: 默认弱密码 admin/admin，首次运行不强制修改
3. **RISK-03**: 数据库明文存储 API Key
4. **RISK-04**: 调试模式跳过所有认证
5. **RISK-05**: 无请求体大小限制（proxy 层 32MB）
6. **RISK-06**: 缺少速率限制和 IP 黑名单

**优先级建议**：
- 本地使用：P3（当前架构面向本地/内网）
- 公网部署前：P0（必须修复 RISK-01~06）
