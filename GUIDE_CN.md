# API Switch 使用指南

本文档面向日常使用者，说明如何添加渠道、启用模型、配置模型分组、接入客户端以及排查常见问题。

---

## 快速开始

### 1. 添加渠道

进入「渠道管理」，点击「添加渠道」，填写：

| 配置项 | 说明 |
|--------|------|
| 名称 | 自定义名称，方便区分不同 Key 或不同服务商 |
| API 类型 | 选择上游服务协议类型 |
| Base URL | 上游 API 地址，通常不需要带 `/v1` |
| API Key | 服务商或中转站提供的密钥 |
| 备注 | 可选，用于记录用途、账号或额度信息 |

填写后点击「获取模型列表」。如果上游支持模型列表接口，API Switch 会拉取模型并展示可选列表；如果中转站不支持拉取模型，可以保存渠道后在「模型管理」中手动添加模型。

### 2. 筛选、测速并选择模型

渠道编辑弹窗中的模型列表支持：

- 按发布时间筛选最近 3 / 6 / 12 个月模型。
- 搜索模型名。
- 勾选需要同步到模型池的模型。
- 对当前筛选范围内的模型进行批量测速。

模型测速不再限制数量，采用有限并发批量执行，并显示 `已完成/总数` 进度。测速会真实请求上游模型，可能消耗请求次数或少量额度；大批量测速前建议先用搜索或时间范围缩小列表。

### 3. 启用模型

保存渠道后进入「模型管理」：

- 绿点表示条目启用并可参与路由。
- 红点表示条目处于冷却中，冷却期间不会被路由命中。
- 灰色或关闭状态表示用户禁用，该条目不会参与路由。
- 可拖拽调整同一分组内的默认尝试顺序。

### 4. 接入客户端

OpenAI 兼容客户端的 API Base URL 设置为：

```text
http://127.0.0.1:9090/v1
```

Claude / Anthropic 协议客户端通常使用根地址：

```text
http://127.0.0.1:9090
```

如果没有开启「强制验证访问密钥」，API Key 可留空或填写任意值；开启后必须填写 API Switch 中创建的 Access Key。

---

## 模型分组

模型分组用于把多个上游模型包装成一个对外模型名。例如创建 `code` 分组后，客户端可以请求：

```text
model = code
```

API Switch 会在 `code` 分组内按排序选择可用模型；如果第一个模型失败或冷却，会自动尝试分组内下一个模型。

### 创建分组

1. 进入「模型分组」。
2. 点击「新建分组」，填写分组名和说明。
3. 点击该分组的「选择模型」。
4. 勾选要加入分组的模型。
5. 修改排序数字并保存。

页面中显示的排序数字由使用者设置，数字越大越靠前。保存后，模型管理页拖拽排序和模型分组页的排序数字会使用同一套顺序数据；在模型管理页调整上下顺序后，模型分组页会同步显示新的排序。

### 系统分组和禁用规则

- `auto` 是系统分组，不能删除，始终启用。
- 普通分组可以启用或禁用。
- 同一个模型可以同时加入多个普通分组，保存某个分组不会把模型从其他分组移走。
- 禁用分组不会作为分组名参与路由，也不会作为分组模型出现在 `/v1/models`。
- 删除普通分组时，组内模型会自动移回 `auto`。
- 旧版本已经存在但没有配置项的 `group_name` 会自动兼容，并默认启用。

---

## 路由规则

客户端请求中的 `model` 字段可以填写 `auto`、分组名或具体模型名。

| 请求模型 | 行为 |
|----------|------|
| 空模型名 | 按 `auto` 处理 |
| `auto` | 在 AUTO 组中选择已启用、未冷却、渠道可用的条目 |
| 分组名，如 `code` | 优先按分组名精确匹配，命中后只在该分组内按排序尝试 |
| 具体模型名，如 `gpt-4o` | 先按模型名或别名匹配；无可用条目时 fallback 到 AUTO 组 |
| 禁用分组名 | 不参与分组匹配，继续进入模型匹配和 AUTO fallback |

同一候选集合内，默认按用户排序尝试。排序策略还可切换为最快优先或最新优先；同一模型配置了多个渠道时，失败后会继续尝试下一个候选。

托盘菜单只调整 AUTO 组前列模型优先级，不切换业务分组。

---

## 支持的上游 API 类型

| API 类型 | 说明 | 默认或常见 Base URL |
|----------|------|---------------------|
| OpenAI | 标准 OpenAI Chat Completions | `https://api.openai.com` |
| OpenAI Responses | OpenAI Responses API 兼容入口 | `https://api.openai.com` |
| Anthropic / Claude | Claude Messages 协议，自动转换 | `https://api.anthropic.com` |
| Google Gemini | Gemini OpenAI 兼容端点和部分原生端点 | `https://generativelanguage.googleapis.com` |
| Azure OpenAI | 使用 Deployment 名称路由 | Azure Endpoint |
| OpenAI-compatible | 兼容 OpenAI 协议的第三方服务或中转站 | 按服务商说明填写 |

Base URL 通常填写站点根地址，不需要手动追加 `/v1`。API Switch 会在探测时自动尝试常见变体，并校准可用端点。

---

## Web Admin 与无头模式

无头模式适合服务器、NAS、远程主机或没有桌面环境的机器。

Windows：

```powershell
.\api-switch.exe --headless
```

Linux / macOS：

```bash
./api-switch --headless
```

也可以使用环境变量：

```bash
API_SWITCH_HEADLESS=1 ./api-switch
```

启动后默认地址：

| 服务 | 地址 |
|------|------|
| 代理 API | `http://127.0.0.1:9090/v1` |
| Web Admin | `http://127.0.0.1:9090/admin` |

默认 Web Admin 账号密码为 `admin / admin`。可在「系统设置 → Web 管理」中修改，也可在启动前设置 `API_SWITCH_ADMIN_USER` 和 `API_SWITCH_ADMIN_PASS` 覆盖。

---

## 下游接入端点

客户端通过代理端口接入，默认端口为 `9090`。

| 端点 | 方法 | 说明 |
|------|------|------|
| `/v1/chat/completions` | POST | OpenAI Chat Completions |
| `/v1/responses` | POST | OpenAI Responses |
| `/v1/messages` | POST | Anthropic Messages |
| `/v1/models` | GET | OpenAI 格式模型列表，包含启用的分组模型 |
| `/anthropic/v1/models` | GET | Anthropic 格式模型列表 |
| `/v1beta/models` | GET | Gemini 格式模型列表 |
| `/openai/deployments` | GET | Azure Deployment 列表 |
| `/v1beta/models/{model}:generateContent` | POST | Gemini 原生格式 |
| `/openai/deployments/{deployment}/chat/completions` | POST | Azure 原生格式 |
| `/health` | GET | 健康检查 |

---

## 推荐配置示例

### MiniMax

| 配置项 | 值 |
|--------|-----|
| API 类型 | `openai` 或 `anthropic` |
| Base URL | `https://api.minimaxi.com` 或 `https://api.minimax.chat` |
| API Key | 你的 Key |
| 模型管理添加模型 | 按上游实际模型名填写 |

### CODING PLAN / 兼容中转站

| 配置项 | 值 |
|--------|-----|
| API 类型 | `openai` |
| Base URL | 按服务商文档填写 |
| API Key | 你的 Key |
| 获取模型 | 如果不支持模型列表接口，保存后手动添加 |
| 模型管理添加模型 | 例如 `gemini-2.0-flash`、`gemini-2.5-pro` 等 |

此类中转站的模型列表接口不一定可用，拉取失败不代表聊天接口不可用。可以在渠道中保存 Key 后，到「模型管理」点击「添加 API」手动填写模型名称。

---

## 常见问题

### 代理启动失败：端口被占用

进入「系统设置 → 代理设置」，将监听端口改为其他端口，然后重启代理或程序。

### 请求返回 401

1. 如果开启了「强制验证访问密钥」，请求必须携带：

   ```text
   Authorization: Bearer sk-xxx
   ```

2. 如果没有开启访问密钥校验，检查上游渠道的 API Key 是否正确。

### 请求返回 "No available provider"

按顺序检查：

1. 渠道是否启用。
2. 模型条目是否启用。
3. 模型是否处于冷却中。
4. 请求的分组是否已启用。
5. 请求的模型名是否能匹配到模型、别名或分组。
6. AUTO 组中是否还有可用条目作为 fallback。

### 模型显示红点或冷却中

模型请求失败后会自动冷却，冷却期间不参与路由。默认冷却时间可在「系统设置 → 熔断机制」中调整，冷却到期后会自动恢复。

### 拉取模型失败

1. 检查 Base URL 是否正确，通常不需要带 `/v1`。
2. 检查 API Key 是否有效。
3. 检查网络是否能访问上游 API。
4. 如果中转站不支持模型列表接口，改为手动添加模型。

### 分组模型没有出现在 `/v1/models`

确认该分组已启用，并且分组中至少有模型条目。禁用分组不会出现在模型列表中。

### 分组排序和模型管理页顺序不一致

刷新页面后再检查。模型管理页拖拽排序和模型分组页排序数字共用同一套数据；模型分组页数字越大越靠前。

### 托盘菜单模型顺序不对

托盘只显示 AUTO 组中靠前的启用模型。进入「模型管理」，切到 `auto` 分组并拖拽调整顺序。

### 日志中看到 `(auto)` 前缀

表示客户端请求的 `model` 是 `auto`，括号后是实际命中的模型名称。

---

> 本文档会持续更新，如果遇到其他问题欢迎提 [Issue](https://github.com/yumengv/API-Switch/issues)。
