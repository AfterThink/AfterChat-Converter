# 构建与开发指南

本仓库采用 **Cargo Workspace Monorepo** 结构：各转换器（`crates/*`）与 Tauri 桌面端（`gui/`）统一组织在同一代码库中。构建桌面端时会自动编译各转换器并作为 Sidecar 二进制一同打包。

---

## 快速开始

进行桌面端开发时，执行以下步骤：

1. **安装前端依赖（首次运行）**:
   ```powershell
   cd gui
   bun install
   ```

2. **启动桌面端开发模式**:
   ```powershell
   bun run dev
   ```
   > **说明**: `bun run dev`（底层调用 `tauri dev`）直接加载原生 HTML/JS/CSS，不经过 Vite 等打包器。运行前会自动执行预处理脚本编译各转换器并注入至 Sidecar 目录。

---

## 目录结构

| 路径 | 说明 |
| --- | --- |
| `crates/afterchat-chatformat` | 输出契约公共库（纯 lib），提供模型定义、渲染与打包能力 |
| `crates/afterchat-ai-studio` | Google AI Studio 转换器，产物二进制 `ai-studio` |
| `crates/afterchat-cherry` | Cherry Studio 备份转换器，产物二进制 `cherry` |
| `crates/afterchat-qwen` | Qwen 转换器，产物二进制 `qwen` |
| `crates/afterchat-claude` | Claude 转换器，产物二进制 `claude` |
| `crates/afterchat-rikka` | RikkaHub 备份转换器，产物二进制 `rikka` |
| `gui` | 基于 Tauri + Bun 的桌面客户端，通过 Sidecar 调用上述转换器 |
| `docs` | 格式契约、系统架构、GUI 规范及各转换器技术文档 |

- **Workspace 管理**: 根目录 `Cargo.toml` 将所有 crate 及 `gui/src-tauri` 纳入 workspace members，共享统一的 `Cargo.lock` 与 `target/` 输出目录。
- **独立运行**: 每个转换器均为独立的 crate，支持通过 `cargo run -p <pkg> -- <input>` 独立运行，亦可直接将输入文件拖拽至编译出的可执行文件上。
- **命名规范**: Crate 包名与目录名均以 `afterchat-` 为前缀，二进制产物名保持简短（如 `rikka`、`claude`），桌面端 Sidecar 机制直接绑定该短名称。

---

## 构建命令

### 编译所有命令行转换器
```powershell
cargo build --release
```

### 构建桌面端发布安装包
```powershell
cd gui
bun run build
```

> **注意**: 执行 `cargo check -p afterchat-converter` 时，需要 `gui/src-tauri/bin/` 目录下已包含 Sidecar 文件；可提前执行 `bun run prepare:sidecars`。

---

## 测试

执行工作区单元测试与集成测试：

```powershell
cargo test --workspace --exclude afterchat-converter
```

`afterchat-converter`（桌面端）编译依赖预先生成的 Sidecar 二进制，因此在工作区测试中显式排除；桌面端主要通过交互操作进行功能验证。

`crates/afterchat-chatformat` 覆盖转义规则、文件命名、时间解析、会话排序、重名处理与失败报告生成；各转换器在 `tests/integration_cli.rs` 中通过合成测试数据断言 CLI 执行结果与 ZIP 包结构。

---

## CI / CD 流程

### 持续集成测试（`.github/workflows/ci.yml`）

在向 `master` 推送代码、提交 Pull Request 或手动触发时执行：

1. 代码格式检查：`cargo fmt --all -- --check`
2. 版本号一致性校验：检查 `gui/package.json`、`gui/src-tauri/Cargo.toml`、`gui/src-tauri/tauri.conf.json` 三处版本号是否完全一致
3. 依赖锁定校验：`bun install --frozen-lockfile`（验证 `gui/bun.lock` 一致性）
4. 运行测试：`cargo test --workspace --exclude afterchat-converter`

### 自动发布（`.github/workflows/release.yml`）

**发布版本号以 `gui/src-tauri/tauri.conf.json` 中的 `package.version` 为准。**

发布流程：

1. 同步更新以下三个配置文件中的版本号：
   - `gui/package.json` → `version`
   - `gui/src-tauri/Cargo.toml` → `[package] version`
   - `gui/src-tauri/tauri.conf.json` → `package.version`
2. 提交更改并推送到 `master` 分支；
3. CI 检查对应 `v<version>` 标签：
   - **标签已存在**：跳过发布流程；
   - **标签不存在**：执行 GUI 与 Sidecar 构建 → 自动创建 `v<version>` Git 标签 → 创建 GitHub Release；
4. 发布的产物包包括：
   - `afterchat-converter_<version>_x64-setup.exe`：NSIS 安装包（包含桌面端与 5 个转换器）
   - `afterchat-converter-<version>-windows-x64.zip`：绿色免安装压缩包（解压即可运行）

> 发布流程仅使用 GitHub Actions 内置的 `GITHUB_TOKEN`，无需额外配置访问凭证。

---

## Monorepo 维护与扩展

### 新增转换器流程

1. 在 `crates/afterchat-<name>` 目录下创建新 crate（配置 `Cargo.toml` 与 `src/`），引入公共依赖 `chatformat = { package = "afterchat-chatformat", path = "../afterchat-chatformat" }`；
2. 在根目录 `Cargo.toml` 的 `members` 中注册该 crate；
3. 在 `gui/scripts/prepare-sidecars.mjs` 的 `binaries` 配置数组中登记 `{ pkg, bin }`；
4. 在 `gui/src-tauri/tauri.conf.json` 的 `tauri.bundle.externalBin` 中追加 `bin/<bin>`；
5. 在 `gui/src-tauri/src/main.rs` 中的 `ConverterKind`、`ALL_CONVERTERS` 以及 Sidecar 名称映射中注册；
6. 在 `gui/src/main.js` 中增加文件名匹配路由与输出类型判断；
7. 更新 `gui/src/index.html` 中的支持格式列表；
8. 编写对应文档 `docs/converters/<name>.md`，并在 `docs/ARCHITECTURE.md` 的数据映射表中补充说明。
