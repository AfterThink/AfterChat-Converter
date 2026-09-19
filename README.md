# AfterChat Converter

把各家 AI 对话备份转换为 **AfterChat 对话 Markdown / ZIP** 的转换器集合，并附带一个 Tauri 桌面 GUI。

本仓库是 monorepo：所有转换器与 GUI 共用一个 Cargo workspace 与 `Cargo.lock`，输出格式由公共库 `chatformat` 统一保证。

## 仓库结构

```
Cargo.toml            workspace 根
crates/
  afterchat-chatformat/  输出契约的公共实现（纯 lib，无 I/O 副作用）
  afterchat-ai-studio/   Google AI Studio  → Markdown
  afterchat-cherry/      Cherry Studio     → Markdown ZIP
  afterchat-qwen/        Qwen 网页版       → Markdown ZIP
  afterchat-claude/      Claude            → Markdown ZIP
  afterchat-rikka/       RikkaHub (SQLite) → Markdown ZIP
gui/                     Tauri + Bun 桌面 GUI（sidecar 调用上面的转换器）
docs/                    契约、架构与各转换器说明
```

## 转换器

| crate | 二进制 | 输入 | 输出 | 说明 |
| --- | --- | --- | --- | --- |
| `afterchat-chatformat` | — | — | — | 渲染 / 命名 / 时间 / ZIP 打包的公共实现 |
| `afterchat-ai-studio` | `ai-studio` | Google AI Studio 导出的 JSON | 单个 `.md` 或 `.md` 目录树 | |
| `afterchat-cherry` | `cherry` | Cherry Studio 备份（JSON / 含 `data.json` 的 ZIP） | Markdown ZIP | |
| `afterchat-qwen` | `qwen` | Qwen 网页版导出的 JSON | Markdown ZIP | |
| `afterchat-claude` | `claude` | Claude 导出的 JSON / ZIP / 目录 | Markdown ZIP | |
| `afterchat-rikka` | `rikka` | RikkaHub 备份（内含 SQLite） | Markdown ZIP | |

全部转换器都已对齐 AfterChat ChatFormat：

```powershell
cargo run -p afterchat-rikka -- path\to\backup.zip
cargo run -p afterchat-claude -- path\to\data-export.zip
```

## GUI

`gui/` 是基于 Tauri + Bun 的拖拽式桌面应用：把备份文件拖进窗口，它会按文件名自动路由到对应转换器，并把结果写到「输出目录」。

## 文档

| 文档 | 内容 |
| --- | --- |
| [docs/CHATFORMAT.md](./docs/CHATFORMAT.md) | 输出契约（§1–§7），改行为前必读 |
| [docs/ARCHITECTURE.md](./docs/ARCHITECTURE.md) | 仓库分层、`chatformat` 职责、新增转换器的步骤 |
| [docs/GUI.md](./docs/GUI.md) | GUI 的功能与转换器路由规则 |
| [docs/TARGET.md](./docs/TARGET.md) | 项目目标与验收口径 |
| [docs/converters/](./docs/converters/) | 各备份格式的输入结构与实现细节 |

## 构建

```powershell
# 全部转换器（CLI）
cargo build --release

# 桌面 GUI（含打包）
cd gui
bun install
bun run build
```

详见 [BUILD.md](./BUILD.md)。
