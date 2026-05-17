# API Switch — Agent Instructions

###项目必须执行的规定
-PLAN.MD 是项目的主计划文件。有项目开发和修改的内容要求必须在计划中予以同步更新记载
-每一次提交，必须先将项目的版本号递增+1

## 版本管理

### 版本号文件清单（必须保持同步）

以下文件包含项目版本号，发布时必须全部更新为相同版本：

1. **`package.json`** - 前端和构建脚本版本
2. **`src-tauri/Cargo.toml`** - Rust 后端版本
3. **`src-tauri/Cargo.lock`** - 自动生成，由 Cargo.toml 决定
4. **`src-tauri/tauri.conf.json`** - Tauri 应用配置版本

### 单一事实源

**`package.json` 是版本的唯一事实源**。

版本更新流程：
1. 修改 `package.json` 中的 `version` 字段
2. 手动同步其他文件：运行 `node scripts/bump-version.cjs` 或手动修改
3. 检查 `Cargo.toml` 和 `tauri.conf.json` 是否已同步

### Pre-commit Hook (已禁用)

> **注意：由于兼容性问题，pre-commit hook 已被禁用（.git/hooks/pre-commit 已重命名为 pre-commit.disabled）。现在需要手动递增版本号。**

项目脚本 `scripts/bump-version.cjs` 仍可手动使用：
- 自动从 `package.json` 读取版本号
- 同步更新 `Cargo.toml`、`Cargo.lock`、`tauri.conf.json`

### 版本不一致故障排查

如发现版本号不一致：
1. 确认 `package.json` 版本正确
2. 手动运行 `node scripts/bump-version.cjs`
3. 检查各文件版本并手动修正

## 项目架构速查

- 前端：React + TypeScript + TanStack Query + i18next
- 后端：Tauri (Rust) + SQLite
- 通信：Tauri `invoke` 命令，Rust 端 `#[tauri::command]` 函数
- 设置存储：SQLite `config` 表 key-value，Rust `AppSettings` 结构体
- 路由核心：`src-tauri/src/proxy/router.rs`，读取 `settings.default_sort_mode` 决定排序策略
