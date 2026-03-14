# 构建指南

## 1. 环境初始化

本项目使用 Git Submodule 管理依赖。**首次克隆**或**拉取代码后**，必须初始化子模块，否则构建会失败。

```bash
# 克隆主仓库
git clone <repo-url> && cd <repo-dir>

# 初始化并更新子模块 (必须执行)
git submodule update --init --recursive
```

> **提示**：建议配置 Git 自动处理子模块，避免每次 pull 后忘记更新：
> `git config --global submodule.recurse true`

## 2. 构建项目

本项目为 Cargo Workspace，支持整体构建或单独构建。

### 构建所有成员
```bash
cargo build
```

### 构建单个成员
```bash
# 示例：构建特定转换器
cargo build -p google-ai-studio-json-converter
cargo build -p cherry-studio-backup-json-converter
cargo build -p qwen-json-converter
```

## 3. 子模块维护 

子模块在主仓库中记录的是**特定的 Commit Hash**，而非分支。更新子模块分为两种场景：

### 场景 A：更新项目依赖 (需提交)
希望主仓库正式升级，让所有协作者和 CI 都使用子模块的新版本：

1.  **更新子模块指向**：
    ```bash
    git submodule update --remote
    ```
2.  **在主仓库暂存变更** (关键步骤)：
    ```bash
    git add path/to/submodule
    ```
    *此时 `git status` 会显示子模块路径有修改，代表主仓库记录的 Hash 已更新。*
3.  **提交主仓库**：
    ```bash
    git commit -m "chore: update submodule to latest version"
    git push
    ```

### 场景 B：仅本地同步 (不提交)
如果只是想获取子模块的最新代码进行本地测试，不改变主仓库的依赖版本：

```bash
# 进入子模块目录拉取最新代码
cd path/to/submodule
git pull

# 或者在主目录批量更新 (本地状态变更，未提交)
git submodule update --remote
```
*注意：此操作仅改变你本地的子模块指向，不会影响其他协作者。*
