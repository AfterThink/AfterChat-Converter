# AfterChat Converter

<p align="center">
  <img src="https://img.shields.io/badge/Rust-2024-orange?style=for-the-badge&logo=rust" alt="Rust 2024" />
  <img src="https://img.shields.io/badge/Tauri-1.x-24C8DB?style=for-the-badge&logo=tauri" alt="Tauri" />
  <img src="https://img.shields.io/badge/Converters-5-success?style=for-the-badge" alt="5 converters" />
  <img src="https://img.shields.io/badge/Platform-Windows-0078D6?style=for-the-badge&logo=windows" alt="Windows" />
</p>

把各主流 AI 平台的**对话备份**解析并转换为统一规范的 **AfterChat Markdown / ZIP** 格式，用于本地归档、数据迁移与离线阅读。

支持 **5 种备份来源**：Google AI Studio、Cherry Studio、Qwen、Claude 与 RikkaHub。生成的 Markdown 文档可直接导入 [AfterChat](https://github.com/AfterThink/AfterChat-App-Download) 工作区。

> 在线对话可通过 [AfterChat — LLM Chat Exporter](https://github.com/AfterThink/AfterChat-Script) 导出；本地已有的备份文件则通过本工具进行转换。

## 支持的备份

| 来源 | 输入 | 输出 |
| --- | --- | --- |
| Google AI Studio | 导出的 JSON | Markdown（单文件，或目录树） |
| Cherry Studio | 备份 ZIP | Markdown ZIP |
| Qwen | 网页版导出的 JSON | Markdown ZIP |
| Claude | 数据导出 ZIP | Markdown ZIP |
| RikkaHub | 备份 ZIP | Markdown ZIP |

转换后的结构示例：

```
chat-export-claude-all-1730000000000.zip
├── 20241114-101500-第一次对话.md
├── 20241113-090000-第二次对话.md
└── export-failures.md          # 无法解析或无有效消息的对话记录在此
```

按助手分目录的平台（Cherry / Qwen / RikkaHub）会生成分组子目录：

```
chat-export-rikka-all-1730000000000.zip
└── Gemini/
    └── 20241114-101500-第一次对话.md
```

## 下载

访问 [Releases](https://github.com/AfterThink/AfterChat-Converter/releases) 下载预编译版本：

| 文件 | 说明 |
| --- | --- |
| `afterchat-converter_<版本>_x64-setup.exe` | 安装版：桌面应用（含开始菜单快捷方式与卸载程序） |
| `afterchat-converter-<版本>-windows-x64.zip` | 绿色免安装版：解压后运行 `afterchat-converter.exe` 即可使用 |

## 使用方法

### 桌面应用

1. 打开应用，将备份文件（或包含备份的文件夹）拖入窗口；
2. 程序根据文件特征自动匹配对应的转换器；
3. 转换完成后，导出的 `chat-export-<平台>-all-<时间戳>.zip` 默认保存在源文件同级目录下。

可在窗口底部勾选「输出到自定义目录」。详细支持格式与路由规则参见 [docs/GUI.md](./docs/GUI.md)。

### 命令行工具

安装包或免安装压缩包中均包含以下 5 个独立可执行文件：

```powershell
rikka     RikkaHub-backup.zip        # RikkaHub 备份（自动读取压缩包内的 SQLite 数据库）
cherry    cherry-backup.zip          # Cherry Studio 备份
qwen      qwen-all.json              # Qwen 导出（支持传入多个文件）
claude    data-export.zip            # Claude 数据导出包
ai-studio prompt.json                # Google AI Studio 导出文件
```

默认输出至**源文件同级目录**，亦可通过 `-o` 选项指定输出目录或输出文件名：

```powershell
rikka backup.zip -o out\              # 输出至 out\chat-export-rikka-all-<时间戳>.zip
rikka backup.zip -o out\my-name.zip   # 显式指定 ZIP 文件名
```

亦可直接将文件拖拽至单个可执行文件图标上运行（`ai-studio` 默认输出单文件 Markdown，建议通过命令行运行）。

## 特性

- **统一输出规范**：各转换器输出结构高度一致。
- **异常会话汇总**：无有效消息的空会话不会生成空文件，而是统一汇总至包内的 `export-failures.md`。
- **容错处理**：单个会话解析失败不影响批处理中其他会话的转换，进程正常退出。


## 构建

构建环境要求：Rust (stable) 与 [Bun](https://bun.sh/)：

```powershell
# 编译 5 个命令行转换器
cargo build --release

# 编译桌面应用（会自动将转换器作为 sidecar 一同打包）
cd gui
bun install
bun run build
```
