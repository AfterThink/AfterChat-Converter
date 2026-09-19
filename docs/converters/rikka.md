# rikka

> 输出契约见 [`../CHATFORMAT.md`](../CHATFORMAT.md)（下称「契约」）。
> 本文只描述 RikkaHub 这一平台的输入解析与实现细节。**契约第 1～7 节必须逐字遵守。**
>
> 实现思路对齐 **cherry-studio-backup-json-converter**（按助手分目录、补充 Metadata 键、兜底命名）。

---

## 1. 范围与产物

把 **RikkaHub 备份 ZIP**（内含 SQLite 库 `rikka_hub.db` + `rikka_hub-wal` + `rikka_hub-shm` + `settings.json`）
转换成 AfterChat 对话 Markdown，并打包成一个 ZIP。

| | 输入 | 输出 |
|---|---|---|
| 备份包 | `*.zip` | 一个 `.zip` |
| 裸数据库 | `*.db`（可选同目录 `-wal` / `settings.json`） | 一个 `.zip` |

- 始终产出 ZIP（不因只有一条会话而退化为单 `.md`）。
- ZIP 名：`chat-export-rikka-all-{毫秒时间戳}.zip`。
- 条目名：`{助手名}/{YYYYMMDD-HHmmss}-{标题}.md`（按助手分组，契约 §6.2 允许自定义分组目录）。

---

## 2. 输入读取

### 2.1 ZIP 备份

- 在压缩包内按**条目 basename**（大小写不敏感、任意层级）查找，不落地解压：
  - 数据库：优先 `rikka_hub.db`，否则任意 `*.db`（取层级最浅者，同层按名字典序）。
  - WAL：与所选数据库同 basename 的 `<dbname>-wal`。
  - 设置：`settings.json`（缺失则降级，见 §4.4）。
- **不读取 `-shm`**：共享内存文件易变，复制进来反而阻止 SQLite 回放 WAL（实测带 `-shm` 会少 3 个 node）。
- 把 db 与（存在的）wal 写入**临时目录**，文件名保持 `<name>` / `<name>-wal`，用 `rusqlite` 以读写模式打开，让 SQLite 重建 shm 并回放 WAL；处理完删除临时目录。

### 2.2 裸 `.db`

- 直接使用该文件；同目录存在 `<dbname>-wal` / `settings.json` 时一并读取。
- 同样先拷贝到临时目录再打开，绝不动原文件。

### 2.3 拒绝的输入

- 路径不存在 / 不是文件；ZIP 内找不到 `*.db`；SQLite 打开失败；`ConversationEntity` 表缺失。→ 整体报错退出。

---

## 3. 数据模型与重建

```sql
ConversationEntity(
  id TEXT PK, assistant_id TEXT, title TEXT, nodes TEXT,   -- nodes 新版恒为 "[]"
  create_at INTEGER, update_at INTEGER, custom_system_prompt TEXT)

message_node(
  id TEXT PK, conversation_id TEXT, node_index INTEGER,
  messages TEXT,        -- JSON: List<UIMessage>
  select_index INTEGER)
```

> **版本兼容**：旧版 RikkaHub 的 `ConversationEntity` 列更少（如 DB v17 没有 `custom_system_prompt` /
> `mode_injection_ids` / `lorebook_ids` / `workspace_cwd` / `folder_id`）。因此读会话时用 `SELECT *`
> 后按**列名**动态取值，缺失的列用默认值（字符串 → `""`、时间 → `None`），**不得写死列清单**。
> 极老版本没有 `message_node` 表时，回退解析 `ConversationEntity.nodes`（§3.1 兜底）。

### 3.1 消息重建（与 App `Conversation.currentMessages` 同口径）

```
nodes := SELECT messages, select_index FROM message_node
         WHERE conversation_id = ? ORDER BY node_index ASC

current_messages := 对每个 node：
    arr := parse_json(node.messages) as array
    idx := clamp(select_index, 0, arr.len()-1)      # 越界回退 0；arr 空则跳过
    arr[idx]
```

- 兜底（旧备份）：若无 `message_node` 行而 `ConversationEntity.nodes` 非 `[]`，
  按旧结构解析数组元素 `{ "messages": [...], "selectIndex": n }`，同样取 `messages[selectIndex]`。

### 3.2 UIMessage / UIMessagePart

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

- `metadata` 字段忽略。
- **工具类 part 与未知 type 一律忽略**（不进正文；当前样本无此类）。
- `role` 非 `user`/`assistant`/`system` → 按契约兜底成 `Assistant`。

### 3.3 消息体拼装（契约 §4.2 / §4.3 / §5）

| part | 落到 |
|---|---|
| `text` | 回复正文 |
| `reasoning` | 思维链 |
| `image` | 回复正文：`![image](url)` |
| `document` | 回复正文：`[fileName](url)`（fileName 空则 `document`） |
| `video` | 回复正文：`![video](url)` |
| `audio` | 回复正文：`![audio](url)` |
| 工具类 / 未知 | 忽略 |

- **user**：正文片段 `\n\n` 连接；`trim()` 后为空 → 整条跳过。
- **assistant**：`thoughts` = reasoning 片段；`responses` = text/媒体片段；两者都空 → 整条跳过。
  - 有 thoughts → `#### 🤔 Thought Process`；若 responses 非空再出 `#### 💡 Response`。
  - 无 thoughts → 角色头后直接跟正文，**不输出 `#### 💡 Response`**。
  - 只有 thoughts → 不输出空的 `#### 💡 Response`。
- 空白片段先丢弃；片段内部**不 trim 行、不去重**。
- `strip_hashes` 作用于 user 正文、thoughts、responses（§5）。

---

## 4. settings.json

```jsonc
{
  "providers": [ { "models": [
      { "id": "uuid", "modelId": "gemini-3-flash-preview", "displayName": "gemini-3-flash-preview" } ] } ],
  "assistants": [ {
      "id": "uuid", "name": "...", "systemPrompt": "...",
      "allowConversationSystemPrompt": false } ]
}
```

- 剥 UTF-8 BOM；解析失败 → 空设置（正文照常导出）。

### 4.1 Model

1. 会话模型 = 按重建顺序**第一条 `modelId` 非空的 assistant 消息**的 id。
2. 解析链：`displayName` → `modelId` → 原始 UUID → `Unknown`。

### 4.2 System（与 App `GenerationLoop` 同口径）

```
effective = if (assistant.allowConversationSystemPrompt && custom_system_prompt 非空)
                custom_system_prompt
            else assistant.systemPrompt
```

- 非空 → 作为对话第一条消息输出 `### ⚙️ System`。
- 不导出运行时动态注入（memory / 工具 prompt / modeInjections / lorebooks）。
- 找不到 assistant 时：`custom_system_prompt` 非空则用之，否则省略 System。

### 4.3 Assistant 名

- 用于条目目录与 Metadata：`assistants[].name`，空则 `Assistant`。

### 4.4 设置缺失 / 未知模型

- 正文照常；`Model` 走解析链兜底到 `Unknown`；System 可能省略；目录 / 名称用 `Assistant`。

---

## 5. `strip_hashes`（契约 §5，逐字）

复用已在 qwen / cherry 验证过的实现：

1. 逐行扫描，**围栏代码块内（``` / ~~~，按类型配对）原样保留**。
2. 围栏外 `^#{1,6}\s+(.+)$` → `**inner.trim()**`；`#` 0 或 >6、`#` 后无空白、无内容 → 原样。
3. 包裹前 `remove_bold_outside_code`：删掉**不在行内代码段内**的 `**`，使整条标题落在一个加粗里。
4. 行内代码按 CommonMark 规则界定（N 个反引号开始 / 同样 N 个结束），其中 `**` 保留（glob `**/*.js`）。

以契约 §8 用例表为准。

---

## 6. Metadata

```
## Metadata

- **Model:** `{model}`
- **Time:** {YYYY-MM-DD HH:mm:ss ±HH:MM}
- **Conversation ID:** `{id}`
- **Assistant:** `{assistant_name}`
```

- `Model` 必须反引号包裹、格式精确。
- `Time` 取 `ConversationEntity.create_at`（毫秒）按**本地时区**渲染；缺失 → `unknown`。
- 无 URL（RikkaHub 无网页地址），省略 `URL` 键。
- `Conversation ID` / `Assistant` 为契约允许的补充键，排在推荐键之后。

---

## 7. 命名与打包

### 7.1 文件名

`sanitize_filename`（对齐 cherry，**删除**非法字符而非替换）：

1. 删除 `\ / : * ? " < > |` 与 `\r \n \t`。
2. `trim()`；为空 → `Untitled_Conversation`。
3. 标题按**字符**截断到 80。
4. 目录名用 `sanitize_path_component`，空 / `.` / `..` → `Assistant`。

### 7.2 条目名与顺序

- 前缀：`create_at` 本地 `%Y%m%d-%H%M%S`；缺失 → `00000000-000000`。
- 条目名：`{助手目录}/{前缀}-{标题}.md`；重名追加 `-2`、`-3`（扩展名在末尾）。
- 包内顺序：**按对话时间从新到旧**（qwen 口径）。
- 条目 mtime = 对话时间（2 秒精度，秒数向下取偶；契约已知限制）。
- Deflate 压缩。

### 7.3 失败报告

- 若某会话无可导出消息 / 解析失败，zip 内追加 `export-failures.md`（当前时间 mtime）：
  `# Export Failures` + `## Metadata`（`Platform: rikka`、`Source`、`Skipped`）+ 每条 `## N. 标题` / `Conversation ID` / `Reason`。
- 无失败则不生成。格式对齐 cherry。

---

## 8. CLI

```
rikka <INPUT> [-o <PATH>]
```

无子命令 —— 可把备份 zip / db **直接拖到可执行文件上**。

| 参数 | 必填 | 说明 |
|---|---|---|
| `<INPUT>` | 是 | `.zip` 备份或 `.db` |
| `-o, --output <PATH>` | 否 | 输出目录，或显式 `.zip` 文件；省略则写到源文件同目录 |

日志用 `println!`（中文，对齐 cherry），不使用 env_logger / 进度条。

---

## 9. 错误处理

- 输入不存在 / 不是文件 / 找不到 db / SQLite 打不开 → `bail!`，退出码 `1`。
- 单个会话失败 → 记入 `export-failures.md`，不影响其余；最终退出码 `0`。

---

## 10. 技术选型

- Rust 2024；包名 `afterchat-rikka`，bin 名 `rikka`。
- 依赖：`chatformat`、`anyhow`、`clap`(derive)、`rayon`、`serde`、`serde_json`、
  `zip` 2(deflate)、`rusqlite` 0.32(**bundled**)、`tempfile`。
- 渲染用 `rayon` 并行；ZIP 串行写入。首次编译会编 bundled SQLite。

```
crates/rikka/
├── Cargo.toml
├── src/{main.rs, lib.rs}
└── tests/integration_cli.rs
```

---

## 11. 测试

- 单元：`strip_hashes`（契约 §8 全表 + 代码保护 + 整条加粗断言）、`remove_bold_outside_code`、
  `sanitize_filename`、settings 解析、model 解析链、`select_index` 选中 / 越界 / 空 node / 旧 nodes 兜底、
  空消息跳过、reasoning-only 不出 Response 头、媒体 part 渲染。
- 集成：临时目录用 `rusqlite` 合成 `rikka_hub.db` + `settings.json` → 打包成 zip → 跑二进制，校验
  条目名（`助手/时间-标题.md`）、从新到旧、Metadata、角色头、重名 `-2`、`export-failures.md`。
- WAL 回放：合成带 WAL 的库（不 checkpoint），验证 WAL 中数据可见。
- 手工回归：对本目录 `rikkahub_backup_*.zip` 跑一遍核对。

---

## 12. 已确认决策

1. Metadata **加** `Conversation ID` / `Assistant` 补充键。
2. System **只导静态 system prompt**，不含 memory / 工具 / modeInjection / lorebook 动态注入。
3. 包内排序**按 qwen**（从新到旧）。
4. 媒体只留占位引用 `![image](url)` / `[fileName](url)`（媒体文件已被删除）。
5. 工具类 part **忽略**（当前样本无此类；如需保留可后续加）。
6. zip / db 输入**一律产出 zip**。
