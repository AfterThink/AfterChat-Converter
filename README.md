# rikkahub-db-converter

把 **RikkaHub 的备份**（内含 SQLite 数据库）转换成 **AfterChat 对话 Markdown / ZIP** 的命令行工具（bin 名 `rikka`）。

输出格式严格遵循 [`CHATFORMAT-CONVERTER.md`](./CHATFORMAT-CONVERTER.md)；
实现思路对齐 `cherry-studio-backup-json-converter`：按助手分目录、附加 Metadata 键、兜底命名。

---

## 输入 → 输出

| 输入 | 输出 |
| --- | --- |
| RikkaHub 备份 `*.zip`（含 `rikka_hub.db` / `-wal` / `settings.json`） | 一个 `chat-export-rikka-all-<毫秒时间戳>.zip` |
| 裸 `rikka_hub.db`（可选同目录 `-wal` / `settings.json`） | 同上 |

- Windows 拖拽即用：把备份 zip 拖到 `rikka.exe` 上即可，输出落在**源文件同目录**。
- 始终打包成 zip；每个会话一份 `.md`，按 `<助手名>/<YYYYMMDD-HHmmss>-<标题>.md` 分目录。
- 包内按**对话时间从新到旧**排列；条目修改时间设为对话时间。
- 无对话可导出的会写进 `export-failures.md`（没有失败则不生成）。

## 构建

```bash
cargo build --release
# 产物：target/release/rikka.exe
```

需要 Rust 1.88+（Edition 2024）。`rusqlite` 使用 **bundled** 特性，会编译内置 SQLite，因此需要
可用的 C 工具链（Windows 上为 MSVC Build Tools）。

> 若在 MSYS/Git-Bash 里构建碰到 `link: extra operand`，说明 PATH 里的 GNU `link` 抢在 MSVC 前面，
> 把 MSVC 的 `...\VC\Tools\MSVC\<ver>\bin\Hostx64\x64` 放到 PATH 最前即可（用 PowerShell/开发者命令行则无此问题）。

## 用法

```bash
# 拖拽等价形式：输出到源文件同目录
rikka rikkahub_backup_20260912_135208.zip

# 指定输出目录
rikka backup.zip -o out/

# 指定输出 zip 文件
rikka backup.zip -o my-export.zip
```

### 参数

| 参数 | 说明 |
| --- | --- |
| `<INPUT>` | RikkaHub 备份 `.zip` 或裸 `.db`（必填） |
| `-o, --output <PATH>` | 输出目录，或显式 `.zip`；省略则写到源文件同目录 |

## 输出规范

```markdown
## Metadata

- **Model:** `gemini-3.1-flash-lite`
- **Time:** 2026-08-05 08:27:33 +08:00
- **Conversation ID:** `7b6a18fd-…`
- **Assistant:** `Gemini 3 Flash`

## Conversation

### ⚙️ System

（静态 system prompt，可选）

### 🧑‍💻 User

用户消息……

### 🤖 Assistant

#### 🤔 Thought Process

（思维链，可选）

#### 💡 Response

助手回复……
```

## 实现要点

- **WAL 回放**：备份里的 `rikka_hub.db` 可能有未 checkpoint 的数据在 `-wal` 里。工具把 db + wal
  拷到临时目录后由 SQLite 重建 `-shm` 并回放；**故意不读 `-shm`**（共享内存易变，带进来反而会阻止回放）。
- **消息重建**：按 `message_node.node_index` 排序，每个 node 取 `messages[select_index]`，与 App
  `Conversation.currentMessages` 同口径；旧备份回退解析 `ConversationEntity.nodes` JSON。
- **跨版本兼容**：读 `ConversationEntity` 用 `SELECT *` 按列名取值，老版本缺少
  `custom_system_prompt` 等列也不会报错；没有 `message_node` 表时走 `nodes` JSON 兜底。
- **Model**：取第一条 assistant 消息的 `modelId`，经 `settings.json` 的 `providers[].models[]`
  映射到 `displayName`；查不到则用原始 id，最终兜底 `Unknown`。
- **System**：与 App `GenerationLoop` 同口径 —— `allowConversationSystemPrompt` 且对话自定义提示非空时用
  对话的，否则用助手的 `systemPrompt`。不导出 memory / 工具 / modeInjection / lorebook 等运行时注入。
- **正文**：`reasoning` → 思维链；`text` → 回复；`image`/`video`/`audio`/`document` → 占位引用
  （媒体文件不在备份里）；工具类 part 忽略。
- **标题转义**：正文里的 `#` 标题按契约转成整条加粗，`strip_hashes` 保护代码围栏与行内代码。
- 渲染用 `rayon` 并行；ZIP 串行写入。

## 测试

```bash
cargo test
```

- 单元测试：`strip_hashes`（含代码围栏 / 行内代码 / 整条加粗）、命名、settings 解析、消息重建、渲染。
- 集成测试：临时目录合成 SQLite 库 → 打包成 zip → 运行二进制，校验条目名 / 排序 / Metadata /
  `export-failures.md`，以及 **WAL 回放**。

---

- 输出契约：[`CHATFORMAT-CONVERTER.md`](./CHATFORMAT-CONVERTER.md)
- 实现规格：[`SPEC.md`](./SPEC.md)
