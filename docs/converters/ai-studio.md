# ai-studio

> 输出契约见 [`../CHATFORMAT.md`](../CHATFORMAT.md)。

本工具用于解析包含与大语言模型（LLM）多轮对话（可能包含思考过程）的 JSON 文件，并将其转换为易于阅读的 Markdown 格式。

它使用 Rust 构建，并可编译为独立的可执行文件。

---

## 功能特性

- 解析代表对话的特定 JSON 结构（详见下文格式说明）。
- 生成格式清晰的 Markdown 输出。
- 如果 JSON 中存在元数据（如模型设置、系统指令），则会包含在输出中。
- 区分用户回合 (`🧑‍💻 User`) 和助手回合 (`🤖 Assistant`)。
- 当存在 `isThought` 标志时，使用子标题将助手的思考过程 (`🤔 Thought Process`) 与最终回复 (`💡 Response`) 分开。
- 提供命令行接口（CLI）进行操作。
- 支持 Windows 上的拖放功能，方便转换（将 JSON 文件拖到 `.exe` 文件上）。
- 编译为单个、独立的可执行文件，无外部运行时依赖。

---

## 先决条件

**Rust 开发环境:** 你需要安装 Rust 编译器 (`rustc`) 和包管理器 (`cargo`)。

---

## 构建

1.  克隆或下载此仓库。
2.  在终端中进入项目目录。
3.  运行 `cargo build --release` 进行优化后的发布构建（输出位于 `./target/release/`）。

---

## 使用方法

假设可执行文件名为 google-ai-studio-json-converter.exe (Windows) 或 google-ai-studio-json-converter (Linux/macOS)。

**1. 命令行接口 (CLI)**

```markdown
# 基本用法 (输出文件默认为同目录下的 [输入文件名].md)

./path/to/executable <path/to/your_conversation.json>

# 指定输出文件路径

./path/to/executable <path/to/your_conversation.json> -o <path/to/output/result.md>
./path/to/executable <path/to/your_conversation.json> --output <path/to/output/result.md>

# 显示帮助信息

./path/to/executable --help
```

可选：在运行前设置 RUST_LOG=info 环境变量可以查看更详细的日志信息。

- PowerShell: `$env:RUST_LOG="info"`

- CMD: `set RUST_LOG=info`

- Bash/Zsh: `export RUST_LOG=info`

**2. 拖放操作 (Windows)**

- 构建发布版本 (cargo build --release)。

- 在 ./target/release/ 目录中找到 google-ai-studio-json-converter.exe 文件。

- 直接将你的 .json 对话文件拖拽到文件资源管理器中的 google-ai-studio-json-converter.exe 图标上。

- 一个与 JSON 文件同名（但扩展名为 .md）的 Markdown 文件（例如 your_conversation.md）将在原始 JSON 文件所在的目录下被创建。

## 输入 JSON 格式

JSON 请从 Google Drive 中下载。本工具期望的 JSON 结构大致如下例所示：

```json
{
  "runSettings": {
    // 可选的元数据
    "model": "模型名称",
    "temperature": 1.0
    // ... 其他设置
  },
  "systemInstruction": {
    // 可选的元数据
    "text": "你是一个乐于助人的助手。"
  },
  "chunkedPrompt": {
    // 主要的对话容器
    "chunks": [
      // 对话回合数组
      {
        "text": "用户的第一条消息。",
        "role": "user"
        // tokenCount 等字段（解析器会忽略）
      },
      {
        "text": "助手的思考过程...",
        "role": "model",
        "isThought": true // 区分思考过程的重要标志
      },
      {
        "text": "助手的实际回复。",
        "role": "model",
        "finishReason": "STOP" // 如果 isThought 缺失，默认为 false
      },
      {
        "text": "用户的下一条消息。",
        "role": "user"
      }
      // ... 更多回合
    ]
    // "pendingInputs": [...] // 解析器会忽略
  }
}
```

runSettings 和 systemInstruction 等字段是可选的。

在 chunks 中，role（"user" 或 "model"）和 text 是主要使用的字段。

isThought 布尔标志（在 "model" 回合内）对于区分思考过程至关重要。如果缺失，则默认为 false（被视为回复）。

空的或缺失的 text 字段会被优雅处理（通常导致该部分不输出文本）。

## 输出 Markdown 格式

生成的 Markdown 文件将具有以下结构：

```markdown
Conversation Transcript: [输入文件名（不含扩展名）]

## Metadata (可选部分)

### Run Settings

- **设置 1:** `值1`
- **设置 2:** `值2`

### System Instruction

系统指令文本放在这里。

## Conversation

### 🧑‍💻 User

用户消息内容。

### 🤖 Assistant

#### 🤔 Thought Process (仅当 isThought=true 时出现)

助手的思考过程文本。

#### 💡 Response (仅在思考过程之后出现，或在没有思考过程时直接出现在助手标题下)

助手的回复文本。

### 🧑‍💻 User

下一条用户消息内容。

### 🤖 Assistant

助手的回复文本。
```

## 依赖库

本项目主要使用了以下 Rust crates：

clap: 用于解析命令行参数。

serde / serde_json: 用于 JSON 反序列化。

log / env_logger: 用于日志记录。

anyhow: 提供便捷且带上下文的错误处理。

regex: 用于清理输出中可能存在的多余空行。
