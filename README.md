# Cherry Studio Backup JSON Converter

这是一个用 Rust 编写的高效工具，用于将 Cherry Studio 的备份文件（JSON 或包含 `data.json` 的 ZIP）转换为易于阅读的 Markdown 文档。

它不仅能提取对话内容，还能保留完整的元数据（如模型名称、助手信息、创建时间等），并自动将生成文件的创建/修改时间重置为对话发生的实际时间。

## ✨ 功能特点

- **高性能转换**：利用 Rust 的并行处理能力（Rayon），快速处理大量对话数据。
- **元数据保留**：提取并展示助手名称、模型信息、系统提示词（System Prompt）等。
- **智能文件命名**：自动使用对话标题作为文件名，并处理非法字符（包括换行符）。
- **时间时光机**：生成的 Markdown 文件的时间属性（创建/修改时间）会被重置为对话实际发生的时刻，方便按时间排序归档。
- **思维链展示**：支持提取并格式化展示模型的思维链（Thought Process）。
- **灵活输出**：支持自定义输出目录，默认输出到源文件同级的 `cherry-studio-export` 文件夹。

## 🚀 快速开始

### 1. 编译项目

确保你已经安装了 Rust 环境。

```bash
cargo build --release
```

编译后的可执行文件位于 `target/release` 目录下。

### 2. 使用方法

#### 基本用法

直接指定备份文件路径。支持直接传入 `JSON`，也支持传入 Cherry 导出的 `ZIP`（程序会先解压并读取其中的 `data.json`）。默认会在备份文件同级目录下创建 `cherry-studio-export` 文件夹存放结果。

```bash
# 开发环境运行
cargo run -- path/to/backup.json

# 使用编译后的程序
./cherry-studio-backup-json-converter path/to/backup.json

# 也支持 Cherry 导出的 ZIP 备份
./cherry-studio-backup-json-converter path/to/cherry-backup.zip
```

#### 指定输出目录

使用 `-o` 或 `--output` 参数指定输出文件夹。

```bash
# 开发环境运行
cargo run -- -o my_conversations path/to/backup.json

# 使用编译后的程序
./cherry-studio-backup-json-converter -o my_conversations path/to/backup.json
```

## 📂 输出示例

生成的 Markdown 文件内容结构如下：

```markdown
Conversation Transcript: 对话标题

## Metadata

### Run Settings

- **Topic ID:** `...`
- **Assistant:** `默认助手`
- **Created At:** `2024-03-28T13:31:51.887Z`
- **Model:** `gpt-4-turbo`

### System Instruction

(如果存在系统提示词，会显示在这里)

## Conversation

### 🧑‍💻 User

用户输入的内容...

### 🤖 Assistant

#### 🤔 Thought Process
(如果模型包含思维链，会显示在这里)

#### 💡 Response
模型的回复内容...
```

## 🛠️ 开发构建

如果你想参与开发或修改代码：

1. 克隆仓库
2. 安装依赖：`cargo build`
