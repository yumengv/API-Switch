# 代理层架构整改计划（完备交接版）

> **下次会话接手必读**：本文档是路线图 + 状态快照。按"第三节：当前状态"对齐进度，
> 按"第六节：剩余工作清单"往下执行。所有决策已锁死在第二节，不要再重新讨论原则。

---

## 一、两条公理（不可违反）

**公理一**：api-switch 是**中转和翻译器**，不是内容修改者。

**公理二**：协议以**各自官方文档**为准，这边进来什么，那边出去必须是一样的（往返无损）。

---

## 二、9 条执行决策（已对齐，不再讨论）

1. 中间格式用 OpenAI chat.completions，不换
2. OpenAI 是翻译路径，不是规范仲裁者；每个协议以自己官方文档为准
3. 当前 10 个翻译器合并成 5 个协议模块
4. 每个协议文件遵循统一范式（见第四节）
5. 每协议顶部一个 `ENABLE_UNKNOWN_FIELD_PASSTHROUGH` 源码常量，不做 UI 配置
6. 合并基底选择：
   - `OpenAI`：保持现状（基准协议，无需翻译）
   - `Claude`：以上游 `claude.rs` 为基底（更完整），下游方向补齐
   - `Gemini`：以下游 `gemini_output.rs` 为基底，保持 OpenAI 兼容端点方案
   - `Azure`：清理死码，合并成薄文件
   - `Responses`：以下游 `responses_handler.rs` 翻译部分为基底，**本轮补齐上游方向**，Beta 标记
   - `Custom`：保持现状
7. Responses 上游方向在阶段 3 一次做完（✅ 已完成于 3a3caf6）
8. P1（Claude SSE usage=0）在阶段 3 合并重写时修掉（✅ 已完成于 7e450ce）
9. 不做：同协议直连、换中间协议、Gemini native、引入外部框架、流式强类型 IR（推迟到未来独立项目）、给用户 UI 配置 passthrough 开关

---

## 三、当前状态（2026-05-10，14 个 commit 后）

### 分支

`fix/claude-responses-proxy-issues`，从 `master` 切出。

### Commit 序列（从旧到新）

| # | Hash | 阶段 | 描述 |
|---|---|---|---|
| 1 | `bc5c5f7` | 0 | Claude 协议 round-trip 测试基线（11 测试：8 绿 + 3 红） |
| 2 | `d3f0300` | 0 | Gemini/Azure/OpenAI/Custom round-trip 测试（+8 测试，累计 14 绿 / 5 红） |
| 3 | `3cfd1e8` | 0 | Responses 占位测试（+4 ignored） |
| 4 | `884f738` | 2 | Claude 翻译字段穿透（双向） |
| 5 | `3980efa` | 2 | Gemini 翻译字段穿透（双向） |
| 6 | `0771ec1` | 1 | 提取 `proxy::sse` 公共模块，修复 P3（UTF-8 切字） |
| 7 | `856ed6b` | 0.fix | 测试兼容 Claude system 字段两种形态 |
| 8 | `de62e99` | 3.1a | P1 初版修复尝试 |
| 9 | `7e450ce` | 3.1b | P1 正式修复 + 清理 541 行死代码 |
| 10 | `907ea79` | docs | REFACTOR_PLAN.md 首次详化 |
| 11 | `b33b3a5` | docs | REFACTOR_PLAN.md 完备版 |
| 12 | `d455211` | chore | 清理 Phase 2 遗留的 stale imports |
| 13 | `6577844` | 3.2 | Responses 前端：`ApiType` 加 `"responses"` + UI 选项 |
| 14 | `3a3caf6` | 3.2 | Responses 上游 adapter Rust 实现 + 启用 4 占位测试 |

### 测试状态

```bash
cargo test --lib
→ 231 passed / 0 failed / 0 ignored
```

**关键里程碑**：所有占位测试全绿，P1 已修，核心字段穿透完备。

### Working tree 状态

```
Untracked files:
  ANALYSIS_CLAUDE_TO_RESPONSES.md    # 早期分析文档，可留可删
  FLOW.md                            # 架构流程图，保留
  docs/                              # 用户文档目录（与重构无关）
  src-tauri/target-codex-testrlicDz/ # 其他 agent 的构建产物，忽略
  src-tauri/target-codexu64YjE/      # 其他 agent 的构建产物，忽略
```

**没有未提交代码改动**。阶段 3.2 彻底收口。

### 6 个原问题状态

| 问题 | 状态 | 修复方式 | commit |
|---|---|---|---|
| P1 Claude 流式 output_tokens=0 | ✅ 已修 | ClaudeSSETransformer：usage 捕获前置；usage-only 帧补发 message_delta | `7e450ce` |
| P2 stream_options 无条件覆盖 | ⏳ 待修 | 阶段 4：`insert` 改 `entry().or_insert_with()` 合并 | — |
| P3 UTF-8 切字 → � | ✅ 已修 | 抽出 `proxy::sse::append_utf8_safe`，3 处 `from_utf8_lossy` 改用 | `0771ec1` |
| P4 流式 buffer 无上限 | 🟡 部分 | `responses_handler` 原生有 10MB 上限，其他在阶段 4 合并时统一 | — |
| P5 model:xxx 污染 Responses | ⏳ 待修 | 阶段 4：`ModelAnnotationMiddleware` 中间件化，Responses 入口不装配 | — |
| P6 第二层流无 idle timeout | ⏳ 待修 | 阶段 4：`IdleTimeoutMiddleware` | — |

### 已解决的架构违规（公理二层面）

- ✅ Claude 请求方向未知字段穿透（`884f738`）
- ✅ Claude 响应方向从 whitelist 构造改为 clone+edit-in-place（`884f738`）
- ✅ Gemini 请求方向未知字段穿透（`3980efa`）
- ✅ Gemini 响应方向从 whitelist 构造改为 clone+edit-in-place（`3980efa`）
- ✅ Responses 上游 adapter 双向带 `ENABLE_UNKNOWN_FIELD_PASSTHROUGH` 常量（`3a3caf6`）

### 尚未收口的架构一致性项

- ⏳ Azure 没加 `ENABLE_UNKNOWN_FIELD_PASSTHROUGH` 常量（因为 Azure = OpenAI，翻译是直通的，没字段需要穿透；阶段 3.3 合并时加个常量作为一致性标记）
- ⏳ 协议文件合并（10→5）—— 阶段 3.3 的目标，不是 bug 只是整洁度问题
- ⏳ Responses SSE 流式翻译（目前 adapter 的 `needs_sse_transform=false`，SSE 直通；这是有意的简化，后续版本补齐 SSE 双向翻译）

---

## 四、协议模块统一范式

**一个协议 = 一个文件 = 一套标准套件**。所有 5 个现有协议和未来新增协议都这样。

```rust
// protocol/xxx.rs

// 1. 穿透开关（源码常量，不做 UI 配置）
/// 是否在翻译时穿透本协议官方文档未定义的字段。
/// 默认 true：贯彻"中转不丢失"公理。
/// 应急 false：仅传官方已知字段。如果某上游开始对未知字段返回 400，
/// 定位到是穿透行为导致后改此常量为 false，发布新版本。
const ENABLE_UNKNOWN_FIELD_PASSTHROUGH: bool = true;

// 2. 实现 ProtocolAdapter trait
pub struct XxxProtocol;
impl ProtocolAdapter for XxxProtocol {
    // URL / 鉴权 / 模型列表
    fn build_chat_url(&self, base: &str, model: &str) -> String;
    fn apply_auth(...);
    fn build_models_url(...);
    fn parse_models_response(...);

    // 上游方向：OpenAI 中间格式 → 本协议
    fn transform_request(&self, body: &mut Value, model: &str);
    fn transform_response(&self, body: &mut Value);
    fn transform_sse_line(&self, line: &str) -> Option<String>;
    fn needs_sse_transform(&self) -> bool;
    fn extract_sse_usage(&self, line: &str) -> (i64, i64);
}

// 3. 下游方向翻译（供入口 handler 使用）
pub fn xxx_to_openai_request(xxx: &Value) -> Value;
pub fn openai_to_xxx_response(openai: &Value) -> Value;
pub struct XxxSSETransformer { ... }  // 如果需要流式

// 4. 单元测试：round-trip / 未知字段穿透 / 官方文档样本
```

**加第 6 个协议的代价**：复制已有文件、填翻译逻辑、在 `mod.rs` 的 `get_adapter` 加一行、前端 `ApiType` 加选项。**核心不动**。

---

## 五、已完成部分的技术细节

### 阶段 0：测试基线（commits 1-3, 7）

**新增文件**：
- `src-tauri/src/proxy/protocol/roundtrip_tests.rs`（~779 行，23 个测试）
- `REFACTOR_PLAN.md`

**修改文件**：
- `src-tauri/src/proxy/protocol/mod.rs`：加 `#[cfg(test)] mod roundtrip_tests;` 并更新既有 `claude_transform_request_basic` 测试兼容 system 字段 array 形态

**测试组织**：
```
mod claude_roundtrip       11 tests
mod gemini_roundtrip        4 tests
mod azure_roundtrip         2 tests
mod openai_roundtrip        2 tests
mod responses_roundtrip     4 tests（阶段 3.2 后已 un-ignore）
mod helpers                 1 test
```

**测试设计哲学**：
- **round-trip**：`A → openai → A'`，断言 `A ≡ A'`
- **未知字段穿透**：输入带 `x_api_switch_tracking_id` 之类非官方字段，输出必须还在
- **usage 时序**：Claude 流式帧序列精确模拟 OpenAI 真实帧（role / content / finish / usage-only / [DONE]），断言 `message_delta.usage.output_tokens` 能拿到真实值

### 阶段 1：`proxy::sse` 公共模块（commit `0771ec1`）

**新增文件**：`src-tauri/src/proxy/sse.rs`（175 行，8 个单元测试）

**导出**：
- `append_utf8_safe(buffer, remainder, bytes)`：跨 chunk UTF-8 安全拼接
- `sse_data_payload(line)`：解析 SSE `data:` 行

**UTF-8 安全拼接原理**：
1. 维护 `utf8_remainder: Vec<u8>` 保存"上次留下的不完整 UTF-8 字节"
2. 新 chunk 来了先 extend 到 remainder 尾巴
3. `std::str::from_utf8(&remainder)` 尝试整体解析
4. 成功：全部推入 buffer，remainder 清空
5. 失败：按 `valid_up_to()` 切——合法前缀推入 buffer（drain），不完整后缀留存等下一轮

**修改点**：
- `handlers.rs::handle_messages` 的流式 `unfold` 循环：
  - 原来：`sse_buffer.push_str(&String::from_utf8_lossy(&chunk))`
  - 现在：`super::sse::append_utf8_safe(&mut sse_buffer, &mut sse_utf8_remainder, &chunk)`
  - `unfold` 的 state tuple 从 3 元改 4 元，3 处 return 点同步更新
- `forwarder.rs` 的 `transform_sse_chunk` 和 `append_and_parse_sse`：
  - 函数签名加 `remainder: &mut Vec<u8>` 参数
  - 内部改用 `super::sse::append_utf8_safe`
  - 所有 14 处调用方（1 处生产 + 13 处单元测试）同步加 `let mut remainder = Vec::new()`
- `responses_handler.rs`：删除原有的私有 `append_utf8_safe` 和 `sse_data_payload`，改为调用 `super::sse::` 的公共实现

**P3 消除路径**：之前 4 处独立实现，3 处用 `from_utf8_lossy`（handlers.rs、forwarder.rs 两处）会把多字节字符切坏；现在全改用 `append_utf8_safe`。

### 阶段 2：字段穿透对称化（commits `884f738`、`3980efa`）

**Claude（`884f738`）**：

- `protocol/claude.rs`（上游方向）：
  - 顶部加 `const ENABLE_UNKNOWN_FIELD_PASSTHROUGH: bool = true;`
  - `transform_request_to_anthropic` 最后加 passthrough 块：`for (key, value) in obj.iter() { if !anthropic_obj.contains_key(key) { insert } }`
  - `transform_response_from_anthropic`：原来 `json!({"id":..., "object":..., "choices":[...], "usage":...})` 从头构造；现在 `let mut response_body = obj.clone();` 然后 edit-in-place 改 `choices` / `finish_reason` / `usage` 等已知字段，未知字段随 clone 保留
  - 加 `if !ENABLE_UNKNOWN_FIELD_PASSTHROUGH` 分支：关闭时用白名单过滤到 OpenAI 已知字段

- `protocol/claude_output.rs`（下游方向）：
  - 顶部加同名常量
  - `claude_to_openai_request` 最后加 passthrough 块
  - `openai_to_claude_response`：从 `json!({...})` 构造改为 `openai.clone() + edit-in-place`
  - 同样加白名单 fallback 分支

- `forwarder.rs`：为了让流式响应的未知字段也能经过 transform 保留，`build_streaming_response` 配套调整

**Gemini（`3980efa`）**：

- `protocol/gemini.rs`：顶部加常量（标 `#[allow(dead_code)]`，因为 Gemini 走 OpenAI 兼容端点，此常量留给未来激活 native 方案用）
- `protocol/gemini_output.rs`：
  - 顶部加常量
  - `gemini_to_openai_request`：加 passthrough 块，保留 `safetySettings` / `cachedContent` / `x_future_gemini_field` 等字段
  - `openai_to_gemini_response`：从 `json!({...})` 改为 `.clone() + edit`
  - 加关闭穿透时的白名单分支

**原来 5 个红测试全变绿**。

### 阶段 3.1：P1 修复（commits `de62e99` + `7e450ce`）

**文件**：`protocol/claude_output.rs`

**根因**：`ClaudeSSETransformer::transform_chunk` 原流程：
```
1. parse chunk
2. let Some(choice) = chunk.choices.get(0) else { return events; }  ← 空 choices 直接退
3. capture usage from chunk                                          ← 永远到不了
```

OpenAI 官方流式帧序列（启用 `stream_options.include_usage`）：
```
帧 N  : {"choices":[{"delta":{"content":"..."}}]}
帧 N+1: {"choices":[{"delta":{},"finish_reason":"stop"}]}
帧 N+2: {"choices":[], "usage":{"prompt_tokens":10,"completion_tokens":20}}  ← 空 choices
帧 N+3: [DONE]
```

第 N+1 帧触发 `message_delta` emit，但此时 `self.usage_output_tokens = 0`。第 N+2 帧的 usage 永远写不进来。结果：**Claude 客户端收到的 `message_delta.usage.output_tokens` 永远是 0**。

**修复（`7e450ce`）**：
1. **调序**：usage 捕获从 `let Some(choice)` 之后挪到之前
2. **补发**：在 early return 分支内判断 `if self.message_delta_emitted && self.usage_output_tokens > 0` → 补发一次 `message_delta`（Claude 协议允许多次 message_delta，这是合法的）
3. **状态**：新增 `message_delta_emitted: bool` 字段跟踪 finish 帧是否已 emit
4. **死代码清理**：`protocol/mod.rs` 的 `pub use` 清理阶段 2 已删除的符号（`openai_to_azure_response` / `transform_azure_error` / `AzureSSETransformer` / `transform_gemini_error` / `GeminiSSETransformer`）。这些原来是死代码，在阶段 2 的 Gemini/Azure 重构里删掉了但 `pub use` 残留导致编译失败
5. **相关死代码**：`7e450ce` 一次性删除 541 行（`azure_output.rs` 大半 + `gemini_output.rs` 里 `build_gemini_native_*` / `transform_request_to_gemini` / `GeminiSSETransformer` 等 `#[allow(dead_code)]` 函数）

### 阶段 3.2：Responses 上游 adapter（commits `6577844` + `3a3caf6`）

**前端（`6577844`）**：

- `src/types.ts`：
  - `ApiType` 联合类型加 `"responses"`
  - `API_TYPE_OPTIONS` 加 `{ value: "responses", label: "OpenAI Responses (Beta)" }`
  - `API_TYPE_DEFAULT_URLS.responses = "https://api.openai.com"`
- `src/features/channels/ChannelManager.tsx`：`API_TYPES` 数组加 Responses (Beta) 选项
- 副作用：`ChannelManager.tsx` 还发生了大量格式化（509 行 diff 里多数是 whitespace），功能改动很小

**Rust 后端（`3a3caf6`）**：

- **新增 `protocol/responses.rs`**（795 行）：
  - `struct ResponsesAdapter`
  - `impl ProtocolAdapter for ResponsesAdapter`：
    - `build_chat_url`：`{base}/v1/responses`
    - `apply_auth`：Bearer token
    - `build_models_url`：`{base}/v1/models`（和 OpenAI 一致）
    - `transform_request` → 调 `transform_request_to_responses`：
      - `messages[]` 分 `system/user/assistant/tool` 分别翻译成 Responses `input[]` items
      - `role: system/developer` → 顶层 `instructions`（如果已有 instructions，后续 system 消息降级为 input[] 里的 message item）
      - `role: user` → `{type:"message",role:"user",content:[input_text parts]}`
      - `role: assistant` 带 text → `{type:"message",role:"assistant",content:[output_text parts]}`
      - `role: assistant` 带 tool_calls → 每个 tool_call 变成 `{type:"function_call",call_id,name,arguments}` item
      - `role: tool` → `{type:"function_call_output",call_id,output}` item
      - `max_tokens` → `max_output_tokens`
      - `tools`：function 类型翻译为 Responses 格式（无 `function` wrapper，name/description/parameters 平铺），其他 tool 类型原样穿透
      - `response_format: json_schema/json_object` → `text.format`
      - `ENABLE_UNKNOWN_FIELD_PASSTHROUGH=true` 时穿透未知字段
    - `transform_response` → 调 `transform_response_from_responses`：
      - `output[]` 里的 `message/role:assistant` → `choices[0].message`
      - `output[]` 里的 `function_call` → `choices[0].message.tool_calls[]`
      - `status: completed/incomplete/failed` → `choices[0].finish_reason`
      - `usage.input_tokens` → `usage.prompt_tokens`，`output_tokens` → `completion_tokens`
      - `reasoning.summary` 透传到 provider_specific
      - clone + edit-in-place 保留未知字段
    - `needs_sse_transform: false`（**v1 简化**，SSE 暂时直通 chat.completions 格式，完整 Responses SSE 翻译后续版本补齐）
    - `extract_sse_usage`：从 `response.completed` 事件里读 `response.usage.input_tokens / output_tokens`
  - 文件内 30+ 个单元测试覆盖各翻译分支

- **`protocol/mod.rs`**：
  - 顶部加 `mod responses;`
  - `get_adapter` match 加分支：`"responses" => Box::new(responses::ResponsesAdapter),`

- **`protocol/roundtrip_tests.rs`**：删除 4 个测试的 `#[ignore]` 属性

**集成时发现并修复的 bug**：
- `extract_text_content(Some(Value::Null))` 返回 `"null"`（因为走的是 `serde_json::to_string(&Value::Null) = "null"` 分支）
- 导致 `messages` 里 `assistant` 消息 content 为 null 时（通常同时带 tool_calls）会多 emit 一个空 output_text item
- 修复：`Some(Value::Null) | None => String::new()`

**验收**：231 pass / 0 fail / 0 ignored

---

## 六、剩余工作清单

### 阶段 3.3：协议模块合并（10 → 5 个文件）

**目标**：按第四节范式把每个协议的上下游代码整合到一个文件。**纯重构，无行为变化**。

**合并前文件清单**：
```
protocol/claude.rs          (1179 行, 上游 adapter)
protocol/claude_output.rs   (1293 行, 下游翻译器)   ← 合并到 claude.rs
protocol/gemini.rs          ( 554 行, 上游 adapter)
protocol/gemini_output.rs   ( ~300 行 after 清理, 下游翻译器)  ← 合并到 gemini.rs
protocol/azure.rs           ( 128 行, 上游 adapter)
protocol/azure_output.rs    ( 102 行 after 清理, 下游翻译器)  ← 合并到 azure.rs
protocol/responses.rs       ( 892 行, 双向 adapter)  ← 已完成范式
protocol/openai.rs          (  88 行, 基准)
protocol/custom.rs          (  91 行, OpenAI 兼容 fallback)
protocol/common.rs          (  17 行, join_url 等工具)
```

**合并后目标**：
```
protocol/claude.rs          (~2400 行)
protocol/gemini.rs          (~850 行)
protocol/azure.rs           (~230 行)
protocol/responses.rs       (~892 行, 已就绪)
protocol/openai.rs          (  88 行, 不变)
protocol/custom.rs          (  91 行, 不变)
protocol/common.rs          (  17 行, 不变)
```

删除：`claude_output.rs`、`gemini_output.rs`、`azure_output.rs`。

**执行步骤**（每合并一个协议一个 commit）：

#### 3.3.A Azure 合并（最简单，先做）

目的：把 `azure_output.rs` 仅剩的 `azure_to_openai_request` 挪到 `azure.rs`，统一入口。

1. 读 `azure_output.rs` 全部内容，识别还被使用的 pub 函数（应只有 `azure_to_openai_request`，因为 `openai_to_azure_response` / `transform_azure_error` / `AzureSSETransformer` 已在阶段 2 删掉）
2. 把 `azure_to_openai_request` 及其测试挪到 `azure.rs` 尾部
3. 在 `azure.rs` 顶部加 `const ENABLE_UNKNOWN_FIELD_PASSTHROUGH: bool = true;`（加 `#[allow(dead_code)]`，因为 Azure body 几乎直通，不需要独立的 passthrough 代码）
4. 删除 `protocol/azure_output.rs`
5. 更新 `protocol/mod.rs`：
   - 删 `mod azure_output;`
   - 修改 `pub use`：`pub use azure_output::azure_to_openai_request;` → `pub use azure::azure_to_openai_request;`
6. `handlers.rs` 不用改（只引用 `super::protocol::azure_to_openai_request`）
7. 跑 `cargo test --lib`，必须 231 pass
8. commit: `Phase 3.3a: merge azure_output.rs into protocol/azure.rs`

#### 3.3.B Gemini 合并

1. 读 `gemini_output.rs` 识别还被使用的：应为 `gemini_to_openai_request` 和 `openai_to_gemini_response`（`transform_gemini_error` / `GeminiSSETransformer` 已删）
2. 把这两个函数及其测试挪到 `gemini.rs`
3. `gemini.rs` 顶部已有 `ENABLE_UNKNOWN_FIELD_PASSTHROUGH`（标 `#[allow(dead_code)]`），保持不变
4. 删除 `protocol/gemini_output.rs`
5. 更新 `protocol/mod.rs`：删 `mod gemini_output;`，修 `pub use gemini_output::{...}` → `pub use gemini::{...}`
6. 跑测试
7. commit: `Phase 3.3b: merge gemini_output.rs into protocol/gemini.rs`

#### 3.3.C Claude 合并（最大）

1. 读 `claude_output.rs` 识别公共 API：`claude_to_openai_request` / `openai_to_claude_response` / `ClaudeSSETransformer` / `transform_claude_error`
2. 全部挪到 `claude.rs`
3. **注意 `transform_claude_error` 目前在 mod.rs 的 `pub use` 里有 warning**（未使用），挪过去时保持 `#[allow(dead_code)]` 或考虑删除
4. **注意 `ClaudeSSETransformer` 是下游方向 SSE 翻译器**，和上游方向 `ClaudeAdapter::transform_sse_line` 是两个独立的 public item，不要合并
5. 挪 claude_output.rs 的测试到 claude.rs 测试模块
6. 删除 `protocol/claude_output.rs`
7. 更新 mod.rs 的 `mod` 和 `pub use`
8. `handlers.rs` 不需要改（只引用 `super::protocol::{claude_to_openai_request, ...}`）
9. 跑测试，**特别关注 P1 测试**（`sse_claude_usage_tokens_not_dropped`）依然绿
10. commit: `Phase 3.3c: merge claude_output.rs into protocol/claude.rs`

#### 3.3.D Responses 整合（复杂但解耦度高）

当前 `responses_handler.rs`（1978 行）既包含 HTTP handler 又包含大量翻译逻辑。需要分离。

1. **识别纯翻译函数**（移到 `protocol/responses.rs`）：
   - `input_to_messages`（L33，360 行）
   - `convert_tools`（L360）
   - `passthrough_output_item`（L422）
   - `merge_tool_delta`（L436）
   - `is_function_tool_call`（L417）
   - 小辅助函数：`sse_line` / `sse_done` 保留在 handler 里（SSE 响应构造，不是翻译）
2. **SSE 重包装代码**（`responses_handler.rs:820-1100` 约 900 行，把 chat.completions SSE 翻译成 Responses SSE 事件流）：
   - 如果能抽成 `protocol::responses::ResponsesSSETransformer`，抽出来
   - 如果抽离成本太高（涉及 state、usage 累积等复杂度），**本次不强求**，留在 handler 里加注释："TODO: move to protocol::responses in a future refactor"
3. `responses_handler.rs` 只保留 HTTP 路由入口 + 薄调用层
4. 保持 `handle_responses` / `get_response` / `delete_response` / `cancel_response` 这 4 个 pub handler 函数签名不变
5. 跑测试（231 pass）
6. commit: `Phase 3.3d: move Responses translation helpers to protocol/responses.rs`

### 阶段 3.4：收尾（可选，低优先）

1. 删除 `ANALYSIS_CLAUDE_TO_RESPONSES.md`（早期分析文档，已被 REFACTOR_PLAN.md 取代）
2. 更新 `FLOW.md` 反映新结构
3. 跑 `cargo clippy --lib` 清掉所有 warning：
   - `transform_claude_error` unused import
   - `assert_json_eq` never used
   - `SessionInfo.expires_at` field never read
   - `ProxyError::BadRequest` variant never used
4. `fs_append` `.gitignore` 加入 `target-codex*/`（忽略其他 agent 的构建产物）
5. commit: `Phase 3.4: cleanup and doc refresh`

### 阶段 4：横切特性剥离成中间件（解决 P2 / P5 / P6）

**目标**：把 `forwarder.rs` 里的横切逻辑剥离成可装配的中间件链。不同入口装配不同中间件，解决 P2 / P5 / P6。

**当前 forwarder.rs 里的横切点**：
1. `forwarder.rs:491-499` stream_options 无条件覆盖（P2）
2. `forwarder.rs:624-634` `should_append_model_info`（P5 的源头）
3. `forwarder.rs:1199-1206` `model_info_delta` 注入（P5）
4. `forwarder.rs:200-300` log_usage / push_attempt（token 统计）
5. `forwarder.rs:380-420` cool_down_entry / disable_entry（熔断）
6. `forwarder.rs:765` `STREAMING_IDLE_TIMEOUT` 常量（目前只有第一层有，P6）

#### 4.1 定义中间件 trait

新文件：`src-tauri/src/proxy/middleware.rs`

```rust
pub struct RequestContext<'a> {
    pub caller_kind: CallerKind,  // OpenAI / Claude / Gemini / Azure / Responses
    pub entry: &'a ApiEntry,
    pub access_key: Option<&'a AccessKey>,
    pub requested_model: &'a str,
}

pub enum CallerKind {
    OpenAiChat,
    ClaudeMessages,
    GeminiNative,
    AzureChat,
    Responses,
}

pub trait ForwarderMiddleware: Send + Sync {
    fn on_request(&self, body: &mut Value, ctx: &RequestContext) { }
    fn on_response_complete(&self, body: &mut Value, ctx: &RequestContext) { }
    fn on_sse_chunk(&self, chunk: &mut Bytes, ctx: &RequestContext) { }
}
```

#### 4.2 P2 修复：`StreamOptionsMiddleware`

```rust
pub struct StreamOptionsMiddleware;
impl ForwarderMiddleware for StreamOptionsMiddleware {
    fn on_request(&self, body: &mut Value, _ctx: &RequestContext) {
        if !body.get("stream").and_then(|s| s.as_bool()).unwrap_or(false) {
            return;
        }
        let so = body
            .as_object_mut()
            .and_then(|m| m.entry("stream_options".to_string()).or_insert(json!({})).as_object_mut());
        if let Some(so) = so {
            so.entry("include_usage".to_string()).or_insert(json!(true));
        }
    }
}
```

关键改动：从 `insert` 改为 `entry().or_insert()`，**用户已有的 stream_options 字段不再被覆盖**。

#### 4.3 P5 修复：`ModelAnnotationMiddleware`

把 `forwarder.rs` 里 `should_append_model_info` 和 `model_info_delta` 相关代码抽成独立 middleware。关键：

```rust
impl ForwarderMiddleware for ModelAnnotationMiddleware {
    fn on_sse_chunk(&self, chunk: &mut Bytes, ctx: &RequestContext) {
        // Responses 入口不装这个中间件，本方法永不被调用
        // OpenAiChat / ClaudeMessages / Azure 装，所以对这些入口照常注入
    }
}
```

中间件装配：
- `forward_with_retry` 调用方按 `caller_kind` 构造中间件链
- `handle_responses` 调 `forward_with_retry` 时的 ctx.caller_kind = Responses，**不装** ModelAnnotationMiddleware
- 其他入口装配正常中间件

#### 4.4 P6 修复：`IdleTimeoutMiddleware`

把 `STREAMING_IDLE_TIMEOUT` 逻辑抽出来成独立 middleware，每层 SSE 流处理都装上。

#### 4.5 其他中间件

- `UsageLoggingMiddleware`：`log_usage` 相关
- `CircuitBreakerMiddleware`：`cool_down_entry` / `disable_entry` / 熔断决策
- 这两个**所有入口都装**

#### 4.6 执行步骤

每个中间件一个 commit，按此顺序：

1. `Phase 4a: add middleware trait and RequestContext`
2. `Phase 4b: extract StreamOptionsMiddleware (fixes P2)`
3. `Phase 4c: extract ModelAnnotationMiddleware (fixes P5)`
4. `Phase 4d: extract IdleTimeoutMiddleware (fixes P6)`
5. `Phase 4e: extract UsageLoggingMiddleware`
6. `Phase 4f: extract CircuitBreakerMiddleware`
7. `Phase 4g: wire middleware chains per caller_kind`

每个 commit 跑 `cargo test --lib`，必须 231+ pass。

---

## 七、恢复执行的快速启动（新会话必读）

1. **读本文档到本节**
2. 确认分支和 commit 状态：
   ```bash
   git log --oneline master..HEAD
   ```
   期望：14 个 commit，最新是 `3a3caf6`（Phase 3.2 Responses backend）
3. 看 working tree：
   ```bash
   git status
   ```
   期望：干净，无 modified 文件；可能有 untracked 文档和 target-codex* 目录
4. 跑测试确认基线：
   ```bash
   cd src-tauri && cargo test --lib 2>&1 | grep "test result"
   ```
   期望：`231 passed; 0 failed; 0 ignored`
5. 按**第六节的阶段 3.3.A (Azure 合并)** 开始执行（最简单）
6. 每个步骤完成后跑 `cargo test --lib`，再 commit
7. 每做完一个小阶段，更新本文档的"当前状态"节

---

## 八、调试提示

### 找 Responses 反向翻译的参考

- 正向（Responses → OpenAI）：`responses_handler.rs` 的 `input_to_messages`（L33）、`convert_tools`（L360）
- 反向（OpenAI → Responses）：`protocol/responses.rs::transform_request_to_responses` 已实现
- SSE 翻译：v1 简化为直通（`needs_sse_transform=false`），完整 SSE 双向翻译是未来版本的工作

### 现有 adapter 模板

- **最完整**：`protocol/claude.rs` 的 `ClaudeAdapter`
- **最简单**：`protocol/openai.rs` 的 `OpenAiAdapter`
- **新范式参考**：`protocol/responses.rs`（双向翻译 + passthrough 常量一条龙示例）

### 测试运行

```bash
# 跑单个模块
cargo test --lib proxy::protocol::roundtrip_tests::responses_roundtrip

# 跑单个测试
cargo test --lib sse_claude_usage_tokens_not_dropped

# 跑全量
cargo test --lib

# 仅编译（不跑）
cargo test --lib --no-run

# 强制跑 ignored（现在应该没有 ignored）
cargo test --lib -- --include-ignored

# 跑测试 + 打印 stdout（调试用）
cargo test --lib <test_name> -- --nocapture
```

### 常见坑（已踩过）

1. **`json!({ "x": ... })` 构造响应会丢字段** — 改用 `.clone() + edit-in-place`
2. **`serde_json::to_string(&Value::Null)` 返回 `"null"`** — 字符串提取函数必须特判 Null 为空串
3. **`#[cfg(test)]` 模块外加 `#[test]` 不允许**
4. **Rust 函数签名变更要同步所有调用方**，包括 13 处测试里的 `let mut remainder = Vec::new()`
5. **中文 commit message** 在 Windows PowerShell 下编码会坏（作者名也会坏），改用英文
6. **批量改 `#[ignore]`** 用 `str_replace` 逐个改，PowerShell 的 `-replace` 和不在仓库根运行的 Python 都可能不生效
7. **阶段 1 里 `unfold` 的 state tuple 改 4 元后**，3 处 return 点都要同步加 `sse_utf8_remainder` 字段
8. **阶段 3.1 的 `pub use` 清理**：`pub use xxx::{A, B, C}` 里任一符号删除后整条 `pub use` 会失败，必须逐项核对

### 常用命令参考

```bash
# 看 commit 详情
git show --stat <hash>

# 看文件历史
git log -p -- src-tauri/src/proxy/protocol/claude.rs

# 对比两个 commit
git diff 884f738 3a3caf6 -- src-tauri/src/proxy/protocol/

# 回滚某个 commit（新 commit 形式撤销，不改历史）
git revert <hash>

# 重置到某个 commit（**不要 force push**）
git reset --soft <hash>

# 看当前分支所有改动
git log master..HEAD --stat
```

---

## 九、不做什么（明确拒绝）

- 同协议直连（翻译几毫秒可接受，架构清晰更重要）
- 换中间协议（OpenAI chat.completions 继续）
- 激活 Gemini native dead code
- 引入 LiteLLM / MCP / 其他外部框架
- 流式 IR 强类型化（收益不足以匹配 800 行重写风险，**推迟到未来独立项目**）
- 给用户 UI 配置 passthrough 开关（源码常量即可）
- 为优化而优化（代码工整 vs 小的性能差异，选工整）

---

## 十、提交规范

- 每阶段独立 commit 序列
- commit message 用**英文**（避免 Windows 编码问题）
- commit message 首行 < 80 字符，描述用现在时动词开头（`add`/`fix`/`refactor`/`extract`/`merge`/`implement`）
- commit message 正文每段用 `-m` 分隔，不用换行符（PowerShell 下复杂 shell 转义容易出问题）
- 每阶段结束跑 `cargo test --lib`，必须 `231+ passed; 0 failed`
- 任一阶段都可独立 `git revert` 回滚，不留半成品

---

## 十一、验收标准

### 阶段 3 完整完成的标志

- [ ] `protocol/` 目录下只有 5 个协议文件 + `common.rs` + `mod.rs` + `roundtrip_tests.rs`
- [ ] 没有 `xxx_output.rs` 后缀的文件
- [ ] 每个协议文件顶部都有 `const ENABLE_UNKNOWN_FIELD_PASSTHROUGH`
- [ ] `cargo test --lib` 231+ pass / 0 fail / 0 ignored

### 阶段 4 完整完成的标志

- [ ] P2 / P5 / P6 全部标记为已修
- [ ] `forwarder.rs` 行数显著下降（横切逻辑剥离到 middleware.rs）
- [ ] 新增 P2 / P5 / P6 的回归测试，全绿
- [ ] `cargo test --lib` ≥240 pass / 0 fail / 0 ignored

### 整体项目完成的标志

- [ ] master 分支 merge 本分支
- [ ] 删除分支
- [ ] README 或 FLOW.md 反映新架构
- [ ] BUG.MD 里的 P1-P6 全部标记为已修

---

## 附录 A：文件地图

```
src-tauri/src/proxy/
├── mod.rs                 # 模块声明
├── server.rs              # Proxy HTTP server 启动
├── auth.rs                # Access key 校验
├── circuit_breaker.rs     # 熔断器数据结构
├── router.rs              # 模型路由（精确/模糊匹配）
├── forwarder.rs           # 转发 + 重试 + usage 日志（阶段 4 重构目标）
├── handlers.rs            # 5 个入口 handler（chat/messages/models/gemini/azure）
├── responses_handler.rs   # /v1/responses handler（阶段 3.3d 瘦身目标）
├── sse.rs                 # SSE 公共设施（阶段 1 产物）
└── protocol/
    ├── mod.rs             # ProtocolAdapter trait + get_adapter factory
    ├── common.rs          # join_url 等工具
    ├── openai.rs          # OpenAI 基准（不翻译）
    ├── custom.rs          # 宽容版 OpenAI
    ├── claude.rs          # Claude 上游 adapter（阶段 3.3c 合并目标）
    ├── claude_output.rs   # Claude 下游翻译（阶段 3.3c 删除）
    ├── gemini.rs          # Gemini 上游 adapter（阶段 3.3b 合并目标）
    ├── gemini_output.rs   # Gemini 下游翻译（阶段 3.3b 删除）
    ├── azure.rs           # Azure 上游 adapter（阶段 3.3a 合并目标）
    ├── azure_output.rs    # Azure 下游翻译（阶段 3.3a 删除）
    ├── responses.rs       # Responses 双向 adapter（阶段 3.2 已完成）
    └── roundtrip_tests.rs # 24 个 round-trip 测试
```

## 附录 B：协议翻译对照

| 概念 | OpenAI chat.completions | Claude | Gemini | Azure | Responses |
|---|---|---|---|---|---|
| 入口路径 | `/v1/chat/completions` | `/v1/messages` | `/v1beta/models/*:generateContent` | `/openai/deployments/*/chat/completions` | `/v1/responses` |
| 消息容器 | `messages[]` | `messages[]`（简化） | `contents[]` | `messages[]` | `input[]` |
| system | `{role:"system"}` | 顶层 `system` | 顶层 `systemInstruction` | `{role:"system"}` | 顶层 `instructions` |
| 工具定义 | `tools[].function` | `tools[]` (flat) | `tools[].functionDeclarations[]` | `tools[].function` | `tools[]` (flat) |
| 工具调用 | `choices[].message.tool_calls[]` | `content[].{type:"tool_use"}` | `candidates[].content.parts[].functionCall` | `choices[].message.tool_calls[]` | `output[].{type:"function_call"}` |
| token 用量字段 | `usage.prompt_tokens/completion_tokens` | `usage.input_tokens/output_tokens` | `usageMetadata.{prompt,candidates}TokenCount` | 同 OpenAI | `usage.input_tokens/output_tokens` |
| 结束原因 | `finish_reason: stop/length/tool_calls` | `stop_reason: end_turn/max_tokens/tool_use` | `finishReason: STOP/MAX_TOKENS` | 同 OpenAI | `status: completed/incomplete/failed` |
| 鉴权 | `Authorization: Bearer` | `x-api-key` + `anthropic-version` | `?key=` query param | `api-key` header | `Authorization: Bearer` |
