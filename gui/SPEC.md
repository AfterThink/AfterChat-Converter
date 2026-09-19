# 转换器 GUI SPEC

## 1. 概述

**Converters GUI** 是一款基于 [Tauri v2](https://tauri.app/) 构建的跨平台桌面应用程序。它提供了一个统一、友好的用户界面，用于将各种大语言模型（LLM）对话历史格式转换为 Markdown。该应用程序封装了多个命令行界面（CLI）工具（侧车程序/Sidecars）以执行实际的转换操作。

## 2. 架构

应用程序遵循标准的 Tauri 架构：

*   **前端 (Frontend)**：使用原生 JavaScript、HTML 和 CSS 构建。负责处理用户交互、拖放事件和状态管理。它通过 Tauri 的 IPC（进程间通信）机制与后端通信。
*   **后端 (Backend, Rust)**：Tauri 核心进程 (`src-tauri`) 管理应用程序窗口、文件系统交互，并协调侧车程序二进制文件的执行。
*   **侧车程序 (Sidecars)**：执行特定格式转换的独立可执行文件。这些文件与应用程序一起打包。

### 2.1 组件交互

1.  **用户操作**：用户将文件或文件夹拖放到应用程序窗口上。
2.  **前端**：捕获拖放事件并调用 `inspect_input` Tauri 命令。
3.  **后端**：
    *   分析输入路径（文件或目录）。
    *   如果是目录，递归检查是否包含 `.json` 文件。
    *   将 `InputInfo` 返回给前端。
4.  **前端**：更新 UI 以显示“就绪”状态。
5.  **用户操作/自动触发**：前端调用 `run_conversion`。
6.  **后端**：
    *   如果请求中指定了 `converter`，则仅尝试该转换器。
    *   如果未指定，则按顺序尝试所有转换器（AiStudio -> Cherry -> Qwen -> Claude -> Rikka），第一个返回退出码 0 的视为成功。
    *   构建参数并执行侧车程序。
7.  **侧车程序**：执行实际转换。
8.  **后端**：捕获输出并将结果返回。
9.  **前端**：显示结果并提供“显示文件位置 (Reveal)"选项。

## 3. 功能

### 3.1 启动配置与参数
*   **命令行参数**：支持 `-o`/`--output` 指定默认输出路径，`-l`/`--lang` 指定启动语言。
*   **配置获取**：前端启动时通过 `get_launch_config` 获取这些初始配置。

### 3.2 拖放界面
*   **拖放区域 (Drop Zone)**：整个应用程序窗口作为拖放区域。
*   **视觉反馈**：
    *   **就绪 (Ready)**：初始状态，等待输入。
    *   **悬停 (Hovering)**：当文件拖过窗口时的视觉提示。
    *   **处理中 (Processing)**：转换期间的 Spinner/加载状态。
    *   **成功 (Success)**：绿色对勾，带有“显示文件位置”选项。
    *   **错误 (Error)**：红色叉号，带有错误详情。

### 3.3 格式检测 (`inspect_input`)
后端现在更通用地检查输入是否为有效的 JSON 文件或包含 JSON 的目录，而不再在 `inspect_input` 阶段进行硬编码的键匹配检测。具体的格式识别由 `run_conversion` 阶段通过尝试不同的侧车程序来完成。

### 3.4 转换过程 (`run_conversion`)
*   **自动尝试机制**：后端会自动轮询所有支持的侧车程序（AiStudio, Cherry, Qwen），直到找到能处理该输入的程序。
*   **侧车程序执行**：通过 Tauri 的 `Command::new_sidecar` 异步调用。

### 3.5 输出管理
*   **预测输出路径**：后端根据输入是文件还是目录，以及是否指定了输出路径，来预测并返回最终的 `output_path`。
*   **显示文件位置 (Reveal)**：支持在不同操作系统（Windows/macOS/Linux）下打开文件管理器并选中目标文件。

### 3.6 国际化 (i18n)
*   支持 **英文 (en)** 和 **中文 (zh)**。
*   UI 中包含语言切换。
*   持久化语言偏好设置。

### 3.7 窗口配置
*   **尺寸**：固定大小 (360x400)。
*   **样式**：无边框 (decorations: false)，背景透明。
*   **行为**：不可调整大小，启动时居中。

## 4. 技术实现细节

### 4.1 项目结构
```
converter/gui/
├── src/                # 前端源码 (HTML/JS/CSS)
│   ├── index.html
│   ├── main.js
│   └── styles.css
├── src-tauri/          # 后端源码 (Rust)
│   ├── src/main.rs     # 主应用程序逻辑
│   ├── tauri.conf.json # Tauri 配置
│   └── bin/            # 编译后的侧车程序二进制文件位置
├── scripts/            # 构建和工具脚本
│   └── prepare-sidecars.mjs # 构建并复制侧车程序的脚本
└── package.json        # Node 依赖和脚本
```

### 4.2 构建与部署
*   **侧车程序准备**：`prepare-sidecars.mjs` 脚本至关重要。它：
    1.  检测主机架构（Rust 目标三元组）。
    2.  使用 `cargo build` 从工作区 (`../ai-studio`, `../cherry`, `../qwen`) 构建转换器二进制文件。
    3.  将二进制文件复制并重命名到 `src-tauri/bin/`，遵循 Tauri 要求的 `<name>-<target-triple>` 命名约定。
*   **本地构建**：`tauri build` 将前端和准备好的侧车程序捆绑到单个安装程序/可执行文件中。
*   **CI/CD (GitHub Actions)**：
    *   配置文件位于 `.github/workflows/release.yml`。
    *   当推送版本标签 (`v*`) 时自动触发。
    *   在 Windows 环境下构建并发布 GitHub Release，包含 NSIS (.exe) 安装包。

### 4.3 侧车程序参数
后端为侧车程序构建命令行参数：输入路径作为位置参数。如果指定了输出路径，则通过 `-o` 传递。

### 4.4 数据结构 (IPC)

**LaunchConfig (后端 -> 前端)**:
```rust
struct LaunchConfig {
    output_path: Option<String>,
    lang: Option<String>,
}
```

**ConverterKind 枚举**:
```rust
enum ConverterKind {
    AiStudio,
    Cherry,
    Qwen,
}
```

**ConvertRequest (前端 -> 后端)**:
```rust
struct ConvertRequest {
    converter: Option<ConverterKind>, // 可选，不指定则自动尝试
    input_path: String,
    output_path: Option<String>,
}
```

**InputInfo (后端 -> 前端)**:
```rust
struct InputInfo {
    path: String,
    name: String,
    is_dir: bool,
}
```

**ConvertResponse (后端 -> 前端)**:
```rust
struct ConvertResponse {
    exit_code: i32,
    stdout: String,
    stderr: String,
    output_path: String,
}
```

## 5. 支持的格式与自动识别

应用程序不再依赖前端或后端的预先硬编码检测逻辑，而是通过**依次尝试执行侧车程序**并检查返回码来实现自动识别。

| 格式 | 侧车程序标识 | 内部二进制名称 |
| :--- | :--- | :--- |
| **Google AI Studio** | `ai-studio` | `google-ai-studio-json-converter` |
| **Cherry Studio** | `cherry` | `cherry-studio-backup-json-converter` |
| **Qwen** | `qwen` | `qwen-json-converter` |
| **Claude** | `claude` | `claude-json-converter` |
| **RikkaHub** | `rikka` | `rikkahub-db-converter` |
