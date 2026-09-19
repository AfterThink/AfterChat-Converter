# 架构

本仓库把「**输入各家备份 → 输出统一 ChatFormat**」拆成两层：

1. **`crates/afterchat-chatformat`**：与平台无关的公共库。所有转换器都只负责「把自家 JSON/SQLite 映射成 `Conversation`」，剩下的渲染、命名、时间、ZIP 打包全部由它完成。
2. **`crates/afterchat-<platform>`**：各平台的解析器 + CLI，尽量只保留「读输入、抽字段」的逻辑。

这样做的目的：5 个转换器输出的 Markdown 与 ZIP 结构**逐字节一致**，改契约时只动一个地方。

---

## 分层

```
crates/afterchat-chatformat   纯 lib，不读文件、不写文件（ZIP 写入除外，见 §6）
  ├── lib.rs                  数据模型 Conversation / Message / Role / MetadataLine + render()
  ├── markdown.rs             契约 §5：# 标题 → **加粗**，代码围栏 / 行内代码保护
  ├── naming.rs               契约 §7：非法字符清理、长度截断、路径段兜底
  ├── time.rs                 本地时间格式化、RFC3339 / epoch 解析、ZIP DOS 时间
  └── zip.rs                  契约 §6：排序、条目命名、重名 -2/-3、失败报告、mtime

crates/afterchat-<platform>   bin，只做输入解析 → Conversation
gui/                          Tauri 桌面壳，sidecar 调用上面的 bin
```

### chatformat 的数据模型

```rust
Conversation {
    title, model, time_secs, sort_ms, url, extra, group, id, messages
}
Message { role: Role, thinking: Vec<String>, body: Vec<String> }
Role::{System, User, Assistant}
```

- `model` 缺失时由转换器填 `UNKNOWN_MODEL = "Unknown"`（契约 §2 要求 Model 键始终存在）。
- `extra` 用来放平台特有的 Metadata 键（如 `Conversation ID` / `Topic ID` / `Assistant`）。
- `group` 非空时，ZIP 条目会放进 `<group>/` 子目录（cherry / rikka 用助手名分组）。
- `thinking` 只对 `Role::Assistant` 有意义：非空时渲染 `#### 🤔 Thought Process`，并在有正文时补 `#### 💡 Response`。

### 转换器映射

| crate | 输入 | 时间来源 | `group` | 额外 Metadata |
| --- | --- | --- | --- | --- |
| `ai-studio` | `chunkedPrompt.chunks[]` | 输入文件 mtime | 无（单文件 / 目录树输出） | `Temperature` / `Top P` / `Top K` / `Max Output Tokens` |
| `cherry` | `data.json` 的 topics + message_blocks | `createdAt`（RFC3339 或数字） | 助手名 | `Topic ID` / `Assistant` |
| `qwen` | 导出的 sessions JSON | session 时间字段 | 助手名 | `Conversation ID` |
| `claude` | `conversations.json` | `created_at`（RFC3339 UTC → 本地） | 无 | — |
| `rikka` | SQLite（`ConversationEntity` + `message_node`） | `createAt` 毫秒 | 助手名 | `Conversation ID` / `Assistant` |

### 输出形态

- `claude` / `cherry` / `qwen` / `rikka`：**永远输出 ZIP**（`chat-export-{platform}-all-{ms}.zip`）。
  - 空对话不生成 md，改为汇总进包内 `export-failures.md`，进程仍然退出 0。
- `ai-studio`：**保留 Markdown 形态**（JSON 本就单会话，输出 `.md` / `.md` 目录树），不做 ZIP 包装。

---

## 契约

输出格式的权威定义是 [CHATFORMAT.md](./CHATFORMAT.md)（§1–§7）：

1. §1 顶层结构：Metadata + Conversation
2. §2 Metadata 键（Model / Time / URL + 平台扩展）
3. §3 消息结构（System / User / Assistant、Thought Process / Response）
4. §4 角色兜底规则
5. §5 行内语法：`#` 标题转 `**加粗**`，代码围栏与行内代码不动
6. §6 ZIP 打包：`{group}/{YYYYMMDD-HHmmss}-{title}.md`、时间降序、`export-failures.md`
7. §7 文件名：删除非法字符、长度上限、空标题兜底 `Untitled_Conversation`

任何转换器**不得**自行拼 Markdown / 自行写 ZIP；需要新行为时先改契约，再改 `chatformat`。

---

## 测试策略

- `crates/afterchat-chatformat`：单元测试覆盖转义、命名、时间、排序、重名、失败报告。
- 每个转换器：`tests/integration_cli.rs` 用**合成 fixture**（不提交真实备份）跑一遍完整 CLI，断言 ZIP 条目名、排序、Metadata 与消息结构。
- GUI：`gui/` 下的路由逻辑是纯 JS 分支，改动时以手动拖拽验证为主。

---

## 历史

本仓库原先是「umbrella repo + 5 个 git submodule」。submodule + Cargo workspace 会导致双重提交、跨仓库 CI token、构建强耦合，因此已用 `git read-tree` 把 5 个子仓库的历史并入本仓库（`git log` 中仍能看到各自的提交），旧的独立仓库保留作为备份。详见提交 `2f599dc`。
