//! AfterChat ChatFormat 的公共实现（纯库，无副作用）。
//!
//! 权威契约见仓库 `docs/CHATFORMAT.md`。本 crate 把跨平台的部分全部收口：
//!
//! - [`markdown`]：§5 的 `#` → `**` 转义（含围栏 / 行内代码保护）
//! - [`naming`]：§7 的文件名与路径组件清洗
//! - [`time`]：§3 的时间解析与本地格式化
//! - [`zip`]：§6 的 ZIP 打包、条目命名、`export-failures.md`
//! - [`render`]：§2–§4 的 Markdown 骨架
//!
//! 每个平台的转换器只负责「解析输入 → 映射成 [`Conversation`]」，
//! 输出一律经本 crate 产出，保证所有平台的产物逐字节一致。

pub mod markdown;
pub mod naming;
pub mod time;
pub mod zip;

use std::borrow::Cow;

use indicatif::{ProgressBar, ProgressStyle};

pub use markdown::{remove_bold_outside_code, strip_hashes};
pub use naming::{
    NameStyle, sanitize_filename, sanitize_filename_js, sanitize_filename_with,
    sanitize_path_component,
};
pub use zip::{ExportFailure, ZipExport, default_zip_name, write_zip};

/// 模型名缺失时的兜底值。
pub const UNKNOWN_MODEL: &str = "Unknown";

/// 单条导出文件名上限（对齐契约 §7 的 60～80 建议值）。
pub const SINGLE_NAME_MAX: usize = 60;

/// 消息角色。无法识别的角色由转换器兜底成 [`Role::Assistant`]（契约 §4.2）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    System,
    User,
    Assistant,
}

/// 一条消息：正文 + （助手专有的）思维链。
#[derive(Debug, Clone)]
pub struct Message {
    pub role: Role,
    pub thinking: Vec<String>,
    pub body: Vec<String>,
}

impl Message {
    pub fn system(text: impl Into<String>) -> Self {
        Self {
            role: Role::System,
            thinking: Vec::new(),
            body: vec![text.into()],
        }
    }

    pub fn user(text: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            thinking: Vec::new(),
            body: vec![text.into()],
        }
    }

    pub fn assistant(text: impl Into<String>) -> Self {
        Self {
            role: Role::Assistant,
            thinking: Vec::new(),
            body: vec![text.into()],
        }
    }

    pub fn assistant_blocks(thinking: Vec<String>, body: Vec<String>) -> Self {
        Self {
            role: Role::Assistant,
            thinking,
            body,
        }
    }
}

/// Metadata 里 `Model` / `Time` / `URL` 之外的附加键（契约 §3 允许）。
#[derive(Debug, Clone)]
pub struct MetadataLine {
    pub key: String,
    pub value: String,
    /// 是否用反引号包裹（如 `Topic ID` / `Assistant` 用，`Time` 类不用）。
    pub code: bool,
}

impl MetadataLine {
    pub fn code(key: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            value: value.into(),
            code: true,
        }
    }

    pub fn plain(key: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            value: value.into(),
            code: false,
        }
    }
}

/// 平台无关的对话模型：各转换器把自家 JSON 映射到这里。
#[derive(Debug, Clone)]
pub struct Conversation {
    /// 原始标题（用于文件名与 ZIP 条目名，不做转义）。
    pub title: String,
    /// 已解析的模型名；缺失时由转换器填 [`UNKNOWN_MODEL`]。
    pub model: String,
    /// 对话时间（epoch 秒）。
    pub time_secs: Option<i64>,
    /// ZIP 排序用的毫秒时间戳；`None` 时回退到 `time_secs * 1000`。
    pub sort_ms: Option<i64>,
    pub url: Option<String>,
    pub extra: Vec<MetadataLine>,
    /// ZIP 内子目录（如助手名）；`None` 表示平铺。
    pub group: Option<String>,
    /// 平台侧 ID，用于失败报告。
    pub id: Option<String>,
    pub messages: Vec<Message>,
}

impl Default for Conversation {
    fn default() -> Self {
        Self {
            title: String::new(),
            model: UNKNOWN_MODEL.to_string(),
            time_secs: None,
            sort_ms: None,
            url: None,
            extra: Vec::new(),
            group: None,
            id: None,
            messages: Vec::new(),
        }
    }
}

impl Conversation {
    /// 排序时间：优先 `sort_ms`，否则 `time_secs * 1000`。
    pub fn sort_ms(&self) -> Option<i64> {
        self.sort_ms.or_else(|| self.time_secs.map(|secs| secs * 1000))
    }

    pub fn render(&self) -> String {
        render(self)
    }
}

/// 渲染完整 Markdown（契约 §2–§4），返回以单个 `\n` 结尾的字符串。
pub fn render(conversation: &Conversation) -> String {
    let mut lines: Vec<String> = Vec::new();
    lines.push("## Metadata".to_string());
    lines.push(String::new());
    lines.push(format!("- **Model:** `{}`", conversation.model));
    if let Some(secs) = conversation.time_secs {
        lines.push(format!("- **Time:** {}", time::format_local_time(secs)));
    }
    if let Some(url) = non_empty(conversation.url.as_deref()) {
        lines.push(format!("- **URL:** {url}"));
    }
    for extra in &conversation.extra {
        if extra.code {
            lines.push(format!("- **{}:** `{}`", extra.key, extra.value));
        } else {
            lines.push(format!("- **{}:** {}", extra.key, extra.value));
        }
    }
    lines.push(String::new());
    lines.push("## Conversation".to_string());
    lines.push(String::new());
    lines.push(render_messages(&conversation.messages));
    format!("{}\n", lines.join("\n").trim_end())
}

fn render_messages(messages: &[Message]) -> String {
    let mut lines: Vec<String> = Vec::new();

    for message in messages {
        match message.role {
            Role::System => {
                let text = join_non_empty(&message.body);
                if text.is_empty() {
                    continue;
                }
                lines.push("### ⚙️ System".to_string());
                lines.push(String::new());
                lines.push(markdown::strip_hashes(&text));
                lines.push(String::new());
            }
            Role::User => {
                let text = join_non_empty(&message.body);
                if text.is_empty() {
                    continue;
                }
                lines.push("### 🧑‍💻 User".to_string());
                lines.push(String::new());
                lines.push(markdown::strip_hashes(&text));
                lines.push(String::new());
            }
            Role::Assistant => {
                let thoughts = join_non_empty(&message.thinking);
                let body = join_non_empty(&message.body);
                if thoughts.is_empty() && body.is_empty() {
                    continue;
                }

                lines.push("### 🤖 Assistant".to_string());
                lines.push(String::new());

                if !thoughts.is_empty() {
                    lines.push("#### 🤔 Thought Process".to_string());
                    lines.push(String::new());
                    lines.push(markdown::strip_hashes(&thoughts));
                    lines.push(String::new());
                    if !body.is_empty() {
                        lines.push("#### 💡 Response".to_string());
                        lines.push(String::new());
                    }
                }

                if !body.is_empty() {
                    lines.push(markdown::strip_hashes(&body));
                    lines.push(String::new());
                }
            }
        }
    }

    lines.join("\n")
}

/// 去掉空片段，其余按 `\n\n` 连接。
pub fn join_non_empty(parts: &[String]) -> String {
    parts
        .iter()
        .map(|part| part.trim())
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn non_empty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|text| !text.is_empty())
}

/// 统一的进度条样式。
pub(crate) fn make_progress_bar(total: u64, unit: &str) -> ProgressBar {
    let pb = ProgressBar::new(total);
    let template =
        "{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} {msg} ({per_sec}, ETA {eta})";
    let style = ProgressStyle::with_template(template)
        .unwrap_or_else(|_| ProgressStyle::default_bar())
        .progress_chars("=> ");
    pb.set_style(style);
    pb.set_message(Cow::Owned(unit.to_string()));
    pb
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Conversation {
        Conversation {
            title: "T".to_string(),
            model: "gemini-2.5-pro".to_string(),
            time_secs: Some(1_726_189_351),
            sort_ms: None,
            url: Some("https://example.com/c/1".to_string()),
            extra: vec![MetadataLine::code("Assistant", "A")],
            group: None,
            id: Some("1".to_string()),
            messages: vec![
                Message::system("# 你是助手"),
                Message::user("你好\n### 标题"),
                Message::assistant_blocks(vec!["**想**".to_string()], vec!["# 答复".to_string()]),
            ],
        }
    }

    #[test]
    fn renders_contract_skeleton() {
        let text = sample().render();
        assert!(text.starts_with("## Metadata\n"), "{text}");
        assert!(text.contains("- **Model:** `gemini-2.5-pro`\n"), "{text}");
        assert!(text.contains("- **Time:** 2024-"), "{text}");
        assert!(text.contains("- **URL:** https://example.com/c/1\n"), "{text}");
        assert!(text.contains("- **Assistant:** `A`\n"), "{text}");
        assert!(text.contains("\n## Conversation\n"), "{text}");
        assert!(text.contains("### ⚙️ System\n\n**你是助手**\n"), "{text}");
        assert!(text.contains("### 🧑‍💻 User\n\n你好\n**标题**\n"), "{text}");
        assert!(text.contains("#### 🤔 Thought Process\n\n**想**\n"), "{text}");
        assert!(text.contains("#### 💡 Response\n\n**答复**\n"), "{text}");
        assert!(text.ends_with('\n') && !text.ends_with("\n\n"), "{text}");
    }

    #[test]
    fn section_headers_are_alone_on_their_line() {
        let text = sample().render();
        for header in ["#### 🤔 Thought Process", "#### 💡 Response"] {
            assert!(text.contains(&format!("\n{header}\n")), "{text}");
        }
    }

    #[test]
    fn omits_optional_blocks_when_empty() {
        let mut conv = sample();
        conv.messages = vec![Message::assistant("# 只有回复")];
        let text = conv.render();
        assert!(!text.contains("#### 🤔 Thought Process"), "{text}");
        assert!(!text.contains("#### 💡 Response"), "{text}");
        assert!(text.contains("### 🤖 Assistant\n\n**只有回复**"), "{text}");
    }

    #[test]
    fn skips_empty_messages() {
        let mut conv = sample();
        conv.messages = vec![Message::user(""), Message::user("   ")];
        let text = conv.render();
        assert!(!text.contains("### 🧑‍💻 User"), "{text}");
    }

    #[test]
    fn sort_ms_falls_back_to_time() {
        let mut conv = sample();
        assert_eq!(conv.sort_ms(), Some(1_726_189_351_000));
        conv.sort_ms = Some(5);
        assert_eq!(conv.sort_ms(), Some(5));
    }
}
