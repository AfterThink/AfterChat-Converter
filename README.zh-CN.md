<h1 align="center">AfterChat Converter</h1>

<p align="center">
  <a href="https://github.com/AfterThink/AfterChat-Converter/releases">
    <img src="https://img.shields.io/github/v/release/AfterThink/AfterChat-Converter?style=for-the-badge&logo=github&color=blue" alt="Latest Release" />
  </a>
  <img src="https://img.shields.io/badge/Platform-Windows-0078D6?style=for-the-badge&logo=windows" alt="Platform: Windows" />
  <a href="./LICENSE">
    <img src="https://img.shields.io/badge/License-AGPL--3.0-blue.svg?style=for-the-badge" alt="License: AGPL-3.0" />
  </a>
</p>

<p align="center">
  <a href="./README.md">English</a> | 简体中文
</p>

把分散在各大 AI 平台与第三方客户端的历史聊天备份，一键转换为干净、统一的 **Markdown 笔记**。

支持 **ChatGPT、Claude、Cherry Studio、RikkaHub、Qwen、Google AI Studio** 等官方导出与客户端备份。完整保留 **思考过程（Reasoning / CoT）** 与 **引用来源**，无缝接入 **Obsidian、Notion**，或导入 **[AfterChat 桌面端](https://github.com/AfterThink/AfterChat-App-Download)** 构建本地个人知识库。

<p align="center">
  <img src="https://fastly.jsdelivr.net/gh/AfterThink/assets@9e4d7b0/afterchat-converter/demo.gif" alt="AfterChat Converter 演示动图" width="85%" />
  <br>
  <sub>💡 支持拖拽转换：拖入任意备份文件或文件夹，自动识别来源并批量输出 Markdown</sub>
</p>

## 下载与安装

直接前往 [Releases 页面](https://github.com/AfterThink/AfterChat-Converter/releases) 获取预编译程序。

## 支持的备份来源

| 来源平台 / 客户端 | 支持的备份格式 | 
| --- | --- | 
| **ChatGPT** | 官方设置中导出的数据 ZIP | 
| **Claude** | 官方账号导出的完整数据包 ZIP | 
| **Cherry Studio** | 客户端备份 ZIP 压缩包 | 
| **RikkaHub** | 客户端备份 ZIP 压缩包 | 
| **Qwen** | 网页版导出的 JSON 文件 | 
| **Google AI Studio** | Google Drive 导出的 JSON 文件 | 

## 使用方法

### 1. 桌面端应用（推荐）

1. **打开软件**：启动 `afterchat-converter`；
2. **直接拖入**：将任意支持的备份文件（ZIP / JSON）拖拽到软件窗口中；
3. **极速解析**：软件会自动根据文件特征**智能识别所属平台**并完成转换；
4. **获取结果**：转换完成后，生成的标准 Markdown 归档包默认保存在源文件同级目录下。可在窗口底部勾选「输出到自定义目录」更改输出路径。

### 2. 命令行工具（CLI / 高级自动化）

程序包内内置了 6 个针对各来源的独立可执行工具，方便脚本自动化或批量接入知识库（如 Obsidian）：

```powershell
# 基础转换（默认在源文件同级目录生成归档）
chatgpt   chatgpt-export.zip         # 转换 ChatGPT 官方导出包
claude    data-export.zip            # 转换 Claude 官方导出包
cherry    cherry-backup.zip          # 转换 Cherry Studio 客户端备份
rikka     RikkaHub-backup.zip        # 转换 RikkaHub 客户端备份
qwen      qwen-all.json              # 转换 Qwen 导出的 JSON
ai-studio prompt.json                # 转换 Google AI Studio 导出文件
```

## 源码构建

如需自行编译，需要安装 Rust (stable) 与 [Bun](https://bun.sh/)：

```powershell
# 编译命令行转换工具
cargo build --release

# 编译桌面图形应用（会自动打包命令行组件作为 sidecar）
cd gui
bun install
bun run build
```

## 相关项目

- [AfterChat-Script](https://github.com/AfterThink/AfterChat-Script)：浏览器用户脚本，在网页端与 AI 对话时一键导出或直存。
- [AfterChat 桌面端](https://github.com/AfterThink/AfterChat-App-Download)：开箱即用的 AI 会话本地管理与离线知识库工具。

## 开源许可证

本项目基于 [GNU Affero General Public License v3.0](LICENSE) (AGPL-3.0) 许可证开源。
