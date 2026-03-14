## 构建方法

## Submodule 更新

本项目使用 git submodule，克隆后需要初始化：

```bash
git submodule update --init --recursive
```

更新 submodule：

```bash
git submodule update --remote
```

### 构建所有项目

在项目根目录执行：

```bash
cargo build
```

调试模式构建输出在 `target/debug/` 目录。

### 构建 Release 版本

```bash
cargo build --release
```

Release 版本输出在 `target/release/` 目录。

### 构建单个项目

```bash
# Google AI Studio JSON 转换器
cargo build -p google-ai-studio-json-converter

# Cherry Studio 备份 JSON 转换器
cargo build -p cherry-studio-backup-json-converter

# Qwen JSON 转换器
cargo build -p qwen-json-converter
```
