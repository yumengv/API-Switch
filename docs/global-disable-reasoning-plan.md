# 全局关闭 Reasoning/Thinking 计划

## 概述

本计划用于设计一个全局请求改写开关，在下游请求进入 API Switch 后、转发到上游前，统一关闭 reasoning/thinking 相关能力，避免部分上游模型默认返回 `reasoning_content` 而非标准 `content`，导致下游兼容问题。

触发背景：NVIDIA `qwen/qwen3.5-122b-a10b` 在默认请求下返回内容位于 `choices[].message.reasoning_content`，缺少标准 `choices[].message.content`；实测在请求体中设置 `reasoning_effort: "none"` 后，上游返回标准 `content`。

本计划只定义全局策略，不做渠道级、模型级专用规则。由于项目同时支持 OpenAI-compatible / OpenAI-Responses / Claude / Gemini / Azure 五套协议，关闭 reasoning/thinking 的逻辑必须放在协议转换后的中间层统一处理：各协议机先归一成 OpenAI-compatible 上游请求体，再由公共改写点执行一次过滤与注入，避免每个协议各写一套规则。

核心判断：只要下游请求不触发思维链，上游通常不会主动进入 reasoning/thinking 输出模式。因此本功能的重点不是识别具体上游，而是在统一中间层切断下游传入的 thinking 触发字段。

## 目标

- 增加一个全局开关，控制是否关闭上游 reasoning/thinking 模式。
- 开关开启时，对所有进入代理转发链路的请求执行统一过滤/改写。
- 不按渠道、不按模型做特例判断。
- 尽量减少下游客户端差异和上游 thinking 字段泄漏造成的兼容问题。

## 非目标

- 不实现按渠道、按模型的 allowlist/denylist。
- 不实现 reasoning/thinking 的正常透传能力。
- 不缓存、不回放、不修复多轮 reasoning 历史。
- 不递归改写用户文本内容中的普通字符串。
- 不改变默认行为；开关默认关闭。

## 系统建模

### 输入

触发方：
- 客户端通过 OpenAI-compatible `/v1/chat/completions` 调用 API Switch。
- 客户端通过 OpenAI Responses `/v1/responses` 调用 API Switch。
- 客户端通过 Claude / Gemini / Azure 兼容入口调用 API Switch。

输入数据：
- 各协议入口解析后的下游请求体 JSON。
- 各协议机归一后的 OpenAI-compatible 上游请求体 JSON。
- 全局设置项，例如 `disable_reasoning`。

可能出现的 reasoning/thinking 字段：

```json
{
  "thinking": true,
  "reasoning": { "effort": "high" },
  "reasoning_content": "...",
  "reasoning_effort": "high"
}
```

### 处理

开关关闭：
- 请求保持现有行为，不主动改写 reasoning/thinking 字段。

开关开启：
- 在五套协议各自完成入口解析、格式转换之后，在公共上游请求发送前，对统一请求体顶层执行一次规则：

```text
删除 thinking
删除 reasoning
删除 reasoning_content
强制 reasoning_effort = "none"
```

等价伪代码：

```rust
if settings.disable_reasoning {
    body.remove("thinking");
    body.remove("reasoning");
    body.remove("reasoning_content");
    body["reasoning_effort"] = json!("none");
}
```

### 输出

转发给上游的请求应满足：

```json
{
  "reasoning_effort": "none"
}
```

且不再包含顶层：

```text
thinking
reasoning
reasoning_content
```

上游若支持该参数，应返回标准 `content` 字段；若不支持，可能返回 400，该风险由全局开关说明承接。

### 状态

新增或复用全局设置状态：
- 存储位置：SQLite `config` 表。
- Rust 结构：`AppSettings`。
- 前端设置页：系统设置中的代理/兼容性配置区域。
- 默认值：`false`。

状态更新路径：

```text
Settings UI
  -> apiAdapter / Tauri invoke 或 Web Admin HTTP
  -> Rust settings service
  -> SQLite config
  -> 代理转发链路读取 AppSettings
```

## 分层分解

### 目标层

让用户能够一键关闭全局 reasoning/thinking 请求能力，避免上游返回非标准 reasoning 内容导致下游空响应或兼容异常。

### 策略层

采用全局请求改写策略：
- 不区分渠道。
- 不区分模型。
- 开关默认关闭。
- 开启后只处理请求体顶层字段。
- 强制注入 `reasoning_effort: "none"`。

### 执行层

候选改动点：

| 层级 | 文件/模块 | 说明 |
|------|-----------|------|
| 设置模型 | `src-tauri/src/services/settings_service.rs` 或相关 settings 定义 | 增加 `disable_reasoning` 默认值、读写逻辑 |
| 管理接口 | `src-tauri/src/admin/*` / `src-tauri/src/commands/*` | 确保 Desktop/Web Admin 均可读写该设置 |
| 转发链路 | `src-tauri/src/proxy/forwarder.rs` | OpenAI-compatible 转发前改写请求体 |
| Responses 链路 | `src-tauri/src/proxy/responses_handler.rs` / `protocol/responses.rs` | Responses 转 Chat 后进入上游前应用同一规则 |
| 前端设置 | `src/features/*` 或设置页组件 | 增加全局开关与风险说明 |
| 类型适配 | `src/lib/apiAdapter.ts` / `src/lib/unifiedApiAdapter.ts` | 同步前端设置字段类型 |

### 校验层

必须验证：
- 开关关闭时：请求体不被改写。
- 开关开启时：顶层 `thinking`、`reasoning`、`reasoning_content` 被删除。
- 开关开启时：`reasoning_effort` 被强制设置为 `none`，覆盖下游传入的其他值。
- 流式与非流式请求都生效。
- `/v1/chat/completions` 与 `/v1/responses` 转上游路径都生效。
- NVIDIA `qwen/qwen3.5-122b-a10b` 返回标准 `content`。
- 不支持 `reasoning_effort` 的上游可能返回 400，日志应保留可诊断错误信息。

## 风险评估

| 风险 | 影响 | 缓解 |
|------|------|------|
| 部分上游不接受 `reasoning_effort` | 开关开启后请求可能 400 | 默认关闭；设置文案明确提示 |
| 下游触发字段未清理干净 | 上游仍可能进入 reasoning/thinking 模式 | 在协议转换后的公共中间层统一清理，避免五套协议各自遗漏 |
| 用户本来需要 thinking 输出 | 开关开启后能力被关闭 | 全局开关由用户主动启用 |
| Responses 协议中 `reasoning` 是官方字段 | 开关开启后会删除 reasoning 配置 | 这是本开关目标；需在文案中说明 |
| 递归删除可能误伤用户文本 | 用户内容被破坏 | 第一版只处理请求体顶层字段 |
| 多入口漏处理 | 某些协议路径仍带 reasoning 字段 | 不在各协议入口分散实现；利用各协议机最终归一到 OpenAI-compatible 请求体的架构事实，在公共上游发送前统一处理 |

## 技术决策

### 决策 1：全局开关，不做专用规则

采用用户指定方案：不按模型、不按渠道做差异处理。

理由：
- 操作简单，便于观察全局效果。
- 避免维护模型/渠道规则表。
- 用户当前目标是整体关闭 reasoning/thinking，而不是细粒度兼容。

代价：
- 兼容性风险由用户主动开启该开关承担。

### 决策 2：删除三个字段，强制设置一个字段

开关开启时：

```text
remove: thinking
remove: reasoning
remove: reasoning_content
set: reasoning_effort = "none"
```

理由：
- 删除显式开启 thinking 的字段，避免和 `reasoning_effort: none` 冲突。
- 保留 `reasoning_effort: none` 是 NVIDIA 模型实测有效的关键。

### 决策 3：处理请求体顶层与 `messages[]` 消息对象字段

理由：
- 请求体顶层是 provider 参数最常见位置。
- 历史对话中的 assistant message 也可能携带 `reasoning_content`，会触发部分上游进入 thinking 协议状态。
- 第一版只处理对象字段，不递归改写 `messages[].content` 等用户文本内容，避免破坏真实输入。

## 开发进度

| 任务 | 状态 |
|------|------|
| 明确全局策略，不做渠道/模型专用 | ✅ 已完成 |
| 编写方案计划 | ✅ 已完成 |
| 评审风险与遗漏路径 | ✅ 已完成 |
| 确认实现入口 | ✅ 已完成 |
| 实现设置项 | ✅ 已完成 |
| 实现请求改写 | ✅ 已完成 |
| 补充测试与验证 | ✅ 已完成 |

## 实施结果

- 后端新增全局设置 `disable_reasoning`，默认关闭，持久化在 SQLite `config` 表。
- 前端设置页新增“关闭思维链请求”开关，并同步中英文文案与类型定义。
- 转发链路在协议适配器归一到 OpenAI-compatible 请求体后执行统一改写。
- 开关开启时删除请求顶层和 `messages[]` 对象中的 `thinking`、`reasoning`、`reasoning_content`、`reasoning_text`、`reasoning_details`、`reasoning_effort`。
- 开关开启时强制在请求顶层写入 `reasoning_effort: "none"`。
- 开关开启时跳过请求侧 reasoning 字段归一化，避免从 `reasoning_text` / `reasoning_details` 重新生成 `reasoning_content`。
- 已补充四个测试模型的请求改写单元测试，并通过 `cargo test`、`cargo check`、`pnpm typecheck` 验证。

## 验收标准

1. 设置页存在全局开关，默认关闭。
2. 开关关闭时，现有请求行为不变。
3. 开关开启时，转发上游前请求体顶层不包含：
   - `thinking`
   - `reasoning`
   - `reasoning_content`
4. 开关开启时，转发上游前请求体顶层包含：
   - `reasoning_effort: "none"`
5. 如果下游传入 `reasoning_effort: "high"`，最终上游请求必须被覆盖为 `"none"`。
6. NVIDIA `qwen/qwen3.5-122b-a10b` 非流式请求返回标准 `content`。
7. 流式请求不再只输出 `reasoning_content` delta。
8. 不支持该参数的上游失败时，日志能看到明确上游错误，不静默吞掉。

## 待评审问题

1. 请求改写应放在 `forwarder.rs` 的公共上游请求构造点，还是每个协议入口分别处理？
2. Responses 路径中的 `reasoning` 官方字段删除后，是否需要在文案中特别标注“会关闭 Responses reasoning 配置”？
3. 是否需要同步清理响应侧 `reasoning_content`，还是只处理请求侧？当前计划只处理请求侧。
4. 是否需要把该设置放入现有系统设置结构，还是单独建立兼容性设置分组？
5. 是否需要增加日志标记，记录本次请求已执行 reasoning 关闭改写？
