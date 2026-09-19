# qwen-json-converter

把 **Qwen 网页版导出的 JSON** 转换成 **AfterChat 对话 Markdown / ZIP** 的命令行工具（bin 名 `qwen`）。

输出格式严格遵循 [`CHATFORMAT.md`](./CHATFORMAT.md)（同步自 AfterChat-Script-Dev 的 `docs/ChatFormat.md`），
并与 AfterChat 用户脚本里的 JS 适配器**逐字节对齐**（见下方「一致性」）。

---

## 功能

| 输入 | 输出 |
| --- | --- |
| **单体导出**：顶层 JSON 数组 | 一个 `.md` |
| **全部导出**：`{ success, request_id, data: [...] }` | 一个 `.zip`（内含每个会话一个 `.md`） |

- Windows 拖拽即用：把 JSON 拖到 `qwen.exe` 上即可，输出落在**源文件同目录**
- 多输入：一次传多个文件，各自按形态输出
- 并行渲染（`rayon`）+ 可选进度条（`indicatif`）
- 单条会话解析失败不会中断整体；失败项写入 zip 内的 `export-failures.md`

## 构建

```bash
cargo build --release
# 产物：target/release/qwen.exe
```

需要 Rust 1.85+（Edition 2024）。

## 用法

```bash
# 拖拽等价形式：输入直接作为位置参数，输出到源文件同目录
qwen chat-export-1789787251934.json          # -> “亚”字读音声调.md
qwen chat-export-1789787193900.json          # -> chat-export-qwen-all-<epoch_ms>.zip

# 指定输出目录
qwen single.json -o out/

# 指定输出文件（仅单输入时可用）
qwen single.json -o out/renamed.md

# 多个输入
qwen a.json b.json b-all.json -o out/

# 关掉进度条（CI / 重定向日志时）
qwen big.json --progress false
```

### 参数

| 参数 | 说明 |
| --- | --- |
| `<JSON>...` | 一个或多个输入 JSON 文件（必填） |
| `-o, --output <PATH>` | 输出文件或目录；省略则输出到各自源文件同目录 |
| `--progress <BOOL>` | 强制开关进度条（默认跟随终端是否为 TTY） |

## 输入形态

工具只认两种形态，其余一律报错退出（避免误吞无关 JSON）：

1. **单体导出** —— 顶层是数组，通常只有一个会话
   ```json
   [ { "id": "...", "title": "...", "created_at": 1700000000, "chat": { "messages": [...] } } ]
   ```
2. **全部导出** —— 顶层是包装对象，`data` 是会话数组
   ```json
   { "success": true, "request_id": "...", "data": [ { ... }, { ... } ] }
   ```

兼容输入（按单体处理）：顶层对象里 `data` 是**单个会话对象**，或顶层本身就是**会话对象**。
任何形态都要求最终对象含有 `chat` 字段，否则报错。

> 会话正文取自 `chat.messages`（数组），与 JS 适配器口径一致。

## 输出规范

```markdown
## Metadata

- **Model:** `qwen3.5-plus`
- **Time:** 2026-09-17 21:19:12 +08:00
- **URL:** https://chat.qwen.ai/c/<conversation-id>

## Conversation

### 🧑‍💻 User

...

### 🤖 Assistant

#### 🤔 Thought Process

...

#### 💡 Response

...
```

- **Metadata 只有三个键**：`Model` / `Time` / `URL`（不再输出 Run Settings / Tags / User ID 等）
- `Model` 取值口径：遍历 `chat.messages`，每条消息先看 `modelName`，再回退 `models[0]`，取**首个命中**
- `Time` 来自 `created_at`，转**本地时区**，格式 `YYYY-MM-DD HH:mm:ss ±HH:MM`
- `URL` 固定为 `https://chat.qwen.ai/c/<id>`
- `#### 💡 Response` **仅在存在思考内容时**才出现

### 消息体规则

- 保留原 Markdown；正文中的 `#`/`##`/`###` 标题会转成 `**加粗**`
- **代码围栏（```` ``` ```` / `~~~`）内部不做转换** —— 否则 Python/Shell 的 `# 注释` 会被误改成加粗
- 助手消息按 `content_list[].phase` 分派：

  | phase | 去向 |
  | --- | --- |
  | `think` | 思考（`.content`） |
  | `thinking_summary` | 思考（正文为空，读 `extra.summary_title.content[]` → `**标题**`，`extra.summary_thought.content[]` → `- 条目`） |
  | `answer` | 回复 |
  | `web_search` / `image_gen_tool` / `null` | 工具过程，**忽略** |

  `reasoning_content` 若存在会并入思考段；没有 `content_list` 时回退到 `content`。

### 文件命名

- **单体**：`<清洗后的 title>.md`（无 title 则回退 `id`），上限 60 字符
- **全部**：zip 名 `chat-export-qwen-all-<epoch_ms>.zip`；
  zip 内条目名 `YYYYMMDD-HHMMSS-<清洗后的 title>.md`（无时间则用补零序号），上限 100 字符
- 重名自动追加 `-2`、`-3`…；zip 内条目按会话时间**降序**（最新在前）

## 退出码

| 码 | 含义 |
| --- | --- |
| `0` | 全部成功 |
| `1` | 存在失败输入项（详情见 stderr） |

## 测试

```bash
cargo test            # 14 单元 + 10 集成
cargo clippy --all-targets -- -D warnings
```

## 一致性

Rust 渲染结果与 AfterChat-Script-Dev 的 JS 适配器（`afterchat.user.js` 中 qwen 的 `toMarkdown`）
在 golden fixture 上**逐字节相同**（SHA256 一致），可随时复核：

```bash
qwen /path/to/AfterChat-Script-Dev/tests/fixtures/qwen_raw.json -o /tmp/rust.md
cmp /tmp/rust.md /path/to/AfterChat-Script-Dev/tests/fixtures/qwen_golden.md && echo identical
```

## 仓库内容

| 路径 | 说明 |
| --- | --- |
| `src/lib.rs` | 全部转换逻辑 |
| `src/main.rs` | CLI 入口 |
| `tests/integration_cli.rs` | 端到端 CLI 测试 |
| `CHATFORMAT.md` | 输出契约（同步自 AfterChat-Script-Dev，勿在此单独改） |
| `SPEC.md` | 详细规格 |
| `MEMORY.md` | 稳定约定与决策记录 |
| `examples/` | 合成示例（`single.json` / `all.json`） |
| `scripts/` | 冒烟脚本与结构分析脚本 |

## Notes

- 真实导出样本（`examples/chat-export-*.json`）含个人对话内容，已被 `.gitignore` 排除，不提交。
- 目录递归模式、`error.log`、mtime 回写、`models/` 前缀、Tags 等旧行为已全部移除。
