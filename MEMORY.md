# Memory (Project Conventions)

稳定决策记录。改动前先读这里；推翻结论时同步更新本文。

## 输出契约

- 唯一权威源是 `AfterChat-Script-Dev/docs/ChatFormat.md`；本仓库 `CHATFORMAT.md` 是**同步副本**，
  顶部带同步注释，**不要在此单独修改**。
- JS 适配器（`afterchat.user.js` qwen 的 `toMarkdown`）是**格式化基准**：Rust 侧以其为准做逐字节对齐。
- 但「JS 有 bug 时按对的来」，不继承其缺陷（见下方「已知偏离」）。

## 输入形态

- **只保留两种**：顶层数组（单体）、`{ success, request_id, data: [...] }`（全部）。
  旧的目录递归 / 批量 / `small.json`、`large.json` 样例已删除。
- 形态由**结构**决定，不由数量决定：`data` 数组即使只有 1 条也输出 `.zip`。
- 正文来源固定为 **`chat.messages`（数组）**，不使用 `chat.history.messages`（map）。
  理由：与 JS 口径一致；官方导出里两者长度不一致的情况真实存在（195 个会话中 17 个），
  以数组为准可使顺序与网页展示一致。

## Metadata

- 只输出 `Model` / `Time` / `URL` 三键。
- 已**删除**：`Run Settings` 段、`Tags`、`Conversation ID`、`User ID`、`Request ID`、
  `Chat Type`、`Generated At`、以及 `models/<name>` 命名空间前缀。
- `Model` 口径：`chat.messages` 顺序遍历，`modelName` 优先，再 `models[0]`，首个命中即返回，不改大小写。
- `Time`：`created_at` → 本地时区 `YYYY-MM-DD HH:mm:ss ±HH:MM`。
- `URL`：`https://chat.qwen.ai/c/<id>`。

## 内容渲染

- `content_list[].phase` 是唯一分派依据：
  `think` / `thinking_summary` → 思考；`answer` → 回复；工具 phase（`web_search`、`image_gen_tool`）→ 忽略。
- `thinking_summary.content` 是空串，真内容在 `extra.summary_title.content[]`（→ `**标题**`）
  与 `extra.summary_thought.content[]`（→ `- 条目`）。
- 没有思考时不输出 `#### 💡 Response` 头。
- `# 标题` → `**加粗**`。

## 命名 / 打包

- 单体 → `<title>.md`，上限 60 字符，输出到**源文件同目录**（拖拽体验）。
- 全部 → `chat-export-qwen-all-<epoch_ms>.zip`。
- zip 内条目 → `YYYYMMDD-HHMMSS-<title>.md`，上限 100，按会话时间**降序**，重名加 `-2`/`-3`。
- 压缩用 Deflate。
- 解析失败的会话写入 zip 内 `export-failures.md`，不中断整体。

## 健壮性

- 坏项跳过并记录，不 abort 整个转换；但数组为空 / 全坏则报错退出。
- 时间戳容忍秒 / 毫秒 / 数字字符串。
- 读取时剥离 UTF-8 BOM（`serde_json` 不接受 BOM）。
- 会话对象必须含 `chat`，否则拒绝 —— 防止误吞 `tauri.conf.json` 之类无关文件。

## 已知偏离 JS（有意为之）

- **代码围栏内不转换井号**。JS 的 `stripHashes` 是朴素正则，会把 Python/Shell 的 `# 注释`
  改成 `**注释**`（真实数据上 195 个会话里有 866 行受影响）。Rust 侧做了围栏感知，
  且 `CHATFORMAT.md` 本身就要求「保留原 Markdown 内容（代码块…）」。
  golden fixture 内围栏行数为 0，因此对齐测试不受影响。
  > 待办：JS 里 `stripHashes` 被复制粘贴了 30+ 份，尚未统一修；后续建议抽成共享 helper。

## Windows 构建提示

- Git Bash 的 `/usr/bin/link` 会遮蔽 MSVC 的 `link.exe`，导致链接失败。构建前需把 MSVC 工具链前置：
  ```bash
  export PATH="/c/Program Files/Microsoft Visual Studio/2022/Community/VC/Tools/MSVC/14.40.33807/bin/Hostx64/x64:$PATH"
  ```

## 隐私

- `examples/chat-export-*.json` 是真实导出（含私人对话），已 `.gitignore`，**不提交**。
- 分析真实数据时只输出结构统计，不打印标题与正文。
