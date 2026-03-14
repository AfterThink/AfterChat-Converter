# Converters GUI

一个基于 Tauri + Bun + Vite 的桌面 GUI，用来拖拽调用仓库里的三个转换器：

- `google-ai-studio-json-converter`
- `cherry-studio-backup-json-converter`
- `qwen-json-converter`

## 功能

- 拖拽文件或文件夹后立即开始处理
- 根据文件名自动路由转换器
  - 名称包含 `cherry` → `cherry-studio-backup-json-converter`
  - 名称包含 `qwen` → `qwen-json-converter`
  - 其他情况 → `google-ai-studio-json-converter`
- 默认输出到原位置旁边
- 取消勾选后，立即弹原生保存路径对话框
- 记住上一次输出模式

## 环境要求

- `bun`
- `rustc` / `cargo`
- Windows 下建议已安装 WebView2 Runtime

当前仓库已经是 Cargo workspace，GUI 位于 `converter/gui`。

## 国内源

这个目录已经自带 `.npmrc`：

```ini
registry=https://registry.npmmirror.com/
```

`bun install` 会默认走这个源。

## 安装依赖

```powershell
cd converter/gui
bun install
```

## 开发启动

```powershell
cd converter/gui
bun tauri dev
```

这条命令会自动做两件事：

1. 编译 Rust 转换器 sidecar 到工作区 `target/`
2. 复制 sidecar 到 `converter/gui/src-tauri/bin/`
3. 启动 Vite 和 Tauri 开发窗口

## 生产构建

```powershell
cd converter/gui
bun tauri build
```

这条命令会自动：

1. 以 `--release` 编译三个转换器
2. 复制 release sidecar 到 `src-tauri/bin/`
3. 构建前端静态资源
4. 打包 Tauri 应用

打包结果默认在工作区根目录：

- `target/release/bundle/`

当前在 Windows 上实际产物为：

- `target/release/bundle/msi/Converters_0.1.0_x64_en-US.msi`
- `target/release/bundle/nsis/Converters_0.1.0_x64-setup.exe`

## 也可以手动执行的脚本

```powershell
cd converter/gui
bun run prepare:sidecars
bun run prepare:sidecars:release
bun run build
```

## 常见问题

### 1. `bun install` 很慢

已经默认切到 `npmmirror`。如果还慢，可以手动确认：

```powershell
cd converter/gui
Get-Content .npmrc
```

### 2. Tauri 构建时报找不到 sidecar

先执行：

```powershell
cd converter/gui
bun run prepare:sidecars:release
```

确认 `src-tauri/bin/` 下出现带平台 triple 的可执行文件。

### 3. Rust 依赖下载失败

GUI 的 Rust 依赖由 Cargo 负责，和 Bun 源无关；需要你本机的 Cargo 镜像可用。
