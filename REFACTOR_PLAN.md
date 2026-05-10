# 代理层架构整改计划

> 这是执行路线图，不需要审核。所有决策已经对齐，开始执行。

---

## 一、两条公理（不可违反）

**公理一**：api-switch 是**中转和翻译器**，不是内容修改者。

**公理二**：协议以**各自官方文档**为准，这边进来什么，那边出去必须是一样的（往返无损）。

---

## 二、协议模块范式（整改目标）

**一个协议 = 一个文件 = 一套标准套件**。5 个现有协议和将来新增协议统一长这样：

```
protocol/xxx.rs

1. 源码常量：ENABLE_UNKNOWN_FIELD_PASSTHROUGH（默认 true，紧急时改 false）
2. 协议元数据：URL、鉴权头、模型列表
3. 双向翻译（4 个方向对称）：
   - 下游入口：客户端请求 → OpenAI 中间格式
   - 下游入口：OpenAI 中间格式 → 客户端响应
   - 上游 adapter：OpenAI 中间格式 → 上游请求
   - 上游 adapter：上游响应 → OpenAI 中间格式
4. SSE 处理（流式版对应）
5. 单元测试：round-trip、未知字段穿透、官方文档样本
```

**加第 6 个协议 = 复制文件 → 填字段 → 写测试 → 注册**。核心不动。

---

## 三、9 条执行决策（全部对齐完毕）

1. 中间格式继续用 OpenAI chat.completions，不换
2. OpenAI 是翻译路径，不是规范仲裁者；每个协议以自己官方文档为准
3. 当前 10 个翻译器（5 协议 × 2 方向）合并成 5 个协议模块
4. 每个协议文件遵循范式二的统一结构
5. 每协议顶部一个 `ENABLE_UNKNOWN_FIELD_PASSTHROUGH` 源码常量，不做 UI 配置
6. 合并基底选择：
   - `OpenAI`：保持现状（基准，无需翻译）
   - `Claude`：以上游 `claude.rs` 为基底（更完整），下游方向补齐
   - `Gemini`：以下游 `gemini_output.rs` 为基底，保持 OpenAI 兼容端点方案（`v1beta/openai/`），不激活 native 死码
   - `Azure`：清理死码，合并成薄文件
   - `Responses`：以下游 `responses_handler.rs` 的翻译部分为基底，**本轮补齐上游方向**，Beta 标记
   - `Custom`：保持现状（OpenAI 兼容 fallback）
7. Responses 上游方向在阶段 3 一次做完（前端 `ApiType` 加选项、Rust 加 adapter）
8. P1（Claude SSE usage=0）在阶段 3 合并重写时顺手修掉
9. 不做：同协议直连、换中间协议、Gemini native、引入外部框架

---

## 四、整改阶段（执行顺序）

### 阶段 0：测试基线

补一批针对"目标行为"的 round-trip 测试。当前代码有 bug 的路径，测试会是红的（作为修复目标）；当前行为正确的路径，测试是绿的（作为不能破坏的基线）。

**退出条件**：测试能跑，绿的永远不变红，红的通过后续阶段逐个变绿。

**不改生产代码**。

### 阶段 1：流式处理公共设施

抽出 `sse` 公共模块（UTF-8 安全读取、buffer 上限、空闲超时），4 处各写一份的代码收缩到 1 处。

顺带根治 P3/P4/P6（不是目标，是副产品）。

### 阶段 2：响应方向穿透对称化

当前响应方向全部是"白名单构造新对象"，改成"clone + edit-in-place + 未知字段保留"。

### 阶段 3：协议模块合并 + 统一范式

**最大的一步**。把 10 个翻译器合并成 5 个协议模块文件，按范式二结构组织。

顺带做：
- 引入强类型流式事件枚举（替代 SSE 字符串作为中间层）
- P1 Claude SSE 时序 bug 重写时修好
- 新增 `ResponsesAdapter`（Responses 协议的上游方向）
- 前端 `ApiType` 增 `responses` 选项

### 阶段 4：横切特性剥离

`model: xxx` 注入、token 统计、熔断决策从 forwarder 核心剥离成中间件链。每个入口按需装配。

P5 根治（Responses 不装 model 注入中间件）。

---

## 五、10 → 5 合并决策表

| 合并后文件 | 基底 | 动作 |
|---|---|---|
| `protocol/openai.rs` | 现 openai.rs | 保持不动 |
| `protocol/claude.rs` | 现 claude.rs（上游） | 合并 claude_output.rs 的逻辑；SSE transformer 重写修 P1 |
| `protocol/gemini.rs` | 现 gemini_output.rs（下游）的翻译逻辑 | 上游方向保持 `v1beta/openai/` 兼容端点；native dead code 删除 |
| `protocol/azure.rs` | 现 azure.rs（上游） | 合并 azure_output.rs 的逻辑；删 AzureSSETransformer 等死码 |
| `protocol/responses.rs` | 现 responses_handler.rs 的翻译部分 | **新增**；下游保持现逻辑；上游方向本轮补齐 |
| `protocol/custom.rs` | 现 custom.rs | 保持不动 |

---

## 六、不做什么

- 同协议直连（翻译几毫秒可接受，架构清晰更重要）
- 换中间协议（OpenAI 继续）
- 激活 Gemini native dead code
- 引入 LiteLLM / MCP / 其他框架
- 给用户 UI 配置穿透开关（源码常量即可）

---

## 七、提交规范

- 每阶段独立 commit 序列，不跨阶段混合
- commit message 用中文
- 每阶段结束跑全量 `cargo test`，必须绿
- 任一阶段都可独立回滚

开始。
