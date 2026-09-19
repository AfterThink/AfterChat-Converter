# claude

> 输出契约见 [`../CHATFORMAT.md`](../CHATFORMAT.md)（下称「契约」）。

把 Claude 数据导出（`conversations.json`，可选包成 ZIP）转换为符合契约的 **Markdown ZIP**。

## 输入

接受三种形态，按扩展名判定：

| 输入 | 说明 |
| --- | --- |
| `*.zip` | Claude 数据导出压缩包，从包内任意层级、大小写不敏感地找 `conversations.json` |
| `*.json` | 直接给 `conversations.json` |
| 目录 | 递归查找目录下的 `conversations.json`（只取最外层一个） |

`conversations.json` 是**对话数组**，每条对话的字段：

| 字段 | 类型 | 用途 |
| --- | --- | --- |
| `uuid` | string | 平台 ID，写进失败报告 |
| `name` | string | 标题（可能为空） |
| `summary` | string | 目前恒为空，忽略 |
| `created_at` | RFC3339 字符串（含微秒 + `Z`） | `Time` 与排序键（转本地时间） |
| `updated_at` | RFC3339 字符串 | 忽略 |
| `account` | object | 忽略 |
| `chat_messages` | array | 消息列表 |

消息字段：

| 字段 | 类型 | 用途 |
| --- | --- | --- |
| `uuid` | string | 忽略 |
| `text` | string | 纯文本正文（可能为空） |
| `content` | array | 结构化内容，元素 `type` ∈ `text` / `tool_use` / `tool_result` / `token_budget` |
| `sender` | `human` \| `assistant` | 角色映射：`human` → User，`assistant` → Assistant，其它 → Assistant |
| `created_at` / `updated_at` | RFC3339 | 仅排序参考 |
| `attachments` | array | 附件，渲染为 `[name](attachment)` |
| `files` | array | 附带文件，忽略 |

> **没有 `model` 字段。** 已用正则在最外层 key、`content[].input`、`name` 等处搜索 `model` / `sonnet` / `haiku` / `opus` 均无结果，因此 `Model` 一律写 `Unknown`。

## 输出

只有一个产物：`chat-export-claude-all-{毫秒时间戳}.zip`（或 `-o` 指定的 `.zip` / 目录）。

- 条目名：`{YYYYMMDD-HHmmss}-{标题}.md`（契约 §6，本平台不按助手分组）。
- 排序：`created_at` 从新到旧。
- 没有任何消息的对话不生成条目，汇总进 `export-failures.md`（`id` = `uuid`），进程仍以 0 退出。

## 用法

```powershell
cargo run -p afterchat-claude -- data-2026-03-17-12-33-08-batch-0000.zip
cargo run -p afterchat-claude -- conversations.json -o out\
cargo run -p afterchat-claude -- exported-dir/ -o out\result.zip
```

## 实现

- `src/lib.rs`：输入发现（`find_conversations_json`）、`Conversation`/`Message` 映射、`run_conversion`。
- `src/main.rs`：clap CLI。
- 渲染 / 命名 / 打包全部交给 `chatformat`，本 crate 不含 Markdown 拼接逻辑。
