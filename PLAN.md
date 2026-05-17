# API Switch - 项目计划书

> Personal API Management & Forwarding Center
> 版本: 0.6.33 | 更新日期: 2026-05-17

---

## 1. 项目概述

**API Switch** 是一款基于 Tauri v2 的桌面应用，用于统一管理和转发多个 AI API 渠道。面向个人本地使用，默认信任本机环境，不按公网多用户服务进行安全模型设计。

### 项目特性

API-Switch — 个人AI网关，一站管所有

多平台，绿色便携，桌面+Web双管理，三级容错 · 智能路由，个人AI网关
Multi-platform, Portable, Desktop + Web Dual Management, Triple Fault Tolerance · Smart Routing · Personal AI Gateway

一个入口，告别零散笔记。 多家AI服务商的API Key、模型、用量，集中管理、统一调用。

多渠道路由 + 三级容错。 一个入口访问多家服务商，不稳定模型自动冷却恢复、故障转移——分组匹配 → 模型匹配 → fallback，使用无缝衔接。

5套协议自由[对接][转换]： 同时适配 OpenAI-compatible / OpenAI-Responses / Claude / Gemini / Azure 五套协议，下游任意应用直接对接五套协议（支持 Claude、Codex 接入使用）。

灵活的分组能力：分组即路由。 分组本身可当模型名使用；也可不分组，用模型部分名称作为分组。
用量一目了然：Token 消耗统计。 实时数据看板。
详尽的请求日志：日志保留完整分析数据，便于分析定位问题。

全平台覆盖： Windows x64 / macOS (x64+arm64) / Linux x64，有桌面用桌面，无头服务器用 Web Admin。

一句话
API-Switch：绿色便携的个人AI网关——多渠道路由、5协议转换、智能容错，一站管理所有AI服务。


- **模型自定义分组**：用户可自由创建任意分组（如 auto / code / fast / 自定义名称），将模型分配到不同组，按用途组织管理
- **无需定义分组即可使用**：模型可不归属任何分组，直接参与路由；自定义分组仅为组织工具，非必需
- **智能路由**：分组精确匹配 → 模型模糊匹配 → AUTO 组 fallback，三级容错
- **多渠道路由 + 故障转移**：一个入口访问多个 AI 服务商，冷却 + 自动 failover
- **用量可视化**：实时 Dashboard + 请求日志 + Token 消耗统计


- **模型星级/权重**：渠道与模型可设星级，继承 + 覆盖机制，路由时高星级优先
- **流式模型信息显示**：流式回答正常结束时可追加实际命中模型名，设置页可关闭

### 已实现特性总览

> 本节记录当前版本已经具备的用户可见能力，按使用场景分组；实现细节见后续模块、数据流和数据库章节。

#### 1. API 服务配置

- 支持管理 OpenAI、Claude、Gemini、Azure 与 Custom 五类 API 渠道，记录名称、协议类型、Base URL、API Key、启用状态、备注和响应时间。
- 支持渠道新增、编辑、删除、启用/禁用、Base URL 可达性探测、模型列表拉取、模型选择和同步到 API 池。
- 支持 Custom 渠道接入任意 OpenAI 兼容或中转站服务；当模型列表接口不可用时，可在 API 管理页手动添加模型。
- 支持内置模型目录 `models.json`，为模型展示提供发布信息、上下文长度、能力标签、输入/输出模态、费用信息和中英文说明。
- 支持模型目录元数据预计算入库与旧数据回填，UI 渲染优先读取数据库字段，缺失时再 fallback 到前端目录。

#### 2. 模型管理与请求路由

- 支持 API 池条目管理：手动新增模型、删除模型、启用/禁用 AUTO 参与、切换分组、拖拽/排序、响应时间记录和一键测速。
- 支持用户自定义模型分组，`group_name = "auto"` 作为 AUTO 组；未归组模型仍可通过模型名直接参与路由。
- 支持统一路由规则：先按分组精确匹配，再按模型名模糊匹配，最后 fallback 到 AUTO 组；匹配比较不区分大小写并会 trim 输入。
- 支持三种排序策略：自定义顺序（`sort_index`）、最快优先（`response_ms`）、最新优先（`release_date`）。
- 支持冷却感知与熔断感知路由，已禁用、处于冷却期或熔断打开的条目不会进入当前可尝试列表。
- 支持同一模型在多个渠道间 failover；上游失败后自动尝试下一个可用条目，全部失败时返回统一代理错误。

#### 3. 代理协议与兼容端点

- 提供 OpenAI 兼容本地入口，客户端只需把 Base URL 指向 API Switch，即可复用 `/v1/models`、`/v1/chat/completions` 等常见接口。
- 支持 OpenAI、Claude、Gemini、Azure、Custom 五套协议适配器，各自独立处理认证方式、URL 构造、请求转换、响应转换和模型列表解析。
- 支持 Claude Messages 与 OpenAI Chat Completions 之间的请求/响应转换，包含系统消息、工具调用、图片内容、停止原因和用量映射。
- 支持 Gemini OpenAI 兼容端点，并提供 Gemini 原生 `generateContent` / `streamGenerateContent` 到 OpenAI 格式的转换能力。
- 支持 Azure OpenAI deployment 路由，请求可通过部署名优先匹配对应 API 池条目。
- 支持 `/v1/responses` 兼容层，将 OpenAI Responses API 常用输入转换为 Chat Completions 请求，并支持文本、函数工具、图片输入、流式事件和非流式响应。

#### 4. 故障转移、冷却与运行保护

- 支持请求失败后写入失败日志，并根据配置对异常模型设置临时冷却，避免持续命中故障上游。
- 支持内存熔断器，按连续失败次数进入 Open 状态，恢复时间到期后进入 HalfOpen 并允许探测恢复。
- 支持按状态码区分处理策略，例如 401/403/410 可触发自动禁用或长冷却，429/5xx/网络错误可进入可恢复冷却。
- 支持成功请求自动清理冷却状态和失败计数，让恢复后的模型重新参与路由。
- 支持流式请求的超时、错误、正常结束和下游断开等结束原因记录，用于后续冷却策略和日志分析。
- 支持对日志中的敏感 URL 参数进行脱敏，避免 `key`、`api_key`、`token` 等内容直接暴露。

#### 5. 桌面端界面与交互

- 提供 Dashboard、渠道管理、API 管理、令牌管理、使用日志、系统设置、CLI 配置、使用指南等主页面。
- 支持 Dashboard 统计卡片与图表，展示请求量、成功率、Token 消耗、模型分布、调用趋势、模型排行和用户趋势。
- 支持使用日志分页查看、错误筛选、行展开详情和实时日志推送，便于定位上游错误与路由路径。
- 支持测试对话弹窗，可对单个模型直接发送测试请求并查看响应、耗时、Token 和错误信息。
- 支持全局 toast 反馈，用于保存、删除、测速、模型拉取、登录、代理启停、检查更新等用户操作。
- 支持系统托盘菜单，围绕 AUTO 组模型提供快捷优先级调整与窗口显示/隐藏能力。
- 支持首次启动欢迎引导、中英文使用指南、暗色/亮色主题和中英文 UI 切换。

#### 6. 设置中心、持久化与本地运行

- 使用 SQLite 本地持久化渠道、API 池条目、访问密钥、使用日志、审计日志和全局配置；默认启用 WAL 模式。
- 支持启动时自动建表、补齐新增字段和默认配置项，保证老版本数据库升级后的兼容性。
- 支持代理端口、访问密钥校验、冷却时间、熔断阈值、默认排序模式、语言、主题、自动启动、托盘行为、Web Admin 等配置。
- 支持桌面端通过 Tauri IPC 访问后端能力，Web Admin 通过 HTTP Adapter 访问同一领域能力。
- 支持完整对象写回设置契约，Rust 端 `updateSettings()` 期望完整 `AppSettings`，避免局部 patch 导致字段丢失。
- 支持便携式使用，发布包可作为本地个人 API 网关运行，数据默认保存在本地数据库中。

#### 7. 认证、安全与访问边界

- 区分上游供应商 API Key 与客户端访问 Access Key；上游密钥只保存在本地配置中，不下发给客户端。
- 支持 Bearer Access Key 校验，可在设置中开启或关闭强制校验；未开启时 Access Key 仍可用于日志身份追踪。
- 支持 Access Key 新增、复制、启用/禁用和删除。
- 支持 Web Admin 登录、登出、Token 校验与后端鉴权，生产态管理接口需要 Bearer Token。
- 支持审计日志记录关键管理操作，为设置变更、登录行为和管理动作提供追踪依据。
- 当前安全模型定位为个人本地工具，默认信任本机环境，不按公网多用户服务设计；外网部署需另行补齐鉴权、CSP、权限和反向代理策略。

#### 8. Web Admin、CLI 与维护支持

- 支持同一套 React 页面在桌面端和 Web Admin 中复用，通过 `tauriApiAdapter` 与 `webAdminApiAdapter` 适配不同运行环境。
- 支持 Web Admin 独立构建产物 `dist-web-admin/`，并提供登录、状态、设置、渠道、API 池、令牌、统计、日志和代理控制等 HTTP 管理接口。
- 支持 CLI 配置页，为 OpenCode、Claude Code、Codex CLI 等工具生成 API Key、Base URL、Provider、HTTPS_PROXY 等环境变量配置。
- 支持检查 GitHub 最新版本，提供手动更新检查入口和 release 链接。
- 支持 Playwright 端到端测试与 Tauri/Web 双构建命令，覆盖管理端关键交互和页面可用性。
- 支持 GitHub Actions 多平台 release 构建，覆盖 Windows x64、macOS x64、macOS arm64、Linux x64，并生成版本化发布产物。

### 技术特性清单（自动化扫描）

> 本节由自动化扫描生成，记录项目当前已实现的所有技术特性，按功能模块分类。

#### 前端技术特性

| 特性类别 | 实现详情 | 关键文件 |
|----------|----------|----------|
| **页面级组件** | 8个页面组件：ApiPoolPage、ChannelPage、CliPage、DashboardPage、LogPage、SettingsPage、TokenPage、WelcomeGuide | `src/pages/*.tsx` |
| **路由/页面切换** | 使用 `lazy` 动态导入页面，`MainShell` 定义导航栏实现页面切换 | `src/App.tsx`、`src/features/shell/MainShell.tsx` |
| **状态管理** | 使用 TanStack React Query v5 进行数据获取、轮询和缓存 | 10+ 文件使用 `useQuery` |
| **国际化** | 使用 i18next + react-i18next，15+ 文件调用 `useTranslation` | `src/i18n/`、多组件文件 |
| **UI 组件库** | 13个 Radix UI 组件：Button、Card、Checkbox、Dialog、Input、Label、Popover、ScrollArea、Select、Separator、Slider、Switch、Tabs | `src/components/ui/*.tsx` |
| **图表组件** | 使用 Recharts v3 绘制统计图表（柱状图、折线图等） | `src/features/dashboard/DashboardView.tsx` |
| **拖拽功能** | 使用 @dnd-kit 实现模型/渠道的拖拽排序 | `src/features/pool/PoolManager.tsx` |
| **自定义 Hook** | 3个自定义 Hook：useApiAdapter（API 适配）、useDragScroll（滚动交互）、useTauriEvent（Tauri 事件监听） | `src/lib/use*.ts` |
| **表单处理** | 项目当前未使用 react-hook-form，表单通过受控组件实现 | — |
| **错误边界** | 全局 ErrorBoundary 组件捕获渲染错误 | `src/components/ErrorBoundary.tsx` |
| **登录认证** | Web Admin 模式下的登录页面和 Token 管理 | `src/components/LoginScreen.tsx` |
| **代理控制** | 代理启停开关组件，使用 TanStack Query 和 Radix UI | `src/components/proxy/ProxyToggle.tsx` |
| **测试对话** | 测试对话弹窗，直接调用 Tauri 命令请求上游 | `src/components/proxy/TestChatDialog.tsx` |

#### 后端 Rust 技术特性

| 特性类别 | 实现详情 | 关键文件 |
|----------|----------|----------|
| **Tauri 命令** | 10+ 文件包含 #[tauri::command] 函数，覆盖渠道管理、池管理、配置、代理控制、翻译、使用统计等 | `src-tauri/src/commands/*.rs` |
| **数据库 Schema** | 6个表：channels、api_entries、access_keys、usage_logs、audit_log、config | `src-tauri/src/database/schema.rs` |
| **DAO 模块** | 4个数据访问对象：channel_dao、api_entry_dao、access_key_dao、usage_dao、config_dao | `src-tauri/src/database/dao/*.rs` |
| **Axum 代理服务器** | 使用 Axum 0.7 构建 HTTP 代理服务器，支持 CORS 中间件 | `src-tauri/src/proxy/server.rs` |
| **请求处理** | 处理函数：health_check、handle_chat_completions、handle_messages、handle_list_models | `src-tauri/src/proxy/handlers.rs` |
| **智能路由** | 模型解析函数 `resolve`，支持分组精确匹配 → 模型模糊匹配 → AUTO 组 fallback | `src-tauri/src/proxy/router.rs` |
| **请求转发** | `forward_with_retry` 实现 failover、冷却、日志、SSE 流式处理 | `src-tauri/src/proxy/forwarder.rs` |
| **熔断器** | 内存三态熔断器（Closed/Open/HalfOpen），辅助 DB 冷却 | `src-tauri/src/proxy/circuit_breaker.rs` |
| **协议适配** | 5种独立适配器模块，实现 ProtocolAdapter trait | `src-tauri/src/proxy/protocol/*.rs` |
| **配置管理** | AppSettings 结构体，键值表持久化，L1 内存缓存 | `src-tauri/src/database/dao/config_dao.rs` |
| **错误处理** | 3个错误枚举：AppError（全局）、AdminError（管理端）、ProxyError（代理） | `src-tauri/src/error.rs`、`src-tauri/src/admin/error.rs`、`src-tauri/src/proxy/handlers.rs` |
| **日志系统** | 使用 log 宏（info!/warn!/error!/debug!），分布在启动、代理、配置、服务等模块 | 多文件 |
| **系统托盘** | 托盘菜单构建 `build_tray_menu`、事件处理 `handle_tray_menu_event`，支持 AUTO 组模型优先级调整 | `src-tauri/src/lib.rs` |
| **数据库迁移** | 启动时自动建表、补齐新增字段和默认配置项，保证老版本数据库兼容 | `src-tauri/src/database/schema.rs` |

#### API 协议适配器特性

| 适配器 | 认证方式 | 端点 | 请求/响应转换 | SSE 处理 | 错误映射 |
|--------|----------|------|---------------|----------|----------|
| **OpenAI** | Bearer Token | `/v1/chat/completions`、`/v1/models` | 直接透传，仅确保 model 字段存在 | 直接使用 OpenAI SSE | 无专门转换 |
| **Claude (Anthropic)** | x-api-key 头 + anthropic-version | `/v1/messages`、`/v1/models` | 完整双向转换：messages、tools、tool_choice、stop、response_format | ClaudeSSETransformer 双向转换 | transform_claude_error 映射 HTTP 状态码 |
| **Azure OpenAI** | api-key 头 | `/openai/deployments/{model}/chat/completions`、`/openai/models` | 删除请求体中的 model 字段，模型部署名写入 URL | 直接透传 Azure SSE | 同 OpenAI |
| **Google Gemini** | 查询参数 `?key=` | `/v1beta/openai/chat/completions`、`/v1beta/openai/models` | 仅写入 model 字段；提供原生 Gemini 转换函数备选 | 直接透传 OpenAI 兼容 SSE | 无专门转换 |
| **Custom** | Bearer Token | 用户 base_url 完整路径 | 与 OpenAI 相同，简单透传 | 直接透传 | 同 OpenAI |

**通用机制**：
- **熔断/重试**：所有适配器通过 `CircuitBreaker`（`src-tauri/src/proxy/circuit_breaker.rs`）统一记录成功/失败，实现自动冷却。
- **转发核心**：`src-tauri/src/proxy/forwarder.rs` 调用适配器的 `apply_auth`、`transform_request`、`transform_response`，并在错误后触发熔断。
- **模型映射**：每个适配器实现 `parse_models_response`，统一为 `(id, Option<display_name>)` 格式。

#### 配置与设置系统特性

| 特性类别 | 实现详情 | 关键文件 |
|----------|----------|----------|
| **配置结构体** | `AppSettings` 包含代理、Circuit Breaker、UI、Web Admin 等所有可配置项 | `src-tauri/src/database/dao/config_dao.rs` |
| **持久化存储** | SQLite config 表（KV 存储），支持 get_settings / update_settings | `src-tauri/src/database/dao/config_dao.rs` |
| **L1 内存缓存** | `AppState.settings: Arc<RwLock<AppSettings>>`，启动时从 DB 全量加载，运行时统一从 L1 读取 | `src-tauri/src/lib.rs` |
| **前端设置页面** | SettingsPage 组件，使用 i18next、Radix UI、useQuery | `src/pages/SettingsPage.tsx` |
| **Tauri 命令** | `get_settings`、`update_settings`、`check_update` | `src-tauri/src/commands/config.rs` |
| **默认配置** | 在 schema.rs 的 `defaults` 数组中定义，使用 `INSERT OR IGNORE` 补缺失 key | `src-tauri/src/database/schema.rs` |
| **配置验证** | Rust 端 `update_settings()` 期望完整 `AppSettings` 对象，不支持 Partial 更新 | `src-tauri/src/database/dao/config_dao.rs` |
| **Web Admin 设置** | Web 模式支持完整对象 update + PATCH，带版本号冲突检测 | `src/lib/webAdminApiAdapter.ts` |

#### 测试与部署特性

| 特性类别 | 实现详情 | 关键文件 |
|----------|----------|----------|
| **测试框架** | Playwright 端到端测试，配置 testDir 为 `./tests/e2e` | `playwright.config.ts` |
| **测试文件** | 示例测试 `admin.spec.ts`，验证后台管理页面 | `tests/e2e/admin.spec.ts` |
| **构建脚本** | `gen-icons.cjs`（生成多尺寸图标）、`versioned-build.cjs`（版本化产物命名） | `scripts/*.cjs` |
| **CI/CD 工作流** | GitHub Actions 矩阵构建：Windows x64、macOS Intel、macOS Apple Silicon、Linux x64 | `.github/workflows/release.yml` |
| **跨平台支持** | 四平台一键打包，自动处理系统依赖（如 Ubuntu 的 libwebkit2gtk） | CI 矩阵配置 |
| **版本管理** | package.json 版本字段（0.5.0）+ Git 标签触发 CI + 自动产物命名 | `package.json`、`scripts/versioned-build.cjs` |
| **发布流程** | 推送 `v*` 标签 → CI 构建四平台 → 上传 Artifact → 创建 Draft Release | `.github/workflows/release.yml` |
| **Web Admin 构建** | 独立 Vite 构建配置，产物 `dist-web-admin/`，Tauri 模块走 stub | `vite.config.web.ts` |

#### 其他技术特性

| 特性类别 | 实现详情 | 关键文件 |
|----------|----------|----------|
| **双运行时适配** | 同一套 React 页面通过 `tauriApiAdapter` / `webAdminApiAdapter` 适配桌面端和 Web 端 | `src/lib/useApiAdapter.ts`、`src/lib/tauriApiAdapter.ts`、`src/lib/webAdminApiAdapter.ts` |
| **Tauri v2 安全** | capabilities 配置、CSP 策略（当前 csp=null，需后续收紧） | `src-tauri/tauri.conf.json`、`src-tauri/capabilities/` |
| **数据库 WAL 模式** | SQLite 启用 WAL 模式，提升并发读写性能 | `src-tauri/src/database/mod.rs` |
| **请求超时分层** | 全局 read_timeout 120s（临时方案），connect_timeout 15s | `src-tauri/src/proxy/server.rs` |
| **日志脱敏** | 对敏感 URL 参数（key、api_key、token）进行脱敏 | `src-tauri/src/proxy/forwarder.rs` |
| **流式模型名注入** | 流式回答正常结束时追加 `model: <实际命中模型>`，由设置开关控制 | `src-tauri/src/proxy/forwarder.rs` |
| **Responses API 兼容** | `/v1/responses` 端点将 Responses API 格式转换为 Chat Completions 格式 | `src-tauri/src/proxy/responses_handler.rs` |
| **模型目录** | 内置 `models.json` 模型目录，提供发布信息、上下文长度、能力标签等元数据 | `models.json`、`src/lib/modelsCatalog.ts` |
| **限额查询** | 统一查询多个供应商的 Token Plan / 限额信息（Kimi、智谱、MiniMax） | `src-tauri/src/commands/limit.rs` |
| **CLI 配置生成** | 为 OpenCode、Claude Code、Codex CLI 等工具生成环境变量配置 | `src/pages/CliPage.tsx`、`cli.json` |

### 核心价值

- **多渠道路由**：一个入口访问多个 AI 服务商，按模型自动匹配或手动指定
- **提升可用性**：模型冷却 + 自动故障转移，降低单渠道故障对使用的影响
- **用量可视化**：实时 Dashboard + 请求日志 + Token 消耗统计
- **轻量桌面**：Tauri v2 架构，内存占用低，跨平台

---

## 2. 技术架构

### 2.1 技术栈

| 层级 | 技术 | 说明 |
|------|------|------|
| 桌面框架 | Tauri v2 | Rust 后端 + Web 前端 |
| 后端语言 | Rust 1.85+ | 高性能异步运行时 |
| HTTP 框架 | axum 0.7 | 代理服务器 & API 路由 |
| HTTP 客户端 | reqwest 0.12 | 转发请求到上游 (rustls-tls) |
| 数据库 | rusqlite 0.31 (bundled) | 嵌入式 SQLite，WAL 模式 |
| 前端框架 | React 19 + TypeScript 5.8 | UI 渲染层 |
| UI 组件 | Radix UI + Tailwind CSS v4 | 无障碍组件库 |
| 状态管理 | TanStack React Query v5 | 服务端状态缓存与自动刷新 |
| 图表 | Recharts v3 | Dashboard 可视化 |
| 国际化 | i18next + react-i18next | 中/英双语 |
| 拖拽 | @dnd-kit | API 管理排序 |

### 2.2 整体架构

```
┌─────────────────────────────────────────────────────┐
│                    Tauri App Window                   │
│  ┌───────────────────────────────────────────────┐  │
│  │              React Frontend (Vite)             │  │
│  │ Dashboard │ Channel │ API 管理 │ Token │ Logs │ Settings │ Guide │  │
│  └──────────────┬────────────────────────────────┘  │
│                 │ Tauri IPC (invoke)                  │
│  ┌──────────────▼────────────────────────────────┐  │
│  │           Tauri Commands Layer                 │  │
│  └──────────────┬────────────────────────────────┘  │
│                 │                                     │
│  ┌──────────────▼────────────────────────────────┐  │
│  │              AppState (Arc<Database>)          │  │
│  └──────────────┬────────────────────────────────┘  │
│                 │                                     │
│  ┌──────────────▼────────────────────────────────┐  │
│  │           SQLite (rusqlite + Mutex)            │  │
│  │  channels │ api_entries │ access_keys │ logs   │  │
│  └───────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────┘

         ▲ 并行运行
┌─────────────────────────────────────────────────────┐
│              Axum Proxy Server (0.0.0.0:port)        │
│  ┌─────────────┐  ┌─────────┐  ┌────────────────┐  │
│  │   /health   │  │ /v1/models│  │/v1/chat/completions│
│  └─────────────┘  └─────────┘  └───────┬────────┘  │
│                                       │             │
│  ┌──────────▼──────────────────────────────────┐   │
│  │  Auth → Router → Forwarder (retry+failover)  │   │
│  │         ↕ Cooldown (per entry, DB 持久化)     │   │
│  └──────────┬──────────────────────────────────┘   │
│             │ reqwest → Upstream APIs               │
└─────────────────────────────────────────────────────┘
```

---

## 3. 模块详解

### 3.1 后端模块 (`src-tauri/src/`)

| 模块 | 文件 | 职责 |
|------|------|------|
| 入口 | `main.rs`, `lib.rs` | Tauri 初始化、托盘菜单、代理自启 |
| 数据库 | `database/mod.rs`, `schema.rs` | SQLite 连接、建表、兼容迁移 |
| 数据访问 | `database/dao/*.rs` | 渠道/条目/密钥/日志/配置 CRUD |
| 代理服务器 | `proxy/server.rs` | Axum HTTP 服务器、graceful shutdown |
| 请求处理 | `proxy/handlers.rs` | 入口路由、Access Key 验证 |
| 智能路由 | `proxy/router.rs` | AUTO/显式模型匹配、冷却过滤 |
| 请求转发 | `proxy/forwarder.rs` | Failover、冷却、日志、SSE 流式处理 |
| 冷却机制 | `proxy/circuit_breaker.rs` | 内存三态熔断器（辅助 DB 冷却） |
| 协议适配 | `proxy/protocol/*.rs` | 5 种 API 类型独立适配器 |
| 认证 | `proxy/auth.rs` | Bearer Key 提取与验证 |
| 命令层 | `commands/*.rs` | Tauri IPC 接口 |

### 3.2 前端模块 (`src/`)

| 页面 | 文件 | 功能 |
|------|------|------|
| API 管理 | `ApiPoolPage.tsx` | 拖拽排序、启停、状态点（绿/红/灰）、测试对话、一键测速、响应时间显示 |
| 渠道管理 | `ChannelPage.tsx` | 统一添加/编辑弹窗、模型拉取/选择 |
| 令牌管理 | `TokenPage.tsx` | 密钥 CRUD + 复制 |
| 使用日志 | `LogPage.tsx` | 分页、成功/失败筛选、点击行展开详情 |
| 数据看板 | `DashboardPage.tsx` | 统计卡片 + 4 图表 |
| 系统设置 | `SettingsPage.tsx` | 代理、安全、冷却、托盘、通用设置 |
| 使用指南 | 侧边栏菜单项 | 按语言切换中/英文 GUIDE（GUIDE_CN.md / GUIDE.md） |
| 测试对话 | `TestChatDialog.tsx` | 直接调 Tauri 命令请求上游，不走代理 |

---

## 4. 数据流

### 4.1 请求代理流程

```
Client → POST /v1/chat/completions
  │
  ├─ 1. auth::extract_access_key()       ← 从 Header 提取并验证密钥
  ├─ 2. 解析 JSON body → model / stream
  ├─ 3. router::resolve()                ← 先精确匹配分组，再模糊匹配模型，再 fallback 到 AUTO 组，最后才返回错误
  ├─ 4. forwarder::forward_with_retry()
  │     ├─ 遍历 entries:
  │     │   ├─ adapter.build_chat_url() + apply_auth() + transform_request()
  │     │   ├─ reqwest::send()
  │     │   ├─ 成功 → 清除冷却 → 返回客户端
  │     │   └─ 失败 → 设置冷却 → 继续下一个
  │     └─ 全部失败 → 502 AllProvidersFailed
  └─ 5. insert_usage_log()
```

### 4.2 当前路由规则（唯一真相）

| 场景 | 行为 |
|---|---|
| 路由总规则 | 先做**分组精确匹配**，未命中则做**模型模糊匹配**，再未命中则 **fallback 到 AUTO 组**，AUTO 组也没有可用条目时按现有失败流程返回错误 |
| 匹配预处理 | `request.model`、`group_name`、`entry.model` 在匹配前统一 `trim`；空模型名 `""` 直接替换为 `"auto"` |
| 分组匹配 | `request.model` 与 `group_name` 的比较**不区分大小写**；未匹配时继续进入模型模糊匹配 |
| 模型匹配 | `request.model` 与 `entry.model` 的模糊匹配**不区分大小写**；规则为 **`entry.model` 包含 `request.model`** |
| `model = "auto"` | 不做特殊优先分支；和任何模型请求一样，按统一流程参与匹配。由于空模型会先替换成 `auto`，因此空模型请求也按同一流程处理 |
| AUTO 组定义 | AUTO 组就是 `group_name = "auto"`，不再受 settings / API 管理页当前分组 / tray 状态影响 |
| 最终失败 | 当分组精确匹配、模型模糊匹配、AUTO 组 fallback 都没有可用条目时，按当前正常模型请求失败流程处理 |

### 4.3 冷却与可用性规则

```text
正常请求 → 直接返回，并清除该模型 cooldown_until / 连续失败计数
不正常请求 → 写失败日志，设置 cooldown_until = now + N 秒，继续 failover
可用性判断 → 模型是否可用只看 enabled / cooldown_until / 内存熔断结果
AUTO 路由 → 只选择 enabled=true 且未冷却、未熔断的模型
匹配命中后 → 若某条目因 enabled=false / 冷却 / 熔断不可用，则不会进入可尝试列表
用户开关 → enabled 主要用于控制是否进入当前可尝试池，现行实现不再让 settings.active_group 参与可用性判断
```

### 4.4 UI 分组与 Tray 语义

| 项 | 当前语义 |
|---|---|
| 设置里的模型分组 (`active_group`) | 只用于 **API 管理页默认分组 / UI 记忆**，不参与路由 |
| API 管理页分组切换 | 只影响当前页面筛选/恢复体验，不应再升级为系统级 routing/tray 状态 |
| Tray 规则 | tray 只关联 **AUTO 组**，取消分组选择，也不再写入模型分组设置 |
| Tray 点击行为 | 只调整 AUTO 组条目的优先级快捷顺序，不切换分组，不改变 routing 规则 |

---

## 5. 数据库兼容开发规范

> **规则**：每个版本新增数据库字段或配置项时，必须在启动检查机制中补齐，确保老用户升级后数据库自动兼容。

| 检查项 | 机制 | 位置 | 说明 |
|--------|------|------|------|
| 新增表字段 | `ensure_column()` | `schema.rs` → `ensure_*_columns()` | `PRAGMA table_info` 检查，不存在则 `ALTER TABLE ADD COLUMN` |
| 新增 config key | `INSERT OR IGNORE` | `schema.rs` → `defaults` 数组 | 不覆盖用户已有值，只补缺失的 key |
| 旧默认值迁移 | `UPDATE ... WHERE value = '旧值'` | `schema.rs` → `create_tables()` 末尾 | 只迁移未修改的旧默认值 |
| 前端类型同步 | `types.ts` | `ApiEntry` / `AppSettings` | 新增后端字段必须同步前端类型 |

**每次新增字段/配置必须做的事**：

1. `schema.rs`：建表 SQL 中加新字段
2. `schema.rs`：`ensure_*_columns()` 中加 `ensure_column()` 调用
3. `schema.rs`：`defaults` 数组中加新 config key 默认值
4. `config_dao.rs`：`AppSettings` 中加字段 + `get_settings()` 读取 + `update_settings()` 写入
5. `types.ts`：`ApiEntry` / `AppSettings` 中加对应字段
6. 如有旧值需迁移，在 `create_tables()` 末尾加 `UPDATE` 语句

---

## 6. 表结构

| 表名 | 用途 | 关键字段 |
|------|------|----------|
| `channels` | API 渠道 | id, name, api_type, base_url, api_key, available_models(JSON), selected_models(JSON), enabled |
| `api_entries` | 路由池条目 / 对外可见模型 | id, channel_id, model, display_name, sort_index, enabled(AUTO 参与开关), cooldown_until, response_ms, provider_logo, release_date, model_meta_zh/en |
| `access_keys` | 访问密钥 | id, name, key(UUID), enabled |
| `usage_logs` | 请求日志 | 25+ 字段，含 token 统计、延迟、错误信息 |
| `config` | 全局配置 | KV 存储 |

---

## 7. 协议适配

5 种 API 类型各自独立实现 `ProtocolAdapter` trait，互不影响：

| API 类型 | 认证方式 | 聊天端点 | 模型列表端点 | 说明 |
|----------|---------|---------|-------------|------|
| `openai` | Bearer | `/v1/chat/completions` | `/v1/models` | 标准 OpenAI |
| `claude` | x-api-key | `/v1/messages` | `/v1/models` | 完整格式转换 |
| `gemini` | ?key= 查询参数 | `/v1beta/openai/chat/completions` | `/v1beta/openai/models` | Google OpenAI 兼容端点 |
| `azure` | api-key header | `/openai/deployments/{model}/chat/completions` | `/openai/models` | Deployment 名路由 |
| `custom` | Bearer | 用户 base_url 完整路径 | 用户 base_url 完整路径 | 不自动拼接 /v1；若模型列表接口不可用，可手动加入 API 池 |

---

## 8. 设计取舍

| 项 | 当前取舍 | 原因 |
|---|---|---|
| API Key 明文存储 | 接受 | 个人本地工具降低复杂度 |
| Access Key 可关闭 | 接受 | 本机使用优先降低门槛 |
| SQLite + Mutex | 接受 | 单机低并发场景足够 |
| 冷却状态 DB 持久化 | 接受 | 重启后坏模型不会立即恢复 |
| CORS 宽松 | 接受 | 非公网服务 |
| Custom base_url 不拼接 /v1 | 接受 | 用户填写完整版本路径 |
| 不新增 API 类型处理中转站 | 接受 | 用 openai/custom 类型 + 手动添加模型即可；模型列表不可用不影响显式调用 |

---

## 9. 待开发 / 改进项

### P2 — Gemini 原生协议端点补全（不影响当前使用）

> Gemini 原生协议端点 `/v1beta/models/*rest` 当前已支持 `:generateContent`（非流式）和 `:streamGenerateContent`（流式 SSE），以及 `GET /v1beta/models/{model}`（单模型详情）。以下为未实现的 Gemini 原生 API，需要直接转发 Gemini 上游完成，无法通过现有路由 + 格式转换链路实现。

- [ ] **`:countTokens`（POST）** — 计算输入内容的 token 数。需要直接请求 Gemini 上游的 `models/{model}:countTokens` 端点，不经过格式转换和路由选择。
- [ ] **`:embedContent`（POST）** — 文本嵌入。需要直接请求 Gemini 上游的 `models/{model}:embedContent` 端点。
- [ ] **`:batchEmbedContents`（POST）** — 批量文本嵌入。需要直接请求 Gemini 上游的 `models/{model}:batchEmbedContents` 端点。
- [ ] **`:batchGenerateContent`（POST）** — 批量内容生成。需要直接请求 Gemini 上游的 `models/{model}:batchGenerateContent` 端点。
- [ ] **`cachedContents` 系列** — 上下文缓存管理（CRUD），复杂状态管理，当前无对应基础设施。

### P2 — Claude 协议边角一致性优化（不影响当前使用）

> 结论：Claude response 主协议（非流式 message、流式 SSE 主事件序列、tool_use、usage、stop_reason）当前已可正常使用，现有实现不会阻塞当前代理主链路与客户端使用。以下事项属于“协议边角严格一致性优化”，不作为当前版本阻塞项，后续按 P2/P3 继续处理。

- [ ] **Claude 非流式响应边角字段严格对齐**:
    - **现状**: `src-tauri/src/proxy/protocol/claude.rs::openai_to_claude_response()` 已正确映射 `id`、`type`、`role`、`model`、`content`、`tool_use`、`stop_reason`、`usage` 等主字段。
    - **说明**: 主协议字段已满足当前使用；少量边角字段是否以 Claude 官方最原生方式保留，仍可后续继续纯化。
    - **后续关注**:
        1. `stop_sequence` 在输出面上的严格保留策略。
        2. 某些未来扩展字段在 Claude 输出结构中的更细粒度归位。
        3. 在不破坏当前 passthrough 公理的前提下，减少“兼容承载”字段与“官方标准字段”之间的语义混放。

- [ ] **Claude thinking / provider_specific 输出面纯化**:
    - **现状**: 当前实现对 Claude 主协议可用性无影响；thinking 相关信息已具备兼容承载路径。
    - **说明**: 该项属于“更贴近官方原生表达”的优化，而不是当前功能缺陷。
    - **后续关注**:
        1. 评估 `thinking` 信息在 OpenAI ↔ Claude 双向转换中的最原生落位。
        2. 明确哪些字段属于 Claude 官方语义，哪些仅为本代理内部兼容承载。

- [ ] **Claude 流式 SSE 边角事件一致性补测**:
    - **现状**: `ClaudeSSETransformer` 与 `transform_openai_sse_to_claude_stream()` 已覆盖主事件序列，Claude 相关测试当前通过。
    - **说明**: 正常使用不受影响；该项关注极端事件组合与未来协议扩展，不阻塞当前版本。
    - **后续关注**:
        1. 补充多 tool_use 连续切换、空块、边界 usage-only frame、thinking/tool_use 交织等场景测试。
        2. 以 Anthropic 官方流式示例为基线，补一轮逐事件对照测试。

### P0 — 协议架构调整：中立 IR + Capability Router

> 结论：OpenAI Chat Completions 不能继续作为内部中间协议。它只是外部协议之一，不是中立协议模型。当前 `/v1/responses` 为兼容 Chat 上游而被压扁成 Chat 请求，已经在工具调用场景暴露 `function is not set`、`Tools[N].Type invalid` 等问题。后续协议架构必须改为“外部协议 → 自定义中立 IR → 能力路由 → 目标协议 Adapter”。

- [ ] **定义 API Switch 自定义中立 IR（Internal Request/Response）**:
    - **现状**: 当前多个协议转换路径仍以 OpenAI Chat Completions 结构作为事实中间层，Responses、Claude、Gemini 等协议的高阶语义会被迫压扁或透传。
    - **风险**:
        1. Responses 的 `input/output item`、`custom/web_search/file_search` 工具、`previous_response_id`、reasoning、annotations 等无法无损落入 Chat schema。
        2. Chat-only 上游会收到不兼容工具结构，触发上游 400，而不是由本代理提前给出可理解错误。
        3. 未来接入原生 Responses 上游时，如果主链路仍先转 Chat，会破坏原生协议语义。
    - **目标**: 建立比 Chat 更宽的内部表示，至少覆盖 content blocks、tools、tool calls、modalities、reasoning、metadata、streaming state、原始协议扩展字段。
    - **要求**: OpenAI Chat、OpenAI Responses、Claude、Gemini、Custom 都只作为 inbound/outbound adapter，不得作为系统内核中间协议。

- [ ] **引入 Capability Router（请求需求 × 上游能力）**:
    - **现状**: 路由主要按模型名、分组、冷却、排序选择条目；协议能力与工具能力未作为一等过滤条件。
    - **风险**: 含 `custom`、`web_search`、图片输入、Responses 状态字段的请求可能被路由到只支持 Chat function tool 的上游，导致连续 failover 与难以理解的上游错误。
    - **方案**:
        1. 为 channel / api entry 建模能力：`protocol = chat | responses | claude | gemini | custom`、`tool_types`、`input_modalities`、`output_modalities`、`streaming`、`reasoning`、`stateful_responses` 等。
        2. 每次请求先从 IR 推导 `requires`，再由 router 过滤候选上游。
        3. 未知能力默认只允许最小安全集，不能假设支持 Responses 或所有工具类型。
        4. 原生 Responses 上游存在时，`/v1/responses` 优先走原生 Responses adapter，不强制降级到 Chat。

- [ ] **建立显式降级与拒绝策略**:
    - **默认策略**: `strict`。只有可无损或明确等价转换的字段才允许跨协议转换；不可表达的字段必须拒绝并返回清晰错误。
    - **允许转换示例**: 纯文本 input、普通 message、system instructions、`function` tool、temperature/top_p/max_output_tokens 等。
    - **禁止静默转换示例**: 丢弃 `custom tools`、忽略 `previous_response_id`、把非 function 工具伪装成 function、压扁复杂 output item。
    - **可选策略**: 后续可以提供显式 `best_effort`，但必须在日志与响应中记录降级字段、丢弃字段和目标上游能力缺口。

- [ ] **协议错误与日志分类升级**:
    - **现状**: 失败多表现为上游原始错误，例如 `function is not set`、`Tools[N].Type invalid`。
    - **目标**: 在代理内部提前识别能力不匹配，写入结构化错误分类，例如 `gateway_capability_check`、`unsupported_tool_type`、`unsafe_protocol_downgrade`。
    - **收益**: 日志分析可以区分“上游故障”和“本代理拒绝不安全转换”，避免把协议设计问题误判为模型或 API Key 问题。
- [ ] **流式错误冷却策略分层（防误判冷却）**:
    - **现状**: `forwarder.rs` 的流式 `stream_read` 错误分支会把所有 upstream body 读取错误都记录为 `upstream_error` 并触发 `spawn_cool_down_entry()`。当上游已返回 HTTP 200、首 chunk 已到，但后续 body decode/read timeout 时，也会被误判为模型/渠道不可用。
    - **高频误判样例**: `status_code=200`、`first_token_ms>0`、`chunk_count>0`、`streamed_bytes>0`、`error.is_timeout=true`、`error.is_decode=true`、`error.is_body=false`、`has_valid_output=false`。
    - **临时策略**: 这类错误仍记录为本次请求失败，但归类为 `decode_timeout`，不触发普通冷却、不增加长期 disable 阈值。
    - **后续正式方案**:
        1. 抽取 `classify_stream_failure()`，把冷却动作分为 `cooldown` / `suppress` / `short_cooldown` / `downrank`。
        2. 扩展 `StreamEndReason`：`ReadTimeout`、`DecodeTimeout`、`IdleTimeout`、`NoValidOutput`、`SseError`、`BufferLimit`。
        3. 在 `usage_logs.other` 中记录 `cooldown_action`、`cooldown_decision_reason`、`stream_failure_class`，避免 UI 和后续分析只能解析错误字符串。
        4. 只有上游明确拒绝、建连/发送失败、HTTP 429/5xx、SSE 明确 error、首 chunk 前超时等真正表明上游不可用的情况才推进冷却和连续失败计数。
    - **涉及文件**: `src-tauri/src/proxy/forwarder.rs`, `src-tauri/src/proxy/circuit_breaker.rs`, `src-tauri/src/database/dao/usage_dao.rs`。


- [ ] **统一运行时适配层契约（Desktop/Web Adapter Parity）**:
    - **现状**: `src/lib/useApiAdapter.ts` 已通过 `isTauriRuntime()` 在 `tauriApiAdapter` 与 `webAdminApiAdapter` 间切换；桌面端 `src/lib/api.ts` 仍是纯 Tauri `invoke()` 能力面。
    - **风险**: 页面化后如果共享页面直接依赖 Tauri `invoke`、`window.__TAURI__`、或某个 Web adapter 未覆盖的能力，会出现“页面能渲染但按钮不可用”。
    - **要求**:
        1. 以 `api.ts` 的完整能力面为基线，建立 `Tauri IPC Adapter` 与 `Web HTTP Adapter` 的能力对照表。
        2. 统一领域接口、参数结构、返回结构和错误模型（建议 `{ code, message, retriable, source }`）。
        3. UI 组件禁止直接调用 `invoke()` 或拼 HTTP；只能依赖 adapter 暴露的领域方法。
        4. 对 desktop-only 能力（如托盘、系统环境变量、窗口操作）提供显式 capability 标识和 Web 端降级行为。

- [ ] **设置更新契约统一（完整对象写回）**:
    - **现状**: Rust 端 `updateSettings()` 期望完整 `AppSettings`；前端已有 `{ ...settings, key: value }` 的完整对象写回模式。
    - **风险**: 页面化或 Web adapter 若改成 `Partial<AppSettings>` patch，会导致字段丢失、反序列化失败或配置被默认值覆盖。
    - **要求**:
        1. 明确文档化：桌面/Web 两端更新设置时都必须传完整 `AppSettings`。
        2. 如未来支持 patch，必须在后端新增独立 patch 命令，不能复用当前完整对象接口。
        3. 对 `as any`、`Partial<T>` 绕过类型检查的设置更新调用做专项检查。

- [ ] **Tauri v2 安全基线：CSP + capabilities 最小化**:
    - **现状**: `tauri.conf.json` 中 `security.csp = null`；`capabilities/default.json` 当前包含 `core:default`、`opener:default`、`process:default`。
    - **风险**: 桌面端管理 API Key、Access Key、日志和本地代理；`csp: null` 与过宽权限不适合作为发布态安全基线。页面化后如果新增插件能力但 capability 未同步，会出现运行时报权限错误。
    - **要求**:
        1. 制定 dev/prod 两套 CSP，生产态尽量 `default-src 'self'`，外链统一走 opener，避免任意外部脚本。
        2. 审计 `process:default` 是否必要，所有权限按窗口/运行模式最小化。
        3. 建立功能权限矩阵：desktop-only / web-available / degraded。
        4. 新增 Tauri 插件能力时必须同步 capability 与 Web 降级策略。

- [ ] **桌面壳能力解耦（ShellBridge）**:
    - **范围**: 窗口、托盘、菜单、单实例、窗口状态、外链打开、系统环境变量写入、更新检查。
    - **风险**: 同一 React 页面若直接依赖桌面 API，在 Web 管理端或普通浏览器中不可用。
    - **方案**:
        1. 把桌面壳能力收敛为 `ShellBridge`/runtime capability 接口。
        2. Tauri 端实现真实动作；Web 端提供 no-op、禁用态或替代 UX。
        3. UI 上做到“不可用能力提前禁用并说明原因”，避免点击后才报错。

- [ ] **Tauri 桌面成熟机制补齐**:
    - **单实例**: 接入 `tauri-plugin-single-instance`，避免多进程争抢端口、SQLite、托盘；第二次启动时显示并聚焦已有主窗口。
    - **窗口状态**: 接入 `tauri-plugin-window-state`，持久化窗口大小、位置、最大化状态，并处理多显示器变化后的不可见窗口兜底。
    - **检查更新体验**: 当前已有 `check_update` 基础命令；短期先完善设置页手动检查、loading/success/fail toast、release 链接；完整自动更新后续再评估 `tauri-plugin-updater`。
    - **托盘策略收敛**: 当前存在 `EXPERIMENTAL_LAZY_TRAY_REFRESH` 半状态；需在主动刷新与懒构建之间选定正式策略，避免长期实验分支。

- [ ] **页面化路由与静态资源策略**:
    - **现状**: `App.tsx` 当前使用 `currentPage` state 切换主页面，不是 URL 路由；Web 模式有登录门禁。
    - **风险**: 页面化后若需要浏览器刷新恢复、深链接、前进后退、权限守卫，纯 state 切页不够；Web 部署若使用 history 路由但服务端无 fallback 会 404。
    - **建议**:
        1. 短期保留 state 切页，避免扩大改动；若要外部可分享/刷新恢复，再规划 URL 路由化。
        2. Web 管理端部署必须明确 hash/history 策略和静态资源 base。
        3. 增加深链访问、刷新恢复、登录态重定向的验证项。

- [ ] **Web Admin 与 Proxy 运行模式解耦评估**:
    - **现状**: `lib.rs` 中 Web Admin single-port mode 与 proxy server 运行存在耦合提示：proxy 关闭时 single-port Web Admin 不可用。
    - **风险**: 页面化部署后，用户可能期望管理端独立可达；若仍依赖 proxy 状态，会出现“页面部署了但管理端不可用”。
    - **建议**:
        1. 明确 Combined / Standalone / Web Admin 三种运行模式的启动条件和用户提示。
        2. 设置页展示当前运行模式、代理端口、Web Admin 状态、数据库位置。
        3. 对 proxy 未运行但 Web Admin 需要访问的场景给出可理解错误与启动引导。

- [ ] **跨平台 WebView 与发布兼容矩阵**:
    - **风险**: Tauri 桌面使用系统 WebView（Windows WebView2、macOS WKWebView、Linux WebKitGTK），行为不完全等同于最新 Chrome；页面化后 CSS、下载、剪贴板、外链、弹窗策略可能出现平台差异。
    - **要求**:
        1. 定义最低 WebView / 浏览器基线。
        2. 建立 Windows / macOS / Linux 冒烟矩阵，覆盖启动、托盘、设置保存、代理启停、Web Admin 登录、关键页面加载。
        3. 前端使用能力探测而不是 UA 猜测。
        4. 发布时同步校验桌面壳版本与 Web 页面版本兼容性。

- [ ] **全局用户反馈与错误语义统一**:
    - **现状**: `sonner` Toaster 已接入，但页面级成功/失败/loading 反馈仍需全量闭环。
    - **风险**: IPC 与 HTTP 错误结构不同，若 UI 直接消费原始错误，两端表现会不一致。
    - **要求**:
        1. adapter 层统一错误结构和重试语义。
        2. 所有保存/删除/测速/拉取模型/设置更新/登录/检查更新都必须有 toast 反馈。
        3. 禁止静默 catch；技术日志与用户提示文案分层。

### P0 — Web Admin 当前路线落地收尾

> 当前方向：同一套 React 页面 + `src/lib/unifiedApiAdapter.ts` 统一运行时适配。Desktop 侧在 Tauri 环境下走 `invoke`，Web Admin 侧走 HTTP `/admin/*`，页面层只依赖 `ApiAdapter` 契约；后续重点是能力降级、错误语义、运行状态可观测化和部署约束。

- [x] **Web Admin endpoint / adapter 契约收敛**:
    - **现状**: 旧 `tauriApiAdapter` / `webAdminApiAdapter` 已合并为 `unifiedApiAdapter`，`useApiAdapter()` 统一返回同一个 adapter，由 adapter 内部根据运行时选择 Tauri invoke 或 HTTP fetch。
    - **已清理**:
        1. 删除分裂的 Desktop/Web adapter 文件，避免同一接口双实现长期漂移。
        2. `ApiAdapter` 继续作为页面层唯一契约边界。
        3. Web 端不支持的 runtime-only 能力需要在 capability 层提前禁用或给出替代说明。
    - **后续要求**:
        1. 新增/修改接口时必须同时检查 Tauri invoke 路径和 HTTP `/admin/*` 路径。
        2. 不允许页面组件直接调用 Tauri invoke 或裸 fetch；必须经由 `ApiAdapter`。

- [ ] **Web Admin 设置更新与版本冲突闭环**:
    - **现状**: Web `settings.get()` 返回 `{ data, _version }` 并记录 `lastSettingsVersion`；`settings.update()` 使用完整对象 + `_version`；`settings.patchSettings()` 支持 PATCH 且不做版本号检查。
    - **风险**:
        1. `lastSettingsVersion` 仅在 get / patch 后更新，完整 update 返回 `RestartResponse` 后前端未显式刷新版本，后续保存可能触发版本冲突。
        2. Web patch 路径与“完整对象写回”契约并存，容易让调用方误以为 `Partial<AppSettings>` 是通用安全写法。
        3. 多标签页同时打开设置时，版本冲突需要可理解提示和重新加载策略。
    - **要求**:
        1. 明确 Web Admin 设置页默认只走完整对象 update；patch 仅限明确场景，并在后端做字段白名单和版本策略。
        2. 完整 update 成功后必须刷新 settings 或更新本地 `_version`。
        3. 版本冲突时提示用户“设置已被其他页面/进程修改”，提供重新加载并保留本地修改的方案。

- [ ] **Web Admin 登录 / token / 鉴权收尾**:
    - **现状**: 已有 login / logout / validateToken / Bearer token；后端生产态 `require_auth` 校验 token，debug 态跳过鉴权。
    - **风险**:
        1. `validateToken()` 网络异常时返回 `true`，会让用户在网络断开或 Admin server 不可达时进入主界面，随后所有请求失败。
        2. token 存储在 `localStorage`，需要明确同源部署、CSP 和 XSS 风险边界。
        3. debug 态跳过鉴权，发布验证必须覆盖 release / non-debug 行为。
    - **要求**:
        1. 网络异常时区分“服务不可达”和“token 有效”，进入主界面前给出可见错误或离线状态。
        2. 401/403 统一清理 token 并回到登录页。
        3. 登录失败、锁定、剩余次数、登出成功/失败均使用 toast 呈现。
        4. 发布前必须用 release 构建验证鉴权链路。
    - **release 验证清单**:
        1. 未登录访问 `/admin/status` 返回 401。
        2. 未登录访问 `/admin/settings` 返回 401。
        3. 登录成功返回 token。
        4. 带正确 token 访问 `/admin/status` 返回 200。
        5. 带错误 token 访问 `/admin/status` 返回 401。
        6. logout 后旧 token 返回 401。
        7. 登录失败返回剩余次数。
        8. 多次失败返回 429 / RATE_LIMITED。
        9. 修改用户名/密码后旧 session 失效。

- [ ] **Web Admin desktop-only 能力降级矩阵**:
    - **现状**: 部分能力已自然降级，例如外链在 Web 端走 `window.open`；但仍有功能在 Web 端运行时报错。
    - **需要标记的能力**:
        - 托盘刷新 / 托盘优先级快捷菜单
        - 系统环境变量写入（CLI 配置）
        - 桌面窗口操作 / 单实例 / 窗口状态
        - 本地文件路径 / 数据库路径展示
        - `translation.translateAndRelay()` 这类 runtime-only 动作
    - **要求**: 建立 `RuntimeCapabilities`，页面根据 capability 隐藏、禁用或展示替代说明，禁止“点了才报不支持”。

- [ ] **Web Admin 运行模式与 Proxy/Admin 耦合可观测化**:
    - **现状**: 后端已有 `AdminMode = Disabled / Standalone / Combined`；Combined 模式下 Web Admin 与 proxy 端口耦合；`lib.rs` 中已有 proxy 未运行时 single-port Web Admin 不可用的日志警告。
    - **风险**: 用户只看到页面打不开或请求失败，不知道是 Web Admin 未启用、proxy 未运行、端口冲突、还是 token 失效。
    - **要求**:
        1. 设置页展示当前 AdminMode、Web Admin 地址、proxy 状态、Admin server 状态、运行模式 Combined/Standalone。
        2. 当 single-port Web Admin 因 proxy 未运行不可用时，UI 给出启动 proxy 的引导。
        3. Admin 健康检查 `/admin/health` 与版本 `/admin/version` 在前端有明确状态展示。

- [ ] **Web Admin 操作反馈与错误语义统一**:
    - **现状**: `unifiedApiAdapter` 已统一 Desktop/Web 的调用入口；`sonner` Toaster 已接入，部分页面已补齐错误提示，但全量操作反馈仍需验收。
    - **风险**: IPC 错误、HTTP 错误、登录错误、网络错误在 UI 层表现不一致；静默失败会让用户误判操作已完成。
    - **要求**:
        1. 统一错误结构：`{ code, message, retriable, source, status? }`。
        2. 所有保存、删除、测速、拉取模型、登录、登出、检查更新、启动/停止 proxy 都必须有 success/error/loading toast。
        3. 禁止空 catch；技术日志与用户文案分层。

- [ ] **Web Admin 构建部署约束文档**:
    - **现状**: `vite.config.web.ts` 已有独立 Web 构建，产物 `dist-web-admin/`，并通过 stub alias 替换 Tauri API。
    - **风险**: 真实部署时仍需明确 `/admin` API 同源、反向代理、静态资源 base、刷新策略和缓存策略。
    - **要求**:
        1. 明确 Web Admin 推荐部署形态：同源 `/admin` API + 静态资源由 Admin server 提供。
        2. 记录 `base: "/"` 的约束；若未来部署到子路径，必须同步 Vite base 和后端静态资源路由。
         3. 增加刷新首页、加载 assets、登录、进入各主页面、调用一个写操作的 smoke 验证。

### P0 — 需立即处理的关键问题

- [ ] **状态200但输入输出都为零的上游错误被标记为成功**:
    - **问题**: 某些请求上游返回 HTTP 200，但输入和输出均为零/空值，当前被错误标记为成功，实际上应视为失败或异常。
    - **需要**: 调查并修复成功判定逻辑，确保 zero-input-output 场景不被误判。

- [ ] **THINK阶段模型失败调查**:
    - **问题**: 部分模型在 THINK（思考阶段）失败，可能原因：超时、模型不支持、参数错误等。
    - **需要**: 分析日志，识别失败模式，提出改进方案，提升稳定性。

### P1 — 个人使用体验与稳定性

- [ ] **Responses hosted tools 的 Chat fallback 可观测降级**:
    - **背景**: 当前 `/v1/responses` 在没有原生 Responses 上游时，会走 Responses → Chat-compatible 上游的降级路径。Responses 原生 hosted tools（如 `web_search`、`file_search`、`image_generation`、`custom`）不能作为原生工具盲透传给 Chat 上游，否则会触发 `function is not set` / `Tools[N].Type invalid` 等上游 schema 错误。
    - **当前临时策略**:
        1. R→O（Responses client → OpenAI/Chat-compatible upstream）路径中，`function` tools 正常转换为 Chat function tools。
        2. 非 function hosted tools 不透传给 Chat 上游，避免污染 failover 与上游错误统计。
        3. 给下游模型注入 system 提示，说明不要声称调用 Responses 原生工具，应改用当前运行环境可用的本地方式完成任务，例如 PowerShell、curl、Python、浏览器、Playwright、HTTP API、本地命令、文件系统搜索、数据库查询、本地索引或其他可调用工具。
        4. 提示词禁止要求用户“粘贴文件内容”或“切换上游”，避免把能力降级暴露为用户负担。
    - **后续修复方向**:
        1. 将降级信息写入 `usage_logs.other`，例如 `conversion=best_effort_degraded`、`skipped_tools=[...]`、`degradation_prompt_injected=true`。
        2. 在完整中立 IR + Capability Router 落地后，改为按上游能力选择原生 Responses、Chat fallback 或本地 Tool Runtime。
        3. 若未来 API Switch 实现本地 Tool Runtime，再把 `web_search/file_search/image_generation/custom` 映射到真实可执行工具，而不是仅靠提示词。
    - **涉及文件**: `src-tauri/src/proxy/protocol/responses.rs`, `src-tauri/src/proxy/responses_handler.rs`, `src-tauri/src/proxy/forwarder.rs`。

- [ ] **真正无头 / Server-only 运行模式**:
    - **问题**: 当前 `--headless` / `--standalone` 只是在 Tauri 启动后跳过托盘和窗口管理，仍会进入 `tauri::Builder::run()`，不能保证在无 DISPLAY、无 WebView、Docker、systemd、Linux VPS 等真正无头环境中稳定启动。
    - **目标**: `--headless` 或 `API_SWITCH_HEADLESS=1` 时完全绕过 Tauri GUI runtime，不初始化窗口、WebView、托盘，只启动后端服务能力。
    - **方案**:
        1. 在 `main.rs` 入口早期判断 headless 参数 / 环境变量，headless 时调用新的 `run_headless_server()`，非 headless 保持现有桌面路径不变。
        2. 抽取 DB 初始化、settings 加载、`admin::apply_admin_env()`、Proxy/Admin server 启动逻辑，供桌面模式和 headless 模式复用。
        3. 将 `ProxyServer` / `AdminState` 中的 `tauri::AppHandle` 依赖改为可选或抽象运行时句柄；headless 下所有 emit、托盘刷新、窗口相关操作降级为 no-op。
        4. headless 模式支持 `proxy_enabled` 自动启动代理；支持 Web Admin 独立端口或与 proxy 合并端口运行。
        5. 增加 Ctrl+C / shutdown signal 处理，确保服务可被 systemd/Docker 正常停止。
    - **验收标准**:
        1. 桌面模式现有启动、窗口、托盘、Tauri IPC 不回归。
        2. `API_SWITCH_HEADLESS=1` / `--headless` 不调用 `tauri::Builder::run()`，不依赖 DISPLAY/WebView/托盘。
        3. 无头模式可启动 `/health`、代理 `/v1/chat/completions`、Web Admin 登录与核心管理接口。
        4. `cargo check`、`pnpm build:renderer` 通过；至少补充一次无 GUI 环境的 smoke 验证记录。
    - **预估**: 可用版 1-2 天；若补齐 Dockerfile、systemd unit、日志与部署文档，发布级 3-5 天。
    - **涉及文件**: `src-tauri/src/main.rs`, `src-tauri/src/lib.rs`, `src-tauri/src/runtime_mode.rs`, `src-tauri/src/proxy/server.rs`, `src-tauri/src/admin/mod.rs`, `src-tauri/src/admin/state.rs`。

- [ ] **proxy http_client read_timeout 优化（超时分层）**:
    - **问题**: `reqwest::Client` 全局 `read_timeout` 仍是粗粒度兜底，对慢推理模型/大上下文请求可能误伤业务长耗时请求。
    - **已落地临时修复**: `server.rs` 的全局 `read_timeout` 已从 60s 放宽至 120s（2026-05-09），先缓解慢响应模型 61 秒左右被切断的问题。
    - **正式方案**: 将超时从全局客户端提升为分层控制——connect_timeout / send_timeout / first_byte_timeout / idle_timeout / total_timeout，流式与非流式分开配置，支持按通道/模型可配。去掉对 `reqwest::Client::read_timeout` 的业务语义依赖，仅保留其作为连接保活兜底。

- [x] **模型目录信息预计算入库**:
    - **方案**: `api_entries` 表新增 `provider_logo`、`release_date`、`model_meta_zh`、`model_meta_en` 四个字段。
    - **写入时机**: 手动添加模型时（`AddApiDialog`）和 Channel 选择模型时（`selectModels`）均从 `models.json` 计算 metadata 并直接写入 DB。
    - **旧数据回填**: 进入 API 管理页时检测缺失字段的 entry，批量调用 `backfillEntryCatalogMeta` 补齐。
    - **渲染主路径**: UI 优先读 `entry.provider_logo / release_date / model_meta_zh / model_meta_en`，缺失时 fallback 到前端 `modelsCatalog.ts`。
    - **排序**: `latest` 模式前端和后端均直接用 `entry.release_date` 排序。
    - **AUTO 路由**: `custom` → sort_index，`fastest` → response_ms，`latest` → release_date。
    - **Tray Top5**: 按 AUTO 组当前优先级（`sort_index`）展示，不再跟随 `default_sort_mode` 切换。
- [ ] **托盘菜单懒构建（Lazy Tray Build）**:
    - **问题**: 当前托盘菜单在每个写操作（toggle/reorder/delete/create/update_settings/test_entry_latency/backfill/forwarder事件等）后都主动调用 `build_tray_menu` 重建，导致联动点分散在 ~10 处，维护成本高且容易遗漏。
    - **方案**: 改为**惰性构建**——托盘右键弹出时（`on_menu_event` 或 Tauri 的 `MenuEvent`）才实时读 DB + L1 缓存构建菜单，去掉所有散落的 `build_tray_menu` 调用。
    - **收益**: 零联动维护成本，菜单永远是最新数据，新增写操作无需关心托盘。
    - **注意**: 需验证 Tauri v2 托盘菜单是否支持按需构建（而非启动时固定），以及延迟是否影响体验。
- [x] **Responses API 支持**: 新增 `/v1/responses` 路由，支持 OpenAI Responses API 格式（Codex / GPT-5.5 等新模型需要）。在入口层将 Responses API 格式转换为 Chat Completions 格式，复用现有 router + forwarder + auth 链路。实现文件：`src-tauri/src/proxy/responses_handler.rs`。当前定位为 **Responses → Chat Completions compatibility shim**，优先支持个人 Hub 常用的 text / function tools / streaming 子集。
- [x] **Responses API P0/P1 补齐（个人 Hub 范围）**:
    - **目标边界**: 已完成个人 Hub 所需的 P0/P1 兼容能力；P2 以后能力明确不支持。该端点不是 OpenAI Responses 平台完整替代品，只提供个人 API 管理与转发 Hub 所需的兼容层。
    - **已落地**:
        1. 真 streaming：边读 upstream Chat Completions SSE 边输出 Responses API SSE，不再 collect 完整 frames 后一次性返回。
        2. 基础 Responses streaming event graph：`response.created`、`response.output_item.added`、`response.output_text.delta`、`response.function_call_arguments.delta/done`、`response.output_item.done`、`response.completed`、`data: [DONE]`。
        3. `response.completed.output` 包含最终文本与 tool call 输出；同时提供 `output_text` convenience 字段。
        4. 非流式 upstream 4xx/5xx 规范化为失败 Response JSON；streaming 错误输出 `response.failed` 后 `[DONE]`。
        5. invalid JSON body 返回 400 `invalid_request_error`。
        6. `finish_reason=length/content_filter` 映射为 `status=incomplete` 与 `incomplete_details.reason`。
        7. response `model` 优先使用上游实际返回模型，缺失时 fallback 到请求模型。
        8. `text.format` → Chat Completions `response_format` 映射，支持 `json_object` / `json_schema`；无法映射时明确 400。
        9. function tool streaming 支持 Chat `delta.tool_calls` 参数增量；tool_calls arguments 支持 string/object 两种形态。
        10. input image subset 支持 URL / data URL 的 `input_image` 转 Chat vision `image_url` content。
        11. passthrough-first：保留未知 structured input item、非 function tool 与 provider-specific tool 字段，尽量交由上游决定。
    - **明确不支持（P2+）**: `background/cancel`、完整 `store/retrieve/delete` 状态层、`conversation` 生命周期、`previous_response_id` 本地状态续接、MCP tools、built-in `web_search/file_search/code_interpreter`、完整 file input / Files API / vector store、stream retrieve `starting_after/include_obfuscation`、`reasoning.encrypted_content` 等 OpenAI 平台级能力。
- [ ] **客户端断开精准检测与冷却抑制**:
    - **问题**: `StreamLogGuard::drop()` 在下游客户端强制断开时，如果上游尚未返回任何 token（`prompt_tokens=0 && completion_tokens=0`），会错误判定为"失败"并触发 `cool_down_entry`，导致正常渠道被假性冷却。
    - **根因**: 当前 drop 路径无法区分"上游真的挂了"还是"客户端主动断开"。判断依据仅有 `status_code` 和 token 计数，缺少客户端断开信号。
    - **修复方向**:
        1. 在 stream body 的 `poll_fn` 中引入一个 `AtomicBool`（如 `client_gone`），当 Axum 检测到对端连接断开时标记。
        2. `StreamLogGuard::drop()` 检查该标记：若为 true，无论是否有 token 都记录 `StreamEndReason::Dropped` 且**不触发冷却**。
        3. 细分三种 drop 原因: `client_gone`（客户端断开→不冷却）、`runtime_cancel`（tokio cancel→不冷却）、`other_drop`（未知→保守处理但降低冷却时长）。
    - **涉及文件**: `forwarder.rs` (StreamLogGuard, build_streaming_response)
- [ ] **前端统一信息提示呈现方式**: 参考 0.5.0 版实现（sonner toast），所有用户操作反馈（成功/错误/警告/信息）统一使用 toast 呈现，替代零散 `alert()` 和静默失败。**渠道新增/编辑中的信息提示方式必须调整**：表单提交成功/失败、验证错误、网络异常等消息统一使用 `toast.success/error/warning/info`，保持全项目一致的交互体验。
    - **涉及文件**: `ChannelFormDialog.tsx`, `ChannelManager.tsx` 等渠道组件，全局 `Toaster`（`main.tsx`）已就位。
- [ ] **程序放外网**: 打包发布到公网，支持外部访问
- [ ] **SSE PING 注入兼容性修复**:
    - **问题**: 当前 `forwarder.rs` 在流式输出中每 10 秒注入 `: PING\n\n` 作为 SSE comment。虽然这对标准 SSE 客户端合法，但部分下游客户端会把 comment 帧错误拼接进 JSON，导致 `JSON parsing failed` / `Property name must be a string literal`。
    - **现状**: 已临时注释掉下游 PING 注入，先保证兼容性。
    - **后续方向**: 评估是否完全移除该 heartbeat，或改成对下游绝对透明的保活方案；保留现有 idle timeout 作为异常流保护。
- [x] **错误冷却策略优化 (个人模式)**:
    - **历史说明**: 以下内容保留的是当时关于 `enabled`、显式模型和冷却策略的阶段性设计记录。当前生效的路由规则、UI 分组语义与 tray 语义，统一以第 4 节为准。
    - **当前规则**:
        1. **entry.enabled 只表示是否进入 AUTO**；`enabled=false` 不是不可用状态，显式模型名仍可调用。
        2. **模型是否可用只看冷却 / 熔断**：`cooldown_until > now` 或内存 circuit open 时跳过。
        3. **AUTO**：只从 `enabled=true` 且未冷却的 API 池条目中选择。
        4. **显式模型名**：从 API 池所有同名条目中选择（包括 `enabled=false`），但跳过冷却中条目；失败后设置冷却并继续 AUTO fallback。
        5. **/v1/models**：返回 API 池所有可见模型，不按 `enabled` 过滤。
        6. **成功请求**：清除该 entry 的冷却和连续失败计数。
        7. **失败请求**：写失败日志，设置临时冷却；达到连续失败阈值时可进入长冷却 / 自动关闭策略。
    - **已落地**:
        - `ProxyState.failure_counts` 纯内存连续失败计数。
        - `forwarder.rs` 失败冷却、成功清理、尝试路径日志。
        - `pool.rs` 手动开启时清除计数和冷却。
        - `router.rs` 区分 `all_entries` 与 `auto_entries`，显式模型可用范围与 AUTO 范围分离。
    - **仍需观察**:
        - 长冷却文案是否需要区分 `冷却 Ns` / `故障 Nh`。
        - 达阈值后是否继续把 `enabled=false` 作为自动关闭手段，可能与“enabled 只控制 AUTO”语义冲突；若保留自动关闭，应视为“移出 AUTO”，而不是“不可显式使用”。
    - **数据库**: 零改动，复用 `enabled`, `cooldown_until`。

- [x] **API 管理一键测速真实响应验证**:
    - **已落地**: `test_entry_latency` 不再只发送 `hi` 并丢弃响应体；现在发送真实对话探测请求，要求上游返回 HTTP 200 且响应体非空。
    - **当前规则**:
        1. 所属渠道禁用、请求失败、非 200、响应体读取失败或响应体为空 → 写入 `api_entries.response_ms = "X"` 并关闭该 API Entry。
        2. HTTP 200 且响应体非空 → 写入实际毫秒延迟，并开启该 API Entry。
        3. Prompt 仍建议模型“请只回复 OK”，但不强制包含 OK，避免啰嗦模型误判失败。
    - **涉及文件**: `src-tauri/src/services/pool_service.rs`。

- [x] **渠道一键测速改为 Base URL 端点探测**:
    - **已落地**: 渠道页“响应”表头的一键测速不再调用 `test_channel`，不再选模型、不拉模型、不真实对话；改为对 `channel.base_url` 调用 `probe_url`。
    - **当前规则**:
        1. `probe_url` 通过 HEAD 探测 Base URL，失败再 GET；`status < 500` 视为端点可达。
        2. 端点可达 → 写入 `channels.response_ms`。
        3. 端点不可达或探测异常 → 走标准 `update_channel(enabled=false)` 流程关闭渠道，并同步关闭该渠道下的 API entries。
    - **涉及文件**: `src/features/channels/ChannelManager.tsx`, `src-tauri/src/services/channel_service.rs`。

- [ ] **多命中条目响应时间优先**: 在分组精确匹配和模型模糊匹配命中多个 `api_entries` 时，按 `response_ms` 升序排序可选条目，优先选择反应时间最低的条目；若 `response_ms` 未设置则排到最后。
- [ ] **渠道 & 模型星级/权重系统**:
    - **需求**: 渠道可设默认星级/权重（0-5 星），模型默认继承所属渠道的星级；模型可显式设置自己的星级**覆盖**渠道默认值（CSS 继承模型）。路由时星级高的条目优先被选中。
    - **自动评级计算** (补充): 除手动设置星级外，系统应基于三个客观维度自动计算综合权重，为用户手动设置提供参考基线，或在无手动星级时直接作为路由权重。
        1. **渠道速度**: 聚合该渠道下所有 `api_entries.response_ms` 的平均值（排除 "X" 和空值），反映渠道整体响应水平。
        2. **模型速度**: 当前条目自身的 `response_ms`（一键测速结果），反映该模型在该渠道的具体延迟。
        3. **模型发布信息**: 条目的 `release_date`，反映模型新鲜度（越新权重越高，鼓励优先使用最新模型）。
        - **综合算法**:
            - 每个维度映射到 0-5 分（区间映射，如 response_ms < 500ms → 5分，500-1000ms → 4分，1000-2000ms → 3分，2000-5000ms → 2分，>5000ms → 1分，无数据 → 0分；release_date 按距今月份：近1月→5分，1-3月→4分，3-6月→3分，6-12月→2分，>1年→1分，无数据→0分）。
            - 三权重按可配置比例加权（默认: 渠道速度 30%、模型速度 40%、发布新鲜度 30%），计算得 `auto_rating_score`（0-5 浮点），最终映射为 `auto_star_rating`（0-5 整数，四舍五入）。
            - 路由优先级整合: `effective_star = COALESCE(api_entries.star_rating, auto_star_rating, channels.star_rating)`。即手动覆盖 > 自动评级 > 渠道默认。
        - **更新时机**:
            - 一键测速完成后批量更新受影响的 `response_ms`，触发相关条目的 `auto_star_rating` 重算。
            - `release_date` 来自 `models.json` 或目录回填数据，数据更新时触发重算。
            - 手动设置 `star_rating` 时，该条目不再依赖自动评级（手动优先）。
        - **存储建议**: 推荐在 `api_entries` 表新增 `auto_star_rating INTEGER` 持久化（需 `ensure_column` 迁移），以降低路由时的查询/计算开销。若追求简洁也可每次路由时实时计算。
    - **数据库改动**:
        1. `channels` 表新增 `star_rating INTEGER NOT NULL DEFAULT 0`（0-5，渠道默认星级）
        2. `api_entries` 表新增 `star_rating INTEGER`（`NULL` = 继承渠道；`0-5` = 显式覆盖）
        3. `schema.rs` `create_tables` / `ensure_channel_columns` / `ensure_api_entry_columns` 补齐迁移逻辑
    - **继承逻辑（Rust 端）**:
        - 路由查询时 JOIN `channels` 表，`effective_star = COALESCE(api_entries.star_rating, channels.star_rating)` 作为有效星级
        - `pool_service.rs` 的 `list_entries` / 路由查询 DAO 方法需返回带 `channel_star_rating` 的条目，便于前端展示继承来源
        - 写入路径：渠道星级变更时**不**级联更新模型（继承在读取时计算）；模型星级设为与渠道相同值时写为 `NULL`（保持继承语义），设为不同值时才写显式值
    - **路由优先级整合**:
        - `router.rs` `resolve()` 在 `available_entries` 获取可用条目后、`apply_sort_mode` 之前，按 `effective_star` 降序稳定排序（同星级保持现有 sort_index/response_ms/release_date 顺序）
        - 排序优先级: **星级 > 现有排序维度**（sort_index / response_ms / release_date）。即先按星级分层，层内再按用户选择的排序模式排列
        - 与 "多命中响应时间优先" 的关系: 星级优先于响应时间；同等星级下才按响应时间择优
    - **前端改动**:
        1. **渠道编辑弹窗** (`ChannelFormDialog.tsx`): 增加星级选择器（0-5 星，可点击星标），修改后写入 `channels.star_rating`
        2. **API 池列表** (`PoolManager.tsx`): 
            - 每个条目行展示有效星级（只读星标），继承的用浅色/虚线星标 + tooltip "继承自渠道 X"，显式覆盖的用实心深色星标
            - 条目编辑/右键菜单增加 "星级覆盖" 操作（0-5 星 or "恢复继承"=设为 NULL），调用 `update_entry_star_rating` 命令
        3. **类型定义** (`types.ts`): `ApiEntry` 增加 `star_rating?: number | null`，`Channels` 增加 `star_rating: number`
    - **涉及文件**:
        - `schema.rs` (表结构 + 迁移)
        - `database/` 相关 DAO（查询 JOIN、更新方法）
        - `pool_service.rs` (list_entries 返回 channel 星级)
        - `router.rs` (resolve 排序)
        - `admin/` 命令层（更新 star_rating 的 Tauri commands）
        - `ChannelFormDialog.tsx`, `PoolManager.tsx`, `types.ts` (前端)
    - **向后兼容**: 现有渠道默认 `star_rating=0`，现有条目 `star_rating=NULL`（继承渠道0），路由行为不变。用户主动设置星级后才生效。
- [ ] **Responses API 代理架构限制**:
    - **背景**: `/v1/responses` 端点将 Responses API 格式转换为 Chat Completions 格式转发上游，属于**协议转换代理**。部分 Responses API 特性依赖 OpenAI 服务端状态管理，代理架构下无法实现。
    - **不可实现项**:
        - `previous_response_id`: 多轮对话串联依赖 OpenAI 服务端存储历史 Response，代理无状态无法回溯。多轮场景必须通过 `input` 数组显式传递完整上下文。
        - `conversation`: 与 `previous_response_id` 互斥的另一条多轮路径，同样依赖服务端状态。
        - `background`: 异步执行模式，代理所有请求同步转发，无法支持。
        - `include`: 控制服务端额外输出（如 `reasoning.encrypted_content`、`message.output_text.logprobs`），代理无法控制上游行为。
        - `max_tool_calls`: 服务端强制限制内置工具调用次数，代理无法拦截计数。
    - **影响**: 官方 OpenAI SDK 的多轮对话（基于 `previous_response_id`）在代理模式下不工作，客户端必须改用 `input` 数组传递上下文。Codex 等工具已通过 `input` 数组实现多轮，不受影响。
    - **后续方向**: 若需要完整 Responses API 支持，需考虑直连 OpenAI（非代理模式）或实现服务端状态存储层。

- [x] **流末尾显示实际模型名（个人模式）**:
    - **目标**: 在流式回答正常结束时，让聊天正文末尾可见本次实际命中的模型名。
    - **当前实现**:
        1. 设置页新增“显示对话模型”开关，对应 `show_conversation_model`，默认开启，可随时关闭。
        2. `forwarder.rs` 在检测到上游 `data: [DONE]` 时，若本轮已经出现过普通正文 `delta.content`，则在 `[DONE]` 前追加一个普通 Chat Completions 文本 delta：`\n\nmodel: <entry.model>`。
3. 若上游没有 `[DONE]`、开关关闭、或本轮是纯 tool_calls/function_call 无正文文本，则不追加，避免干扰工具调用阶段。
         4. 追加内容是普通文本，不是额外 metadata/control event；后续 Claude / Responses 转换层按普通正文自然转换。
     - **2026-05-10 修复：工具调用注入门槛收紧**:
         - **问题**: 请求带 `tools`/`function_call` 或响应出现文本 + tool_calls 混合流时，模型信息仍被注入，导致下游 CALL 复用时夹带 `model: ...` 并重复显示。
         - **根因**: `should_append_model_info` 只排除了结构化输出，未排除工具调用请求；`stream_chunk_has_tool_calls` 只检测 `delta.tool_calls` 与 `finish_reason`，未覆盖 `delta.function_call`、`message.tool_calls`、`message.function_call` 等路径。
         - **修复**:
             1. 新增 `request_uses_tool_calling()`，递归检测请求体中 `tools`/`tool_choice`/`functions`/`function_call`/`parallel_tool_calls`/`max_tool_calls`/`tool_calls` 字段，提前禁止注入。
             2. `stream_chunk_has_tool_calls()` 扩展为检测 `delta` 与 `message` 容器下的 `tool_calls`/`function_call`，以及 `finish_reason` 为 `tool_calls`/`function_call`。
             3. 新增 8 条单元测试覆盖混合流、纯工具、旧格式、message 层级等场景。
         - **涉及文件**: `src-tauri/src/proxy/forwarder.rs`
     - **取舍**: 个人工具优先可见性，接受正文尾部出现模型名；对结构化输出/严格 JSON 如有影响，可通过设置关闭。

- [ ] **首字前模型元信息事件（P2 备选）**:
    - **目标**: 若后续需要非正文污染的实时模型追踪，可在每个流的首个 token 前输出独立 metadata/control event，使 UI 能实时显示模型切换轨迹。
    - **当前取舍**: 现阶段已采用“流末尾追加普通文本 `model: ...`”满足个人可见性需求；首字前 metadata event 暂不实施，避免为 Claude / Responses 等转换层引入额外事件透传/翻译复杂度。
    - **可行性**: `forwarder.rs` 的 `poll_fn` 闭包持有 `entry.model`，每次重试会重新创建流，技术上可实现每次命中模型的独立事件。
    - **后续触发条件**: 需要不污染正文、需要实时显示重试模型轨迹、或需要前端专门监听模型事件时再评估。

- [x] **建议策略**: ✅ 已采用"流末尾追加普通文本"方案，优先满足个人可见性；首字前 metadata event 作为 P2 备选保留。

- [ ] **模型别名/显示名称利用**:
    - **背景**: 当前添加模型时支持填 `display_name`（显示名称），后端已存入 `api_entries.display_name`，但前端卡片只展示 `entry.model`，未利用 `display_name`。该字段实际上是一个**模型别名**，可用于模型改名场景：当上游模型 ID 变更（如 `gpt-4o` → `gpt-4o-2024-08-06`）时，用户只需修改 `display_name`，无需更改 `model`（`model` 保留原始 ID 用于路由匹配）。
    - **目标**: 充分利用 `display_name`，使其成为对外展示名，`model` 作为原始 ID 仅用于路由，实现"改名不改路由"。
    - **方案要点**:
        1. **前端卡片主标题**：`PoolManager.tsx` `CardBody` 中第 350 行 `{entry.model}` → `{entry.display_name || entry.model}`，同时在小字中显示原始 `({entry.model})`。
        2. **搜索过滤**：当前已在搜索时包含 `display_name`（第 596 行），无需改动。
        3. **测试聊天标题**：`TestChatDialog.tsx` 第 138 行 `entry?.display_name || entry?.model` 已在用，无需改动。
        4. **流末尾模型名显示**：`forwarder.rs` 注入的 `model: <entry.model>` 保持不变（原始模型 ID），不改为 `display_name`，确保下游客户端看到的是实际路由用的模型 ID。
        5. **模型列表接口**：`handlers.rs` 中 `/v1/models` 的 `id/display_name` 字段已使用 `display_name` 优先，无需改动。
        6. **编辑入口**：`AddApiDialog` 的"显示名称"字段已存在，无需改动。后续可考虑在卡片上增加"编辑显示名称"的快捷操作。
        7. **渠道选模**：`ChannelFormDialog.tsx` / `ModelSelectionDialog.tsx` 中拉取 available_models 时无 `display_name` 概念，不做改动（渠道选模的 model ID 就是原始模型 ID）。
    - **涉及文件**: `src/features/pool/PoolManager.tsx`（主要在 `CardBody` 渲染）
    - **优先级**: P1，改动量小（单文件改 1 行 + 添加一行小字），对日常使用体验提升明显。
### P2 — 常用体验增强

- [ ] **渠道禁用机制补全**：当前 `channel_handlers.rs` 的 `update()` 在禁用渠道（`enabled = false`）时，`channel_service::update_channel()` 内部会调用 `db.disable_entries_for_channel()` 修改 Pool 数据（禁用该渠道下所有 API 条目），但 handler 层未调用 `state.mark_pool_dirty()`，导致 Pool 状态机未能同步刷新。修复方案：在 `update()` handler 中根据 `payload.enabled == Some(false)` 条件补充 `state.mark_pool_dirty()`，确保禁用渠道时 Pool 脏标记正确触发。

- [ ] **指定模型名禁用自动路由开关**: 在设置页增加开关；开启后，当请求使用明确模型名称时，只尝试匹配到的指定模型条目，不再在指定模型不可用或失败后 fallback 到 AUTO 自动路由。`model = "auto"` 仍保持现有 AUTO 路由行为。需同步后端 `AppSettings`、前端 `AppSettings` 类型、设置页 UI、配置默认值和 `router/forwarder` 调用链。
- [x] **模型/渠道测速**: 对指定模型或渠道进行延迟测试（TTFB），排序展示结果，帮助用户选择最优渠道；显示统一为秒 (`s`)，内部存储仍使用毫秒字符串便于排序
- [ ] **渠道模型数口径修复**: 渠道页模型数应显示 `API池条目数 / available_models数`。当渠道模型列表接口不可用但手动加入 1 个模型时，应显示 `1 / 0`，不能显示 `0 / 0`。
- [ ] **CLI 配置片段生成**: PowerShell / bash 环境变量片段复制
- [x] **auto 模式实际模型名可见**: 流式回答正常结束时可在正文末尾追加 `model: <实际命中模型>`，由设置页“显示对话模型”开关控制。日志仍记录 `resolved_model`。
- [ ] **响应式布局优化**: 改善小窗口、分屏使用体验
- [x] **Web 管理端**: 已完成。同一套 `src/App.tsx` UI 通过 `ApiAdapter` 边界同时服务桌面和浏览器；当前实现已收敛为 `src/lib/unifiedApiAdapter.ts`，运行时自动选择 Tauri invoke 或 HTTP `/admin/*`。构建入口 `vite.config.web.ts`，产物 `dist-web-admin/`，登录/token 完整。架构详见下方 §15。
- [ ] **Gemini 流式模式**: 当前 Gemini 协议适配器已支持非流式和模型拉取，流式（streaming）模式尚未实现。需要在 `proxy/protocol/gemini.rs` 中实现 SSE 流式转发，支持 `stream: true` 请求的实时输出。
- [ ] **渠道多 KEY 熔断**:
    - **背景**: 当前渠道仅支持单个 `api_key`，当 Key 触发限流、余额不足或临时故障时，整个渠道不可用，即使拥有多个备用 Key。对于拥有多个 API Key 的渠道（如多个 OpenAI Project Key / 多个硅基流动账号），单 Key 故障不应导致整个渠道瘫痪。
    - **定位**: 个人本地工具的可用性增强，不做企业级 Key 池/配额/审计系统；不改变现有「模型条目 → 渠道 → 协议适配器 → forwarder」主链路，多 KEY 只作为同一渠道下的备用凭据。
    - **目标**: 同一渠道可以保存多个 API Key，主 Key 不可用时自动试下一个，减少手动复制/替换 Key 的麻烦；对外仍表现为同一个渠道、同一个模型条目。
    - **非目标**: 不做多租户计费隔离；不做复杂负载均衡；不做跨渠道 Key 池；不改变 Access Key（客户端访问凭证）语义；不追求精确到每个 Key 的完整企业审计。
    - **方案要点**:
        1. **数据模型（轻量）**: 优先考虑在 `channels` 表新增 `api_keys_json` 字段，结构为 `[{ "key": "...", "label": "主Key", "enabled": true, "cooldown_until": 0, "failure_count": 0 }]`；旧 `api_key` 保留并作为默认主 Key，避免大规模迁移。若后续发现 JSON 维护困难，再拆 `channel_keys` 表。
        2. **UI**: 渠道编辑弹窗在 API Key 下方增加「备用 Key」折叠区域，支持多行添加/删除/启用/备注即可；不做拖拽，按列表顺序决定优先级；默认保持单 Key 界面简洁。
        3. **选择策略**: 默认使用第一条启用且未冷却的 Key；失败时在同渠道内顺序尝试下一条 Key；所有 Key 都失败后，再走现有 entry/channel failover。
        4. **失败处理**: 只对明确 Key 问题或瞬态问题做短冷却：`401/403`、quota/rate limit、`429`、`5xx`、timeout/connect；`400/model_not_found/context_length` 不冷却 Key，避免请求问题误伤 Key。
        5. **转发层改动**: `forwarder.rs` 获取 channel 后解析候选 Key 列表，并把选中的具体 Key 传给 `ProtocolAdapter::apply_auth()`；最多尝试同渠道前 2~3 个可用 Key，防止请求放大。
        6. **日志记录（轻量）**: 不新增日志表字段，先把 `key_label`、`key_index`、`key_attempt_path` 写入 `usage_logs.other` JSON；日志展开时可显示“使用 Key: 主Key / 第 1 个”。
        7. **测速联动（可选）**: 短期只测当前优先 Key；后续再加“测试全部备用 Key”按钮。
    - **实施阶段**:
        1. **Phase 1：备用 Key 保存** — `api_keys_json` + UI 折叠编辑，旧单 Key 行为不变。
        2. **Phase 2：顺序故障转移** — 当前 Key 失败时尝试下一条，日志记录 key attempt path。
        3. **Phase 3：短冷却与手动恢复** — 给失败 Key 设置简单冷却时间，UI 显示“冷却中/恢复”。
    - **与现有冷却的关系**: 渠道级 `cooldown_until`（`api_entries` 表）继续保留用于模型级严重故障；Key 级熔断为更细粒度的瞬态故障处理，两者互补不冲突。
    - **风险与约束**: 多 KEY 会增加请求放大风险，必须限制同渠道 Key 尝试次数；错误分类必须保守，避免因请求格式错误误伤所有 Key；旧 `channels.api_key` 必须长期保留作为主 Key/回滚字段。
    - **依赖**: 先完成轻量保存和日志可观测性，再接入自动冷却；如果个人使用中确实需要更复杂能力，再考虑拆表和轮询负载均衡。

### P3 — 可选增强

- [ ] **上游关键 header 记录**: request-id / rate-limit 记录到日志
- [ ] **SSE ping / timeout 配置化**: 当前 ping=10s, idle timeout=300s 为代码常量
- [ ] **自动更新**: 当前仅检查更新，Tauri updater 可后续集成
- [ ] **Gemini 原生格式验证**: 原生格式转换函数作为备选
- [ ] **Azure deployment 端到端验证**: 待有 Azure 资源后验证
- [ ] **监听地址可配置**: 127.0.0.1 / 0.0.0.0 可选
- [ ] **SQLite 日志定期清理**: `usage_logs` 表持续增长，当前约 600-800 条/天。建议定期清理历史日志以保持数据库性能。
    - **性能阈值参考**: 
        - 行数: <100万优秀，100-1000万需优化，>1000万考虑归档
        - 文件: <1GB无影响，1-10GB需优化，>10GB需归档
        - 当前状态: 2.4万行 / 42MB，远低于阈值
    - **清理策略**: 保留最近 30 天数据，每月手动或定时执行
    - **清理 SQL**: `DELETE FROM usage_logs WHERE created_at < strftime('%s', 'now', '-30 days'); VACUUM;`
    - **建议实现**: 在设置页添加"清理历史日志"按钮，或后台定时任务自动清理
    - **已完成**: 2026-05-09 手动清理 561 条失败记录，数据库从 43.83MB 压缩至 41.79MB

### 未来愿景（非核心）

- [ ] API Key 加密存储（系统 Keychain / DPAPI）
- [ ] 多用户隔离（按 Access Key 配额）
- [ ] 插件系统（日志脱敏、请求改写）

---

## 10. 推荐渠道配置

### MiniMax（硅基流动）

| 配置项 | 值 |
|--------|-----|
| API 类型 | `openai` 或 `anthropic` |
| Base URL | `https://api.minimaxi.com` 或 `https://api.minimax.chat` |
| API Key | 你的 Key |
| API 管理添加模型 | `MiniMax-M2.7`（需手动填写） |

### CODING PLAN

| 配置项 | 值 |
|--------|-----|
| API 类型 | `openai` |
| Base URL | `https://api.rcouyi.com` |
| API Key | 你的 Key |
| 拉取模型 | 不支持，需手动添加 |
| API 管理添加模型 | `gemini-2.0-flash`、`gemini-2.5-pro` 等（需手动填写） |

> 此类中转站的模型列表接口不可用，拉取会失败。直接到「API 管理」点击「添加 API」手动填写模型名称即可。

---

## 11. 验证矩阵

| API 类型 | 拉取模型 | 非流式聊天 | 流式聊天 | 工具调用 | 图片输入 | 错误码透传 | 状态 |
|---|---|---|---|---|---|---|---|
| OpenAI | ✅ | ✅ | ✅ | 待验证 | 待验证 | ✅ | 主要链路可用 |
| Custom | ✅ | ✅ | ✅ | 依赖上游 | 依赖上游 | ✅ | OpenAI 兼容上游优先 |
| Claude | ✅ | ✅ | ✅ | 待验证 | 待验证 | ✅ | 格式转换复杂 |
| Gemini | ✅ | 待验证 | 待验证 | 待验证 | 待验证 | 待验证 | 使用 OpenAI 兼容端点 |
| Azure | ✅ | 待验证 | 待验证 | 待验证 | 待验证 | 待验证 | 缺 Azure 资源 |

---

## 12. 开发环境

```bash
# 前置要求
- Rust 1.85+ (rustup)
- Node.js 18+ / pnpm
- Tauri CLI (pnpm add -D @tauri-apps/cli)

# 开发
pnpm install
pnpm dev              # 启动 tauri dev
pnpm typecheck        # TypeScript 类型检查

# 构建
pnpm build            # 生产构建

# 数据库位置
# Windows: EXE 同目录下 api-switch.db（绿色便携版）
```

---

## 13. 文件索引

```
api-switch/
├── src-tauri/
│   ├── Cargo.toml                          # Rust 依赖
│   ├── tauri.conf.json                     # Tauri 配置
│   └── src/
│       ├── main.rs                         # 入口
│       ├── lib.rs                          # Tauri setup、托盘、代理自启
│       ├── error.rs                        # AppError 枚举
│       ├── database/
│       │   ├── mod.rs                      # Database struct + 连接管理
│       │   ├── schema.rs                   # 建表 + 兼容迁移
│       │   └── dao/
│       │       ├── channel_dao.rs          # 渠道 CRUD + 模型管理
│       │       ├── api_entry_dao.rs        # 路由池条目 + 冷却
│       │       ├── access_key_dao.rs       # 访问密钥
│       │       ├── usage_dao.rs            # 日志 + 统计
│       │       └── config_dao.rs           # KV 配置
│       ├── commands/
│       │   ├── channel.rs                  # 渠道命令
│       │   ├── pool.rs                     # 池命令
│       │   ├── token.rs                    # 密钥命令
│       │   ├── usage.rs                    # 统计命令
│       │   ├── config.rs                   # 配置命令
│       │   ├── proxy_cmd.rs               # 代理控制命令
│       │   └── test_chat.rs              # 测试对话（直接调适配器）
│       └── proxy/
│           ├── server.rs                   # Axum 服务器
│           ├── handlers.rs                 # 请求处理
│           ├── router.rs                   # 智能路由
│           ├── auth.rs                     # 认证
│           ├── forwarder.rs                # 转发 + 冷却 + 日志
│           ├── circuit_breaker.rs          # 内存熔断器（辅助）
│           └── protocol/                   # 协议适配
│               ├── mod.rs                  # ProtocolAdapter trait + 工厂
│               ├── common.rs               # join_url
│               ├── openai.rs               # OpenAI
│               ├── claude.rs               # Anthropic
│               ├── gemini.rs               # Gemini + 原生格式备选
│               ├── azure.rs                # Azure OpenAI
│               └── custom.rs               # 自定义
├── src/
│   ├── main.tsx                            # React 入口
│   ├── App.tsx                             # 主布局 + 导航 + 使用指南
│   ├── types.ts                            # 类型定义
│   ├── lib/
│   │   ├── api.ts                          # Tauri IPC 封装
│   │   └── utils.ts                        # cn() 工具
│   ├── components/
│   │   ├── ui/                             # Radix UI 组件
│   │   ├── proxy/
│   │   │   ├── ProxyToggle.tsx             # 代理启停
│   │   │   └── TestChatDialog.tsx          # 测试对话
│   │   └── WelcomeGuide.tsx               # 首次启动引导
│   ├── pages/
│   │   ├── DashboardPage.tsx               # 数据看板
│   │   ├── ChannelPage.tsx                 # 渠道管理
│   │   ├── ApiPoolPage.tsx                 # API 管理
│   │   ├── TokenPage.tsx                   # 令牌管理
│   │   ├── LogPage.tsx                     # 使用日志
│   │   └── SettingsPage.tsx                # 系统设置
│   └── i18n/locales/                       # 中英文翻译
├── GUIDE.md                                # 使用指南（英文）
├── GUIDE_CN.md                             # 使用指南（中文）
├── package.json
└── PLAN.md
```

---

## 14. 变更日志

> **说明**：以下变更日志保留历史开发过程记录。若其中任何阶段性规则与第 4 节“当前路由规则 / 冷却与可用性规则 / UI 分组与 Tray 语义”冲突，一律以第 4 节为准。

### 2026-05-09 — Responses 兼容补齐 / 流末尾模型名显示 / 超时缓解

| # | 改动项 | 说明 |
|---|--------|------|
| 1 | **Responses API passthrough-first** | 保留未知 structured input item、非 function tool 与 provider-specific tool 字段；function tool 转换时保留额外字段。 |
| 2 | **Responses streaming tool call 强化** | 支持 Chat `delta.tool_calls` 的 function arguments 增量，arguments 兼容 string/object；最终 `response.completed.output` 包含 text/tool 输出。 |
| 3 | **Responses 真流式输出补齐** | 流式路径实时输出 Responses event graph，结束时带完整 `response.completed`、`output_text`、usage 与实际模型 fallback。 |
| 4 | **显示对话模型开关** | 新增 `show_conversation_model` 设置，默认开启；设置页可关闭。 |
| 5 | **流末尾追加实际模型名** | `forwarder.rs` 在 `data: [DONE]` 前追加普通文本 delta `model: <entry.model>`；仅当本轮出现正文 `delta.content` 时追加，纯 tool_calls 不追加。 |
| 6 | **proxy read_timeout 缓解** | `server.rs` 全局 `read_timeout` 从 60s 放宽到 120s，后续仍需分层超时正式方案。 |

### 2026-05-01 — 设置 L1 缓存 / AUTO 路由设置闭环

| # | 改动项 | 说明 |
|---|--------|------|
| 1 | **设置 L1 内存缓存** | `AppState` 新增 `settings: Arc<RwLock<AppSettings>>`，启动时从 DB 全量加载设置，运行时统一从 L1 读取。 |
| 2 | **统一设置接口** | `get_settings` 直接读取 L1；`update_settings` 写 DB 后刷新 L1，避免设置读写分散。 |
| 3 | **转发热路径去 DB 设置读** | 鉴权、AUTO 排序、熔断阈值、冷却时长、自动关闭状态码均改为读 L1；单次转发 settings DB 读降为 0。 |
| 4 | **代理状态同步 L1** | `start_proxy` / `stop_proxy` 更新 `proxy_enabled` 时同步刷新 L1，避免 DB 与内存状态漂移。 |
| 5 | **AUTO 路由排序闭环** | API 池切换排序同步 `default_sort_mode`；设置页修改排序先写本地再写后端，进入 API 池和 AUTO 路由保持一致。 |
| 6 | **验证** | `cargo check` 与 `pnpm exec tsc --noEmit` 均通过；仅剩既有 unused/dead code warnings。 |

### 2026-05-01 — 路由规则收敛 / 排序与可见性修正

> 历史说明：本节记录的是“路由规则收敛”阶段的过渡口径。若与第 4.2 / 4.3 / 4.4 节冲突，以第 4 节现行规则为准；尤其不要再把这里的 `enabled` / 显式模型 / AUTO fallback 设计描述误读为当前唯一真相。

| # | 改动项 | 说明 |
|---|--------|------|
| 1 | **API 池可见性规则** | API 池中的模型均为 `/v1/models` 可见模型；`enabled` 不再表示模型不可用，仅表示是否进入 AUTO。 |
| 2 | **AUTO 与显式模型分流** | AUTO 使用 `enabled=true + 未冷却` 条目；显式模型名使用 API 池所有同名条目（含 `enabled=false`），但冷却中跳过。 |
| 3 | **失败后 fallback 规则** | 显式模型调用失败后设置冷却并继续 AUTO fallback；冷却中的显式模型直接 fallback 到 AUTO。 |
| 4 | **排序规则同步** | `latest` 按发布日期倒序且不区分 enabled；`fastest` 按响应时间升序；`custom` 按 sort_index。AUTO、`/v1/models`、前端展示统一复用规则。 |
| 5 | **发布日期格式** | 保留 `YYYY-MM-DD` / `YYYY-MM`，兼容 `YYYYMMDD`；不再把完整日期截断为年月。 |
| 6 | **测速显示统一** | 响应时间显示统一为秒 (`s`)，不再显示 `ms`；内部仍按毫秒解析/排序。 |
| 7 | **渠道不可拉模型清单场景** | 对模型列表接口不可用的渠道，允许手动加入 API 池；渠道模型数口径调整为 API 池条目数 / available_models 数（如 `1 / 0`）。 |
| 8 | **规则类计划清理** | 废弃“disabled 不可正式路由”“AUTO 和显式模型都按 enabled 过滤”等旧规则；保留冷却作为可用性判断核心。 |

### 2026-04-28 — API 池一键测速 / 使用指南中英双语 / 渠道智能选模 / 体验优化（v0.3.0-dev）

| # | 改动项 | 说明 |
|---|--------|------|
| 1 | **API 池一键测速** | 新增测速按钮，逐个测试所有模型延迟，测试中显示旋转图标，成功显示绿色响应时间，失败显示红色 ✗，测试中列表不跳动 |
| 2 | **API 池响应时间字段** | `api_entries` 表新增 `response_ms TEXT DEFAULT ''`，兼容迁移自动补齐；新增 `test_entry_latency` 和 `update_entry_response_ms` Tauri 命令 |
| 3 | **渠道测速体验优化** | 改为测试所有渠道（不限于已启用），使用本地 state 逐个回填结果，避免列表跳动 |
| 4 | **渠道列表 nowrap** | 状态、响应时间、模型数列添加 `whitespace-nowrap`，防止换行 |
| 5 | **使用指南中英双语** | `GUIDE.md` → `GUIDE_CN.md`（中文原版），新建英文 `GUIDE.md`；侧边栏按 `i18n.language` 自动切换 |
| 6 | **渠道空时自动弹窗** | 进入渠道页时若无渠道自动弹出添加对话框，每次进入都触发 |
| 7 | **模型智能预选** | 拉取模型后自动选中 6 个月内发布的新模型 + 当前渠道已有模型 |
| 8 | **新增模型默认开启** | `sync_entries_for_channel` 新建条目 `enabled` 从 0 改为 1 |
| 9 | **选择同步修复** | 渠道保存时无论选择是否为空都调用 `selectModels`，清空选择能正确删除已有条目 |
| 10 | **API 池缓存刷新** | 渠道保存后同时 invalidate `entries` 和 `channels`，切换页面即时看到数据 |

### 2026-04-30 — CLI 连接页 / API 管理细节 / 流核心论证

| # | 改动项 | 说明 |
|---|--------|------|
| 1 | **API 管理交互增强** | 新增 Ctrl/Cmd+Click 开启并置顶、Shift+Click 当前筛选范围一键全开/全关。 |
| 2 | **API 管理列表细节修复** | 一键测速同渠道只显示单 spinner；测速按钮显示 `33/66` 进度；测速期间禁止重复点击；搜索框改为 sticky，并增加输入框内 `X` 清空按钮。 |
| 3 | **测速失败联动关闭** | 一键测速中 `failed` 状态会关闭模型，并在前端立即更新可见状态。 |
| 4 | **API 条目删除** | 在测试按钮与开关之间新增删除按钮，带确认弹窗。 |
| 5 | **CLI 连接页** | 新增“连接 CLI”页面和侧边栏入口；基于 `cli.json` 自动生成卡片；默认只显示最小 ENV，展开后显示扩展 ENV。 |
| 6 | **系统环境变量写入** | 新增 Tauri 命令 `set_user_env_vars`，Windows 下通过 `setx` 写入用户环境变量；CLI 页面点击“连接”后直接写入系统，而不是仅复制脚本。 |
| 7 | **CLI 数据远程加载 + 本地缓存** | 优先从 GitHub `cli.json` 拉取，成功则缓存到本地；失败时降级到本地缓存；再失败时降级到仓库内置 `cli.json`。 |
| 8 | **默认 CLI 值** | CLI 页面中默认 `API KEY = auto`、`model = auto`。 |
| 9 | **删除确认按钮视觉修复** | `destructive` 按钮前景色修正为白色，避免红底红字。 |
| 10 | **SSE PING 临时禁用** | 已暂时注释掉下游 `: PING\n\n` 注入，避免部分下游把 comment 帧拼进 JSON 导致解析失败。 |
| 11 | **流式核心论证文档** | 新增 `_internal_stream_core_review.md`，对比 NEW-API 核心稳定性来源，分析未来如何在保留项目特色的前提下逐步演进。 |
| 12 | **API 池 provider logo 显示** | API 池卡片左侧新增 provider/logo 区块，常见品牌按 `family > namespace alias > model prefix > custom` 规则显示 SVG logo，缺失时回退 `custom.svg`。 |

### 2026-04-29 — 错误冷却策略优化 (个人模式) / 空模型名修复

| # | 改动项 | 说明 |
|---|--------|------|
| 1 | **冷却策略改为"连续失败关闭"** | 任何错误都计数，达到阈值（默认 5 次）→ `enabled=false` + 6h 冷却。计数纯内存，重启归零。用户手动开启时清除计数+冷却。 |
| 2 | **`ProxyState` 新增 `failure_counts`** | `HashMap<String, u32>` 内存计数器，与 `AppState` 共享，Tauri 命令和代理服务器共用。 |
| 3 | **`forwarder.rs` 冷却逻辑重写** | `cool_down_entry` / `spawn_cool_down_entry` 均改为：计数+1 → 未达阈值则临时冷却 → 已达阈值则移出 AUTO 并设置 6h 长冷却；显式模型仍以冷却状态判断可用性。 |
| 4 | **`record_circuit_success` / `spawn_record_circuit_success` 清除计数** | 成功请求时清除内存计数。 |
| 5 | **`toggle_entry` 手动开启时重置** | 用户手动开启入口时清除 `failure_counts` + `cooldown_until`。 |
| 6 | **设置页标签语义更新** | "连续失败次数"→"连续失败关闭次数"，"恢复等待时间(秒)"→"冷却恢复时间(秒)"，英文同步。 |
| 7 | **空模型名 `""` 归一化为 `auto`** | `handlers.rs` + `router.rs` 同时处理，避免空字符串误走指定模型路径。 |
| 8 | **数据库零改动** | 计数器纯内存，复用 `enabled` / `cooldown_until` 字段。 |

代理核心（forwarder / handlers / router / circuit_breaker / server）扫描发现的问题及修复计划。

| # | 优先级 | 模块 | 问题 | 修复方案 | 状态 |
|---|--------|------|------|----------|------|
| 1 | 🔴 | forwarder | 流式 poll 内同步调用 DB 写日志 | `log_usage` 改为 `tokio::spawn` 异步写入 | ✅ |
| 2 | 🔴 | server | 代理 HTTP client 缺 read_timeout / gzip | 添加 `read_timeout(120s)` + `gzip(true)`，connect_timeout 30→15s | ✅ |
| 3 | 🔴 | forwarder | 流式错误后 stream drop 可能重复冷却 | `StreamLogGuard::drop` 检查错误类型，upstream_error 跳过重复冷却 | ✅ |
| 4 | 🟡 | circuit_breaker | `try_read`/`try_write` 静默失败 | `is_available` lock 竞争时返回 false（原为 true） | ✅ |
| 5 | 🟡 | handlers | 上游错误信息透传泄露内部细节 | 截断 error_body 至 300 字符 | ✅ |
| 6 | 🟡 | router | failover 不按延迟排序 | 解析 `response_ms` 按延迟升序排列可用 entry | ✅ |
| 7 | 🟢 | handlers | body 限制 10MB | 改为 32MB | ✅ |
| 8 | 🟢 | router | 每次请求查 DB | 保留现状，低并发场景可接受 | — |

### v0.4.1 发布前验证清单

流式路径和 circuit breaker 行为变更引入了新的失败模式，发布前需验证以下场景：

| # | 场景 | 关注点 |
|---|------|--------|
| 1 | **流式请求日志完整性** | 正常完成 / 客户端断开 / 上游错误 三种场景下日志是否写入 |
| 2 | **并发 circuit breaker** | 10+ 并发请求下无误跳过可用 entry |
| 3 | **未测速 entry 排序** | `response_ms` 为空的 entry 在 `fastest` 模式排在已测速条目之后，但保持稳定顺序 |
| 4 | **压缩兼容性** | gzip 上游 / 无压缩上游 / 不支持 identity 的上游 |
| 5 | **慢速渠道连接** | connect_timeout 15s 是否过短（偏远地区/慢服务器） |
| 6 | **长思考模型** | read_timeout 120s 是否够用（深度推理模型可能 >2min） |
| 7 | **错误截断** | 300 字符是否覆盖常见上游错误格式 |
| 8 | **新建渠道保存** | 不拉取模型直接保存功能正常 |
| 9 | **添加模型弹窗** | 无控件溢出 |

### 2026-04-28 — 渠道保存按钮修复 / gzip 解压支持

| # | 改动项 | 说明 |
|---|--------|------|
| 1 | **渠道新增保存按钮解锁** | 新建渠道时保存按钮不再强制要求先拉取模型，填完名称/URL/API Key 即可保存 |
| 2 | **gzip 解压支持** | reqwest 启用 `gzip` feature，修复上游返回 gzip 压缩响应时 `error decoding response body` 错误 |
| 3 | **添加模型弹窗精简** | 移除 AddApiDialog 中模型元信息提示框，减少无效信息干扰 |

### 2026-05-XX — 设置页恢复等待时间改为滑块控件

设置页 Circuit Breaker 区域的"恢复等待时间(秒)"从数字输入框改为滑块控件，操作更直观。

| # | 改动项 | 说明 |
|---|--------|------|
| 1 | **Slider 组件** | 新建 `src/components/ui/slider.tsx`，自定义范围滑块，支持 min/max/step/value |
| 2 | **恢复等待时间控件** | 从 `<Input type="number">` 改为 `<Slider>`，范围 300-1800s，步长 30s |
| 3 | **默认值调整** | `circuit_recovery_secs` 默认值从 300 改为 600 秒 |

### 2026-04-27 — 智能模型拉取 / API 池模型目录增强 / 自动禁用修正（v0.3.0-dev）

围绕"尽量让用户少填少猜"的目标，模型拉取、API 池展示、自动禁用策略和测试交互都做了收敛与修正。

| # | 改动项 | 说明 |
|---|--------|------|
| 1 | **单按钮智能拉模型** | 移除两步化检测 UI，渠道页只保留一个“拉取模型”按钮；后台先校对 API 类型与 Base URL，再执行多方式模型拉取 fallback |
| 2 | **URL / 类型自动回填** | 支持从错误的 endpoint/path 回退到正确的 base site；识别成功后自动回填 `api_type` 与 `base_url` |
| 3 | **会话内校对标志** | 校对状态只存在于当前 Add/Edit 弹窗内存，不落库；用户修改 URL / Key / API 类型后自动失效并重新校对 |
| 4 | **避免 Gemini 误判** | 收紧类型校对条件：只有命中 Gemini 权威 `/v1beta/openai/*` 路径才判为 `gemini`，避免 OpenAI-compatible 网关被误判 |
| 5 | **模型拉取与校对解耦** | 校对只负责推导推荐保存值；真正拉模型时仍会按多种协议/路径 fallback，避免特殊网关被“猜错后锁死” |
| 6 | **过滤 `auto` 模型** | 上游返回的 `auto` 不再进入渠道模型列表，避免被误保存到 API 池 |
| 7 | **API 池本地模型目录** | 新增 `models.json` + `modelsCatalog.ts` 本地索引，不落库；API 池卡片和手动添加弹窗实时显示发布、能力、上下文、输出等元信息 |
| 8 | **API 池文案压缩** | 标题改为 `渠道 / 模型`，冷却提示改为内联 `(冷却 5m)`；模型元信息压成一行，适配中英文短标签 |
| 9 | **近似模型匹配** | 支持 `provider/model`、`-free`、`-preview` 等后缀清洗和相似度匹配，提升聚合网关模型名识别率 |
| 10 | **自动禁用默认值恢复** | 自动禁用状态码默认改为 `401,403,410`，并在设置页显式开放输入框让用户自行增减 |
| 11 | **正式代理自动禁用生效** | 正式代理链路在收到命中状态码时会直接 `enabled=false`，同时保留 cooldown 作为“系统关闭”标识 |
| 12 | **日志尝试路径修复** | 使用日志详情中的 `attempt_path` 从对象数组正确格式化，不再显示 `[object Object]` |
| 13 | **测试对话关闭修复** | `TestChatDialog` 增加请求序号隔离与关闭强制收尾，避免 X 掉卡住请求后下一个测试持续转圈 |
| 14 | **渠道响应时间** | 编辑渠道保存时，自动将 URL 探测的 `latency_ms` 换算成秒，保存到 `response_ms` 字段；渠道列表"响应"列显示响应时间 |
| 15 | **渠道批量测速** | 表头"响应"列加刷新按钮，点击后逐个测试所有启用渠道的 URL 响应时间，测试中显示旋转图标，完成后更新显示；超时显示红色 ✗ |
| 16 | **托盘菜单恢复** | 恢复"Open Main Window"菜单项（最顶部），右键托盘图标可直接打开主窗口 |

### 2026-04-26 — 个人版模型冷却机制（v0.2.0-dev）

> 历史说明：本节反映的是 v0.2.0-dev 时期的冷却机制设计。这里关于“显式模型路由”“enabled 语义”等表述已经不是当前文档的最高优先级说明；若与第 4 节冲突，以第 4 节为准。

放弃 NEW-API 风格的复杂状态码/关键词路由策略，改为个人版稳定优先的"模型冷却"机制。

| # | 改动项 | 说明 |
|---|--------|------|
| 1 | **数据库兼容检查** | `api_entries` 新增 `cooldown_until INTEGER`；启动时自动补字段 |
| 2 | **正式路由过滤冷却模型** | 冷却中模型不参与 AUTO 和显式模型路由 |
| 3 | **失败统一冷却** | 任意上游非正常设置 `cooldown_until = now + 300s`，继续 failover |
| 4 | **成功清除冷却** | 非流式/流式成功后清除 `cooldown_until` |
| 5 | **用户开关语义固定** | `enabled` 只表示是否进入 AUTO；显式模型是否可用由冷却状态决定。 |
| 6 | **取消复杂策略配置** | 删除自动禁用状态码、自动重试状态码、自动禁用关键词、504/524 特判 |
| 7 | **默认冷却参数** | 连续失败 1 次，冷却 300 秒；启动迁移旧默认值 `4/60` → `1/300` |
| 8 | **设置页精简** | 熔断卡片只保留连续失败次数和恢复等待时间 |
| 9 | **API 池状态点** | 红点=冷却中、灰点=未开启、绿点=已开启未冷却 |
| 10 | **测试对话直连上游** | 改用 Tauri `test_chat` 命令直接请求上游，不走代理端口，不触发 fallback |
| 11 | **日志点击行展开** | 移除三角图标，点击整行展开详情 |
| 12 | **侧边栏使用指南** | 系统设置下方新增"使用指南"菜单，外链 GitHub GUIDE.md |
| 13 | **API 管理光标** | 拖拽手柄改为 `cursor-pointer`，避免 Windows 上 `cursor-grab` 锯齿 |

---

## 10. 待办与预留功能

### 10.1 通用限额查询接口 (已实现)

**目标**：统一查询多个供应商的 Token Plan / 限额信息，支持 Kimi、智谱、MiniMax 等国产 Coding Plan 供应商。

**设计**：
- 输入：`baseUrl` + `apiKey`
- 输出：统一的 `LimitQueryResult` 结构
- 自动识别供应商并路由到对应 API

**返回结构**：
```ts
export interface LimitQueryResult {
  provider: string; // "kimi" | "zhipu" | "minimax_cn" | "minimax_global"
  credentialStatus: "valid" | "expired" | "not_found" | "parse_error";
  credentialMessage: string | null;
  success: boolean;
  tiers: LimitTier[];
  error: string | null;
  queriedAt: number | null;
  raw: unknown | null; // 原始响应，便于扩展
}

export interface LimitTier {
  name: string; // "five_hour" | "weekly_limit" 等
  utilization: number; // 0-100 百分比
  resetsAt: string | null; // ISO 8601 重置时间
}
```

**已支持供应商**：
| 供应商 | 识别 URL | 限额接口 |
|--------|---------|----------|
| Kimi For Coding | `api.kimi.com/coding` | `GET /coding/v1/usages` |
| 智谱 GLM | `bigmodel.cn` / `api.z.ai` | `GET /api/monitor/usage/quota/limit` |
| MiniMax 国内 | `api.minimaxi.com` | `GET /v1/api/openplatform/coding_plan/remains` |
| MiniMax 国际 | `api.minimax.io` | `GET /v1/api/openplatform/coding_plan/remains` |

**实现状态**：
- [x] Rust 后端实现 (`src-tauri/src/commands/limit.rs`)
- [x] Tauri command 注册 (`query_limit`)
- [x] TypeScript 类型定义
- [ ] 前端 UI 集成（待需求明确）

**扩展点**：
- 新增供应商只需在 `detect_coding_plan_provider()` 添加 URL 规则
- 新增供应商查询函数 `query_xxx()` 后在 `query_limit_by_url()` 路由
- `raw` 字段保留原始响应，方便后续调试或提取更多字段

---

### 2026-04-26 — 托盘菜单同步刷新修复（v0.2.0-dev）

API 管理页排序/开关/创建模型、Channel 选择/更新/删除后，都会刷新系统托盘菜单。

### 2026-04-25 — 转发核心对齐 NEW-API（v0.2.0-dev）

- AUTO 仅从 enabled 条目选择；显式模型可调用 API 池可见同名条目
- Claude SSE 标准 frame
- 流式日志结束原因（done/upstream_error/timeout/dropped）
- 重试路径记录（attempt_path）
- HTTP 连接超时 + 流式 idle timeout
- HTTP Client 复用
- SSE Ping 保活
- AUTO 排序稳定性修正

### 2026-04-25 — UI 体验优化

- 令牌管理表格重构
- 渠道默认类型改为 custom
- 移除拖拽滚动

### 2026-04-25 — v0.1.0 首版发布

- 绿色便携版（数据库 EXE 同目录）
- 托盘菜单、首次启动引导
- 主题切换、更新检查
- 实时日志推送

### 2026-04-24 — 协议适配模块化重构

- 单体 `protocol.rs` 拆分为 5 个独立适配器
- `ProtocolAdapter` trait 统一接口
- 88 个单元测试

---

*本文档随开发持续更新。*

---

## 15. Web Admin 架构

> `WEB_ADMIN_PLAN.md` 已废弃，架构决策记录在此。

### 15.1 核心原则

```text
一套 React UI + 一个 ApiAdapter 边界 + 两种运行时调用方式
```

- `src/App.tsx` 是 Desktop/Web 共同入口
- 页面通过 `useApiAdapter()` 获取数据，不直接调用 Tauri invoke 或 HTTP fetch
- 运行时选择：`unifiedApiAdapter` 内部检测 Tauri runtime；桌面走 Tauri invoke，Web Admin 走 HTTP `/admin/*`。
- **禁止翻译层、Schema 翻译、动态 UI 翻译、第二套 Web 页面**

### 15.2 当前实现

| 组件 | 说明 |
|------|------|
| `src/App.tsx` | 统一入口。Desktop 走 Tauri API，Web 走 HTTP adapter + 登录门 |
| `src/components/LoginScreen.tsx` | Web 模式登录页。POST `/admin/login` 拿 token 存 localStorage |
| `src/lib/useApiAdapter.ts` | 返回统一 `apiAdapter`，页面层不关心 Desktop/Web 调用方式 |
| `src/lib/unifiedApiAdapter.ts` | 统一 adapter：Tauri runtime 下调用 invoke，Web Admin 下调用 HTTP `/admin/*`，统一错误和 token 处理 |
| `vite.config.web.ts` | 把 `src/` 构建到 `dist-web-admin/`，Tauri 模块走 stub |
| `src/stubs/tauri-*.ts` | Web 构建用 Tauri 模块桩 |
| `src-tauri/src/admin/static_files.rs` | Rust 静态文件服务，`dist-web-admin/` 映射到 `/` |
| `src-tauri/src/admin/auth.rs` | HTTP auth 中间件，debug 构建自动 bypass |

### 15.3 数据流

```text
Desktop: App.tsx → useApiAdapter() → unifiedApiAdapter → Tauri invoke → Rust commands → Service → DB
Web:     App.tsx → LoginScreen → token → useApiAdapter() → unifiedApiAdapter → HTTP /admin/* → Rust handlers → Service → DB
```

### 15.4 已验证

- [x] 9099 根路径 `/` 呈现同一套 UI
- [x] 所有页面（Settings/Channels/Pool/Tokens/Logs/Dashboard）通过 `useApiAdapter` 加载数据
- [x] 登录 → token → 认证 API 调用完整链路
- [x] Desktop 不受影响（跳过登录，走 invoke）

### 15.5 已清理项

- [x] 移除 dev auth bypass（`src-tauri/src/admin/auth.rs`）
- [x] 删除旧 `src/web-admin/` 壳（WebAdminApp.tsx、旧 api.ts、旧 vite.config.ts）
- [x] 删除旧分裂 adapter（`src/lib/tauriApiAdapter.ts`、`src/lib/webAdminApiAdapter.ts`），统一到 `src/lib/unifiedApiAdapter.ts`

### 15.6 后续待做

- [ ] `/admin/*` API prefix 是否迁移到根路径（当前保持 `/admin/*`）
- [ ] Standalone / Headless 模式验证：当前 `--headless` 仍只是跳过窗口/托盘，真正 Server-only 模式见 P1「真正无头 / Server-only 运行模式」。
- [ ] Web 构建与部署 smoke：保留 `vite.config.web.ts` + `dist-web-admin/`，验证刷新、登录、页面加载和一次写操作。
