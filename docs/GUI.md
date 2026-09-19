# 转换器桌面端技术规范 (GUI SPEC)

## 1. 概述

**AfterChat Converter GUI** 是基于 [Tauri](https://tauri.app/) 构建的桌面端应用程序，提供图形化交互界面，用于将主流大语言模型（LLM）对话备份转换为统一规范的 Markdown 与 ZIP 归档。桌面端通过 Sidecar 机制调用各平台的独立 CLI 转换工具完成实际数据解析。

## 2. 架构设计

应用程序采用标准 Tauri 架构：

*   **前端 (Frontend)**：使用原生 JavaScript、HTML 与 CSS 构建，负责交互响应、拖拽事件监听与 UI 状态管理，通过 Tauri IPC 机制与后端通信。
*   **后端 (Backend, Rust)**：Tauri 核心主进程 (`src-tauri`) 管理窗口生命周期、文件系统 I/O，并负责调度与监控 Sidecar 二进制子进程。
*   **Sidecar 子进程**：各平台独立的 CLI 转换器，随桌面端应用一同编译和打包分发。

### 2.1 交互流程

1.  **用户拖拽**：用户将备份文件或文件夹拖入应用窗口；
2.  **前端捕获**：监听拖拽事件并触发 `inspect_input` 命令；
3.  **后端分析**：
    *   检查输入路径属性（文件或目录）；
    *   若为目录，递归探测是否包含 `.json` 等待处理数据；
    *   将 `InputInfo` 结构返回前端；
4.  **状态更新**：前端更新界面展示为就绪状态；
5.  **触发转换**：前端发起 `run_conversion` 请求；
6.  **后端调度**：
    *   若请求中明确指定了 `converter`，则仅调用该转换器；
    *   若未指定，则按预设顺序尝试匹配各转换器（AiStudio -> Cherry -> Qwen -> Claude -> Rikka），以首个返回退出码 0 的结果作为成功响应；
    *   组装执行参数并调用对应 Sidecar；
7.  **执行转换**：Sidecar 转换器执行数据解析并写入结果文件；
8.  **结果返回**：后端捕获标准输出与退出状态，将结果返回前端；
9.  **完成提示**：前端展示处理结果，并提供“在文件管理器中显示”操作入口。

## 3. 功能特性

### 3.1 启动参数与配置
*   **命令行参数**：支持通过 `-o`/`--output` 指定默认输出路径，通过 `-l`/`--lang` 指定启动界面语言；
*   **配置读取**：前端启动时调用 `get_launch_config` 获取初始配置参数。

### 3.2 拖拽交互
*   **拖拽区域 (Drop Zone)**：整个应用窗口均支持作为拖拽受击区；
*   **视觉状态机**：
    *   **就绪 (Ready)**：初始等待输入状态；
    *   **悬停 (Hovering)**：文件拖入窗口上方时的即时视觉反馈；
    *   **处理中 (Processing)**：转换执行期间的加载指示；
    *   **成功 (Success)**：转换完成提示，并提供定位文件选项；
    *   **失败 (Error)**：错误提示及详细错误信息展示。

### 3.3 输入检测 (`inspect_input`)
后端检查输入路径是否为有效的数据文件或包含备份文件的目录，不再在检测阶段做硬编码字段匹配，具体的格式辨识交由 `run_conversion` 阶段通过转换器自身校验。

### 3.4 转换调度 (`run_conversion`)
*   **自动匹配机制**：后端按注册顺序依次调用各 Sidecar 转换器，直至匹配能够处理该数据的工具；
*   **异步调用**：通过 Tauri 的异步命令执行机制启动 Sidecar，避免阻塞 UI 线程。

### 3.5 输出管理
*   **输出路径预测**：后端根据输入类型与用户设置预测最终输出路径；
*   **定位文件 (Reveal)**：跨平台支持在系统文件管理器（Windows 资源管理器 / macOS Finder / Linux 文件管理器）中定位并高亮目标文件。

### 3.6 国际化 (i18n)
*   内置支持 **英文 (en)** 与 **简体中文 (zh)**；
*   支持界面语言动态切换，并自动持久化用户偏好。

### 3.7 窗口规范
*   **尺寸**：固定大小 (360x400)；
*   **样式**：无边框窗口 (decorations: false)，支持毛玻璃与圆角效果；
*   **行为**：禁止手动缩放，启动默认居中。

## 4. 技术实现细节

### 4.1 项目结构
```
gui/
├── src/                # 前端源码 (HTML/JS/CSS)
│   ├── index.html
│   ├── main.js
│   └── styles.css
├── src-tauri/          # 后端源码 (Rust)
│   ├── src/main.rs     # Tauri 主程序逻辑
│   ├── tauri.conf.json # Tauri 配置文件
│   └── bin/            # 编译生成的 Sidecar 二进制存放目录
├── scripts/            # 构建与预处理脚本
│   └── prepare-sidecars.mjs # 编译并同步 Sidecar 二进制
└── package.json        # 前端依赖与脚本配置
```

### 4.2 构建与打包
*   **Sidecar 预处理**：`prepare-sidecars.mjs` 脚本负责：
    1.  获取当前构建平台 Target Triple；
    2.  调用 `cargo build` 编译工作区中各转换器；
    3.  将生成的可执行文件按 `<name>-<target-triple>` 命名格式同步至 `src-tauri/bin/` 目录；
*   **完整打包**：执行 `tauri build` 将前端资源与 Sidecar 统一打包为安装包或独立二进制。

### 4.3 Sidecar 启动参数
后端将输入文件路径作为位置参数传递给 Sidecar；若用户指定了输出目录，则通过 `-o` 选项附加传递。

### 4.4 IPC 数据结构

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
    Claude,
    Rikka,
}
```

**ConvertRequest (前端 -> 后端)**:
```rust
struct ConvertRequest {
    converter: Option<ConverterKind>, // 可选，省略则自动匹配
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

## 5. 支持格式与 Sidecar 映射

| 平台 | Sidecar 标识 | 对应二进制名称 |
| :--- | :--- | :--- |
| **Google AI Studio** | `ai-studio` | `ai-studio` |
| **Cherry Studio** | `cherry` | `cherry` |
| **Qwen** | `qwen` | `qwen` |
| **Claude** | `claude` | `claude` |
| **RikkaHub** | `rikka` | `rikka` |
