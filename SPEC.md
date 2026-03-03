# 规格说明（SPEC）

## 1. 目标范围
本工具用于将 Qwen 风格的 JSON 会话导出转换为 Markdown 对话文件。  
输出格式为程序内置固定结构，不依赖外部模板文件。

## 2. CLI 设计

## 命令
- `qwen-json-converter convert -i <input> [-o <output>] [--progress true|false]`
- Windows 拖拽模式：`qwen-json-converter <path1> [path2 ...]`

## 参数
- `-i, --input <path>`：输入 JSON 文件或目录
- `-o, --output <path>`：
  - 目录路径：用于目录模式或批量拆分输出
  - `.md` 文件路径：仅在单会话输出时允许
- `--progress <bool>`：是否显示 `indicatif` 进度条

## 参数校验
- 缺少输入路径：报错退出
- 多输入时 `-o` 指向单个 `.md` 文件：报错退出

## 3. 输入识别模型

## 单会话
- 顶层为对象且不含 `data` 数组，或顶层数组长度为 1 的会话对象数组

## 大 JSON（包装结构）
- 顶层对象包含：
  - `success: bool`
  - `request_id: string`
  - `data: [session, session, ...]`

## 目录模式
- 递归扫描目录下所有 `*.json`

## 4. 输出 Markdown 约定

## 结构分区
- `## Metadata`
- `### Run Settings`
- `## Conversation`

## 文件命名规则
- 单会话输出：
  - 优先：清洗后的 `title`
  - 回退：清洗后的 `id`
  - 最终回退：输入源文件名（不含扩展名）
- 大 JSON 拆分输出：
  - 使用 `{sanitized_title_or_id}.md`
  - 重名自动追加后缀：`-2`、`-3`、...
  - 不使用前置序号

## Metadata 字段（当前实现）
- `Model`（写为 `models/<modelname>`）
- `Tags`（归一化后）
- `Conversation ID`
- `User ID`
- `Request ID`（包装结构时可写）
- `Chat Type`
- `Sub Chat Type`
- `Source`
- `Generated At (UTC)`

## 消息区格式
- 用户消息头：`### 🧑‍💻 User`
- 助手消息头：`### 🤖 Assistant`
- 若存在思考段：
  - `#### 🤔 Thought Process`
  - `#### 💡 Response`

## 分支展开（重答）
- 同一个用户消息有多个助手子节点时：
  - 重复该用户消息
  - 每个分支分别配对应助手回答

## 5. 字段语义与提取

## tags 归一化
- 来源：优先 `session.meta.tags`
- 支持：数组或字符串
- 规则：
  - 去首尾空白、外层引号/反引号、前导 `#`
  - 分隔优先级：逗号 `,` > 分号 `;` > 空白
  - 大小写不敏感去重，保留首次写法
  - 若检测到“乱码字符集合”标签（如大量单字符符号数组），判定为无效并忽略
  - 忽略后回退到默认标签策略（`chat/...`、`sub/...` 或 `untagged`）

## model 规则
- 来源优先级：assistant `modelName` > assistant `model` > `unknown`
- 输出统一加命名空间：`models/<modelname>`

## thinking 规则
- 可能来源：
  - `reasoning_content`
  - `content_list` 中 phase 包含 `thinking` 的项
  - `content_list[].extra` 内的 summary/thought 结构
- 统一渲染到 `#### 🤔 Thought Process`

## 时间戳与文件时间
- 来源：消息或会话级时间字段
- 毫秒时间戳会归一化为秒
- 写回输出 Markdown 文件时间（mtime）

## 6. 并发与性能策略
- 使用 `rayon` 并行处理
- 线程数来源：`std::thread::available_parallelism()`
- 目录/多文件：并行处理各 JSON
- 大 JSON 拆分：按 `1000` 会话分批，批内并行写入

## 7. 错误处理与退出码

## 行为
- 单文件解析/写入失败会记录，不中断整体批处理
- 最终错误写入 `error.log`
- 对于脏 `meta.tags`（字符袋）不视为致命错误，仅记录 warning 并继续转换

## 退出码
- `0`：全部成功
- `1`：存在失败项（见 `error.log`）

## 8. 日志与进度
- 进度条：`indicatif`
  - 批处理显示文件级进度
  - 大 JSON 单文件可显示会话级进度
- 日志：
  - 受 `RUST_LOG` 控制
  - 默认 `info`
  - `debug` 级可输出更多提取细节（如 tags）

## 9. 测试覆盖目标
- 单文件小 JSON 转换断言
- 大 JSON 拆分文件数与 `data.len()` 一致
- 目录模式输出结构校验
- `meta` 缺失时回退策略校验
- Windows 拖拽式调用校验

## 10. 性能目标（参考）
- 目标：8C/16G 环境下，1 万文件处理时间不超过约 30 秒
- 依赖因素：磁盘性能、JSON 大小与结构复杂度
