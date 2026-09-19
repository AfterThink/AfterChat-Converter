# AfterChat Converter 桌面客户端 (GUI)

基于 Tauri + Bun 构建的桌面端图形化应用，通过拖拽操作调用底层的 CLI 转换器：

- `ai-studio` (Google AI Studio)
- `cherry` (Cherry Studio)
- `qwen` (通义千问)
- `claude` (Claude)
- `rikka` (RikkaHub)

---

## 核心功能

- **拖放即转换**：文件或目录拖入窗口后立即启动转换处理；
- **智能路由分发**：
  - 文件名包含 `cherry` → 调用 `cherry`
  - 文件名包含 `qwen` → 调用 `qwen`
  - 文件名包含 `claude` → 调用 `claude`
  - 文件名包含 `rikka` → 调用 `rikka`
  - 其他情况 → 默认调用 `ai-studio` 或轮询探测
- **灵活的输出管理**：默认保存至原文件同级目录；取消勾选后自动调用原生目录选择对话框；
- **设置持久化**：自动记录上一次的输出偏好与界面语言；
- **集成导入模式**：通过 `-o` 启动参数指定输出目录，自动适配外部调用工作流。

---

## 环境要求

- [Bun](https://bun.sh/)
- Rust 工具链 (`rustc` / `cargo`)
- Windows 环境须具备 WebView2 Runtime

---

## 开发与构建

### 1. 安装前端依赖

```powershell
cd gui
bun install
```

> 目录中已配置 `.npmrc` 使用 npmmirror 镜像源以优化依赖下载。

### 2. 本地开发启动

```powershell
cd gui
bun run dev
```

该命令自动执行以下步骤：
1. 编译各 Rust 转换器至工作区 `target/`；
2. 同步 Sidecar 二进制至 `gui/src-tauri/bin/`；
3. 启动 Tauri 开发窗口（加载原生 HTML/JS/CSS）。

### 3. 生产发布构建

```powershell
cd gui
bun run build
```

该命令自动编译 release 模式的 Sidecar 二进制并生成桌面端安装包，产物输出至工作区 `target/release/bundle/` 目录。

---

## 高级运行模式与参数

### 导入模式（CLI `-o`）

通过命令行参数 `-o` / `--output` 启动时，客户端进入导入模式：

```powershell
afterchat-converter.exe -o C:\target\output
```

导入模式下的行为特性：
- 隐藏“保存在原文件同级目录”选项；
- 引导文案切换为“拖拽文件以导入”；
- 拖入文件后直接输出至 `-o` 指定的路径，不弹出保存目录对话框。

### 指定界面语言（CLI `-l`）

通过 `-l` / `--lang` 参数指定界面启动语言：

```powershell
afterchat-converter.exe -l zh
afterchat-converter.exe -l en
```

支持参数值：`zh`（简体中文）、`en`（英文）。未显式指定时跟随系统语言或沿用上次设置。

---

## 常见问题排查

### 1. Tauri 构建提示缺失 Sidecar 二进制

可手动执行预处理脚本编译并同步 Sidecar：

```powershell
cd gui
bun run prepare:sidecars:release
```

执行后确认 `gui/src-tauri/bin/` 目录下已生成包含目标架构标识的可执行文件。

### 2. Rust 依赖下载缓慢或超时

Tauri 后端依赖由 Cargo 管理，需配置 Cargo 国内镜像源（如 crates.io 镜像），与前端 npm/bun 源独立。
