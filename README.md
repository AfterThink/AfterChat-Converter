# AfterChat Converter

<p align="center">
  <img src="https://img.shields.io/badge/Rust-2024-orange?style=for-the-badge&logo=rust" alt="Rust 2024" />
  <img src="https://img.shields.io/badge/Tauri-1.x-24C8DB?style=for-the-badge&logo=tauri" alt="Tauri" />
  <img src="https://img.shields.io/badge/Converters-5-success?style=for-the-badge" alt="5 converters" />
  <img src="https://img.shields.io/badge/Platform-Windows-0078D6?style=for-the-badge&logo=windows" alt="Windows" />
</p>

把各家 AI 服务的**对话备份**转换成统一的 **AfterChat Markdown / ZIP**，用于个人备份、迁移和离线阅读。

支持 **5 种备份来源**：Google AI Studio、Cherry Studio、Qwen、Claude、RikkaHub。
所有转换器共用同一套输出契约 [ChatFormat](./docs/CHATFORMAT.md)，导出的 Markdown 可以直接放进 [AfterChat](https://github.com/AfterThink/AfterChat-App-Download) 工作区。

> 在线对话用 [AfterChat — LLM Chat Exporter](https://github.com/AfterThink/AfterChat-Script) 一键导出；本地已有的备份文件交给本仓库。

## 支持的备份

| 来源 | 输入 | 输出 |
| --- | --- | --- |
| Google AI Studio | 导出的 JSON | Markdown（单文件，或目录树） |
| Cherry Studio | 备份 JSON / 含 `data.json` 的 ZIP | Markdown ZIP |
| Qwen | 网页版导出的 JSON（可一次传多个） | Markdown ZIP |
| Claude | 数据导出 ZIP / JSON / 目录 | Markdown ZIP |
| RikkaHub | 备份 ZIP，或裸 `.db`（SQLite） | Markdown ZIP |

转换后的结构：

```
chat-export-claude-all-1730000000000.zip
├── 20241114-101500-第一次对话.md
├── 20241113-090000-第二次对话.md
└── export-failures.md          # 没有任何可导出消息的对话会记在这里
```

按助手分目录的平台（Cherry / Qwen / RikkaHub）会多一层：

```
chat-export-rikka-all-1730000000000.zip
└── Gemini/
    └── 20241114-101500-第一次对话.md
```

## 下载

到 [Releases](https://github.com/AfterThink/AfterChat-Converter/releases) 下载：

| 文件 | 说明 |
| --- | --- |
| `afterchat-converter_<版本>_x64-setup.exe` | 桌面应用，拖拽即转 |
| `afterchat-converters-<版本>-windows-x64.zip` | 5 个命令行转换器，解压即用 |

## 用法

### 桌面应用

1. 打开应用，把备份文件（或整个文件夹）拖进窗口
2. 程序按文件名自动选择对应的转换器
3. 完成后，`chat-export-<平台>-all-<时间戳>.zip` 会出现在源文件旁边

窗口底部可以勾选「输出到自定义目录」。具体支持格式与路由规则见 [docs/GUI.md](./docs/GUI.md)。

### 命令行

解压 `afterchat-converters-<版本>-windows-x64.zip`，里面有 5 个可执行文件：

```powershell
rikka     RikkaHub-backup.zip        # RikkaHub 备份（自动解压并读 SQLite）
cherry    cherry-backup.zip          # Cherry Studio 备份
qwen      qwen-all.json              # Qwen 导出（可以跟多个文件）
claude    data-export.zip            # Claude 数据导出
ai-studio prompt.json                # Google AI Studio 导出
```

默认输出到**源文件同目录**，也可以用 `-o` 指定输出目录或输出文件：

```powershell
rikka backup.zip -o out\              # 写到 out\chat-export-rikka-all-<时间戳>.zip
rikka backup.zip -o out\my-name.zip   # 直接指定 zip 文件名
```

把文件拖到单个 exe 上也能用（`ai-studio` 除外，它输出 Markdown 单文件，建议用命令行）。

## 特性

- **统一输出**：渲染、命名、时间、ZIP 打包全部由公共库 [`chatformat`](./crates/afterchat-chatformat) 完成，5 个转换器的输出结构逐字节一致。
- **保留思维链**：模型的思考过程写成 `#### 🤔 Thought Process`，正文写成 `#### 💡 Response`。
- **不破坏内容**：Markdown 里的 `#` 标题会转成 `**加粗**`，但代码围栏、行内代码里的 `#` 和 `**` 原样保留。
- **空对话不丢**：没有可导出消息的会话不会生成空文件，而是汇总进包内 `export-failures.md`。
- **出错不中断**：单个会话转换失败不影响其它会话，进程仍以 0 退出。

## 输出格式

完整契约见 [docs/CHATFORMAT.md](./docs/CHATFORMAT.md)：

| 章节 | 内容 |
| --- | --- |
| §1–§2 | 顶层结构、Metadata 键（`Model` / `Time` / `URL` + 平台扩展） |
| §3–§4 | 消息结构（System / User / Assistant、思考 / 回复）、角色兜底 |
| §5 | 行内语法：`#` → `**加粗**`，代码原样保留 |
| §6 | ZIP 打包：条目命名、时间降序、`export-failures.md` |
| §7 | 文件名：非法字符清理、长度上限、空标题兜底 |

## 文档

| 文档 | 内容 |
| --- | --- |
| [docs/CHATFORMAT.md](./docs/CHATFORMAT.md) | 输出契约，改行为前必读 |
| [docs/ARCHITECTURE.md](./docs/ARCHITECTURE.md) | 仓库分层、`chatformat` 职责、新增转换器的步骤 |
| [docs/GUI.md](./docs/GUI.md) | 桌面应用的功能与转换器路由 |
| [docs/converters/](./docs/converters/) | 各备份格式的输入结构与实现细节 |
| [docs/BUILD.md](./docs/BUILD.md) | 本地构建、CI 与发布流程 |

## 构建

需要 Rust（stable）与 [Bun](https://bun.sh/)：

```powershell
# 5 个命令行转换器
cargo build --release

# 桌面应用（会自动把转换器编成 sidecar 一起打包）
cd gui
bun install
bun run build
```

发布是全自动的：把 `gui/src-tauri/tauri.conf.json` 的 `version` 改成新版本并 push 到 `master`，CI 就会自动跑测试、打 tag、构建并上传安装包与转换器 zip。详见 [docs/BUILD.md](./docs/BUILD.md)。
