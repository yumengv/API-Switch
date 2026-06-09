# API Switch

> 本地优先的个人 AI API 管理与转发中心<br>
> 多渠道路由 · 模型分组 · 自动故障转移 · 批量测速 · 桌面与无头部署

API Switch 用一个本地入口统一管理多个 AI API 渠道。客户端只需要连接 API Switch，后续由它负责模型匹配、分组路由、故障转移、冷却恢复、日志记录和 Web/Desktop 管理。

---

## 核心功能

| 功能 | 说明 |
|------|------|
| 多渠道路由 | 一个入口接入 OpenAI、Claude、Gemini、Azure OpenAI 和 OpenAI 兼容中转站 |
| 模型分组 | 将多个上游模型组合成一个对外模型 ID，例如客户端请求 `code` 时自动路由到 code 分组 |
| 自动故障转移 | 上游失败后自动跳过冷却条目，并尝试下一个可用渠道或模型 |
| 批量测速 | 渠道模型支持并发批量测速，API 管理页支持一键测速并记录响应时间 |
| 智能模型预选 | 拉取模型后可按发布时间自动筛选近期模型，并保留已有模型 |
| 渠道自动校对 | 自动检测 API 类型、Base URL 可用性和模型列表端点 |
| 熔断与冷却 | 失败模型自动冷却；401/403/410 等不可恢复错误可自动禁用 |
| Web Admin / 无头模式 | 无桌面环境可通过 Web 管理端使用，适合 NAS、服务器和远程主机 |
| 系统托盘 | 桌面端可在托盘查看状态、显示窗口和调整 AUTO 组优先级 |
| 使用日志与看板 | 记录请求、失败路径、耗时、Token 统计，并提供数据看板 |
| 绿色便携 | Windows 可使用单文件 EXE，数据默认存放在程序同目录 |

---

## 快速开始

1. 从 [Releases](https://github.com/yumengv/API-Switch/releases) 下载对应平台版本。
2. 运行程序，首次启动会自动创建数据库。
3. 进入「渠道管理」添加 API 渠道，填写 Base URL 和 API Key。
4. 点击「获取模型列表」，按需要筛选、测速并勾选模型。
5. 进入「模型管理」启用模型、拖拽排序、测试对话或一键测速。
6. 如需对外暴露一个分组模型名，进入「模型分组」创建分组并选择模型。
7. 将客户端 API 地址指向本地代理端口。

客户端配置示例：

```text
API Base URL: http://127.0.0.1:9090/v1
API Key: 任意值；如果开启强制访问密钥校验，则填写 Access Key
Model: auto、模型分组名，或具体模型名
```

Claude / Anthropic 协议客户端通常使用根地址：

```text
API Base URL: http://127.0.0.1:9090
```

---

## 路由规则

| 请求模型 | 行为 |
|----------|------|
| `auto` 或空模型名 | 从 AUTO 组中选择已启用、未冷却、渠道可用的条目 |
| 分组名，如 `code` | 优先按分组名精确匹配，匹配成功后在该分组内按排序选择 |
| 具体模型名，如 `gpt-4o` | 先尝试模型名匹配；无可用条目时 fallback 到 AUTO 组 |
| 禁用分组 | 不作为分组名参与路由，也不会作为分组模型出现在 `/v1/models` |
| 托盘菜单 | 只调整 AUTO 组前列模型优先级，不切换业务分组 |

同一候选集合内默认按用户排序尝试。模型分组页显示的排序数字由使用者设置，数字越大越靠前；保存时会同步到内部 `sort_index`。模型管理页拖拽排序和模型分组页的排序数字使用同一套顺序数据。

---

## 模型分组

模型分组用于把多个上游模型包装成一个对外模型名。典型场景：

```text
客户端请求 model = code
API Switch 在 code 分组中选择优先级最高的可用模型
如果某个模型失败或冷却，自动尝试分组内下一个模型
```

使用方式：

1. 进入「模型分组」。
2. 新建分组，例如 `code`、`fast`、`vision`。
3. 点击「选择模型」，勾选要加入该分组的模型。
4. 修改排序数字；数字越大越靠前。
5. 保存后，客户端即可把 `model` 设置为该分组名。

补充规则：

- `auto` 是系统分组，不能删除，始终启用。
- 同一个模型可以同时加入多个普通分组。
- 删除普通分组时，组内模型会移回 `auto`。
- 旧版本中已经存在但未配置的 `group_name` 会继续兼容并默认启用。
- 在「模型管理」中拖拽排序后，「模型分组」中的数字会同步更新。

---

## 渠道模型测速

渠道编辑弹窗中的「模型测速」用于在保存前验证当前筛选范围内的模型可用性。

- 不再限制模型数量。
- 批量测速采用有限并发执行，界面会显示完成进度。
- 成功模型显示绿色耗时，失败模型显示错误标记和原因。
- 测速会消耗上游请求次数和少量额度，大批量测速前建议先用搜索或时间范围缩小列表。

---

## 支持的 API 类型

| 类型 | 认证方式 | 说明 |
|------|----------|------|
| OpenAI | Bearer Token | 标准 OpenAI Chat Completions |
| OpenAI Responses | Bearer Token | OpenAI Responses API 兼容入口 |
| Anthropic / Claude | `x-api-key` | Claude Messages 协议，自动转换 |
| Google Gemini | Query Parameter | Gemini OpenAI 兼容端点和部分原生端点 |
| Azure OpenAI | `api-key` Header | 使用 Deployment 名称路由 |
| Custom | Bearer Token | OpenAI 兼容第三方服务或中转站 |

中转站通常不提供稳定的 `/models` 接口。如果拉取失败，可以在「模型管理」中手动添加模型名称。

---

## Web Admin 与无头模式

无头模式适合服务器、NAS 或没有桌面环境的机器：

```bash
./api-switch --headless
```

也可以使用环境变量：

```bash
API_SWITCH_HEADLESS=1 ./api-switch
```

默认地址：

| 服务 | 地址 |
|------|------|
| 代理 API | `http://127.0.0.1:9090/v1` |
| Web Admin | `http://127.0.0.1:9090/admin` |

默认 Web Admin 账号密码为 `admin / admin`。可在「系统设置 → Web 管理」中修改，也可通过环境变量 `API_SWITCH_ADMIN_USER` 和 `API_SWITCH_ADMIN_PASS` 覆盖。

---

## 下载与构建

| 平台 | 文件名示例 |
|------|------------|
| Windows x64 | `api-switch-*-windows-x64.exe` |
| macOS Intel | `api-switch-*-macos-x64` |
| macOS Apple Silicon | `api-switch-*-macos-arm64` |
| Linux x64 | `api-switch-*-linux-x64` |

源码构建：

```powershell
pnpm install
pnpm build
```

开发模式：

```powershell
pnpm dev
```

---

## 常用端点

| 端点 | 方法 | 说明 |
|------|------|------|
| `/v1/chat/completions` | POST | OpenAI Chat Completions |
| `/v1/responses` | POST | OpenAI Responses |
| `/v1/messages` | POST | Anthropic Messages |
| `/v1/models` | GET | OpenAI 格式模型列表，包含可用分组模型 |
| `/anthropic/v1/models` | GET | Anthropic 格式模型列表 |
| `/v1beta/models` | GET | Gemini 格式模型列表 |
| `/openai/deployments` | GET | Azure Deployment 列表 |
| `/health` | GET | 健康检查 |

---

## 数据与文件

便携版默认结构：

```text
api-switch.exe          # 主程序
api-switch.db           # SQLite 数据库，首次运行自动创建
```

所有主要配置、渠道、模型、日志和访问密钥都保存在本地 SQLite 数据库中。删除程序和数据库文件即可卸载。

---

## 文档

- [使用指南](GUIDE.md)
- [中文使用指南](GUIDE_CN.md)
- [技术白皮书](WHITEPAPER.md)

---

## 许可证

[MIT License](LICENSE)

---

如果这个项目对你有帮助，欢迎在 [GitHub](https://github.com/yumengv/API-Switch) 点 Star 或提交 Issue。
