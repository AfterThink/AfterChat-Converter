# deepseek 转换器规格

> 输出契约定义参见 [`../CHATFORMAT.md`](../CHATFORMAT.md)。

本工具用于将 DeepSeek 官方导出的数据备份（`deepseek_data-*.zip`，或包含 `chat_sessions/*.json` 的文件夹 / 单个会话 JSON）转换为符合规范的 **Markdown ZIP** 归档包或单会话 Markdown。

---

## 输入格式

支持三种输入形态，程序根据文件扩展名与路径类型自动判定：

| 输入类型 | 说明 |
| --- | --- |
| `*.zip` | DeepSeek 数据导出压缩包（内含 `manifest.json`、`user.json`、`chat_sessions/*.json` 及可选的附件目录），直接在包内检索与提取会话 |
| `*.json` | 显式传入单个会话 JSON 文件，或包含会话数组的 JSON |
| 目录路径 | 递归扫描目录下存在的 `chat_sessions/*.json` 或单个会话 JSON |

### 会话核心结构

DeepSeek 导出会话 JSON (`chat_sessions/<id>.json`) 核心字段定义：

| 字段 | 类型 | 用途 |
| --- | --- | --- |
| `id` | string (UUID) | 会话平台 ID，映射至 Metadata `URL` 与 `Conversation ID` |
| `title` | string | 对话标题（用于条目命名） |
| `current_message_id` | number | 当前活跃分支的末端消息 ID，用于沿 `parent_id` 回溯完整对话主线 |
| `inserted_at` | RFC3339 字符串 | 用于生成 Metadata `Time` 与排序参考 |
| `updated_at` | RFC3339 字符串 | 用于 ZIP 条目排序（毫秒时间戳） |
| `messages` | array | 消息节点树列表 |

### 消息节点与分片（Fragments）

单条消息的核心字段定义：

| 字段 | 类型 | 用途 |
| --- | --- | --- |
| `id` | number | 消息唯一 ID |
| `parent_id` | number \| null | 父级消息 ID（用于回溯活跃路径） |
| `role` | `USER` \| `ASSISTANT` | 角色映射：`USER` → User，`ASSISTANT` → Assistant |
| `thinking_enabled` | boolean | 是否开启深度思考（思维链保留在正文 `#### 🤔 Thought Process`） |
| `fragments` | array | 消息内容分片列表 |

消息分片（Fragment）类型与处理策略：

| 分片类型 (`type`) | 角色 | 处理策略 |
| --- | --- | --- |
| `REQUEST` | User | 用户提问文本正文，并入 User 消息体 |
| `FILE` | User | 用户上传文件，渲染为 `[file_name](attachment)`（若为图片则渲染为 `![file_name](attachment)`） |
| `THINK` | Assistant | 助手深度思考思维链，并入 Assistant `#### 🤔 Thought Process` |
| `RESPONSE` | Assistant | 助手正式回复正文，并入 Assistant `#### 💡 Response` |
| `TOOL_SEARCH` / `TOOL_OPEN` / `TOOL_FIND` / `TOOL_IMAGE_SEARCH` | Assistant | 联网搜索与检索等工具调用过程，作为背景调用予以忽略 |
| `CONTENT_RISK` | Assistant | 平台风险提示横幅，作为 UI 提示予以忽略 |

> **模型字段说明**：
> DeepSeek 官方导出数据中未记录具体底层模型版本 slug，且线上模型会持续迭代及灰度更新，因此 Metadata 中的 `Model` 字段统一填充为 `DeepSeek`。思考模式的有无则通过正文中的 `#### 🤔 Thought Process` 小节真实保留。

---

## 输出规范

### 1. 批量会话（输入为 ZIP 或包含多个会话的目录）

产物为单个 ZIP 归档文件：`chat-export-deepseek-all-{毫秒时间戳}.zip`（或 `-o` 选项指定的路径）：

- **条目命名**：`{YYYYMMDD-HHmmss}-{标题}.md`（遵循契约 §6，不设助手分组目录）；
- **排序规则**：依据 `updated_at`（或 `inserted_at`）从新到旧排列；
- **异常处理**：无有效消息内容的空会话不生成 `.md` 条目，统一记录至包内的 `export-failures.md`。

若命令行指定 `-o <DIR>` 输出到已存在或非 `.zip` 目录，则直接将渲染后的 `.md` 文件及 `export-failures.md` 写入该目录。

### 2. 单个会话（输入为单个 JSON 文件）

若未显式指定 `.zip` 扩展名的输出路径，程序直接输出单个 Markdown 文件 `{标题}.md`。

---

## 使用方法

### 命令行工具 (CLI)

```powershell
# 编译二进制
cargo build --release -p afterchat-deepseek

# 转换官方数据导出压缩包
deepseek deepseek_data-2026-11-20.zip

# 转换至指定目录
deepseek deepseek_data-2026-11-20.zip -o out\

# 转换单个会话 JSON
deepseek session.json
```
