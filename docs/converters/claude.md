# claude 转换器规格

> 输出契约定义参见 [`../CHATFORMAT.md`](../CHATFORMAT.md)。

本工具用于将 Claude 导出的数据备份（`conversations.json`，或包含该文件的 ZIP 压缩包）转换为符合规范的 **Markdown ZIP** 归档包。

---

## 输入格式

支持三种输入形态，程序根据文件扩展名与路径类型自动判定：

| 输入类型 | 说明 |
| --- | --- |
| `*.zip` | Claude 数据导出压缩包，自动在包内递归检索 `conversations.json`（忽略大小写与目录层级） |
| `*.json` | 显式传入 `conversations.json` 文件 |
| 目录路径 | 递归扫描该目录下存在的 `conversations.json`（仅提取首个匹配项） |

`conversations.json` 数据结构为**会话对象数组**，单个会话的核心字段定义：

| 字段 | 类型 | 用途 |
| --- | --- | --- |
| `uuid` | string | 会话平台 ID，解析异常时记入失败报告 |
| `name` | string | 对话标题（允许为空） |
| `summary` | string | 摘要（当前版本多为空值，忽略） |
| `created_at` | RFC3339 字符串（含微秒与 `Z`） | 用于生成 Metadata `Time` 与排序键（转换为本地时区时间） |
| `updated_at` | RFC3339 字符串 | 忽略 |
| `account` | object | 忽略 |
| `chat_messages` | array | 消息列表 |

消息对象的核心字段定义：

| 字段 | 类型 | 用途 |
| --- | --- | --- |
| `uuid` | string | 忽略 |
| `text` | string | 文本正文（可能为空） |
| `content` | array | 结构化内容数组，元素 `type` 包括 `text` / `tool_use` / `tool_result` / `token_budget` |
| `sender` | `human` \| `assistant` | 角色映射：`human` → User，`assistant` → Assistant，其他未知类型兜底为 Assistant |
| `created_at` / `updated_at` | RFC3339 | 仅作为消息时序参考 |
| `attachments` | array | 附件列表，渲染为 `[name](attachment)` |
| `files` | array | 关联文件列表，忽略 |

> **模型字段说明**：Claude 官方导出数据中未包含会话所用模型的标识字段，因此 Metadata 中的 `Model` 字段统一填充为 `Unknown`。

---

## 输出规范

产物为单个 ZIP 归档文件：`chat-export-claude-all-{毫秒时间戳}.zip`（或 `-o` 选项指定的路径）：

- **条目命名**：`{YYYYMMDD-HHmmss}-{标题}.md`（遵循契约 §6，Claude 平台不设助手分组目录）；
- **排序规则**：依据 `created_at` 从新到旧排列；
- **异常处理**：无有效消息内容的空会话不生成 `.md` 条目，统一记录至包内的 `export-failures.md`（记录 `uuid`），程序保持正常退出。

---

## 使用方法

```powershell
# 编译二进制
cargo build --release -p afterchat-claude

# 运行转换
claude data-2026-03-17-12-33-08-batch-0000.zip
claude conversations.json -o out\
claude exported-dir/ -o out\result.zip
```

---

## 模块实现

- `src/lib.rs`：负责输入定位（`find_conversations_json`）、数据映射至 `Conversation`/`Message` 及主转换流调度；
- `src/main.rs`：基于 `clap` 的 CLI 命令入口；
- 文本渲染、非法字符清理与 ZIP 打包统一委托公共库 `afterchat-chatformat` 处理。
