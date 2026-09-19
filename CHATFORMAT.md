<!-- 同步自 AfterChat-Script-Dev/docs/ChatFormat.md（唯一权威源）；请勿在此单独修改，改动请回上游。 -->

# 对话文件格式规范

本节是“供应商 JSON -> AfterChat Markdown”的**目标契约**。后续让 LLM 写转换器时，直接以本节为准。

### 1 物理约束

- 文件扩展名：`.md`
- 编码：UTF-8（允许 BOM）
- 换行：`LF` 或 `CRLF` 均可

### 2 顶部 Metadata 区（可选）

Metadata 位于 `## Conversation` 之前，推荐使用以下键：

- `- **Model:** \`<model-name>\``
- `- **Time:** \`<time>\``
- `- **URL:** \`<url>\``

### 3 Conversation 正文区

正文必须有分隔行：

```markdown
## Conversation
```

消息头规则：

- 用户消息头建议固定为：`### 🧑‍💻 User`
- 助手消息头建议固定为：`### 🤖 Assistant`
- 系统提示消息头建议固定为：`### ⚙️ System`（可选，无系统提示时可省略）
- 未识别的角色可以用 `### 🤖 Assistant` 兜底

消息体规则：

- 一条消息内容范围：当前角色头到下一个角色头之间的文本
- 保留原 Markdown 内容（代码块、列表、引用等）
- 顺序必须与原始对话时间顺序一致
- **不保留 markdown 井号标记**：消息体中的 `#` `##` `###` 等标题，应转换为 `**加粗**` 形式。
  - **代码块内部除外**：围栏（```` ``` ```` / `~~~`）之内的 `#` 是代码/注释，必须原样保留，不得改写。

Thought/Response（可选）：

- 若助手消息内包含以下两个标题，会被拆分为两段显示：
  - `#### 🤔 Thought Process`
  - `#### 💡 Response`
- 标题文本建议保持**完全一致**，以确保稳定拆分

### 4 标准模板

```markdown
## Metadata

### Run Settings
- **Model:** `openai/gpt-4.1`
- **Tags:** `provider/openai, topic/nutrition, lang/zh`
- ...

## Conversation

### 🧑‍💻 User
“大米饭会让人变胖”的说法是怎么来的？

### 🤖 Assistant
#### 🤔 Thought Process
先区分“热量盈余”与“单一食物致胖”这两个层面，再给出证据路径。

#### 💡 Response
“大米饭致胖”这类说法主要来自对精制碳水和总热量摄入的长期观察...
```
