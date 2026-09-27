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
4. **ChatFormat 引用规范 (References RFC) 确立（2026-09-27 补充）**：
   - 在 `docs/CHATFORMAT.md` 中新增 **4.4 全局引用与信源区**；
   - 规定位于整场对话文末（不含 `---` 分隔线，空行承接）、正文角标紧凑连排 `[1][2]`、列表项必须采用 Markdown 超链接 `- [N] [标题](URL)`、URL 必须全局规范化去重。

---

## 4. 下一个会话的待办任务 (Next Steps)

1. **各端引用格式标准化落地**：
   - 详细审计清单与多阶段改造方案见 `AfterChat-Script-Dev/HANDOFF-REFERENCES.md`；
   - 将 `AfterChat-Script` 中其余适配器（Gemini、Grok、Sakana、ChatGPT、PPLX、国内诸模型）统一收敛至本 RFC 规范；
2. **`afterchat-chatformat` Rust 核心库支持**：
   - 评估在 `Conversation` 模型中增加 `references` 结构化支持并提供标准 Markdown 渲染能力。
