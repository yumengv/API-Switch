# disable_keywords & keyword_freeze_scope 配置说明

## 目的
当上游返回错误信息包含特定关键字时，系统会冷却/冻结对应的条目或通道，防止持续错误请求。

## 关键字冷冻范围（keyword_freeze_scope）

新增于 v0.6.58，控制关键字匹配后的熔断层级：

| 值 | 效果 | 适用场景 |
|----|------|----------|
| `"model"`（默认） | 触发 L1 DB 冷却，仅冷却当前模型 300-1800s | 上游为中转站（CODING PLAN、SiliconFlow 等），一个模型配额不足不应影响同渠道其他模型 |
| `"channel"` | 触发 L3 渠道冷冻，冻结同渠道所有模型 6h | 上游为单一供应商，配额耗尽应停用整条渠道 |

设置路径：**Settings → Circuit Breaker → Keyword Freeze Scope**。

## 默认关键字集合
如果用户在 **Settings → Config** 中没有手动配置 `disable_keywords`，系统会使用以下默认关键字列表（与数据库首次创建时的默认值保持一致）：

```
Your credit balance is too low
This organization has been disabled.
You exceeded your current quota
Permission denied
The security token included in the request is invalid
Operation not allowed
Your account is not authorized
insufficient_quota
quota_exceeded_error
token plan limit exhausted
Upstream rate limit exceeded
invalid api key
Unauthorized - Invalid token
```

## 自定义关键字
- 前往 **Settings → Config** 页面，编辑 **disable_keywords** 文本框。
- 每行填写一个关键字，匹配时不区分大小写。
- 保存后立即生效，覆盖默认集合。

## 行为说明
- 当 `disable_keywords` 为空或仅包含空行时，系统自动回退到 **默认关键字集合**。
- 关键字匹配通过 `should_disable_entry_for_message` 实现，匹配任意关键字即触发。
- 默认触发**模型级冷却**（L1），可通过 `keyword_freeze_scope` 改为**渠道级冷冻**（L3，等同于原行为）。
- 渠道级冷冻时长为 6 小时，冷冻后会记录日志 `Freezing channel … because entry … matched upstream error keyword`。

## 设计背景
上游不只是单一供应商，还包括中转站（如 CODING PLAN、SiliconFlow）。中转站整合多个模型提供商的 API，同一渠道下有不同的模型（甚至不同公司）。如果关键字匹配直接冻结整条渠道，会误杀同中转站下其他正常提供商的服务。因此默认改为模型级冷却，仅影响当前模型。

## 关联文档
- `src-tauri/src/database/dao/config_dao.rs`：`AppSettings` 结构体定义，`keyword_freeze_scope` 字段默认值。
- `src-tauri/src/database/schema.rs`：首次创建数据库时写入的默认配置。
- `src-tauri/src/proxy/forwarder.rs`：根据 `keyword_freeze_scope` 条件分支决定冷却或冻结。
- `WHITEPAPER.md` 第 8.5 节：三级容错体系及关键词匹配行为。
