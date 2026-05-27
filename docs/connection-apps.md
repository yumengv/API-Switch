# 连接应用（Connection Apps）功能文档

> 版本: v1.8 | 更新日期: 2026-05-27 | 状态: ✅ 方案定稿

---

## 一、目标

替换旧的 CliPage（连接 CLI），升级为**连接应用**。为各 AI 客户端/CLI 一键生成最小可连通配置文件，界面交互统一按连接应用 UI 规范处理。

### 旧文件清理

- `cli.json` → 删除
- `src/pages/CliPage.tsx` → 删除
- `src-tauri/src/commands/cli.rs` → 删除
- `src-tauri/src/commands/connection_apps.rs` → 新增连接应用命令模块

---

## 二、导航

- 路径: `/link`
- 名称: "连接应用"
- 图标: `Link`（lucide-react 链条图标）
- 位置: `apiPool > channels > tokens > **link** > logs > dashboard > settings`

---

## 三、UI 设计

### 3.1 卡片布局

形式与模型管理页面类似：

```
┌──────────────────────────────┐
│ [图标]  应用名称      [连接]  │
│ 一行简单描述（最多2行）       │
└──────────────────────────────┘
```

- **不可拖拽**
- 左边图标，上方应用名称，靠右触发按钮，下面描述
- 图标从 lucide-react 已有图标挑选，没有则用默认 `Link` 图标
- 押后应用（`hidden: true`）不展示
- 大小和样式参考**模型对话测试窗口（TestChatDialog）**的骨架，去掉输入框等无关元素
- 卡片网格**响应式布局**，全屏 3-4 列，窄屏自动缩减

### 3.2 Windows Write 模式交互

> 仅 Windows 桌面环境允许直接写入本机配置文件。不能直接写入的应用，或非 Windows 环境，统一走弹窗展示配置内容，由用户手动复制。

```
点击 [连接] 按钮
  → 按钮立即 disabled + 显示 loading（防重复点击）
  → 弹出确认框（模型删除弹窗样式）
    ┌───────────────────────────────────────┐
    │  连接 OpenCode                         │
    │                                        │
    │  将在以下文件写入 API-Switch 最小配置： │
    │  ~/.config/opencode/opencode.jsonc     │
    │  若文件已存在，将先备份再覆盖写入。     │
    │  将使用 API-Switch 自动生成的 AUTO access key。 │
    │                                        │
    │              [取消] [确认]              │
    └───────────────────────────────────────┘
  → 确认 → 执行（loading 持续）
  → 已有配置文件 → 重命名为 {原文件}.{yyyyMMdd-HHmmss}.bak
  → 写入成功 → toast "配置已写入: [文件路径]" + "原文件已备份为: [备份路径]"
  → 失败 → toast 错误信息
  → 完成后 → 按钮恢复可点击状态
```

### 3.3 非 Windows / 不能写入应用交互

```
点击 [连接] 按钮
  → 弹窗展示（大小参照 TestChatDialog，内容可滚动）：
    ┌──────────────────────────────┐
    │  连接 OpenCode CLI     [X]   │
    │  ─────────────────────────── │
    │                              │
    │  请手动将以下内容添加到：     │
    │  ~/.config/opencode/opencode.jsonc │
    │                              │
    │  ┌──────────────────────┐    │
    │  │ { 配置内容 }         │    │
    │  │ [复制]               │    │
    │  └──────────────────────┘    │
    │                              │
    │         [关闭]               │
    └──────────────────────────────┘
```

### 3.4 Clipboard 模式交互

```
点击 [连接] 按钮
  → 弹窗展示（React 渲染代码块 + 复制按钮）
  → 内容为具体配置模板
  → 弹窗大小参照**模型对话测试窗口（TestChatDialog）**，内容区域可滚动
```

### 3.5 HTML 渲染

- **不使用 `dangerouslySetInnerHTML`**
- 后端返回纯文本 `content`
- 前端 React 正常渲染：`<pre><code>{content}</code></pre>` + `<Button onClick={copy}>复制</Button>`
- 复制用 `navigator.clipboard.writeText()`

### 3.6 Loading 策略

- 直接显示 loading，不需要"几秒没完成才显示"
- 按钮在 loading 期间 **disabled**，不可重复点击，请求完成后恢复

### 3.7 失败处理

- 失败 → Toast 提示
- **不做应用存在检测**：配置写入和应用是否安装无关
- 只处理文件写入本身的错误（权限不足、路径不可写等）

### 3.8 备份与覆盖规则

- Windows 桌面可写入应用：目标配置文件已存在 → 备份为 `{原文件}.{yyyyMMdd-HHmmss}.bak` → 然后覆盖写入最小配置
- Windows 桌面可写入应用：目标配置文件不存在 → 直接创建并写入最小配置
- 非 Windows 环境：不直接写入文件，统一弹窗展示配置内容
- 不能直接写入的应用：不进入写入流程，统一弹窗展示配置内容
- 写入失败：不继续重试，直接向前端返回错误，由 UI toast 展示失败原因

---

## 四、具体配置模板（基于真实本机配置格式）

### 4.1 OpenCode CLI → `~/.config/opencode/opencode.jsonc`

OpenCode 使用 `provider` 对象声明自定义代理。Provider 名称使用端口号，最小配置包含 `options.baseURL`、`options.apiKey` 和 `models` 字段。

**最小可用配置（备份后直接覆盖写入）：**

```jsonc
{
  "provider": {
    "{port}": {
      "options": {
        "baseURL": "http://127.0.0.1:{port}/v1",
        "apiKey": "{access_key}"
      },
      "models": {
        "auto": {"name": "auto"}
      }
    }
  }
}
```

> - Provider 名称使用端口号（如 `9090`），由后端在运行时替换为 `{port}` 实际值
> - `options.baseURL` 必须：代理地址
> - `options.apiKey` 必须：API-Switch 自动生成的 AUTO Access Key
> - `models.auto` 必须：声明 `auto` 模型供 OpenCode 自动选择
> - `{port}` 和 `{access_key}` 由后端在运行时替换为实际值

### 4.2 Codex CLI → `~/.codex/config.toml`

Codex 使用 TOML 格式配置。最小只需 `base_url` 和 `api_key`，其余字段有默认值。

**最小可用配置（备份后直接覆盖写入）：**

```toml
model = "auto"
model_provider = "api_switch"

[model_providers.api_switch]
name = "API-Switch"
base_url = "http://127.0.0.1:{port}/v1"
api_key = "{access_key}"
```

> - `base_url` 必须：代理地址
> - `api_key` 必须：API Key
> - `name` 必须：Provider 显示名；Codex CLI 缺失时会报 `provider name must not be empty`
> - `model` / `model_provider` 可省略：可通过 CLI `--model` / `--model-provider` 覆盖
> - `{port}` 和 `{access_key}` 由后端在运行时替换为实际值

### 4.3 Claude Code → `~/.claude/settings.json`

Claude Code 使用 `settings.json` 的 `env` 字段传入环境变量。

**最小可用配置（备份后直接覆盖写入）：**

```json
{
  "env": {
    "ANTHROPIC_AUTH_TOKEN": "{access_key}",
    "ANTHROPIC_BASE_URL": "http://127.0.0.1:{port}",
    "ANTHROPIC_MODEL": "auto"
  }
}
```

> - `ANTHROPIC_AUTH_TOKEN` 必须：Bearer Token 认证（或用 `ANTHROPIC_API_KEY`）
> - `ANTHROPIC_BASE_URL` 必须：代理地址
> - `ANTHROPIC_MODEL` 必须：设为 `auto` 让 API-Switch 自动路由模型
> - `{port}` 和 `{access_key}` 由后端在运行时替换为实际值

### 4.4 Zed → `~/.config/zed/settings.json`

Zed 使用 JSON 格式配置文件，支持 OpenAI Compatible API 提供商。需要在 `language_models.openai_compatible` 中配置 API Switch，并在 `agent.default_model` 中设置默认模型。

**最小可用配置（备份后直接覆盖写入）：**

```json
{
  "agent": {
    "default_model": {
      "provider": "API Switch",
      "model": "AUTO"
    }
  },
  "language_models": {
    "openai_compatible": {
      "API Switch": {
        "api_url": "http://127.0.0.1:{port}/v1",
        "available_models": [
          {
            "name": "AUTO",
            "display_name": "AUTO",
            "max_tokens": 200000
          }
        ]
      }
    }
  }
}
```

> - `agent.default_model.provider` 必须：与 `language_models.openai_compatible` 中的提供商名称一致
> - `agent.default_model.model` 必须：模型名称，设为 `"AUTO"` 让 API-Switch 自动路由
> - `language_models.openai_compatible.API Switch.api_url` 必须：代理地址
> - `available_models` 必须：声明可用模型列表，`name` 和 `display_name` 设为 `"AUTO"`
> - `max_tokens` 可选：上下文长度，默认 200000
> - `{port}` 和 `{access_key}` 由后端在运行时替换为实际值
> - Zed 还需要设置环境变量 `API_SWITCH_API_KEY`，后端会自动写入 shell 配置文件（~/.zshrc、~/.bashrc、~/.config/fish/config.fish 或 ~/.profile）

### 4.5 Cherry Studio → Clipboard 模式（下一期实现）

> ⏳ **下一期实现**。本期不进入 `link.json`，不在 UI 展示，不实现后端执行逻辑；待补充完整配置资料后再加入。

引用模板格式：
```json
{
  "name": "API-Switch 本地代理",
  "baseUrl": "http://127.0.0.1:{port}/v1",
  "apiKey": "{access_key}",
  "provider": "OpenAI"
}
```

指引文案：打开 Cherry Studio → 设置 → 模型服务 → 添加自定义提供商，填入上述内容。

### 4.6 Gemini CLI → Clipboard 模式（下一期实现）

> ⏳ **下一期实现**。本期不进入 `link.json`，不在 UI 展示，不实现后端执行逻辑；待补充完整配置资料后再加入。

引用模板格式：
```
GEMINI_API_KEY={access_key}
GEMINI_BASE_URL=http://127.0.0.1:{port}/v1
GEMINI_MODEL=auto
```

指引文案：将以上内容添加到 `.env` 文件或 shell 配置中。

### 4.7 Workbuddy → `~/.workbuddy/models.json`

> Workbuddy 是桌面 GUI 应用，配置文件为 JSON 数组，每项代表一个自定义模型提供者。

**最小可用配置（备份后直接覆盖写入）：**

```json
[
  {
    "id": "auto",
    "name": "auto",
    "vendor": "Custom",
    "url": "http://127.0.0.1:{port}/v1",
    "apiKey": "{access_key}",
    "supportsToolCall": true,
    "supportsImages": true,
    "supportsReasoning": true,
    "useCustomProtocol": false
  }
]
```

> - Workbuddy 默认不创建此文件，首次连接时直接创建
> - `id` 必须：唯一标识符，`"auto"` 为特殊值
> - `url` 必须：代理地址
> - `apiKey` 必须：API Key
> - 其余字段可省略（`name`/`vendor`/`supports*`/`useCustomProtocol`），当前模板填写完整值以确保兼容
> - `{port}` 和 `{access_key}` 由后端在运行时替换为实际值
> - `~` 由 Rust 侧通过 `dirs::home_dir()` 展开
> - Windows 实际路径等价于 `%USERPROFILE%\.workbuddy\models.json`

---

## 五、数据源

### 5.1 link.json（本地数据源）

位于项目根目录，**不做远端拉取**。link.json **只存储卡片展示所需的内容**（名称、图标、描述、模式标记），具体执行逻辑在各自的后端接口中处理。

link.json 以 `include_str!` 编译时嵌入 Rust 二进制，前端通过 `list_connection_apps` 接口获取数据。

```typescript
interface ConnectionAppItem {
  id: string;
  name: string;
  description: string;
  icon: string;
  configMode: "write" | "clipboard";
  status?: "available" | "coming_soon";  // coming_soon 时按钮灰化，tooltip "下一版本支持"
  config: {
    file?: string;           // write 模式目标文件
    format?: "json" | "jsonc" | "toml";
    instructions?: string;   // clipboard 模式指引文案
  };
}
```

> **注意**：`link.json` 只保存卡片展示、目标文件路径和格式。实际配置内容由各自的后端接口生成。

### 5.2 扩展策略

- 只有配置格式、目标路径、使用方式已经补充完整的应用才进入 link.json
- 押后、待确认、资料不完整的应用不进 link.json
- `coming_soon` 仅用于资料已完整、但本期暂不开放执行的应用
- `coming_soon` 应用可以展示卡片，但连接按钮 disabled，tooltip 显示“下一版本支持”
- 前端不得对 `coming_soon` 应用调用 `execute_connection_app`
- 后端若收到 `coming_soon` 应用 id，返回 `unsupported_app`
- 每个应用有独立的后端处理接口，互不污染
- 无法抽取出通用规律，所以链接配置不适用远端拉取方案

---

## 六、方案二（已暂停 — 不在此版本实现）

> **状态**：已暂停讨论，写入 `docs/` 供参考。

1. 先检查本地是否有可用的 AUTO 模型
2. 没有 → 走方案一（直接写 + 弹窗告诉用户复制）
3. 有可用模型 → 弹出模型对话框，写 Prompt 让模型完成配置
4. **暂停原因**：涉及 CALL 扩展，当前阶段可行性不明确，暂停讨论

---

## 七、后端接口

### 7.1 命名规范

沿袭已有**桌面/WEB 双通道**模式：
- 桌面：Tauri `invoke` → Rust `#[tauri::command]`
- WEB：HTTP `fetch` → Web Admin 路由

### 7.2 核心功能

| 功能 | 说明 |
|------|------|
| `list_connection_apps` | 返回 link.json 全部数据 |
| `execute_connection_app(id)` | 执行连接配置 |

### 7.3 execute 核心逻辑

```
传入 app id
  → 查找 link.json 对应条目
  → 读端口（settings.listen_port，默认 9090）
  → 读 access key（固定名 AUTO，不存在则创建）
  → 展开路径（支持 ~、%APPDATA%、%LOCALAPPDATA%、%USERPROFILE% 等）
  → 将模板中的 {port} 替换为实际端口，{access_key} 替换为 AUTO access key 的实际值
  → 按 app id 进入对应的独立接口处理
  → 判断运行环境 + 平台：
    - 所有平台 + write → 文件已存在则备份 → 覆盖写入最小配置 → 返回 { action: "write", file_path, backup_path }
    - Zed + Linux/macOS → 自动设置 API_SWITCH_API_KEY 环境变量到 shell 配置文件
    - 不能直接写入的应用 → 转为 clipboard → 返回指引文案 + 内容
    - clipboard 模式 → 返回指引文案 + 内容
```

**运行环境说明**：
- 本功能不单独区分 Web Admin 文案，统一使用连接应用 UI 规则
- Windows、Linux、macOS 桌面环境均允许直接写入固定配置目录
- Windows 下使用 `%APPDATA%`、`%LOCALAPPDATA%`、`%USERPROFILE%` 等环境变量展开路径
- Linux/macOS 下使用 `~` 展开为用户主目录，支持 XDG 规范路径（如 `~/.config/`）
- Zed 应用在 Linux/macOS 下会自动设置 `API_SWITCH_API_KEY` 环境变量到 shell 配置文件
- 不能直接写入的应用不进入文件写入流程，统一展示配置内容

**端口来源**：默认 9090，但必须查询 `settings.listen_port` 取实际值，不能硬编码。

**API Key 来源**：所有应用统一使用名为 `AUTO` 的 Access Key，不存在则自动创建，取实际值替换到配置模板中。

**link.json 仅用于卡片展示**：具体执行的配置文件内容由各自的后端接口生成，互不污染。

### 7.4 各应用写入逻辑细节

**统一策略：备份 → 覆盖写入最小配置**

所有应用采用相同流程：
1. 检查目标配置文件是否存在
2. 存在 → 备份为 `{原文件}.{yyyyMMdd-HHmmss}.bak`
3. 写入最小可用配置（直接覆盖，不合并）
4. 不存在 → 直接创建

**OpenCode（jsonc）：**
- 不解析旧文件，不合并旧配置
- 若目标文件存在，先备份
- 直接覆盖写入最小 JSONC 配置（`provider.api-switch` + `model`）
- 写入内容保持合法 JSON / JSONC

**Codex（toml）：**
- 直接写入最小 TOML 配置（`[model_providers.api_switch]` + `model` / `model_provider`）

**Claude Code（json）：**
- 直接写入最小 JSON 配置（`env.ANTHROPIC_BASE_URL` + `env.ANTHROPIC_AUTH_TOKEN`）

**Zed（json）：**
- 直接写入最小 JSON 配置（`agent.default_model` + `language_models.openai_compatible`）
- Linux/macOS 下自动设置 `API_SWITCH_API_KEY` 环境变量到 shell 配置文件（~/.zshrc、~/.bashrc、~/.config/fish/config.fish 或 ~/.profile）
- Windows 下使用 `setx` 命令设置用户环境变量

**Workbuddy（json 数组）：**
- 直接写入最小 JSON 数组（单条 `auto` 条目）

### 7.5 响应结构

```typescript
interface AppConfigResult {
  action: "write" | "clipboard";
  file_path?: string;
  backup_path?: string;
  content?: string;          // clipboard 模式：配置文本
  instructions?: string;     // clipboard 模式：指引文案
}

interface AppConfigError {
  code:
    | "app_not_found"
    | "unsupported_app"
    | "key_error"
    | "path_expand_failed"
    | "backup_failed"
    | "write_failed";
  message: string;           // 面向用户展示的中文错误信息
  detail?: string;           // 可选技术细节，用于日志和排查
}
```

错误码说明：

| code | 含义 | 前端处理 |
|------|------|---------|
| `app_not_found` | link.json 中找不到对应应用 | toast 错误 |
| `unsupported_app` | 应用资料完整但本期未开放执行，或应用不能执行当前动作 | toast 错误或按钮保持 disabled |
| `key_error` | AUTO access key 获取或创建失败 | toast 错误 |
| `path_expand_failed` | `~` 无法展开为用户目录 | toast 错误 |
| `backup_failed` | 原配置文件备份失败 | toast 错误，不继续覆盖写入 |
| `write_failed` | 配置文件写入失败 | toast 错误 |

### 7.6 API Key 策略

不论什么应用，统一使用 API-Switch 自动生成的 `AUTO` Access Key。不存在则创建，存在则复用。

| 项目 | 策略 |
|------|------|
| Key 名称 | 固定 `AUTO` |
| 创建规则 | 不存在则创建，存在则复用 |
| 存储方式 | 明文写入目标应用配置文件 |
| 模板占位符 | 所有模板中的 `{access_key}` 由后端从 `access_keys` 表读取 `AUTO` Key 的实际值替换 |
| 端口来源 | 默认 `9090`，但须从 `settings.listen_port` 读取实际值，替换模板中的 `{port}` 占位符 |
| 环境变量 | 尽量不要用 ENV，除非目标应用只能通过 ENV 配置 |

说明：
- 即使当前 API-Switch 未强制校验 API Key，也统一写入 `AUTO` Key，保证目标应用配置格式稳定。
- 如果用户后续开启 API Key 校验，已写入的 `AUTO` Key 可继续作为有效凭证使用。
- 对于不支持配置 API Key 的应用，只写入其支持的最小配置字段。

---

## 八、应用清单

### ✅ 本期做（4 个）

| 应用 | 图标 | 模式 | 目标文件 |
|------|------|------|---------|
| **OpenCode CLI** | ExternalLink | write | `~/.config/opencode/opencode.jsonc` |
| **Codex CLI** | Terminal | write | `~/.codex/config.toml` |
| **Claude Code** | MessageSquare | write | `~/.claude/settings.json` |
| **Zed** | ExternalLink | write | `~/.config/zed/settings.json` |

### ⏳ 下一期（资料完整后进入 link.json，UI 展示，后端待实现）

| 应用 | 模式 | 说明 |
|------|------|------|
| **Cherry Studio** | clipboard | 配置模板已定，后端内容生成逻辑待实现 |
| **Gemini CLI** | clipboard | 环境变量模板已定，后端内容生成逻辑待实现 |

### ⏸️ 押后（不进 link.json，等配置方式确认后再加入）

| 应用 | 状态 | 原因 |
|------|------|------|
| **Cursor** | 待确认 | 配置字段路径待确认 |
| **CodeBuddy** | 待确认 | 配置方式待查文档 |
| **Bitfun** | 等待资料 | 等待补充资料 |
| **Hermes** | 等待资料 | 等待补充 |
| **Cline (Roo Code)** | 待确认 | 需确认是否有独立 CLI 版本 |

---

## 九、前端变更清单

### 新增

| 文件 | 说明 |
|------|------|
| `src/pages/LinkPage.tsx` | 卡片列表页面 |

### 修改

| 文件 | 变更 |
|------|------|
| `src/features/shell/MainShell.tsx` | 导航增加 `link` 项，MainPage 加 `'link'` |
| `src/App.tsx` | lazy import `LinkPage`，switch case 加 `"link"` |
| `src/lib/apiAdapter.ts` | ApiAdapter 增加 `connectionApps: { list(), execute(id) }` |
| `src/lib/unifiedApiAdapter.ts` | 实现 Tauri invoke + HTTP fetch 双通道 |
| `src/i18n/locales/zh.json` | 新增 `nav.link` = "连接应用" + 以下 `link.*` key |
| `src/i18n/locales/en.json` | 新增 `nav.link` = "Connect Apps" + 以下 `link.*` key |

完整 i18n key 清单（zh / en）：

| Key | 中文 | English |
|-----|------|---------|
| `nav.link` | 连接应用 | Connect Apps |
| `link.title` | 连接应用 | Connect Apps |
| `link.connect` | 连接 | Connect |
| `link.confirm.title` | 连接 {appName} | Connect {appName} |
| `link.confirm.description` | 将在以下文件写入 API-Switch 最小配置：{filePath}。若文件已存在，将先备份再覆盖写入。是否继续？ | Write API-Switch minimal config to: {filePath}. Existing file will be backed up before overwrite. Continue? |
| `link.success` | 配置已写入: {filePath} | Config written: {filePath} |
| `link.backup` | 原文件已备份为: {backupPath} | Original backed up: {backupPath} |
| `link.clipboard.title` | 连接 {appName} | Connect {appName} |
| `link.clipboard.copy` | 复制 | Copy |
| `link.clipboard.copied` | 已复制 | Copied |
| `link.error.app_not_found` | 未找到连接应用 | Connection app not found |
| `link.error.unsupported_app` | 该应用暂不支持连接 | This app is not supported yet |
| `link.error.key_error` | 获取 API Key 失败 | Failed to get API Key |
| `link.error.path_expand_failed` | 获取用户目录失败 | Failed to resolve user directory |
| `link.error.backup_failed` | 备份原配置失败: {reason} | Backup failed: {reason} |
| `link.error.write_failed` | 配置写入失败: {reason} | Write failed: {reason} |

### 删除

| 文件 | 说明 |
|------|------|
| `src/pages/CliPage.tsx` | 旧 CLI 配置页面 |

---

## 十、后端变更清单

### 新增

| 文件 | 说明 |
|------|------|
| `src-tauri/src/commands/connection_apps.rs` | Tauri 连接应用命令模块 |
| `src-tauri/src/admin/connection_apps_handlers.rs` | HTTP handler |

### 修改

| 文件 | 变更 |
|------|------|
| `src-tauri/src/commands/mod.rs` | `mod connection_apps;` 替换 `mod cli;` |
| `src-tauri/src/admin/router.rs` | 增加连接应用路由 |

### 数据

| 文件 | 变更 |
|------|------|
| `link.json` | 移除 Cursor/CodeBuddy 条目（押后不进 link.json）；修正 Codex 目标文件 `~/.codex/config.json` → `~/.codex/config.toml`；修正 Claude Code 路径 `.claude/settings.json` → `~/.claude/settings.json`；新增 Workbuddy 条目 |
| `cli.json` | 删除 |

---

## 十一、确认弹窗规范

- 所有平台可写入应用：点击连接后显示确认弹窗
- 不能直接写入的应用：不显示覆盖写入确认，直接显示配置内容弹窗
- 标题："连接 [应用名]"
- 正文格式：显示 "将在以下文件写入 API-Switch 最小配置：" + 文件路径
- 正文必须提示："若文件已存在，将先备份再覆盖写入。"
- 正文必须提示："将使用 API-Switch 自动生成的 AUTO access key。"
- 底部按钮：[取消] [确认]，按钮在请求期间 disabled
- 风格参考模型删除弹窗

---

## 十二、不做范围（明确标记）

| 项目 | 状态 |
|------|------|
| 方案二（模型写配置） | 已暂停讨论 |
| 远端拉取 link.json | 不做，固定在应用中 |
| Cursor / CodeBuddy / Bitfun / Hermes / Cline | 不进 link.json，等配置方式确认后补 |
| Cherry Studio / Gemini CLI 后端实现 | 下一期 |
| 应用存在检测 | 不做 |
| 旧 CliPage / setx env 模式 | 已删除 |

---

## 十三、验收标准

> 本功能是否完成，以用户实际测试通过为准。自动化验证、构建通过、代码检查通过只能作为提交前基础条件，不能替代用户验收。

### 13.1 基础验收

- 导航出现“连接应用”，位置符合本文档导航顺序。
- 页面展示本期支持的 4 个应用：OpenCode CLI、Codex CLI、Claude Code、Zed。
- 资料不完整的应用不进入 `link.json`，不在页面展示。
- `coming_soon` 应用如进入 `link.json`，按钮必须 disabled，并提示“下一版本支持”。
- 点击连接按钮后，按钮立即 disabled 并显示 loading，请求结束后恢复。

### 13.2 跨平台验收

- 所有 4 个应用（OpenCode CLI、Codex CLI、Claude Code、Zed）在 Windows、Linux、macOS 下均支持自动写入配置文件。
- Windows 下使用 `%APPDATA%`、`%LOCALAPPDATA%`、`%USERPROFILE%` 等环境变量展开路径。
- Linux/macOS 下使用 `~` 展开为用户主目录，支持 XDG 规范路径（如 `~/.config/`）。
- Zed 应用在 Linux/macOS 下会自动设置 `API_SWITCH_API_KEY` 环境变量到 shell 配置文件（~/.zshrc、~/.bashrc、~/.config/fish/config.fish 或 ~/.profile）。
- Zed 应用在 Windows 下会使用 `setx` 命令设置用户环境变量 `API_SWITCH_API_KEY`。
- 已存在配置文件时，先生成 `.bak` 备份，再覆盖写入最小配置。
- 写入成功后 toast 显示目标文件路径；有备份时同时显示备份文件路径。
- 写入失败时 toast 显示失败原因，不静默失败。
- 配置中的端口来自 `settings.listen_port`，不能硬编码 9090。
- 配置中的 Key 使用 API-Switch 自动生成或复用的 `AUTO` Access Key。

### 13.3 安全与渲染验收

- 前端不使用 `dangerouslySetInnerHTML`。
- 配置内容使用 React 文本渲染：`<pre><code>{content}</code></pre>`。
- 所有失败路径必须向用户反馈错误信息。
- 后端返回 `unsupported_app` / `app_not_found` / `key_error` / `path_expand_failed` / `backup_failed` / `write_failed` 时，前端必须展示明确错误提示。
