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
  English | <a href="./README.zh-CN.md">简体中文</a>
</p>

Convert conversation backups scattered across various AI platforms and third-party clients into clean, standardized **Markdown notes** with one click.

Supports official exports and client backups from **ChatGPT, Claude, Cherry Studio, RikkaHub, Qwen, and Google AI Studio**. Fully preserves **reasoning / thinking processes (CoT)** and **citation sources**, ready to seamlessly plug into **Obsidian, Notion**, or import directly into the **[AfterChat Desktop App](https://github.com/AfterThink/AfterChat-App-Download)** to build your local knowledge base.

<p align="center">
  <img src="https://fastly.jsdelivr.net/gh/AfterThink/assets@main/afterchat-converter/demo.gif" alt="AfterChat Converter Demo" width="85%" />
  <br>
  <sub>💡 Drag & drop conversion: drop any backup file or folder to automatically detect the source and batch export to Markdown</sub>
</p>

## Downloads & Installation

Get pre-built binaries directly from the [Releases page](https://github.com/AfterThink/AfterChat-Converter/releases).

## Supported Backup Sources

| Source Platform / Client | Supported Backup Format |
| --- | --- |
| **ChatGPT** | Official data export ZIP |
| **Claude** | Official account data export ZIP |
| **Cherry Studio** | Client backup ZIP archive |
| **RikkaHub** | Client backup ZIP archive |
| **Qwen** | Web exported JSON file |
| **Google AI Studio** | Google Drive exported JSON file |

## Usage

### 1. Desktop Application (Recommended)

1. **Launch**: Open `afterchat-converter`;
2. **Drag & Drop**: Drag any supported backup file (ZIP / JSON) into the application window;
3. **Auto-Detect**: The application automatically identifies the platform based on file characteristics and converts it instantly;
4. **Get Markdown**: Converted Markdown archives are saved next to the original file by default. Check "Output to custom directory" at the bottom to choose an alternative folder.

### 2. Command-Line Tools (CLI / Advanced Automation)

The package includes 6 standalone CLI executables for each source, perfect for scripts and automated ingestion into knowledge bases (e.g., Obsidian):

```powershell
# Basic conversion (creates an archive next to source files by default)
chatgpt   chatgpt-export.zip         # Convert ChatGPT official export
claude    data-export.zip            # Convert Claude official export
cherry    cherry-backup.zip          # Convert Cherry Studio client backup
rikka     RikkaHub-backup.zip        # Convert RikkaHub client backup
qwen      qwen-all.json              # Convert Qwen exported JSON
ai-studio prompt.json                # Convert Google AI Studio export file
```

## Building from Source

Prerequisites: Rust (stable) and [Bun](https://bun.sh/):

```powershell
# Build CLI converter binaries
cargo build --release

# Build desktop application (sidecar binaries are bundled automatically)
cd gui
bun install
bun run build
```

## Related Projects

- [AfterChat-Script](https://github.com/AfterThink/AfterChat-Script): Browser userscript for one-click chat exporting while browsing web LLMs.
- [AfterChat Desktop App](https://github.com/AfterThink/AfterChat-App-Download): Local AI conversation management and offline knowledge base.

## License

Licensed under the [GNU Affero General Public License v3.0](LICENSE) (AGPL-3.0).
