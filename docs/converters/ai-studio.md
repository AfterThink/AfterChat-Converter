# ai-studio 转换器规格

> 输出契约定义参见 [`../CHATFORMAT.md`](../CHATFORMAT.md)。

本工具用于解析 Google AI Studio 导出的对话 JSON 文件（包含多轮对话、思维链及模型配置），并转换为规范的 Markdown 格式。

采用 Rust 构建，编译为无外部运行时依赖的独立二进制程序。

---

## 功能特性

- 解析 Google AI Studio 导出的 JSON 对话结构；
- 输出符合 ChatFormat 规范的 Markdown 文档；
- 提取并保留元数据（模型配置参数、系统指令 System Instruction）；
- 区分用户消息 (`🧑‍💻 User`) 与模型消息 (`🤖 Assistant`)；
- 根据 `isThought` 标记拆分模型的思维链 (`#### 🤔 Thought Process`) 与正式回复 (`#### 💡 Response`)；
- 提供 CLI 命令行操作界面，并支持直接拖拽文件执行转换。

---

## 构建方式

在项目根目录下执行：

```powershell
cargo build --release -p afterchat-ai-studio
```

编译产物位于 `target/release/ai-studio.exe`。

---

## 使用方法

### 1. 命令行接口 (CLI)

```powershell
# 基本用法（默认在输入文件同级目录下生成同名 .md 文件）
ai-studio <path/to/conversation.json>

# 指定输出文件路径
ai-studio <path/to/conversation.json> -o <path/to/output/result.md>
ai-studio <path/to/conversation.json> --output <path/to/output/result.md>

# 查看帮助信息
ai-studio --help
```

可选环境变量：通过设置 `RUST_LOG=info` 控制日志输出级别：
- PowerShell: `$env:RUST_LOG="info"`
- CMD: `set RUST_LOG=info`
- Bash/Zsh: `export RUST_LOG=info`

### 2. 拖拽使用 (Windows)

直接将 `.json` 对话文件拖拽至 `ai-studio.exe` 图标上，程序将在源文件所在目录下生成同名 `.md` 文件。

---

## 输入 JSON 格式

输入数据源自 Google AI Studio 导出的 JSON 文件（如保存在 Google Drive 或本地的会话导出）。预期的 JSON 数据结构如下：

```json
{
  "runSettings": {
    // 模型配置元数据（可选）
    "model": "模型名称",
    "temperature": 1.0
    // ... 其他参数
  },
  "systemInstruction": {
    // 系统提示词（可选）
    "text": "系统指令内容..."
  },
  "chunkedPrompt": {
    // 对话分块数据
    "chunks": [
      {
        "text": "用户消息内容...",
        "role": "user"
      },
      {
        "text": "思维链推导过程...",
        "role": "model",
        "isThought": true // 标记思维链的关键布尔值
      },
      {
        "text": "模型正式回复内容...",
        "role": "model",
        "finishReason": "STOP" // 若 isThought 缺失，默认视为回复内容
      },
      {
        "text": "下一轮用户消息...",
        "role": "user"
      }
    ]
  }
}
```

- `runSettings` 与 `systemInstruction` 为可选对象；
- `chunks` 中主要提取 `role`（`"user"` 或 `"model"`）与 `text` 字段；
- `isThought` 字段用于识别模型思考过程，缺失时默认为 `false`；
- 若 `text` 字段为空或缺失，解析器将跳过该分块，不输出空白内容。

---

## 输出 Markdown 结构

生成的 Markdown 文件结构如下：

```markdown
Conversation Transcript: [输入文件名（不含扩展名）]

## Metadata

### Run Settings

- **Temperature:** `1.0`
- **Model:** `gemini-1.5-pro`

### System Instruction

系统指令内容。

## Conversation

### 🧑‍💻 User

用户消息内容。

### 🤖 Assistant

#### 🤔 Thought Process

助手的思维链内容。

#### 💡 Response

助手的正式回复内容。
```

---

## 核心依赖库

- `clap`：命令行参数解析；
- `serde` / `serde_json`：JSON 结构反序列化；
- `log` / `env_logger`：运行时日志管理；
- `anyhow`：错误处理与上下文追踪；
- `afterchat-chatformat`：统一契约渲染与文本处理。
