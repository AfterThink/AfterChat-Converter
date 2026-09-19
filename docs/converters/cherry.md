# cherry 转换器规格

> 输出契约定义参见 [`../CHATFORMAT.md`](../CHATFORMAT.md)。

本工具用于将 Cherry Studio 的备份文件（JSON 文件或包含 `data.json` 的 ZIP 压缩包）转换为规范的 Markdown 文档。

转换结果直接打包为**单个 ZIP 归档包**：包内各对话保存为独立的 `.md` 文件，按助手名称建立子目录，并依据对话发生时间完成文件命名与时间戳设置。

---

## 功能特性

- **高效并行处理**：基于 Rayon 实现多线程并发解析与渲染；
- **自动打包归档**：直接输出 `chat-export-cherry-all-<时间戳>.zip` 归档包；
- **保留助手目录层级**：ZIP 内采用 `<助手名>/` 分组目录，与 Cherry Studio 组织方式对齐；
- **提取完整元数据**：提取助手名称、模型标识与系统提示词（System Prompt）；
- **规范化文件命名**：条目命名为 `<助手名>/<YYYYMMDD-HHmmss>-<标题>.md`，自动过滤非法字符并处理重名冲突；
- **保留原始时间戳**：ZIP 内各条目的修改时间设为对话实际发生时间，便于基于时间归档；
- **支持思维链**：完整提取并结构化展示模型思维链（Thought Process）；
- **遵循统一契约**：严格遵循 [`../CHATFORMAT.md`](../CHATFORMAT.md) 格式规范。

---

## 使用方法

### 1. 编译构建

```powershell
cargo build --release -p afterchat-cherry
```

编译产物位于 `target/release/cherry.exe`。

### 2. 命令行调用

支持直接传入 `JSON` 备份文件或 Cherry 导出的 `ZIP` 压缩包（自动读取内部的 `data.json`）。默认将导出的 ZIP 保存至源文件同级目录。

```powershell
# 基本用法
cherry path/to/backup.json
cherry path/to/cherry-backup.zip

# 指定输出目录（自动按规范命名生成 ZIP 文件）
cherry -o my_exports path/to/backup.json

# 显式指定输出 ZIP 文件名
cherry -o 2026年备份.zip path/to/backup.json
```

---

## 输出目录结构

转换产物为单个 ZIP 归档文件：

```
chat-export-cherry-all-1789794167679.zip
├── 默认助手/
│   ├── 20250329-204231-对话标题.md
│   └── 20260423-075546-另一个标题.md
├── Gemini 2.5 Pro/
│   └── 20260213-124758-对话标题.md
└── export-failures.md          # 仅当存在无有效消息的主题时生成
```

- ZIP 命名规则遵循 `docs/CHATFORMAT.md` §6：`chat-export-{platform}-all-{毫秒时间戳}.zip`；
- 条目命名：`<助手名>/<YYYYMMDD-HHmmss>-<标题>.md`（按对话时间排序）；
- 条目重名时自动追加 `-2`、`-3` 等后缀；
- 条目时间戳对应对话时间（受 ZIP DOS 时间格式限制，精度为 2 秒）。

---

## Markdown 内容结构

每份 `.md` 文件的结构示例如下：

```markdown
## Metadata

### Run Settings

- **Model:** `gpt-4-turbo`
- **Time:** 2024-03-28 21:31:51 +08:00
- **Topic ID:** `...`
- **Assistant:** `默认助手`

## Conversation

### ⚙️ System

系统提示词内容。

### 🧑‍💻 User

用户输入内容。

### 🤖 Assistant

#### 🤔 Thought Process

助手的思维链内容。

#### 💡 Response

助手的回复内容。
```

### 消息体处理规则

- 保持 Markdown 原始排版（代码块、列表、引用等）；
- 正文中的 `#`/`##`/`###` 标题统一转换为 `**加粗**`；
- **标题内层加粗吸收**：标题中原有的 `**` 会被吸收（例如 `## 方案一：**“…”**` 转换为 `**方案一：“…”**`），规避 CommonMark 定界符错配；行内代码内的 `**` 保持原样；
- **代码围栏保护**：```` ``` ```` 或 `~~~` 内的代码不执行任何标题转换；
- 助手消息根据 `thinking` 与正文块分别输出至 `#### 🤔 Thought Process` 与 `#### 💡 Response`。

---

## Python 参考实现

`python/convert-json-to-markdowns.py` 提供同等业务逻辑的 Python 参考实现，其输入与输出行为与 Rust 版本保持一致（在相同备份输入及参数下产出逐字节相同的 ZIP 归档），可作为免编译环境下的补充工具：

```bash
# 默认读取当前目录下的 data.json
python python/convert-json-to-markdowns.py

# 指定输入与输出路径
python python/convert-json-to-markdowns.py backup.json -o out/
python python/convert-json-to-markdowns.py backup.zip  -o 2026备份.zip
```

运行环境要求：Python 3.8 及以上（仅依赖标准库）。
