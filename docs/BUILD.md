# 构建与开发指南

本仓库是一个 **Cargo Workspace monorepo**：所有转换器（`crates/*`）与 Tauri GUI（`gui/`）都在同一个仓库里。GUI 会自动把各转换器编译成 sidecar 并一起打包。

---

## 🚀 快速开始 (推荐开发流程)

如果你主要进行 GUI 开发，**只需要**执行：

1. **安装依赖 (仅首次)**:
   ```powershell
   cd gui
   bun install
   ```

2. **启动 GUI 开发模式**:
   ```powershell
   bun run dev
   ```
   > **💡 提示**: `bun run dev` (即 `tauri dev`) 直接加载原生 HTML/JS/CSS，不依赖 Vite。它会自动调用预处理脚本编译所有转换器并作为 Sidecar 注入。

---

## 🛠️ 目录结构

| 路径 | 说明 |
| --- | --- |
| `crates/afterchat-chatformat` | 输出契约的公共实现（纯 lib），被所有转换器依赖 |
| `crates/afterchat-ai-studio` | Google AI Studio 转换器，二进制 `ai-studio` |
| `crates/afterchat-cherry` | Cherry Studio 备份转换器，二进制 `cherry` |
| `crates/afterchat-qwen` | Qwen 转换器，二进制 `qwen` |
| `crates/afterchat-claude` | Claude 转换器，二进制 `claude` |
| `crates/afterchat-rikka` | RikkaHub 备份转换器，二进制 `rikka` |
| `gui` | Tauri + Bun 桌面 GUI，通过 Sidecar 调用上面的转换器 |
| `docs` | 契约、架构、GUI 与各转换器说明 |

- **Workspace**: 根目录的 `Cargo.toml` 把所有 crate 与 `gui/src-tauri` 列为成员，共享同一个 `Cargo.lock` 与 `target/`。
- 每个转换器仍然是独立的 crate 和独立可执行文件，可以单独 `cargo run -p <pkg> -- <input>` 使用，也可以把文件直接拖到单个 exe 上。
- **包名与目录名带 `afterchat-` 前缀，二进制名保持短名**（`rikka` / `claude` …），GUI 的 sidecar 只认二进制名。

---

## 📦 进阶构建

### 整体构建 (命令行版本)
```powershell
cargo build --release
```

### 构建 GUI 发布包
```powershell
cd gui
bun run build
```

> `cargo check -p afterchat-converter`（GUI 的 tauri crate）需要 `gui/src-tauri/bin/` 下已存在 sidecar 文件；先跑一次 `bun run prepare:sidecars`。

---

## ✅ 测试

```powershell
cargo test --workspace --exclude afterchat-converter
```

`afterchat-converter`（GUI）需要 sidecar 才能构建，所以测试时排除；它本身没有单元测试，靠手动拖拽验证。

`crates/afterchat-chatformat` 覆盖转义、命名、时间、排序、重名与失败报告；每个转换器都有自己的 `tests/integration_cli.rs`，用合成 fixture 跑完整 CLI 并断言 ZIP 内容。

---

## 🔄 CI

### Push 测试（`.github/workflows/ci.yml`）

push 到 `master`、提 PR 或手动触发时运行：

1. `cargo fmt --all -- --check`
2. 检查 GUI 三处版本号是否一致（`gui/package.json`、`gui/src-tauri/Cargo.toml`、`gui/src-tauri/tauri.conf.json`）
3. `bun install --frozen-lockfile`（确保 `gui/bun.lock` 没漂）
4. `cargo test --workspace --exclude afterchat-converter`

### 自动发布（`.github/workflows/release.yml`）

**发布版本号的唯一来源是 `gui/src-tauri/tauri.conf.json` 的 `package.version`。**

发布步骤：

1. 把三个文件的版本号改成同一个新版本：
   - `gui/package.json` → `version`
   - `gui/src-tauri/Cargo.toml` → `[package] version`
   - `gui/src-tauri/tauri.conf.json` → `package.version`
2. commit 并 push 到 `master`
3. CI 检查 `v<version>` 标签是否已存在：
   - **已存在** → 跳过，什么都不做
   - **不存在** → 构建 GUI + sidecar → 自动打 `v<version>` 标签 → 创建 Release
4. 上传的产物：
   - `afterchat-converter_<version>_x64-setup.exe` — GUI 安装包（NSIS）
   - `afterchat-converters-<version>-windows-x64.zip` — 5 个命令行转换器打包

> 发布只用仓库内置的 `GITHUB_TOKEN`，不需要任何跨仓库 token；仓库私有或公开都能用。

---

## 🔧 Monorepo 维护

### 新增一个转换器

1. 在 `crates/afterchat-<name>` 下创建 crate（`Cargo.toml` + `src/`），依赖 `chatformat = { package = "afterchat-chatformat", path = "../afterchat-chatformat" }`。
2. 加入根 `Cargo.toml` 的 `members`。
3. 在 `gui/scripts/prepare-sidecars.mjs` 的 `binaries` 数组登记 `{ pkg, bin }`。
4. 在 `gui/src-tauri/tauri.conf.json` 的 `tauri.bundle.externalBin` 追加 `bin/<bin>`。
5. 在 `gui/src-tauri/src/main.rs` 的 `ConverterKind`、`ALL_CONVERTERS`、sidecar 名称匹配中登记。
6. 在 `gui/src/main.js` 增加文件名路由与输出类型判断（参考现有分支）。
7. 更新 `gui/src/index.html` 的支持格式列表。
8. 补一份 `docs/converters/<name>.md`，并在 `docs/ARCHITECTURE.md` 的映射表里加一行。
