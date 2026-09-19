# 构建与开发指南

本项目采用 **Cargo Workspace** 结构，并结合 **Git Submodules** 管理多个独立转换器。GUI 入口基于 **Tauri**，并自动集成所有转换器。

---

## 🚀 快速开始 (推荐开发流程)

如果你主要进行 GUI 开发，**只需要**执行以下步骤：

1. **环境初始化 (仅首次或拉取后)**:
   ```powershell
   git submodule update --init --recursive
   ```

2. **启动 GUI 开发模式**:
   进入 GUI 目录并启动，它会**自动编译**所有子模块转换器：
   ```powershell
   cd converter/gui
   bun install
   bun run dev
   ```
   > **💡 提示**: `bun run dev` (即 `tauri dev`) 现在直接加载原生 HTML/JS/CSS，不再依赖 Vite 编译。它依然会自动调用预处理脚本编译转换器并作为 Sidecar 注入。

---

## 🛠️ 核心架构说明

- **Workspace**: 根目录的 [Cargo.toml](file:///c:/Users/Mutsumi/Desktop/converters/Cargo.toml) 管理所有成员。
- **Submodules**: [converter/ai-studio](file:///c:/Users/Mutsumi/Desktop/converters/converter/ai-studio), [converter/cherry](file:///c:/Users/Mutsumi/Desktop/converters/converter/cherry), [converter/qwen](file:///c:/Users/Mutsumi/Desktop/converters/converter/qwen), [converter/claude](file:///c:/Users/Mutsumi/Desktop/converters/converter/claude), [converter/rikka](file:///c:/Users/Mutsumi/Desktop/converters/converter/rikka) 是独立仓库。
- **GUI**: [converter/gui](file:///c:/Users/Mutsumi/Desktop/converters/converter/gui) 是本地成员，通过 Sidecar 机制调用转换器。

---

## 📦 进阶构建

### 整体构建 (命令行版本)
```powershell
cargo build
```

### 构建 GUI 发布包
```powershell
cd converter/gui
bun run build
```

---

## 🔄 子模块 (Submodule) 维护指南

由于转换器是独立仓库，维护时请参考以下场景：

### 场景 A：同步他人修改 (最常见)
当你在其他地方更新了转换器仓库，或者协作者更新了代码：
1. **拉取最新子模块**:
   ```powershell
   git submodule update --remote --merge
   ```
2. **提交主仓库指针**:
   ```powershell
   git add .
   git commit -m "chore: 升级转换器到最新版本"
   ```

### 场景 B：在本仓库中修改转换器代码
如果你想在当前工程下直接改转换器代码：
1. **进入目录并切换分支** (关键：避免 Detached HEAD):
   ```powershell
   cd converter/ai-studio
   git checkout main
   ```
2. **修改、提交并推送**:
   ```powershell
   git add .
   git commit -m "feat: 改进转换逻辑"
   git push origin main
   ```
3. **回到主仓库更新指针**:
   ```powershell
   cd ../..
   git add converter/ai-studio
   git commit -m "chore: 更新子模块指针"
   ```

### ⚠️ 核心注意事项
- **不要在游离状态提交**: 始终确保在子模块目录执行了 `git checkout main` 后再提交代码。
- **双重提交**: 修改子模块后，必须先在子模块内 `push`，再在主仓库 `commit` 指针变更。
