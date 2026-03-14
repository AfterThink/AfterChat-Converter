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
    *   读取文件头或目录内容以检测格式。
    *   将 `InputInfo`（包括检测到的 `ConverterKind`）返回给前端。
4.  **前端**：更新 UI 以显示“就绪”状态（或错误）。
5.  **用户操作**：用户点击“转换”（由拖放操作隐含，或如果需要确认），或者如果设计为自动处理则直接开始（目前，如果有效，拖放/检测后立即处理）。
6.  **后端**：调用 `run_conversion`。它构建参数并生成相应的侧车程序进程。
7.  **侧车程序**：读取输入，进行转换，并写入输出。
8.  **后端**：捕获 stdout/stderr/退出码并将结果返回给前端。
9.  **前端**：显示成功或错误消息，并提供“显示文件位置 (Reveal)"选项。

## 3. 功能

### 3.1 拖放界面
*   **拖放区域 (Drop Zone)**：整个应用程序窗口作为拖放区域。
*   **视觉反馈**：
    *   **就绪 (Ready)**：初始状态，等待输入。
    *   **悬停 (Hovering)**：当文件拖过窗口时的视觉提示。
    *   **处理中 (Processing)**：转换期间的 Spinner/加载状态。
    *   **成功 (Success)**：绿色对勾，带有“显示文件位置”选项。
    *   **错误 (Error)**：红色叉号，带有错误详情。

### 3.2 格式检测 (`inspect_input`)
后端根据文件内容（JSON 结构）或目录特征自动检测输入格式。

*   **Google AI Studio**：如果 JSON 对象包含键：`runSettings`、`chunkedPrompt` 或 `systemInstruction`，则检测到。
*   **Cherry Studio**：如果 JSON 对象包含键：`indexedDB` 或 `localStorage`，则检测到。
*   **Qwen**：
    *   **文件**：如果 JSON 对象包含键：`data`（数组）、`chat`、`messages`、`meta`、`chat_type` 或 `sub_chat_type`，则检测到。如果根节点是对象的 JSON 数组，也会被检测到。
    *   **目录**：如果拖放的是目录，检查其是否包含 `.json` 文件。默认使用 `Qwen` 转换器进行批量处理。

### 3.3 转换过程 (`run_conversion`)
执行相应的侧车程序二进制文件：
*   `google-ai-studio-json-converter`
*   `cherry-studio-backup-json-converter`
*   `qwen-json-converter`

### 3.4 输出管理
*   **同目录保存 (In-Place)**：输出文件保存在与源文件相同的目录中。
*   **自定义位置 (Custom Location)**：*在后端 `predict_output_path` 逻辑中实现，由前端切换/选择支持。*
*   **显示文件位置 (Reveal)**：“显示”按钮打开文件资源管理器并定位到输出文件/文件夹位置。

### 3.5 国际化 (i18n)
*   支持 **英文 (en)** 和 **中文 (zh)**。
*   UI 中包含语言切换。
*   持久化语言偏好设置。

### 3.6 窗口配置
*   **尺寸**：固定大小 (360x400)。
*   **样式**：无边框 (decorations: false)，背景透明。
*   **行为**：不可调整大小，启动时居中。

## 4. 技术实现细节

### 4.1 项目结构
```
converter/gui/
├── src/                # 前端源码 (HTML/JS/CSS)
├── src-tauri/          # 后端源码 (Rust)
│   ├── src/main.rs     # 主应用程序逻辑
│   ├── tauri.conf.json # Tauri 配置
│   └── bin/            # 编译后的侧车程序二进制文件位置
├── scripts/            # 构建和工具脚本
│   └── prepare-sidecars.mjs # 构建并复制侧车程序的脚本
└── package.json        # Node 依赖和脚本
```

### 4.2 构建系统
*   **侧车程序准备**：`prepare-sidecars.mjs` 脚本至关重要。它：
    1.  检测主机架构（Rust 目标三元组）。
    2.  使用 `cargo build` 从工作区 (`../ai-studio`, `../cherry`, `../qwen`) 构建转换器二进制文件。
    3.  将二进制文件复制并重命名到 `src-tauri/bin/`，遵循 Tauri 要求的 `<name>-<target-triple>` 命名约定。
*   **Tauri 构建**：`tauri build` 将前端和准备好的侧车程序捆绑到单个安装程序/可执行文件中。

### 4.3 侧车程序参数
后端为侧车程序构建命令行参数：
*   **通用**：输入路径作为位置参数或通过 `-i` 传递。输出路径通过 `-o` 传递。
*   **Qwen 特定**：传递 `--progress false` 以在 GUI 执行期间禁用 CLI 进度条。

### 4.4 数据结构 (IPC)

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
    converter: ConverterKind,
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
    converter_kind: Option<ConverterKind>,
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

## 5. 支持的格式

| 格式 | 检测键 (Detector Keys) | 侧车程序二进制文件 (Sidecar Binary) |
| :--- | :--- | :--- |
| **Google AI Studio** | `runSettings`, `chunkedPrompt`, `systemInstruction` | `google-ai-studio-json-converter` |
| **Cherry Studio** | `indexedDB`, `localStorage` | `cherry-studio-backup-json-converter` |
| **Qwen** | `data` (array), `chat`, `messages`, `meta`, `chat_type`, `sub_chat_type`, 或数组根节点 | `qwen-json-converter` |

## 6. 未来考量
*   **进度追踪**：目前，`Qwen` 进度已禁用。实现从 stdout 解析实时进度将改善大批量转换的用户体验 (UX)。
*   **配置**：特定转换器选项（例如 Qwen 的特定标志）的 UI 目前较为精简。