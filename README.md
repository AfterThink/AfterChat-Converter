# qwen-markdown-converter

Rust CLI tool for converting exported Qwen-style JSON chat records into Markdown files.

## 中文说明

### 功能
- 单文件模式：将单会话 JSON 转为一个 `.md`
- 批量拆分模式：若 JSON 顶层为 `{ success, request_id, data: [...] }`，则把 `data` 中每个会话转为独立 `.md`
- 目录模式：递归处理目录下所有 `.json`，并保持输出目录树
- 支持 Windows 拖拽：将 JSON 文件拖到可执行程序上可直接转换（无子命令）
- 并行处理：使用 `rayon` 并行处理大量文件
- 错误日志：单文件失败不会中断批量流程，最终失败写入 `error.log`
- 时间回写：从会话时间戳提取时间并设置输出 Markdown 文件时间

### 依赖
- Rust 1.85+（Edition 2024）

### 构建
```bash
cargo build --release
```

### 命令示例
```bash
# 标准转换
cargo run -- convert -i examples/small.json -o out

# 包装大 JSON 拆分转换
cargo run -- convert -i examples/large.json -o out

# 目录递归转换
cargo run -- convert -i examples/batch -o out

# 拖拽模拟（不写子命令）
cargo run -- examples/small.json
```

### 输入识别规则
- 顶层 `object` 且包含 `data` 数组：按大 JSON 拆分模式
- 顶层 `array`：按会话数组处理（长度为 1 时输出单文件；长度 > 1 时拆分输出）
- 顶层 `object`（非包装结构）：按单会话处理

### 输出规范（核心）
- 输出 Markdown 包含：
  - `## Metadata`
  - `### Run Settings`
  - `## Conversation`
- `Model` 字段固定写为 `models/<modelname>`（若原值已是 `models/...` 则保持）。
- 文件命名：
  - 单会话默认优先使用会话 `title` 作为文件名（非法字符自动清理）
  - 若 `title` 缺失则回退到会话 `id`，再回退到源文件名
  - 大 JSON 拆分不再带序号前缀；如重名自动追加 `-2/-3` 后缀
- 消息头：
  - `### 🧑‍💻 User`
  - `### 🤖 Assistant`
- 若助手消息包含思考与回答分段，则渲染：
  - `#### 🤔 Thought Process`
  - `#### 💡 Response`
- 对于“重新提问/重答”分支，会重复用户问题并分别附上各分支回答

## English

### Features
- Single file conversion from JSON to one Markdown file
- Wrapped large JSON split mode for `{ success, request_id, data: [...] }`
- Recursive directory mode with output tree preservation
- Windows drag-and-drop support (no subcommand required)
- Parallel processing with Rayon
- Error logging to `error.log` without aborting the whole batch
- Output file timestamp backfill from extracted chat timestamps

### Build
```bash
cargo build --release
```

### Usage
```bash
cargo run -- convert -i examples/small.json -o out
cargo run -- convert -i examples/large.json -o out
cargo run -- convert -i examples/batch -o out
cargo run -- examples/small.json
```

## Testing
```bash
cargo test
cargo clippy -- -D warnings
```

## Notes
- Markdown 输出格式为程序内置固定格式（与 `chatformat.txt` 约定一致），无需额外模板文件。
- When converting multiple inputs, `-o` must be a directory path.
