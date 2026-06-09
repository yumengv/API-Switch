# API Switch — 技术白皮书

> 文档定位：架构、模块、数据流、数据库、协议适配、运行模式与实现细节  
> 文档层级：程序实现可归纳为 `WHITEPAPER.md`；`WHITEPAPER.md` 可归纳为 `PLAN.md`。反向看，`PLAN.md` 的扩展是 `WHITEPAPER.md`，`WHITEPAPER.md` 的扩展是程序实现。  
> 更新日期：2026-06-09

---

## 1. 系统定位

API Switch 是一个本地优先的个人 AI API 管理与转发中心。它在客户端工具和多家上游 AI 服务之间提供统一入口，负责渠道管理、模型池管理、模型分组、协议适配、路由选择、故障转移、批量测速、日志记录和 Web/Desktop 管理。
面向个人本地使用，默认信任本机环境，不按公网多用户服务进行安全模型设计。
核心运行路径：

```text
Client / AI Tool
  -> API Switch Proxy Endpoint
  -> protocol parser / compatibility layer
  -> router / failover / cooldown
  -> upstream provider
  -> response converter / stream relay
  -> usage log / dashboard stats
```

管理路径：

```text
Desktop UI / Web Admin
  -> Unified ApiAdapter
  -> Tauri commands or Admin HTTP API
  -> Backend services (settings, channels, pool, tokens, logs)
```

## 2. 运行时架构与统一 UI 约束

### 2.1 双运行时架构

项目维护**一套 UI 代码**，同时运行于两种环境：

| 运行时 | 运行方式 | 数据刷新方案 |
|--------|----------|-------------|
| **Desktop** | Tauri v2 (WebView) | IPC `invoke` + Tauri Event → Rust `DirtyFlags` → `useDirtyPolling` |
| **Web Admin** | HTTP 管理界面 | fetch HTTP API → `/state-version` 版本对比 → `useDirtyPolling` |

### 2.2 统一适配层

所有 UI 到后端的通信统一经过 `src/lib/unifiedApiAdapter.ts` 中的 `apiAdapter`，在运行时根据环境自动分发：

```
apiAdapter.dirty.take(module)
  Desktop → tauriCmd('take_dirty', { params: { module } }) → 读 Rust AtomicBool
  Web     → GET /state-version → 对比版本号
```

```typescript
// 1. 数据获取 — 统一走 React Query + useDirtyPolling（推荐）
import { useQuery } from "@tanstack/react-query";
import { useDirtyPolling } from "@/lib/useDirtyPolling";

useDirtyPolling('log');  // 桌面用 dirty flag, Web 用 state-version
const { data } = useQuery({ queryKey: ["usageLogs"], queryFn: () => api.usage.getLogs() });

// 2. 变更操作 — 统一走 useMutation，onSuccess 中 invalidateQueries
import { useMutation, useQueryClient } from "@tanstack/react-query";

const mutation = useMutation({
  mutationFn: (name: string) => api.tokens.create(name),
  onSuccess: () => queryClient.invalidateQueries({ queryKey: ["accessKeys"] }),
});

// 3. ❌ 禁止 — 直接使用 Tauri Event 做数据刷新（桌面专用，Web 无效）
// import { useEvent } from "@/lib/events";
// useEvent("new-usage-log", () => invalidateQueries(...));
// 改用 useDirtyPolling 代替

// 4. ❌ 禁止 — 独立 setInterval 轮询（不经过 dirty polling，双平台行为不一致）
// useEffect(() => { const id = setInterval(fetchData, 5000); return () => clearInterval(id); }, []);
// 改用 useDirtyPolling + React Query 代替
```

### 2.3 核心约束

> **所有 UI 层的数据刷新和状态同步，必须使用 `useDirtyPolling` + React Query 的 `invalidateQueries` 模式，禁止引入仅桌面可用的方案。**

原因：
- 维护两套 UI（Desktop 专用 + Web 专用）成本极高，项目早期已决定共享代码
- Tauri Event 仅桌面可用，`useDirtyPolling` 的 `apiAdapter.dirty.take()` 在双端都有实现
- 独立的 `setInterval` / `setTimeout` 轮询不经过 dirty polling 状态机，会导致双平台行为不一致和请求冗余

此约束已在 v0.6.59 中强制执行：LogPage/ChannelManager/PoolManager/TokenPage 均使用 `useDirtyPolling`，TokenManager 从独立 `setInterval` 迁移到 `useQuery` + `useDirtyPolling`。

---

## 2. 运行模式

### 2.1 Desktop 模式

Desktop 模式通过 Tauri v2 启动完整 GUI runtime，提供：

- React 管理界面
- Tauri IPC 命令调用后端能力
- 系统托盘菜单
- 本地窗口显示/隐藏
- 本地代理服务和管理服务启动控制

### 2.2 Web Admin 模式

Web Admin 使用同一套 React 页面，但通过 HTTP Admin API 访问后端。前端通过统一 ApiAdapter 判断当前是否处于 Tauri runtime：

- Tauri runtime：调用 `@tauri-apps/api/core` 的 `invoke`
- Web runtime：调用同源 `/admin/*` HTTP API

HTTP Admin API 使用 Bearer Token 鉴权，登录成功后前端保存 token；401/403 时清理 token 并触发认证过期事件。

**连接应用行为**：Web Admin 和桌面端行为一致，都允许自动写入配置文件。这是个人开源系统，代码公开，方便性优先于过度安全限制。无头环境下用户通过 Web Admin 点击"连接应用"即可自动配置，无需手动操作。

### 2.3 Server-only / Headless 模式

Server-only 模式用于无 GUI 环境运行。启动参数或环境变量可绕过 Tauri GUI runtime，只启动后端服务能力：

- `--headless`
- `API_SWITCH_HEADLESS=1`

该模式适合服务器、NAS、远程主机或只需要代理/API 管理服务的场景。

### 2.4 Android Mobile 构建基线

Android 基线使用 Tauri v2 mobile 工程生成 Android 壳，当前只要求能完成工程生成和 APK 构建；真机代理监听、WebView 生命周期、cleartext/loopback 策略和 Android 客户端联调属于后续真机验证。

Windows 本机 Android SDK 原始安装路径位于 `D:\Program Files\Android\Sdk`，该路径包含空格，NDK 构建会报 `NDK path cannot contain spaces`。因此 Android 构建统一使用无空格 junction 路径：

- `ANDROID_HOME=D:\Android\Sdk`
- `ANDROID_SDK_ROOT=D:\Android\Sdk`
- `ANDROID_NDK_HOME=D:\Android\Sdk\ndk\27.3.13750724`
- `JAVA_HOME=D:\Program Files\Android\Android Studio\jbr`

重新构建 Android APK 时使用以下命令：

```powershell
$env:JAVA_HOME='D:\Program Files\Android\Android Studio\jbr'
$env:ANDROID_HOME='D:\Android\Sdk'
$env:ANDROID_SDK_ROOT='D:\Android\Sdk'
$env:ANDROID_NDK_HOME='D:\Android\Sdk\ndk\27.3.13750724'
$env:CARGO_BUILD_JOBS='1'

pnpm android:build
```

当前已验证的 Android 构建产物路径：`src-tauri/gen/android/app/build/outputs/apk/universal/release/app-universal-release-unsigned.apk`。

---

## 3. 技术架构

### 3.1 技术栈

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

### 3.2 整体架构

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
│  │ channels │ api_entries │ model_groups │ keys │ logs │ │
│  └───────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────┘

         ▲ 并行运行
┌─────────────────────────────────────────────────────┐
│              Axum Proxy Server (0.0.0.0:port)        │
│  ┌─────────────┐  ┌─────────┐  ┌────────────────┐  │
│  │   /health   │  │ /v1/models│  │/v1/chat/completions│  │
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

## 4. 前端架构

### 4.1 React 页面层

管理界面主要包括：

- Dashboard：统计概览、趋势图、排行和模型分布
- 渠道管理：渠道新增、编辑、删除、测速、拉取模型和同步 API 池
- API 管理：API 池条目管理、分组筛选、排序、启用/禁用、测速和测试对话
- 模型分组：分组配置、启用/禁用、选择模型、排序数字和 `/v1/models` 对外分组名
- 令牌管理：Access Key 创建、启用/禁用、删除
- 使用日志：分页、筛选、行展开、失败尝试路径查看
- 系统设置：代理、冷却、运行模式和 UI 设置
- CLI 配置：生成 OpenCode、Claude Code、Codex CLI 等工具的连接配置

### 4.2 模块清单

| 页面 | 文件 | 功能 |
|------|------|------|
| API 管理 | `ApiPoolPage.tsx` | 分组筛选、拖拽排序、启停、状态点（绿/红/灰）、测试对话、一键测速、响应时间显示 |
| 模型分组 | `GroupManagementPage.tsx` / `GroupManagement.tsx` | 分组 CRUD、启停、模型选择、排序数字维护 |
| 渠道管理 | `ChannelPage.tsx` | 统一添加/编辑弹窗、模型拉取/选择、渠道模型批量测速 |
| 令牌管理 | `TokenPage.tsx` | 密钥 CRUD + 复制 |
| 使用日志 | `LogPage.tsx` | 分页、成功/失败筛选、点击行展开详情 |
| 数据看板 | `DashboardPage.tsx` | 统计卡片 + 4 图表 |
| 系统设置 | `SettingsPage.tsx` | 代理、安全、冷却、托盘、通用设置 |
| 使用指南 | 侧边栏菜单项 | 加载本地使用指南文档（GUIDE.md / GUIDE_CN.md） |
| 测试对话 | `TestChatDialog.tsx` | 直接调 Tauri 命令请求上游，不走代理 |

### 4.3 Unified ApiAdapter

`src/lib/unifiedApiAdapter.ts` 是 Desktop/Web 管理端共用的运行时适配层。它把前端业务调用抽象成统一接口，并在运行时分派到：

- Tauri IPC command
- Web Admin HTTP endpoint

典型模块包括：

- `channels`
- `pool`
- `usage`
- `tokens`
- `settings`
- `proxy`
- `dirty`
- `translation`

### 4.4 分页与无限加载

部分页面使用分页或无限加载降低大列表渲染和传输成本。API 池页面的前端数据集来自已加载分页，因此基于当前前端列表执行的批量动作只覆盖已加载行；若业务语义要求覆盖当前分组全量数据，需要后端提供按条件查询 ID 或后端批量执行接口。

### 4.5 Dirty 状态刷新

业务数据变化通过 dirty 标记驱动页面刷新。当前实现中，Desktop/Web Admin 仍存在消费式 dirty bool 和页面事件直刷并存的历史残留；理想模型是业务变更只写 dirty 标记，客户端用非消费式模块版本号各自判断是否需要刷新。

---

## 5. 后端分层

### 5.1 模块清单

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

### 5.2 Commands / Admin Router

后端对管理端暴露两套入口：

- Tauri commands：供 Desktop IPC 调用。
- Admin HTTP router：供 Web Admin 调用。

两套入口应保持语义一致，统一调用 service 层，避免 Desktop 与 Web 管理行为分叉。

### 5.2.1 API 池条目别名边界

API 池条目的核心身份由 `channel_id + model` 决定，渠道和模型名称用于路由匹配与上游请求，不应在模型管理页编辑时被修改。模型管理页的“编辑模型”只允许修改 `display_name`（界面称为“别名”），用于本地展示和搜索，不改变实际请求模型名、所属渠道、路由匹配、冷却状态或测速归属。

Desktop 入口通过 `update_entry_display_name` Tauri command 更新别名；Web Admin 入口通过 `PUT /admin/pool/:id/display-name` 更新别名。两者都走 `pool_service::update_entry_display_name`，更新后标记 pool dirty，并触发条目刷新。

### 5.3 Service 层

Service 层承接业务逻辑，例如：

- 渠道保存与模型同步
- API 池条目创建、删除、分组、排序、测速
- 设置读取与更新
- 代理启动/停止
- 日志查询与统计聚合

### 5.4 DAO 层

DAO 层负责 SQLite 持久化访问。主要职责包括：

- 构造 SQL 查询
- 处理分页、筛选、排序
- 写入和更新渠道、API 池、日志、设置等数据
- 将数据库字段映射为 Rust 结构体

## 6. 数据模型

### 6.1 表结构

| 表名 | 用途 | 关键字段 |
|------|------|----------|
| `channels` | API 渠道 | id, name, api_type, base_url, api_key, available_models(JSON), selected_models(JSON), enabled |
| `api_entries` | 路由池条目 / 对外可见模型 | id, channel_id, model, display_name, group_name, sort_index, enabled, cooldown_until, response_ms, score, provider_logo, release_date, model_meta_zh/en |
| `model_groups` | 模型分组配置 | name, description, enabled, priority, sort_index, is_system |
| `model_group_entries` | 模型分组成员关系 | group_name, entry_id, sort_index, created_at, updated_at |
| `access_keys` | 访问密钥 | id, name, key(UUID), enabled |
| `usage_logs` | 请求日志 | 25+ 字段，含 token 统计、延迟、错误信息 |
| `config` | 全局配置 | KV 存储 |

### 6.2 channels

渠道表保存上游服务配置，核心字段包括：

- 渠道 ID、名称、协议类型
- Base URL
- API Key
- 启用状态
- 响应时间
- 可用模型快照 `available_models`
- 已选择模型快照 `selected_models`

`available_models` 用于恢复渠道编辑器里的模型列表；`selected_models` 用于恢复勾选状态和保存时同步 API 池。

### 6.3 api_entries

API 池条目是运行时路由的事实源，核心字段包括：

- 条目 ID
- 所属 channel ID
- 原始模型名 `model`
- 展示名 / 别名 `display_name`
- 分组名 `group_name`
- 启用状态
- 排序索引
- 响应时间 `response_ms`
- 模型目录元信息

路由、测速、启用/禁用和排序都以 `api_entries` 为准。`group_name` 保留为旧数据兼容和模型管理页主分组显示；模型分组的真实成员关系由 `model_group_entries` 维护。模型管理页拖拽排序和模型分组页排序数字最终都写入同一条目的 `sort_index`。

### 6.4 model_groups

`model_groups` 保存模型分组的配置层信息，核心字段包括：

- 分组名 `name`
- 描述 `description`
- 分组启用状态 `enabled`
- 分组展示优先级 `priority`
- 分组列表排序 `sort_index`
- 是否系统分组 `is_system`

`auto` 是系统分组，初始化时由 `ensure_default_model_groups` 保证存在，并在冲突更新时保持启用和系统标记。普通分组可以禁用；禁用分组不会参与分组精确匹配，也不会作为分组模型出现在 `/v1/models`。删除普通分组时，组内 `api_entries.group_name` 会回写为 `auto`。

为了兼容旧数据，`list_model_groups()` 会合并两类来源：

- `model_groups` 中已经配置的分组。
- `api_entries.group_name` 中存在但尚未写入配置表的历史分组。

历史分组会以启用状态返回，确保升级后已有路由不被静默关闭。

### 6.5 model_group_entries

`model_group_entries` 是模型分组的多对多成员表。一个 `api_entries.id` 可以同时加入多个 `group_name`，因此同一个上游模型可以暴露到多个业务分组中。

关键规则：

- 启动迁移时，如果该表为空，会从旧的 `api_entries.group_name` 导入初始成员关系。
- 新增模型条目时会写入默认分组成员关系，默认是 `auto`。
- 保存某个分组时只替换该分组自己的成员，不删除该模型在其他分组里的成员关系。
- 删除普通分组时，相关成员关系会删除，并给这些条目补回 `auto` 成员关系。
- 路由时会按该表展开分组候选；按具体模型名或别名请求时会按 `api_entries.id` 去重，避免同一条目因属于多个分组而重复尝试。

### 6.6 usage_logs

使用日志记录每次代理请求的运行结果，常用字段包括：

- 请求模型
- 实际命中模型
- channel / entry 标识
- HTTP 状态码
- 成功/失败
- 错误信息
- 输入、输出、缓存、推理 token
- 首 token 延迟和总耗时
- 失败尝试路径
- 扩展信息 `other`

`other` 用 JSON 保存结构化补充信息，例如 `requested_model`、`resolved_model`、`first_token_ms`、`status_code`、`attempt_path`、`stream_end_reason` 等。

### 6.7 access_keys

Access Key 用于客户端访问代理时的身份识别和可选鉴权。关闭强制校验时，Access Key 仍可用于日志身份追踪。

### 6.8 settings

设置表保存代理、管理端、冷却、UI 和运行行为配置。Web Admin 设置更新带版本号，用于处理多页面或多进程修改冲突。

关键运行行为设置：

| 设置项 | 默认值 | 作用 |
|--------|--------|------|
| `disable_reasoning` | `true` | 全局控制 reasoning/thinking 是否传递到上游。默认关闭（`true`），删除 thinking/reasoning 触发字段；开启后原样传递上游，不清理任何字段 |

---

## 7. 渠道与模型同步流程

### 7.1 URL 探测与协议识别

渠道新增/编辑时，URL 测速阶段承担：

- Base URL 连通性探测
- 协议类型识别
- 可用端点校准
- 响应时间记录

#### 探测候选与优先级

候选 URL 分为两个 Scope，USER URL 与 BASE SITE 同时并发探测：

- **USER URL**：用户输入的原始 URL 及其 `/v1` 变体（输入本身已含 `/v1` 时也尝试去掉）
- **BASE SITE**：从用户 URL 抽取的站点根（`extract_base_site`）及其 `/v1` 变体

每个候选 URL 按协议（openai / responses / anthropic / gemini / azure）展开，所有 `(scope, base_url, api_type)` 组合通过 `futures::future::join_all` 并发执行。候选结果状态：

- **Usable**：模型列表非空 + 协议权威性校验通过（gemini 须命中 `/v1beta/...`，azure 须命中 `/openai/deployments`）
- **Reachable**：HTTP 响应存在（401/403/429/4xx/5xx 视为可达；Network/Timeout 显式忽略，不计入可达）
- **NotFound**：网络层失败或未取得响应

最终候选选择优先级（`select_preferred_endpoint_candidate`）：

1. USER URL Usable
2. USER URL Reachable
3. BASE SITE Usable
4. BASE SITE Reachable

**只要 USER URL 命中任一候选（Usable 或 Reachable），BASE SITE 结果不参与最终选择**。协议绿色标记基于全部 Reachable 候选聚合的 `detected_type`，仅需可达即标绿。全失败时尊重用户输入与选择，不强制纠正。

探测过程日志中 URL 需经 `sanitize_url_for_log` 脱敏（剥离 query），避免在日志中泄露可能附带在 query 里的凭据。

获取模型列表只基于已测速/已确认的节点信息执行，避免“探测 URL”和“拉取模型 URL”各自分叉。

### 7.2 模型列表拉取

模型拉取支持标准模型列表接口；对于无法返回模型列表的中转站，可由用户手动添加模型名。

### 7.3 渠道模型批量测速

渠道编辑弹窗中的模型测速基于当前前端筛选结果执行，不再限制模型数量。前端复制 `filteredModels` 作为测速队列，并启动最多 8 个 worker 并发调用 `channels.testModel()`；每个 worker 从共享下标领取下一个模型，完成后更新单模型结果和 `current / total` 进度。

测速结果只保存在编辑弹窗状态中，用于辅助用户选择模型：

- 成功：记录 latency_ms，界面显示绿色耗时。
- 失败：记录错误原因，界面显示失败标记。
- 取消/关闭：通过递增 `testSeqRef` 使旧测速序列停止继续写 UI 状态。

该批量测速会真实请求上游模型，因此会消耗上游请求次数或少量额度。大批量测速前应通过搜索或时间范围缩小列表。

### 7.4 保存渠道并同步模型

保存渠道时会同时维护：

- `channels.available_models`：当前渠道可用模型快照
- `channels.selected_models`：用户选择的模型快照
- `api_entries`：实际参与 API 池与运行时路由的条目

这样渠道编辑器能恢复上次拉取和勾选状态，同时运行时只依赖 API 池事实源。

### 7.5 批量 API Key 创建

批量 API Key 创建会按每个非空 Key 创建渠道，空行忽略；如果输入全部为空，应返回明确错误。

---

## 8. API 池与路由

### 8.1 API 池管理

API 池支持：

- 手动新增模型条目
- 删除条目
- 启用/禁用 AUTO 参与
- 分组筛选和条目分组修改
- 拖拽排序
- 单个条目测速
- 当前列表批量测速

响应时间内部按毫秒保存和排序，UI 统一按秒展示。批量测速会为每个条目刷新 `score`，分数按“模型能力 60% + 渠道速度 20% + 模型对话速度 20%”计算；同一轮测速内只临时缓存“模型名 -> 模型能力分”，测速结束后丢弃缓存。

### 8.2 模型分组管理

模型分组是路由层的对外模型别名集合。配置入口位于「模型分组」，由 `model_groups` 表保存分组元信息，由 `model_group_entries` 保存条目成员关系。

关键规则：

- `auto` 是系统分组，不能删除，始终启用。
- 普通分组可以创建、编辑描述、启用/禁用和删除。
- 删除普通分组会把组内条目移回 `auto`。
- 同一个条目可以加入多个普通分组。
- 选择模型时通过 `replace_model_group_entries()` 只替换目标分组的成员关系，不影响其他分组。
- 分组页排序数字是给使用者看的优先级数字，数字越大越靠前；保存时前端换算为内部 `sort_index` 写回条目。
- 模型管理页拖拽排序和模型分组页排序数字共用 `api_entries.sort_index`，因此任一页面调整后另一个页面应同步显示。

分组模型会被作为模型项加入 `/v1/models`，`owned_by = "group"`。禁用分组不会出现在 `/v1/models`，也不会参与分组精确匹配。

### 8.3 数据流：请求代理流程

```
Client → POST /v1/chat/completions
  │
  ├─ 1. auth::extract_access_key()       ← 从 Header 提取并验证密钥
  ├─ 2. 解析 JSON body → model / stream
  ├─ 3. router::resolve_with_disabled_groups()
  │                                      ← 分组精确匹配 → 模型/别名匹配 → 模糊匹配 → AUTO fallback
  ├─ 4. forwarder::forward_with_retry()
  │     ├─ 遍历 entries:
  │     │   ├─ adapter.build_chat_url() + apply_auth() + transform_request()
  │     │   ├─ 如果 settings.disable_reasoning = true，在归一后的 OpenAI-compatible 请求体上统一关闭 reasoning/thinking
  │     │   ├─ reqwest::send()
  │     │   ├─ 成功 → 清除冷却 → 返回客户端
  │     │   └─ 失败 → 设置冷却 → 继续下一个
  │     └─ 全部失败 → 502 AllProvidersFailed
  └─ 5. insert_usage_log()
```

### 8.4 全局控制 reasoning/thinking 请求

设置项 `disable_reasoning` 控制 reasoning/thinking 数据是否传递到上游。该设置**默认开启**（`true`），即默认不传递 reasoning 数据，存储在 SQLite `config` 表，并通过 `AppSettings` 暴露给 Desktop 与 Web Admin 设置页。

处理位置固定在各协议适配器完成 `transform_request()` 后、`reqwest::send()` 前。此时 Claude / Gemini / Azure / Responses 等入口已经归一为 OpenAI-compatible 上游请求体，因此只需要在 `forwarder.rs` 的公共路径执行一次改写，避免五套协议分别实现造成遗漏或行为分叉。

`disable_reasoning = true`（默认）时：

- 删除请求体顶层的 `thinking`、`reasoning`、`reasoning_content`、`reasoning_text`、`reasoning_details`、`reasoning_effort`。
- 删除 `messages[]` 中每个对象上的同名字段。
- 不往上游注入任何字段。
- 不递归改写 `messages[].content` 等用户文本内容，避免误伤真实输入。

`disable_reasoning = false` 时：

- reasoning/thinking 字段原样传递上游，不做任何处理。

### 8.5 路由匹配顺序（唯一真相）

| 场景 | 行为 |
|---|---|
| 路由总规则 | 先做**分组精确匹配**，未命中则做**模型名精确匹配**，再做**别名精确匹配**，再做**模型模糊匹配**，最后 **fallback 到 AUTO 组** |
| 匹配预处理 | `request.model`、`group_name`、`entry.model` 在匹配前统一 `trim`；空模型名 `""` 直接替换为 `"auto"` |
| 分组匹配 | `request.model` 与 `group_name` 的比较**不区分大小写**；禁用分组不参与分组匹配 |
| 模型精确匹配 | `request.model` 与 `entry.model` 的比较**不区分大小写** |
| 别名精确匹配 | `request.model` 与非空 `entry.display_name` 的比较**不区分大小写** |
| 模型模糊匹配 | `request.model` 与 `entry.model` 的模糊匹配**不区分大小写**；规则为 **`entry.model` 包含 `request.model`** |
| `model = "auto"` | 不做特殊优先分支；和任何模型请求一样，按统一流程参与匹配。由于空模型会先替换成 `auto`，因此空模型请求也按同一流程处理 |
| AUTO 组定义 | AUTO 组就是 `group_name = "auto"`，不再受 settings / API 管理页当前分组 / tray 状态影响 |
| 最终失败 | 当分组精确匹配、模型/别名匹配、模型模糊匹配、AUTO 组 fallback 都没有可用条目时，按当前正常模型请求失败流程处理 |

### 8.6 排序策略

同一候选集合内支持：

- 自定义顺序 (`sort_index`)
- 最快优先 (`response_ms`)
- 最新优先 (`release_date`)

排序策略影响候选尝试顺序；同一模型跨多个渠道时，失败后继续尝试下一个候选。

### 8.7 冷却与熔断 — 三级容错体系

失败处理由三层层叠的冷却/熔断机制组成，范围逐层收缩、时效逐层递增：

| 层级 | 作用域 | 时效 | 触发条件 | 清除条件 |
|------|--------|------|----------|----------|
| **L1：DB 冷却** | 单个 `api_entry` | 可配置（300-1800s，默认 600s） | HTTP 5xx、超时、空流、空响应、关键词匹配（默认） | 请求成功 / 冷却到期 |
| **L2：内存熔断器** | 单 channel + 模型名 | 60s 窗口 → Open→HalfOpen | 同一模型连续 5 次失败 | 试探成功 / 超时 |
| **L3：渠道冷冻** | 整条 channel | 6 小时（21600s） | 上游响应含指定关键词 **且** `keyword_freeze_scope = "channel"` | 时间到期 |

三个层级独立工作、互不阻塞：L1 冷却只跳过当前条目，L2 熔断跳过同一 channel 的同一模型（即使该条目 DB 冷却已到期），L3 冷冻跳过整个渠道的所有条目。

---

#### 关键词匹配行为

关键词匹配触发的熔断层级由设置项 `keyword_freeze_scope` 控制：

| 值 | 行为 | 适用场景 |
|----|------|----------|
| `"model"`（默认） | 触发 L1 DB 冷却，仅冷却当前模型 300-1800s | 上游为中转站（CODING PLAN、SiliconFlow 等），一个模型配额耗尽不应影响同渠道其他模型 |
| `"channel"` | 触发 L3 渠道冷冻，冻结同渠道所有模型 6h | 上游为单一供应商，配额耗尽应停用整条渠道 |

设置入口：**Settings → Circuit Breaker → Keyword Freeze Scope**。

---

#### L1 — DB 冷却（模型级）

**存储**：`api_entries.cooldown_until` 字段，持久化到 SQLite，重启不丢失。

**触发条件（以下任意一项即触发）**：

| 条件 | 说明 |
|------|------|
| HTTP 5xx | 上游返回 502/503/504 等服务端错误 |
| 连接/请求超时 | reqwest 层超时，无任何响应 |
| **空流（DroppedEmpty）** | HTTP 200 OK，SSE 流已开启，但 45s 内未输出有效数据块 |
| **空响应** | HTTP 200 但非流式响应体为空，或无有效 JSON 体 |
| SSE 协议错误 | 解析失败、异常帧、非预期格式 |
| 流中断（Dropped） | 流中途断开，未收到 `[DONE]` 等完成标记 |

**空流检测机制**（新增于 v0.6.56）：

1. SSE 流首字节到达时启动定时器，定时时长 `STREAM_EMPTY_DURATION_MS = 45000ms`。
2. 定时器到期前若收到首个有效数据块（非 keepalive / ping），取消定时器，继续正常处理。
3. 定时器到期且无有效数据 → 判定为 `DroppedEmpty`，结束原因写入 `StreamEndReason`。
4. 空流在 `check_failure_and_cooldown_after_stream` 中判定为失败，触发 L1 冷却。

该机制针对上游返回 HTTP 200 后"挂着不推数据"的静默故障，防止此类场景误记成功。

**清除条件**：收到完整有效响应后，清除冷却状态并重置失败计数。

**启动恢复**：启动时自动清理已过期的 `cooldown_until`，避免冷却状态在 DB 中持续膨胀。

---

#### L2 — 内存熔断器（模型+渠道级）

**状态机**：三态熔断器（Closed → Open → HalfOpen），纯内存，重启复位。

| 状态 | 行为 |
|------|------|
| **Closed** | 正常转发请求 |
| **Open** | 跳过该模型（channel_id + model 对），不转发 |
| **HalfOpen** | Open 持续 60s 后进入，允许 1 次试探请求 |

**状态转换**：
- Closed 下连续失败 5 次 → Open
- Open 维持 60s → HalfOpen（允许 1 次试探）
- HalfOpen 试探成功 → Closed（失败计数清零）
- HalfOpen 试探失败 → Open（重置 60s 计时器）

DB 冷却和内存熔断器独立工作：L1 冷却到期的条目，如果 L2 熔断器仍处于 Open 状态，仍然被跳过。

---

#### L3 — 渠道冷冻（渠道级）

**触发**：上游响应内容匹配 `settings.disable_keywords` 中的关键词（如 `quota_exhausted`、`account suspended`、`insufficient_quota`）。

**作用**：以当前失败条目的 `channel_id` 为单位，批量写入该渠道所有 `api_entry` 的 `cooldown_until = now + 21600s`（6 小时）。

**不修改**：用户 `enabled` 开关，不做永久禁用。

**场景**：账号额度耗尽、密钥被吊销、上游通道级故障。避免逐条 failover 反复重试同一渠道的每个模型。

#### 状态码禁用（与三级冷却平行）

| 状态码 | 处理方式 |
|--------|----------|
| 401 / 403 / 410 | 单个 `api_entries.id` 永久禁用（不扩散到整条 channel），不清除 |
| 429 | 按 L1 DB 冷却处理，可恢复 |
| 502 / 503 / 504 | 按 L1 DB 冷却处理，可恢复 |

> **重要**：`enabled` 开关始终归用户控制。系统永不自动启用/禁用用户关闭的条目，冷却状态只影响路由阶段的跳过逻辑。


---

## 9. 协议适配

### 9.1 支持入口

代理入口覆盖：

- OpenAI Chat Completions
- OpenAI Responses API 兼容层
- Claude Messages
- Gemini OpenAI-compatible endpoint
- 部分 Gemini 原生 endpoint
- Azure OpenAI deployment 路由

### 9.2 协议规范表

5 种 API 类型各自独立实现 `ProtocolAdapter` trait，互不影响：

| API 类型 | 认证方式 | 聊天端点 | 模型列表端点 | 说明 |
|----------|---------|---------|-------------|------|
| `openai` | Bearer | `/v1/chat/completions` | `/v1/models` | 标准 OpenAI |
| `claude` | x-api-key | `/v1/messages` | `/v1/models` | 完整格式转换 |
| `gemini` | ?key= 查询参数 | `/v1beta/openai/chat/completions` | `/v1beta/openai/models` | Google OpenAI 兼容端点 |
| `azure` | api-key header | `/openai/deployments/{model}/chat/completions` | `/openai/models` | Deployment 名路由 |
| `custom` | Bearer | 用户 base_url 完整路径 | 用户 base_url 完整路径 | 不自动拼接 /v1；若模型列表接口不可用，可手动加入 API 池 |

### 9.3 当前内部转换模型

当前多个协议转换路径仍以 OpenAI Chat Completions 作为事实中间层。这便于快速覆盖主流客户端，但对 Responses、Claude、Gemini 的高阶语义存在压扁风险，例如：

- content blocks
- hosted tools
- 多模态输入/输出
- reasoning / thinking 字段
- 服务端状态相关字段
- 流式协议特有 frame

后续 P0 方向是在代理内部引入 API Switch 自定义中立 IR，并由 Capability Router 在路由前判断请求需求和上游能力是否匹配。

#### 公理二：输入协议 -> 中间协议，保真承载

输入协议进入中间协议时，不以目标输出协议的 schema 为约束。目标是让中间层成为最大信息承载层，尽量保留：

- 源协议标准字段
- 源协议显式扩展字段
- 暂时无法消费但后续可能需要的上下文
- 可供路由、审计、兼容补偿使用的信息

因此，`responses_to_openai_chat_request()`、`claude_to_openai_request()`、`gemini_to_openai_request()`、`azure_to_openai_request()` 这类入口转换函数的首要目标是保真归一，不做输出协议边界收口。

#### 公理三：中间协议 -> 输出协议，严格收口

中间协议输出到具体协议时，必须受目标协议边界约束。只允许输出：

- 当前协议标准字段
- 当前协议明确支持的扩展字段
- 已完成语义等价转换的字段

其余字段一律丢弃。也就是说：

- 中间协议是宽承载层
- 输出协议是严边界层

当前已经按此规则收口的输出路径包括：

- OpenAI request/response output
- Azure request/response output
- Responses request/response output
- Claude request/response output
- Gemini request/response output

这一规则只适用于“中间协议 -> 输出协议”，不适用于输入协议进入中间协议的阶段。

### 9.4 输出协议白名单模型

长期正确模型应以白名单为准，而不是继续依赖不可枚举的黑名单。每个输出协议机都应维护三类集合：

1. **标准字段白名单**：目标协议官方标准字段。
2. **显式扩展字段白名单**：项目明确允许输出的扩展字段。
3. **语义翻译表**：从中间协议到目标协议的等价字段转换规则。

统一输出流程应为：

```text
读取中间协议
-> 先做语义翻译
-> 构造目标协议输出对象
-> 仅保留标准字段白名单 + 显式扩展字段白名单
-> 丢弃其余字段
```

当前实现仍主要使用 `DROP_FIELDS` 进行过渡性止血，这有助于快速降低上游/下游 schema 错误，但它只是过渡实现，不是最终模型。

### 9.5 OpenAI-compatible 扩展透传

OpenAI-compatible 的 reasoning / thinking 扩展不做模型名特判，但也不能无限制透传。它们只能在目标协议明确支持为标准字段、显式扩展字段，或可完成语义等价转换时输出。
代理层只归一请求或响应中已经存在的 reasoning 等价字段，不缓存、不回放、不凭空生成思维链历史。

| 字段 | 归一行为 | 输出边界规则 |
|------|----------|--------------|
| `reasoning_content` | 缺少 `reasoning_text` 时补同值字段 | 仅在目标协议允许时保留 |
| `reasoning_text` | 缺少 `reasoning_content` 时补同值字段 | 仅在目标协议允许时保留 |
| `reasoning_details` | 仅字符串值可补到 `reasoning_content` / `reasoning_text` | 数组或对象保持原样，但只有目标协议允许时输出 |
| `reasoning_effort` | 可翻译到 `Responses.reasoning.effort`、`Claude.thinking` 等 | 只能翻译后输出，不可裸透传到不支持协议 |
| `thinking` | 仅在支持 thinking 的协议或显式扩展协议中输出 | 不支持协议必须删除 |

流式 SSE 的归一覆盖原样透传分支和协议适配器转换分支；非 UTF-8 chunk 不改写，按原字节继续透传。但输出到具体协议时，SSE 事件本身也应遵守该协议的白名单边界。

### 9.6 Responses API 兼容层

Responses API 兼容层覆盖个人 Hub 常用子集：

- text 输入输出
- function tools
- streaming
- Chat fallback
- `reasoning.effort` 与 Chat `reasoning_effort` 双向映射；当两者同时存在时，扁平 `reasoning_effort` 只覆盖 `reasoning.effort`，保留 `summary` 等原生字段

Responses hosted tools 在 Chat fallback 中无法等价表示时，当前止血策略是：

- function tool 保留
- 非 function hosted tool 跳过
- 注入降级提示，引导模型说明运行环境无法直接调用 hosted tool
- 避免把 hosted tool 不兼容误计为上游失败

这里必须明确区分两类注入：

- 能力降级注入：只用于 `下游 Responses -> 上游 Chat/non-Responses`，目的是告诉模型当前路由不具备 server-side hosted tools，要求其改用纯对话或显式 function tools 完成任务。
- 模型名展示注入：`forwarder.rs` 在部分非 Responses 下游末尾追加 `model: xxx` 仅用于显示命中模型，不能进入 `Responses` 原生输出；`CallerKind::Responses` 必须禁用该注入，避免污染 `response.output_text`。

为定位这条降级链路上的 high risk / 拒绝来源，系统新增 `record_raw_protocol_data` 调试设置：

- 默认关闭。
- 只在失败日志写入 `usage_logs.other.raw_protocol`，成功请求不记录，避免数据库持续膨胀。
- 记录内容按原值保留 `caller_kind`、`channel_api_type`、`gateway_body`、`upstream_body`、`error_body`、`upstream_response_body`、`x_request_id` 等关键字段，供本地复现时对照同协议透传与 Chat fallback 的真实差异。

对 Responses 输出边界，当前规则已不再允许“未知字段默认透传”。输出到 Responses 时，只允许：

- Responses 标准字段
- 项目明确允许的 Responses 扩展字段
- 可翻译后的字段（如 `messages -> input`、`response_format -> text.format`）

像 `n`、`logit_bias`、`logprobs`、`top_logprobs`、`presence_penalty`、`frequency_penalty`、`seed`、`modalities`、`audio`、`prediction`、`stream_options` 等 Chat-only 字段，不应继续出现在 Responses 输出中。

### 9.7 Claude 转换

Claude Messages 与 OpenAI Chat Completions 之间支持主链路转换，包括：

- system / user / assistant / tool 角色映射
- text content block 映射
- tool use / tool result 映射
- streaming event 映射
- usage 和 stop reason 映射

当前规则下，Claude 输出边界只允许：

- Claude 标准字段（`model`、`messages`、`system`、`max_tokens`、`stop_sequences`、`tools`、`tool_choice`、`metadata`、`thinking` 等）
- 项目明确允许的 Claude 显式扩展字段（如 `x_anthropic_future_field`）
- 通过语义等价转换得到的字段（如 `user -> metadata.user_id`）

不属于 Claude 的字段，如 `response_format`、`logit_bias`、`seed`、`stream_options`、`parallel_tool_calls`、`service_tier`、`input`、`include` 等，必须在输出边界删除。

### 9.8 Gemini 转换

Gemini 支持 OpenAI 兼容端点和部分原生 Gemini 端点。原生端点仍有补齐空间，例如 countTokens、embedding、batch、cachedContents 等。

当前规则下，Gemini 输出边界只允许：

- Gemini 原生标准字段（`contents`、`systemInstruction`、`tools`、`toolConfig`、`safetySettings`、`generationConfig`、`cachedContent`、`labels`）
- `generationConfig` 子字段白名单（如 `temperature`、`topP`、`topK`、`candidateCount`、`maxOutputTokens`、`stopSequences`、`presencePenalty`、`frequencyPenalty`、`responseMimeType`、`responseSchema`、`seed`、`responseLogprobs`、`logprobs`、`thinkingConfig`）
- 项目明确允许的 Gemini 输出扩展字段
- 已完成语义翻译的字段

`model`、`stream_options`、`logit_bias`、`top_logprobs`、`tool_choice`、`parallel_tool_calls`、`service_tier`、`metadata`、`user`、`prompt_cache_key`、`safety_identifier`、`reasoning_effort`、`thinking`、`provider_specific`、`input`、`instructions`、`include`、`prompt`、`max_output_tokens`、`text`、`truncation`、`previous_response_id`、`max_tool_calls` 等字段不应继续出现在 Gemini 输出中。

### 9.9 Azure OpenAI

Azure OpenAI 通过 deployment 名参与路由和上游请求构造。完整端到端验证依赖可用 Azure 资源。

Azure 的输出边界与 OpenAI Chat 基本一致，但有一条额外限制：

- 请求体中不应保留 `model`，因为 deployment 已经在 URL 中表达。

除此之外，仍按“OpenAI Chat 标准字段白名单 + 显式扩展字段白名单 + 语义翻译表”执行收口。

---

## 10. 流式转发

### 10.1 SSE 转发

流式响应通过 SSE 转发给客户端。代理需要处理：

- 上游流式 frame 解析
- 协议间 frame 转换
- token 增量透传
- usage-only frame
- stop reason
- 下游结束事件

### 10.2 实际命中模型名追加

流式回答正常结束时，可在末尾追加实际命中模型名，便于用户确认最终命中的上游模型。该行为可在设置中关闭。

### 10.3 流式错误分类

当前流式错误处理仍需要进一步分层。理想分类包括：

- read timeout
- decode timeout
- idle timeout
- SSE error
- buffer limit
- no valid output
- downstream client disconnected

不同错误类型应对应不同策略，例如 cooldown、short cooldown、suppress cooldown、downrank 或仅记录日志。

---

## 11. 可观测性

### 11.1 Dashboard 聚合

Dashboard 基于使用日志聚合：

- 请求量
- 成功率
- Token 消耗
- 模型分布
- 调用趋势
- 模型排行
- 用户趋势

### 11.2 使用日志

使用日志支持分页、筛选和行展开。失败时记录 attempt path，用于排查路由经过了哪些候选、在哪一步失败。

### 11.3 敏感信息脱敏

代理和日志处理需要对敏感 URL 参数脱敏，避免 API Key、token 或其他 secret 出现在日志展示中。

---

## 12. Web Admin 鉴权与状态

### 12.1 登录与 Token

Web Admin 使用 Bearer Token。前端 HTTP helper 会在请求头附带 token；遇到 401/403 时清理本地 token 并触发认证过期事件。

### 12.2 设置版本冲突

Web Admin 设置读取返回 `_version`。更新时提交当前版本，后端可用版本号识别并发修改冲突。完整闭环需要在 UI 上提示版本冲突，并提供重新加载和保留本地修改的策略。

### 12.3 Desktop-only 能力

Web Admin 与 Desktop 共用页面，但部分能力只存在于桌面环境，例如系统托盘、窗口控制、本地路径或环境变量写入。此类能力应由能力标记控制隐藏、禁用或展示替代说明。

---

## 13. 托盘机制

系统托盘只关联 AUTO 组：

- 展示代理状态
- 调整 AUTO 组条目优先级
- 显示/隐藏桌面窗口

托盘点击不切换业务分组，也不改变路由规则。未来可改为右键弹出时懒构建菜单，减少主动刷新联动点。

---

## 14. 已知技术边界

### 14.1 Chat 作为中间层的边界

以 OpenAI Chat Completions 作为事实中间层可以快速覆盖常用路径，但不适合长期承载所有协议语义。Responses、Claude、Gemini 的高级能力需要中立 IR 和 Capability Router 才能避免隐式降级。

### 14.2 前端分页批量操作边界

前端列表的批量操作如果直接使用已加载行，只能覆盖当前客户端数据集。需要“当前分组全量”语义时，应使用后端条件查询或后端批处理。

### 14.3 流式 timeout 粒度边界

全局 read timeout 是粗粒度兜底，无法区分 connect、send、first byte、idle、total 等阶段。流式与非流式应拆分配置和错误策略。

---

## 15. 数据库兼容开发规范

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

*API Switch — 绿色便携的个人 AI 网关技术白皮书。*
