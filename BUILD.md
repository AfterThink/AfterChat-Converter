# 构建与开发指南

本仓库是一个 **Cargo Workspace monorepo**：所有转换器（`converter/*`）与 Tauri GUI（`converter/gui`）都在同一个仓库里。GUI 会自动把各转换器编译成 sidecar 并一起打包。

---

## 🚀 快速开始 (推荐开发流程)

如果你主要进行 GUI 开发，**只需要**执行：

1. **安装依赖 (仅首次)**:
   ```powershell
   cd converter/gui
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
| `converter/ai-studio` | Google AI Studio 转换器，二进制 `ai-studio` |
| `converter/cherry` | Cherry Studio 备份转换器，二进制 `cherry` |
| `converter/qwen` | Qwen 转换器，二进制 `qwen` |
| `converter/claude` | Claude 转换器，二进制 `claude` |
| `converter/rikka` | RikkaHub 备份转换器，二进制 `rikka` |
| `converter/gui` | Tauri + Bun 桌面 GUI，通过 Sidecar 调用上面的转换器 |

- **Workspace**: 根目录的 `Cargo.toml` 把所有转换器与 `converter/gui/src-tauri` 列为成员，共享同一个 `Cargo.lock` 与 `target/`。
- 每个转换器仍然是独立的 crate 和独立可执行文件，可以单独 `cargo run -p <pkg> -- <input>` 使用，也可以把文件直接拖到单个 exe 上。

---

## 📦 进阶构建

### 整体构建 (命令行版本)
```powershell
cargo build --release
```

### 构建 GUI 发布包
```powershell
cd converter/gui
bun run build
```

---

## 🔄 Monorepo 维护

### 新增一个转换器

1. 在 `converter/<name>` 下创建 crate（`Cargo.toml` + `src/`）。
2. 加入根 `Cargo.toml` 的 `members`。
3. 在 `converter/gui/scripts/prepare-sidecars.mjs` 的 `binaries` 数组登记 `{ pkg, bin }`。
4. 在 `converter/gui/src-tauri/tauri.conf.json` 的 `tauri.bundle.externalBin` 追加 `bin/<bin>`。
5. 在 `converter/gui/src-tauri/src/main.rs` 的 `ConverterKind`、`ALL_CONVERTERS`、sidecar 名称匹配中登记。
6. 在 `converter/gui/src/main.js` 增加文件名路由与输出类型判断（参考现有分支）。
7. 更新 `converter/gui/src/index.html` 的支持格式列表。

### 发布

打 `v*` tag 会触发 `.github/workflows/release.yml`，CI 自动编译全部 sidecar 并打包 Windows 安装包。因为是 monorepo，**不需要任何跨仓库 token**。
