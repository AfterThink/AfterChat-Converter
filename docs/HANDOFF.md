# 项目交接备忘录 (HANDOFF)

**日期**：2026-09-19  
**当前状态**：文档优化已完成；ChatFormat 标准化已确立；工作区留有待审阅的未提交改动。

---

## 1. 本次会话已完成的工作

1. **全仓库文档措辞与风格优化**：
   - 完成对 12 份核心文档的规范化修改，消除装饰性 Emoji、口语化表达与主观修辞，统一专业术语（如 Sidecar、独立 SQLite 数据库文件、保留原始时间戳等）；
   - 严格保留了 [docs/CHATFORMAT.md](./docs/CHATFORMAT.md) 中用于数据解析的角色指示标记与转换算法；
   - 执行 `cargo test --workspace --exclude afterchat-converter`，61 项单元测试与集成测试全部通过。
   - 相关提交：`ecb6ce7 docs: 优化全量文档措辞与格式规范`。

2. **双语 README 建设与英文转正**：
   - 创建了 [README_zh.md](./README_zh.md)，并将主 [README.md](./README.md) 转正为英文版，包含互跳链接；
   - 明确定义了 AfterChat「采集 (Capture) → 归档 (Normalize & Store) → 价值发现 (Discover & Manage)」的全流程生态协同机制；
   - 相关提交：`3396cb9 docs: 转正英文主 README 并新增 README_zh.md 双语支持`。

3. **ChatFormat 规范地位升级**：
   - 将 [docs/CHATFORMAT.md](./docs/CHATFORMAT.md) 升级为 **ChatFormat Specification RFC (1.0.0-draft)**，确立为 AfterChat 生态（Script、Converter、Desktop App）统一遵循的单一事实来源（Single Source of Truth）；
   - 明确声明了双端官方参考实现：
     - **导出端 (Web Capture)**：`AfterChat-Script` (JavaScript/TypeScript)
     - **转换端 (Offline Migration)**：`AfterChat-Converter` (`crates/afterchat-chatformat` Rust 核心库)
     - **消费端 (Workspace & Reader)**：`AfterChat Desktop App`
   - 在中英文 README 中增加了「ChatFormat 规范标准」专章。

---

## 2. 当前工作区未提交状态（Uncommitted Changes）

按用户要求，以下文件的最新改动**已保留在工作区供审阅，尚未执行 Git 提交**：
- `README.md`（新增 ChatFormat Specification 英文专章）
- `README_zh.md`（新增 ChatFormat 规范标准中文专章）
- `docs/CHATFORMAT.md`（升级为 Specification RFC 1.0.0-draft，补充生态参考实现表）

---

## 3. 既定技术与合规决策备忘

1. **框架评估**：维持当前 **Tauri 1.6**，暂不升级 Tauri 2（项目需求简单，Tauri 1.6 稳定且无需重构权限与插件系统）。
2. **开源许可证**：推荐采用 **AGPLv3**。
   - 闭源本体通过命令行独立子进程（Fork-Exec）拉起转换器，符合 FSF 独立程序标准，**不会受到 AGPLv3 传染**；
   - 能够有效防范第三方将转换器部署为云端 SaaS 服务进行白嫖。
3. **标准制定策略**：
   - 阶段一（当前）：以本仓库的 `docs/CHATFORMAT.md` 作为规范草案，借助 `crates/afterchat-chatformat` 和 `AfterChat-Script` 的双端实现作为事实标准背书；
   - 阶段二（长远）：待覆盖平台达到 10+ 或出现外部第三方采纳时，再独立建仓（如 `AfterThink/AfterChat-Format-spec`）并发布中立 SDK。

---

## 4. 下一个会话的待办任务 (Next Steps)

1. **多工作区接入**：
   - 在新会话中确保同时加载了 `AfterChat-Converter` 和 `AfterChat-Script-Dev` 两个工作区；
2. **`AfterChat-Script-Dev` 文档对齐**：
   - 在 ACScript 的 README 中增加对 `AfterChat-Converter` 的推荐与生态闭环说明；
   - 在 ACScript 中引入对 `ChatFormat Specification` 的标准遵循声明（引用 `AfterChat-Converter/docs/CHATFORMAT.md` 作为单一事实来源）；
3. **提交与协议落地**：
   - 确认本仓库当前未提交的 3 个文件并执行提交；
   - 添加 AGPLv3 的 `LICENSE` 文件。
