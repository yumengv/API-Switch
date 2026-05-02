# Web Admin 管理端 — 设计与实施策划

> 版本: 2.1 | 更新日期: 2026-05-02

<environment_details>
Current time: 2026-05-02T15:40:45+08:00
</environment_details>
> 基于 0.5.0 稳定版讨论整理

---

## 1. 背景与动机

### 1.1 现状

API Switch 0.5.0 桌面版已基本稳定，基于 Tauri v2，跨平台。

### 1.2 场景需求

| 场景 | 问题 |
|------|------|
| 有桌面，想远程管理 | 手机/另一台电脑无法访问 |
| Linux 无头服务器 | Tauri 需要 display，无法启动 |
| Docker 容器 | 无 GUI 环境 |
| WSL 环境 | 可能无 X11/Wayland |
| 自动化运维 | 无法脚本化 |

### 1.3 目标

- 桌面+Web 双模式任选
- 无桌面时仅 Web 管理
- 单一二进制，无特殊编译

---

## 2. 设计决策

### 2.1 端口策略

- **双端口（默认）**：代理 9090（CORS 全开），管理 9099（Bearer Token，受限 CORS）
- **单端口**：`web_admin_port == listen_port` 时合并路由，使用不同 `layer` 区分 CORS 与认证
- **切换方式**：`web_admin_port` 配置项，用户在设置页修改即可切换

### 2.2 认证方式

- 设置页配置 `web_admin_username` / `web_admin_password`
- 浏览器访问管理端弹登录页
- 登录成功后返回临时 Token，后续请求使用 `Authorization: Bearer <token>`
- 初版密码明文存储（与现有 API Key 策略一致）

### 2.3 实施策略

- 先独立抽离 Web 前端，保持桌面版结构不变
- 前端复用现有组件、样式，API 调用改为 HTTP `fetch`
- 桌面版零影响

---

## 3. 配置项设计

### 3.1 新增 config key（SQLite KV）

| key | 类型 | 默认值 | 说明 |
|-----|------|--------|------|
| `web_admin_enabled` | bool（"0"/"1"） | "0" | Web 管理端开关 |
| `web_admin_username` | string | "" | 管理用户名 |
| `web_admin_password` | string | "" | 管理密码 |
| `web_admin_port` | i32 | "9099" | 管理端口（等于 `listen_port` 时为单端口模式） |

### 3.2 代码改动概览

- **`config_dao.rs`**：`AppSettings` 结构体新增四字段、`Default` 补默认值、`get_settings`/`update_settings` 完整读写。<br>- **`schema.rs`**：`defaults` `INSERT OR IGNORE` 新增四行，老用户升级自动补齐。<br>- **`types.ts`**：前端 `AppSettings` 接口新增四字段，`DEFAULT_SETTINGS` 补默认值。

### 3.3 启动逻辑

1. 读取 `web_admin_enabled`、`web_admin_username`、`web_admin_password`。
2. 若 enabled 为真且用户名、密码均非空：
   - `web_admin_port == listen_port` → 单端口模式：在 `ProxyServer` 的 Axum Router 中 `merge` 管理路由。
   - 否则 → 双端口模式：启动独立 `AdminServer` 监听 `web_admin_port`。
3. 条件不满足则不启动管理端。

### 3.4 无头环境自动策略

- 启动时检查环境变量 `API_SWITCH_ADMIN_USER` 与 `API_SWITCH_ADMIN_PASS`。
- 若存在且非空，写入 config 并启用 `web_admin_enabled`，便于 Docker/CI 直接配置。

### 3.5 启动失败处理

- 端口占用 → 记录日志并在桌面 UI 中提示，不影响代理服务。

---

## 4. 后端架构设计

### 4.1 运行模式

- **双端口**：`ProxyServer`（9090）+`AdminServer`（9099）独立，均共享 `AppState`。
- **单端口**：单一 `CombinedServer`（9090），通过 `Router::merge()` 合并管理路由，使用不同 `layer` 区分 CORS 与认证。
- **无头环境**：仅 HTTP 服务（代理+管理）运行，无 Tauri 窗口层。

### 4.2 管理路由独立构建

- `build_admin_router(state: AdminState) -> Router` 生成完整 `/admin/*` 路由。
- 双端口：`AdminServer` 直接启动该 Router。
- 单端口：`ProxyServer` 在构建主 Router 时 `merge(build_admin_router(...))`。

### 4.3 关键文件改动

- **`lib.rs`**：`AppState` 新增 `admin: Arc<RwLock<Option<AdminServer>>>`，启动逻辑在 proxy 自启后加入管理端启动/合并步骤。
- **`proxy/server.rs`**：`start()` 接受可选 `admin_router` 参数，在单端口模式下合并后一起 `serve`。
- **`commands/config.rs`**：`update_settings` 若 `web_admin_port` 变化，重启管理端服务。
- **`commands/admin_cmd.rs`**：新增 Tauri command `get_admin_status`，供桌面查询管理端运行状态。

### 4.4 新增模块结构

```
src-tauri/src/admin/
├─ mod.rs          # 导出 build_admin_router、AdminServer
├─ router.rs       # build_admin_router，实现所有 /admin/* 路由
├─ handlers.rs    # HTTP 业务处理，复用 DAO 层
└─ auth.rs        # 登录、Bearer Token 中间件、会话管理
```

### 4.5 AdminServer（双端口模式）

- 字段：`port`, `bind_address`（127.0.0.1）, `shutdown_tx`。
- `start()`：构建 `AdminState` → `build_admin_router` → `TcpListener::bind` → `axum::serve`（优雅关闭）。
- `stop()`：通过 `shutdown_tx` 发送关闭信号。
- `get_status()`：返回运行状态、端口等信息。
- `AdminState` 包含 `db、settings、proxy 引用、failure_counts、app_handle（Option）` 与 `login_sessions`（HashMap<token, expiry>）。

### 4.6 认证流程

1. **登录** `POST /admin/login`：校验 `web_admin_username/password`，成功生成随机 token（UUID），仅在内存 `login_sessions`（HashMap<token, expiry>）中保存 24 h 有效期，返回 token；失败 401。
2. **鉴权** 所有除 `/admin/login` 的路由走 Bearer Token 中间件：从 Authorization Header 提取 token，检查 `login_sessions` 是否存在且未过期，未通过返回 401。
3. **登出** `POST /admin/logout` 删除对应 token。

---

## 5. 前端架构补充

- **技术栈**：React + TypeScript + TanStack Query + i18next。
- **状态管理**：React Context + TanStack Query 缓存 `settings`。
- **路由守卫**：`RequireAuth` 读取 `Authorization` Header。
- **API 封装**：`src/web-admin/src/api.ts` 统一错误结构 `{error:{message,code}}`。
- **页面**：`Settings` 包含 `web_admin_*` 四项，提交时发送完整 `AppSettings`。
- **构建**：开发模式 `npm run dev` 读取文件系统；发布 `npm run build` 产出静态文件，由后端 `rust-embed`（或 `include_str!`）提供。

---

## 6. 安全加固

| 项目 | 实施方案 |
|------|----------|
| 密码存储 | SQLite 明文存储（与现有 API Key 保持），对外 API 永不返回 `web_admin_password`，前端使用 `type="password"` 输入框 |
| CSRF | SameSite=Strict cookie 或 JWT Header `Authorization` |
| CORS | 管理端仅允许可信前端域名，代理端开放所有来源 |
| Rate Limiting | `tower::limit::RateLimitLayer` 限制登录尝试次数 |
| 审计日志 | 关键操作（登录、设置变更）写入 `audit_log` 表 |

---

## 7. 关键陷阱与应对

- **完整对象更新**：`update_settings` 必须收到完整 `AppSettings`，前端强制全量提交，后端缺失字段返回 400。
- **`as any` / `Partial<T>`**：业务层统一使用完整结构体，避免运行时缺失字段导致覆盖。
- **状态冲突**：采用 **方案 A**（`Extension` 注入）保持 `ProxyState` 通过 `.with_state()`，`AdminState` 通过 `Extension` 注入。
- **旧用户升级**：`INSERT OR IGNORE` 自动补齐新 KV，`serde(default)` 保证向后兼容。
- **端口占用**：启动失败记录日志并在桌面 UI 中提示，不影响代理功能。

---

## 8. 实施计划

| 阶段 | 目标 | 关键任务 |
|------|------|-----------|
| Phase 0 | 基础抽取 & 错误统一 | - 抽取 `refresh_tray_if_enabled` 至 `lib.rs`<br>- 新增 `AdminError` 实现统一 HTTP 错误响应 |
| Phase 1 | Service 层 & 状态统一 | - 创建 `src-tauri/src/services/`（entry、channel、proxy、config）<br>- 实现 `Extension` 注入 `AdminState`（方案 A） |
| Phase 2 | 配置扩展 & 启动逻辑 | - `AppSettings` 增四字段及默认值<br>- `schema.rs` 添加默认 KV<br>- `AppState` 增 `admin` 字段并实现自启/合并逻辑 |
| Phase 3 | 前端 Web Admin 开发 | - 搭建 React 项目结构<br>- 实现登录、设置页面、API 调用<br>- 使用 TanStack Query 缓存与错误处理 |
| Phase 4 | 安全与发布 | - CORS、Rate‑limit、审计日志实现<br>- `rust-embed` 打包静态资源，开发/发布模式切换 |
| Phase 5 | 验证 & CI | - 完成功能、性能、回归验证矩阵<br>- CI 添加 `cargo test` + `npm test`，确保构建成功 |

---

## 9. 验证矩阵

| 功能 | 前端 | 后端 | 备注 |
|------|------|------|------|
| 登录 | ✅ UI、错误提示 | ✅ JWT 生成、密码校验 | 失败锁定 5 min |
| 设置读取 | ✅ 表单回填（密码遮蔽） | ✅ 不返回 `web_admin_password` | |
| 设置更新 | ✅ 全量提交 | ✅ 完整对象写入 DB | 部分字段提交应返回 400 |
| 静态资源服务 | ✅ 开发模式实时读取 | ✅ 发布模式 `rust-embed` 读取 | |
| 错误统一 | ✅ 根据 `AdminError` 展示 | ✅ HTTP 状态 + JSON 错误体 | |
| 权限保护 | ✅ 路由守卫 | ✅ `Extension<AdminState>` 检查 token | |

---

## 10. 风险评估

| 风险 | 影响 | 缓解措施 |
|------|------|----------|
| 完整对象更新失误 | 覆盖旧设置导致功能异常 | 前端强制全量提交，后端校验缺失字段返回 400 |
| 状态注入错误 | 500 错误 | 单元测试 `admin` 路由的 `Extension` 提取 |
| 静态资源未打包 | 生产环境 404 | CI 检查 `dist-web-admin` 是否存在，`rust-embed` 编译验证 |
| 密码泄露 | 安全风险 | 响应体永不返回密码字段，存储仅限本地 SQLite（加密文件系统） |
| 并发写入冲突 | 设置更新丢失 | SQLite `INSERT OR REPLACE` 并在 `update_settings` 使用事务 |

---

**此计划即为完整的 WEB ADMIN PLAN，覆盖设计、实现、测试与风险控制。**