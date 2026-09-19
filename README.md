# AfterChat Converter

把各家 AI 对话备份转换为 **AfterChat 对话 Markdown / ZIP** 的转换器集合，并附带一个 Tauri 桌面 GUI。

本仓库是 monorepo：所有转换器与 GUI 都在这里，共享同一个 Cargo workspace 与 `Cargo.lock`。

## 转换器

| 目录 | 二进制 | 输入 | 输出 |
| --- | --- | --- | --- |
| `converter/ai-studio` | `ai-studio` | Google AI Studio 导出的 JSON | Markdown |
| `converter/cherry` | `cherry` | Cherry Studio 备份（JSON / 含 `data.json` 的 ZIP） | Markdown ZIP |
| `converter/qwen` | `qwen` | Qwen 网页版导出的 JSON | AfterChat Markdown / ZIP |
| `converter/claude` | `claude` | Claude 导出的 JSON / ZIP / 目录 | Markdown |
| `converter/rikka` | `rikka` | RikkaHub 备份（内含 SQLite） | AfterChat Markdown / ZIP |

各转换器的输入细节与输出格式见其目录下的 `README.md` / `SPEC.md`。`qwen`、`cherry`、`rikka` 已对齐 AfterChat ChatFormat，`ai-studio`、`claude` 正在对齐中。

## GUI

`converter/gui` 是基于 Tauri + Bun 的拖拽式桌面应用：把备份文件拖进窗口，它会根据文件名自动路由到对应转换器。

## 构建

```powershell
# 全部转换器（CLI）
cargo build --release

# 桌面 GUI（含打包）
cd converter/gui
bun install
bun run build
```

详见 [BUILD.md](./BUILD.md)。
