# rikka 转换器规格

> 输出契约定义参见 [`../CHATFORMAT.md`](../CHATFORMAT.md)。
> 本文档定义 RikkaHub 平台的输入解析规则与实现细节。

---

## 1. 范围与产物

将 **RikkaHub 备份 ZIP**（内含 SQLite 数据库 `rikka_hub.db`、`rikka_hub-wal`、`rikka_hub-shm` 及 `settings.json`）
解析转换为 AfterChat Markdown 文档，并打包为单个 ZIP 归档包。

| 输入类型 | 输入格式 | 输出格式 |
|---|---|---|
| 完整备份包 | `*.zip` | 单个 `.zip` 归档 |
| 独立数据库 | `*.db`（支持同级目录 `-wal` / `settings.json`） | 单个 `.zip` 归档 |

- **输出格式确定性**：统一输出 ZIP 归档文件（不因仅含单条会话而退化为单 `.md`）；
- **ZIP 文件命名**：`chat-export-rikka-all-{毫秒时间戳}.zip`；
- **条目命名规范**：`{助手名}/{YYYYMMDD-HHmmss}-{标题}.md`（按助手名称分目录存放，遵循契约 §6.2）。

---

## 2. 输入读取机制

### 2.1 ZIP 备份包

- 遍历压缩包内部条目（大小写不敏感，支持任意目录层级），无需解压至磁盘：
  - 数据库文件：优先匹配 `rikka_hub.db`，否则匹配首个 `*.db`（层级最浅者优先，同级按字典序排序）；
  - WAL 预写日志：匹配与所选数据库同名的 `<dbname>-wal`；
  - 配置文件：匹配 `settings.json`（缺失时采用默认降级配置，参见 §4.4）。
- **忽略 `-shm` 共享内存文件**：共享内存具有高变动性，直接读取可能阻碍 SQLite 回放 WAL 日志。
- 将 `.db` 与 WAL 文件解包至临时工作目录，通过 `rusqlite` 以读写模式打开，使 SQLite 自动重建 SHM 并完整回放 WAL 日志；解析完成后清理临时目录。

### 2.2 独立 SQLite 数据库文件（`.db`）

- 直接读取指定的 `.db` 文件；若同级目录下存在 `<dbname>-wal` 或 `settings.json` 则协同读取；
- 同样先拷贝至临时工作目录打开，避免直接修改源文件。

### 2.3 异常输入判定

- 文件路径不存在、非文件类型、ZIP 内未找到有效 `*.db`、SQLite 打开失败或缺少 `ConversationEntity` 数据表时，程序终止并返回错误。

---

## 3. 数据模型与会话重建

```sql
ConversationEntity(
  id TEXT PK, assistant_id TEXT, title TEXT, nodes TEXT,   -- 新版 nodes 恒为 "[]"
  create_at INTEGER, update_at INTEGER, custom_system_prompt TEXT)

message_node(
  id TEXT PK, conversation_id TEXT, node_index INTEGER,
  messages TEXT,        -- JSON: List<UIMessage>
  select_index INTEGER)
```

> **版本兼容性**：RikkaHub 历史版本的 `ConversationEntity` 列定义较少（如 v17 数据库缺失 `custom_system_prompt` 等列）。
> 查询会话时采用 `SELECT *` 并按列名动态提取字段值，缺失列填充默认值，避免硬编码列名导致的兼容性问题。
> 早期版本若缺失 `message_node` 表，则回退解析 `ConversationEntity.nodes`（参见 §3.1 兜底逻辑）。

### 3.1 消息序列重建

```
nodes := SELECT messages, select_index FROM message_node
         WHERE conversation_id = ? ORDER BY node_index ASC

current_messages := 遍历各 node：
    arr := parse_json(node.messages) as array
    idx := clamp(select_index, 0, arr.len()-1)      # 越界回退至 0；空数组跳过
    arr[idx]
```

- 历史备份兜底：若不存在 `message_node` 数据而 `ConversationEntity.nodes` 非空，按历史结构解析数组元素 `{ "messages": [...], "selectIndex": n }` 并提取对应索引的消息。

### 3.2 UIMessage 结构定义

```jsonc
UIMessage { "role": "user|assistant|system|tool", "parts": [...], "modelId": null|"uuid" }
part.type = "text"      → { "text": "..." }
          | "reasoning" → { "reasoning": "..." }
          | "image"     → { "url": "..." }
          | "video"     → { "url": "..." }
          | "audio"     → { "url": "..." }
          | "document"  → { "url": "...", "fileName": "..." }
          | "tool" | "tool_call" | "tool_result" | "server_tool" | "search"
```

- 忽略 `metadata` 字段；
- 工具类 part 与未知类型 part 均予忽略；
- 未知 `role` 统一兜底为 `Assistant`。

### 3.3 消息体装配规则

| part 类型 | 输出映射 |
|---|---|
| `text` | 回复正文 |
| `reasoning` | 思维链 |
| `image` | 回复正文：`![image](url)` |
| `document` | 回复正文：`[fileName](url)`（fileName 为空则使用 `document`） |
| `video` | 回复正文：`![video](url)` |
| `audio` | 回复正文：`![audio](url)` |
| 工具类 / 未知 | 忽略 |

- **user 消息**：各正文片段以 `\n\n` 拼接；若 `trim()` 后为空则整条跳过；
- **assistant 消息**：
  - 包含 `reasoning` 时输出 `#### 🤔 Thought Process`；若正文非空再输出 `#### 💡 Response`；
  - 无 `reasoning` 时直接输出正文，不输出 `#### 💡 Response`；
  - 仅含 `reasoning` 时不输出空的 `#### 💡 Response`；
- 执行 `strip_hashes` 标题转换规则（契约 §5）。

---

## 4. settings.json 关联解析

```jsonc
{
  "providers": [ { "models": [
      { "id": "uuid", "modelId": "gemini-3-flash-preview", "displayName": "gemini-3-flash-preview" } ] } ],
  "assistants": [ {
      "id": "uuid", "name": "...", "systemPrompt": "...",
      "allowConversationSystemPrompt": false } ]
}
```

- 自动剥除 UTF-8 BOM；解析失败时使用默认空配置降级处理（正文正常导出）。

### 4.1 模型标识提取

1. 提取当前会话中首条包含 `modelId` 的 assistant 消息；
2. 映射链：`displayName` → `modelId` → 原始 UUID → `Unknown`。

### 4.2 系统提示词 (System Prompt)

```
effective = if (assistant.allowConversationSystemPrompt && custom_system_prompt 非空)
                custom_system_prompt
            else assistant.systemPrompt
```

- 非空时作为会话首条消息输出 `### ⚙️ System`；
- 仅导出静态设定，不包含运行时动态注入项。

### 4.3 助手名称

- 提取 `assistants[].name` 用于条目目录划分与 Metadata 展示，为空时使用 `Assistant`。

---

## 5. Metadata 结构

```markdown
## Metadata

- **Model:** `{model}`
- **Time:** {YYYY-MM-DD HH:mm:ss ±HH:MM}
- **Conversation ID:** `{id}`
- **Assistant:** `{assistant_name}`
```

- `Model` 必填且须用反引号包裹；
- `Time` 提取 `ConversationEntity.create_at`（毫秒时间戳）渲染为本地时区时间；
- RikkaHub 无网页端地址，省略 `URL` 字段；
- `Conversation ID` 与 `Assistant` 作为扩展字段追加在标准键之后。

---

## 6. 文件命名与打包规范

### 6.1 文件名清洗

1. 移除非法字符 `\ / : * ? " < > |` 及 `\r \n \t`；
2. 执行 `trim()`；为空则使用 `Untitled_Conversation`；
3. 标题截断上限为 80 字符；
4. 助手分组目录名清洗后若为空或 `.` / `..`，兜底为 `Assistant`。

### 6.2 条目命名与时序

- 前缀格式：`create_at` 本地 `%Y%m%d-%H%M%S`（缺失时兜底为 `00000000-000000`）；
- 条目路径：`{助手目录}/{前缀}-{标题}.md`；重名冲突自动追加 `-2`、`-3`；
- 排序规则：按对话发生时间从新到旧排列；
- 条目修改时间：设为对话实际时间戳；
- 压缩格式：Deflate。

### 6.3 失败报告

- 会话无有效导出内容或解析失败时，在 ZIP 内追加 `export-failures.md`，记录平台标识、源文件及失败会话列表；
- 全部会话转换成功时不生成该文件。

---

## 7. 命令行接口 (CLI)

```powershell
# 编译二进制
cargo build --release -p afterchat-rikka

# 运行转换
rikka <INPUT> [-o <PATH>]
```

支持直接将 `.zip` 或 `.db` 文件拖拽至 `rikka.exe` 图标上运行。

| 参数 | 必填 | 说明 |
|---|---|---|
| `<INPUT>` | 是 | 输入的 `.zip` 备份文件或 `.db` 数据库文件 |
| `-o, --output <PATH>` | 否 | 指定输出目录或显式 `.zip` 目标文件；省略时输出至源文件同级目录 |

---

## 8. 错误处理策略

- 输入文件不存在、损坏或无法识别时，程序退出并返回非零状态码；
- 批处理中单个会话解析失败记录至 `export-failures.md`，不影响其余会话正常导出，程序正常退出。

---

## 9. 设计与实现规范
1. Metadata 规范输出 `Conversation ID` 与 `Assistant` 扩展字段；
2. System 提示词仅导出静态提示词，不包含运行时动态注入内容；
3. ZIP 内部条目统一按会话时间降序排列；
4. 媒体内容保留 Markdown 占位语法 `![image](url)` 与 `[fileName](url)`；
5. 工具调用类数据不输出至正文；
6. 无论是 `.zip` 还是 `.db` 输入，均统一产出 ZIP 归档包。
