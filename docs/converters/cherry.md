# cherry

> 输出契约见 [`../CHATFORMAT.md`](../CHATFORMAT.md)。

这是一个用 Rust 编写的高效工具，用于将 Cherry Studio 的备份文件（JSON 或包含 `data.json` 的 ZIP）转换为易于阅读的 Markdown 文档。

产出为**一个 ZIP 包**：里面每个对话一份 `.md`，按助手名分目录，并用对话发生时间来命名与标注时间戳。

## ✨ 功能特点

- **高性能转换**：利用 Rust 的并行处理能力（Rayon），快速处理大量对话数据。
- **一次性打包**：每次转换直接产出一个 `chat-export-cherry-all-<时间戳>.zip`，无需再手动整理目录。
- **保留助手结构**：包内按 `<助手名>/` 分目录，与 Cherry Studio 的组织方式一致。
- **元数据保留**：提取并展示助手名称、模型信息、系统提示词（System Prompt）等。
- **智能文件命名**：条目名为 `<助手名>/<YYYYMMDD-HHmmss>-<标题>.md`，自动处理非法字符（包括换行符）与重名。
- **时间时光机**：每个条目的时间属性会被设为对话实际发生的时刻，方便按时间排序归档。
- **思维链展示**：支持提取并格式化展示模型的思维链（Thought Process）。
- **格式契约**：输出严格遵循 [`../CHATFORMAT.md`](../CHATFORMAT.md)（AfterChat 对话文件格式规范）。

## 🚀 快速开始

### 1. 编译项目

确保你已经安装了 Rust 环境。

```bash
cargo build --release
```

编译后的可执行文件位于 `target/release` 目录下。

### 2. 使用方法

#### 基本用法

直接指定备份文件路径。支持直接传入 `JSON`，也支持传入 Cherry 导出的 `ZIP`（程序会先解压并读取其中的 `data.json`）。默认把结果 zip 写在源文件**同目录**。

```bash
# 开发环境运行
cargo run -- path/to/backup.json

# 使用编译后的程序
./cherry-studio-backup-json-converter path/to/backup.json

# 也支持 Cherry 导出的 ZIP 备份
./cherry-studio-backup-json-converter path/to/cherry-backup.zip
```

#### 指定输出位置

`-o` / `--output` 可以是**目录**（zip 按规范名写进去）或**明确的 `.zip` 路径**。

```bash
# 指定输出目录
./cherry-studio-backup-json-converter -o my_exports path/to/backup.json

# 指定 zip 文件名
./cherry-studio-backup-json-converter -o 2026年备份.zip path/to/backup.json
```

## 📦 输出结构

无论备份里有多少个对话，产物都是**一个 zip**：

```
chat-export-cherry-all-1789794167679.zip
├── 默认助手/
│   ├── 20250329-204231-对话标题.md
│   └── 20260423-075546-另一个标题.md
├── Gemini 2.5 Pro/
│   └── 20260213-124758-对话标题.md
└── export-failures.md          ← 仅当有主题没有任何消息时才出现
```

- zip 名遵循 `docs/CHATFORMAT.md` §6：`chat-export-{platform}-all-{毫秒时间戳}.zip`
- 条目名：`<助手名>/<YYYYMMDD-HHmmss>-<标题>.md`（时间取对话时间，包内从旧到新）
- 重名自动加 `-2` / `-3`
- 条目时间戳 = 对话实际时间（受 zip 格式限制，精度 **2 秒**）

## 📂 Markdown 内容示例

每份 `.md` 的内容结构如下：

```markdown
## Metadata

### Run Settings

- **Model:** `gpt-4-turbo`
- **Time:** 2024-03-28 21:31:51 +08:00
- **Topic ID:** `...`
- **Assistant:** `默认助手`

## Conversation

### ⚙️ System

(如果存在系统提示词，会作为第一条消息显示在这里)

### 🧑‍💻 User

用户输入的内容...

### 🤖 Assistant

#### 🤔 Thought Process

(如果模型包含思维链，会显示在这里)

#### 💡 Response

模型的回复内容...
```

### 消息体规则

- 保留原 Markdown（代码块、列表、引用等）
- 正文中的 `#`/`##`/`###` 标题会转成 `**加粗**`
- **整条标题加粗**：标题内原有的 `**` 会被吸收（`## 方案一：**“…”**` → `**方案一：“…”**`），
  否则外层与内层 `**` 同级交错，CommonMark 会错配定界符，导致强调不全或残留可见星号
  - 行内代码里的 `**` 不是强调（如 glob `**/*.js`），原样保留
- **代码围栏（```` ``` ```` / `~~~`）内部不做转换** —— 否则 Python/Shell 的 `# 注释` 会被误改成加粗
- 助手消息按 `thinking` / 正文块拆成 `#### 🤔 Thought Process` 与 `#### 💡 Response` 两段

## 🐍 Python 版

`python/convert-json-to-markdowns.py` 是同一套逻辑的 Python 实现，输入输出与 Rust 版完全一致
（同一份备份、同一组参数下，产出 zip 的条目名、内容、时间戳、权限位均逐字节相同）。
不需要编译，适合临时用一下。

```bash
# 默认读 data.json
python python/convert-json-to-markdowns.py

# 指定输入与输出
python python/convert-json-to-markdowns.py backup.json -o out/
python python/convert-json-to-markdowns.py backup.zip  -o 2026备份.zip
```

仅需标准库（Python 3.8+）。

## 🛠️ 开发构建

如果你想参与开发或修改代码：

1. 克隆仓库
2. 安装依赖：`cargo build`
