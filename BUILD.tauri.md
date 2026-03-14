## 3. 构建 Tauri GUI 应用

GUI 工程位于 `converter/gui`。

### 前置要求

- `Rust` / `cargo`
- `bun`
- Windows 下建议已安装 WebView2 Runtime

### 国内源

`converter/gui/.npmrc` 已经固定使用：

```ini
registry=https://registry.npmmirror.com/
```

所以直接执行 `bun install` 即可。

### 安装依赖

```powershell
cd converter/gui
bun install
```

### 开发模式

```powershell
cd converter/gui
bun tauri dev
```

这会自动：

1. 编译三个 Rust sidecar
2. 复制到 `converter/gui/src-tauri/bin/`
3. 启动 Vite
4. 启动 Tauri 开发窗口

### 生产构建

```powershell
cd converter/gui
bun tauri build
```

这条命令我已经实际跑通。

Windows 下产物位于：

- `target/release/bundle/msi/`
- `target/release/bundle/nsis/`

当前一次验证产物为：

- `target/release/bundle/msi/Converters_0.1.0_x64_en-US.msi`
- `target/release/bundle/nsis/Converters_0.1.0_x64-setup.exe`

### 更多说明

更详细的 GUI 使用和脚本说明见：

- `converter/gui/README.md`
