# 系统架构

本仓库采用分层设计，将「**解析源数据 → 导出规范 ChatFormat**」划分为两个主要层级：

1. **`crates/afterchat-chatformat`**：平台无关的公共核心库。各转换器仅需负责将特定平台的 JSON/SQLite 数据结构映射为标准的 `Conversation` 模型；后续的 Markdown 渲染、文件命名、时间解析与 ZIP 打包归档全部由该库统一处理。
2. **`crates/afterchat-<platform>`**：各平台的专用解析器与 CLI 二进制。职责仅限于读取输入文件并提取有效会话数据。

该架构确保各转换器产出的 Markdown 与 ZIP 归档结构保持严格一致，且在格式规范升级时仅需维护公共库。

---

## 分层设计

```
crates/afterchat-chatformat   纯 lib，无文件 I/O（ZIP 写入除外，见 §6）
  ├── lib.rs                  数据模型 Conversation / Message / Role / MetadataLine + render()
  ├── markdown.rs             契约 §5：# 标题 → **加粗**，代码围栏 / 行内代码保护
  ├── naming.rs               契约 §7：非法字符清理、长度截断、路径段兜底
  ├── time.rs                 本地时间格式化、RFC3339 / epoch 解析、ZIP DOS 时间
  └── zip.rs                  契约 §6：排序、条目命名、重名 -2/-3、失败报告、mtime

crates/afterchat-<platform>   bin，输入解析与字段提取 → Conversation
gui/                          基于 Tauri 的桌面客户端，通过 Sidecar 调用上述 bin
```

### chatformat 数据模型

```rust
Conversation {
    title, model, time_secs, sort_ms, url, extra, group, id, messages
}
Message { role: Role, thinking: Vec<String>, body: Vec<String> }
Role::{System, User, Assistant}
```

- `model` 缺失时由转换器填充 `UNKNOWN_MODEL = "Unknown"`（契约 §2 规定 Model 键为必填）。
- `extra` 用于记录平台特定的 Metadata 扩展键（如 `Conversation ID` / `Topic ID` / `Assistant`）。
- `group` 非空时，ZIP 内条目将归类至 `<group>/` 子目录（cherry / rikka 按助手名称分组）。
- `thinking` 仅针对 `Role::Assistant` 生效：非空时渲染 `#### 🤔 Thought Process`，且在包含回复正文时补充 `#### 💡 Response`。

### 转换器数据映射

| crate | 输入 | 时间戳来源 | `group` | 扩展 Metadata |
| --- | --- | --- | --- | --- |
| `ai-studio` | `chunkedPrompt.chunks[]` | 输入文件 mtime | 无（单文件 / 目录树输出） | `Temperature` / `Top P` / `Top K` / `Max Output Tokens` |
| `cherry` | `data.json` 的 topics + message_blocks | `createdAt`（RFC3339 或数字时间戳） | 助手名称 | `Topic ID` / `Assistant` |
| `qwen` | 导出的 sessions JSON | session 时间字段 | 助手名称 | `Conversation ID` |
| `claude` | `conversations.json` | `created_at`（RFC3339 UTC 转换为本地时间） | 无 | — |
| `rikka` | SQLite（`ConversationEntity` + `message_node`） | `createAt`（毫秒时间戳） | 助手名称 | `Conversation ID` / `Assistant` |

### 输出形态

- `claude` / `cherry` / `qwen` / `rikka`：**统一输出 ZIP 压缩包**（`chat-export-{platform}-all-{ms}.zip`）。
  - 无有效消息的空会话不生成独立的 `.md`，而是汇总记录至包内的 `export-failures.md`，进程正常退出。
- `ai-studio`：**保持 Markdown 格式**（源数据通常为单会话结构，直接输出 `.md` 或 `.md` 目录树），不进行 ZIP 包装。

---

## 规范契约

输出格式的权威定义参见 [CHATFORMAT.md](./CHATFORMAT.md)（§1–§7）：

1. §1 顶层结构：Metadata + Conversation
2. §2 Metadata 键（Model / Time / URL 及平台扩展键）
3. §3 消息结构（System / User / Assistant、Thought Process / Response）
4. §4 角色识别与兜底策略
5. §5 行内语法规则：`#` 标题转 `**加粗**`，代码围栏与行内代码保护
6. §6 ZIP 打包规范：`{group}/{YYYYMMDD-HHmmss}-{title}.md`、时间降序排列与 `export-failures.md`
7. §7 文件名规则：非法字符清理、长度截断与空标题兜底 `Untitled_Conversation`

任何转换器**不得**自行拼接 Markdown 或构建 ZIP；若需变更输出规范，须优先更新契约并调整 `chatformat` 公共库。

---

## 测试策略

- `crates/afterchat-chatformat`：单元测试覆盖字符转义、命名截断、时间解析、排序逻辑、重名处理与失败报告生成。
- 转换器集成测试：每个转换器在 `tests/integration_cli.rs` 中使用**合成测试数据（Fixture）**验证完整 CLI 执行流程，断言 ZIP 条目名、排序、Metadata 与消息体结构。
- GUI：`gui/` 下的路由逻辑为前端分支判断，变更时以手动交互验证为主。

---

## 仓库演进历史

本仓库早期采用「主仓库 + 5 个 Git submodule」架构。由于 submodule 配合 Cargo workspace 存在多重提交繁琐、跨仓库 CI 权限复杂及构建耦合度高的问题，现已通过 `git read-tree` 将各子模块代码及历史提交合并为单一 Monorepo（`git log` 仍完整保留各模块历史），原有独立仓库已归档备份。详细记录参见提交 `2f599dc`。
