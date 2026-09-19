# 规格说明（SPEC）

> 目标契约见 [`CHATFORMAT.md`](./CHATFORMAT.md)。本文描述 `qwen` 工具的具体实现规格。

## 1. 范围

把 Qwen 网页版导出的 JSON 会话转换为 AfterChat 对话 Markdown（单体）或 ZIP（全部）。
输出格式为程序内置，不依赖外部模板。

## 2. CLI 设计

```
qwen <JSON>... [-o <PATH>] [--progress <BOOL>]
```

无子命令 —— 可直接把文件拖到可执行文件上。

| 参数 | 必填 | 说明 |
| --- | --- | --- |
| `<JSON>...` | 是 | 一个或多个输入 JSON 文件路径 |
| `-o, --output <PATH>` | 否 | 输出文件（`.md`/`.zip`）或目录；省略则写到各源文件同目录 |
| `--progress <BOOL>` | 否 | 强制开关进度条；默认 `stdout().is_terminal()` |

### 参数校验

- 无输入 → 报错退出
- 多输入且 `-o` 指向单个文件（扩展名为 `.md`/`.zip`）→ 报错退出
- 输入不存在 / 不是文件 → 该项失败

## 3. 输入识别模型

顶层 JSON 值决定形态：

| 顶层结构 | 形态 | 输出 |
| --- | --- | --- |
| 数组 | 单体导出 | 长度 1 且无坏项 → `.md`；否则 → `.zip` |
| 对象，`data` 是数组 | 全部导出 | **始终** `.zip` |
| 对象，`data` 是对象且含 `chat` | 兼容单体 | `.md` |
| 对象，顶层含 `chat` | 兼容单体 | `.md` |
| 其它 / 缺 `chat` | 拒绝 | 报错退出 |

> 形态由**结构**决定，不由数量决定：`data` 数组即使只有 1 个会话也打包成 `.zip`，
> 这样「全部导出」语义稳定。

### 会话结构（用到的字段）

```jsonc
{
  "id": "...",            // URL 与文件名回退
  "title": "...",         // 文件名
  "created_at": 1700000000,   // Metadata Time（支持秒 / 毫秒 / 数字字符串）
  "updated_at": 1700000000,   // zip 排序（优先 updated_at）
  "chat": {
    "messages": [             // 正文来源
      { "role": "user", "content": "..." },
      {
        "role": "assistant",
        "content": "",
        "reasoning_content": null,      // 可选，并入思考段
        "modelName": "Qwen3.5-Plus",    // 优先
        "models": ["qwen3.5-plus"],     // 回退 models[0]
        "content_list": [
          { "phase": "think",            "content": "..." },
          { "phase": "thinking_summary", "content": "", "extra": {
              "summary_title":   { "content": ["标题"] },
              "summary_thought": { "content": ["要点 1", "要点 2"] }
          }},
          { "phase": "answer",           "content": "..." },
          { "phase": "web_search",       "content": "..." }   // 忽略
        ]
      }
    ]
  }
}
```

## 4. 输出 Markdown 约定

### 结构

```
## Metadata
<空行>
- **Model:** `<model>`
- **Time:** <YYYY-MM-DD HH:mm:ss ±HH:MM>
- **URL:** https://chat.qwen.ai/c/<id>
<空行>
## Conversation
<空行>
### 🧑‍💻 User
<空行>
<正文>
<空行>
### 🤖 Assistant
<空行>
[#### 🤔 Thought Process
<空行>
<思考正文>
<空行>
#### 💡 Response
<空行>]
<回复正文>
<空行>
```

- **Metadata 只有 `Model` / `Time` / `URL` 三键**（对齐 JS 当前实现，并符合 ChatFormat 对 Metadata 的「推荐键」）
- 整篇以 `lines.join("\n")` 拼接，与 JS 的 `lines.join('\n')` 一致
- 没有任何思考时，`#### 💡 Response` 头**不输出**（直接是正文）

### 角色头

- 用户：`### 🧑‍💻 User`
- 助手：`### 🤖 Assistant`
- 思考：`#### 🤔 Thought Process`
- 回复：`#### 💡 Response`

### 消息体规则

- 空正文的 user 消息跳过；思考与回复都为空的 assistant 消息跳过
- 同一段内多个片段（多条 `answer`、多个思考块）以 `\n\n` 连接
- 重复文本按 `trim()` 后去重
- `strip_hashes`：`^#{1,6}\s+(.+)$` → `**$1**`（逐行）
  - **跳过代码围栏内部**（```` ``` ```` / `~~~` 起止），只处理围栏外
  - **吸收标题内的 `**`**（`remove_bold_outside_code`，行内代码段除外），使整条标题落在一个加粗里；
    否则内外层 `**` 同级交错，CommonMark 会错配定界符（表现：强调不全 / 残留可见星号）
  - `strip_hashes` 应用于思考段与回复段整体（含 `thinking_summary` 生成的 `**标题**`，其本身不会被二次改写）

## 5. 字段语义与提取

### model

按 `chat.messages` 顺序遍历，每条消息：

1. `modelName` 非空 → 采用
2. 否则 `models[0]` 为字符串 → 采用

**首个命中即返回**，不做大小写归一（保持原样，例如 `qwen3.5-plus`）。
全部落空 → `unknown`。

### thinking / response 分派

见上表。要点：

- `thinking_summary` 的 `.content` 为空串，真实内容在 `extra.*.content[]`
- `reasoning_content` 可为字符串、数组或对象，递归收集其中所有字符串
- 工具 phase（`web_search`、`image_gen_tool`、`null`）一律忽略
- 无 `content_list` 字段时回退到 `message.content`

### 时间

- 来源：Metadata 用 `created_at`，zip 排序用 `updated_at`（缺失回退 `created_at`）
- 接受秒级 / 毫秒级（`>= 10_000_000_000` 视为毫秒）/ 数字字符串
- 输出按**本地时区**渲染

## 6. 命名规则

`sanitize_filename(name, max_len)`（对齐 JS）：

1. `\ / : * ? " < > |` → `_`
2. 控制符 → 空格
3. 连续空白折叠为单个空格
4. 超长按**字符**截断到 `max_len`，再去掉尾部空白 / `.` / `_` / `-`
5. 结果为空 → `untitled`

| 场景 | 上限 | 结果 |
| --- | --- | --- |
| 单体 `.md` | 60 | `<title>.md`（title 空则用 `id`） |
| zip 文件 | — | `chat-export-qwen-all-<epoch_ms>.zip` |
| zip 内条目 | 100 | `<YYYYMMDD-HHMMSS>-<title>.md` |

- zip 内条目重名追加 `-2`、`-3`…
- 无时间的会话排在有时间的之后（保持原相对顺序），条目前缀改用补零序号

## 7. 并发与性能

- 渲染阶段使用 `rayon` 的 `par_iter` 并行
- ZIP 写入串行（`zip` crate 的 writer 需要可变借用），压缩方式 `Deflate`
- 实测：195 会话 / 9.5 MB 导出 → 1.1 秒产出 1.0 MB zip

## 8. 错误处理与退出码

- 单个输入失败：记录 warning，继续处理其余输入，`failed += 1`
- 数组 / `data` 内单个会话解析失败：记入 `failures`，打包时写入 `export-failures.md`
- 输入数组为空数组 / 全部项非法 → 报错退出
- 退出码：`0` 全部成功；`1` 有失败项

## 9. 日志与进度

- 日志：`env_logger`，`RUST_LOG` 控制，默认 `info`
- 进度条：`indicatif`，渲染阶段按会话递增；非 TTY 时默认关闭

## 10. 编码

- 输入：UTF-8，**允许 BOM**（读取时剥离，因为 `serde_json` 不接受 BOM）
- 输出：UTF-8 无 BOM，`LF`

## 11. 测试覆盖

- 输入形态：顶层数组 / 包装 `data` 数组 / `data` 单对象 / bare 对象 / 非会话 JSON 拒绝
- 关键回归：`data` 数组长度 1 仍应产出 zip
- 内容：phase 分派、`thinking_summary` 的 `extra` 取值、工具 phase 忽略、无思考时不输出 Response 头
- 文本：井号转加粗、**整条标题加粗**（吸收内层 `**`，行内代码除外）、**代码围栏内不转**、BOM 输入
- 命名：`sanitize_filename` 边界、zip 条目时间降序、同时间戳加后缀
- CLI：拖拽式调用、`-o` 目录 / 文件、多输入
- 一致性：与 JS golden fixture 逐字节比对（人工脚本，见 README）
