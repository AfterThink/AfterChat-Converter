//! 把 Claude 导出（ZIP，内含 `conversations.json`）转换为 AfterChat 对话 ZIP。
//!
//! 输出契约见仓库 `docs/CHATFORMAT.md`，全部渲染/命名/打包都走 `chatformat`。
//!
//! Claude 导出里**没有模型标识**（对话级、消息级都没有 `model` 字段），
//! 因此 `- **Model:**` 固定为 `Unknown`。
//!
//! 输入支持 ZIP（主用，如 `data-*-batch-0000.zip`）、单个 JSON、以及包含它们的目录；
//! 输出**始终**是一个 ZIP。

use std::collections::HashSet;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use chatformat::{
    Conversation, ExportFailure, Message, MetadataLine, NameStyle, Role, UNKNOWN_MODEL, ZipExport,
    default_zip_name, time,
};
use serde::{Deserialize, Deserializer};
use walkdir::WalkDir;

const PLATFORM: &str = "claude";
const URL_BASE: &str = "https://claude.ai/chat";
const ZIP_ARCHIVE_NAME: &str = "conversations.json";

#[derive(Debug, Clone)]
pub struct ConvertOptions {
    /// 输入：ZIP / JSON / 目录
    pub input: PathBuf,
    /// 输出目录，或显式的 `.zip` 路径
    pub output: Option<PathBuf>,
    pub show_progress: bool,
}

#[derive(Debug, Default, Clone)]
pub struct RunSummary {
    pub conversations: usize,
    pub failed: usize,
    pub output: Option<PathBuf>,
}

pub fn run_conversion(options: ConvertOptions) -> Result<RunSummary> {
    if !options.input.exists() {
        bail!("input path does not exist: {}", options.input.display());
    }

    let raws = load(&options.input)?;
    if raws.is_empty() {
        bail!(
            "no Claude conversations found in {}",
            options.input.display()
        );
    }

    let mut conversations = Vec::new();
    let mut failures = Vec::new();
    for raw in &raws {
        let conversation = to_conversation(raw);
        if conversation.messages.is_empty() {
            failures.push(ExportFailure {
                title: display_title(&conversation.title).to_string(),
                id: conversation.id.clone().unwrap_or_else(|| "-".to_string()),
                reason: "对话没有任何可导出的消息".to_string(),
            });
        } else {
            conversations.push(conversation);
        }
    }

    if conversations.is_empty() {
        bail!(
            "no exportable Claude conversations in {}",
            options.input.display()
        );
    }

    let target = resolve_target(
        &options.input,
        options.output.as_deref(),
        default_zip_name(PLATFORM),
    )?;
    chatformat::write_zip(&ZipExport {
        platform: PLATFORM,
        conversations: &conversations,
        failures: &failures,
        output: &target,
        source: Some(&options.input),
        name_style: NameStyle::Spec,
        show_progress: options.show_progress,
    })?;

    Ok(RunSummary {
        conversations: conversations.len(),
        failed: failures.len(),
        output: Some(target),
    })
}

// ═══════════════════════════════════════════════════════════
//  输入解析
// ═══════════════════════════════════════════════════════════

#[derive(Debug, Deserialize)]
struct RawConversation {
    #[serde(default, deserialize_with = "null_to_default")]
    uuid: Option<String>,
    #[serde(default, deserialize_with = "null_to_default")]
    name: Option<String>,
    #[serde(default, deserialize_with = "null_to_default")]
    created_at: Option<String>,
    #[serde(default, deserialize_with = "null_to_default")]
    updated_at: Option<String>,
    #[serde(default, deserialize_with = "null_to_default")]
    chat_messages: Vec<RawMessage>,
}

#[derive(Debug, Deserialize)]
struct RawMessage {
    #[serde(default, deserialize_with = "null_to_default")]
    sender: Option<String>,
    #[serde(default, deserialize_with = "null_to_default")]
    text: Option<String>,
    #[serde(default, deserialize_with = "null_to_default")]
    content: Vec<RawPart>,
    #[serde(default, deserialize_with = "null_to_default")]
    attachments: Vec<RawAttachment>,
    #[serde(default, deserialize_with = "null_to_default")]
    files: Vec<RawFile>,
}

impl RawMessage {
    /// 附件名（`attachments` 更详细，`files` 是补充），按出现顺序去重。
    fn file_names(&self) -> Vec<String> {
        let mut seen: HashSet<String> = HashSet::new();
        let mut names = Vec::new();
        for name in self
            .attachments
            .iter()
            .filter_map(|item| item.file_name.as_deref())
            .chain(self.files.iter().filter_map(|item| item.file_name.as_deref()))
        {
            let name = name.trim();
            if !name.is_empty() && seen.insert(name.to_string()) {
                names.push(name.to_string());
            }
        }
        names
    }
}

#[derive(Debug, Deserialize)]
struct RawPart {
    #[serde(rename = "type", default, deserialize_with = "null_to_default")]
    kind: Option<String>,
    #[serde(default, deserialize_with = "null_to_default")]
    text: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawAttachment {
    #[serde(default, deserialize_with = "null_to_default")]
    file_name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawFile {
    #[serde(default, deserialize_with = "null_to_default")]
    file_name: Option<String>,
}

/// `null` 与缺失字段都退化成 `Default`，避免整份导出因个别字段解析失败。
fn null_to_default<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Default + Deserialize<'de>,
{
    Ok(Option::<T>::deserialize(deserializer)?.unwrap_or_default())
}

fn load(input: &Path) -> Result<Vec<RawConversation>> {
    if input.is_dir() {
        let mut all = Vec::new();
        let mut entries: Vec<PathBuf> = WalkDir::new(input)
            .into_iter()
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.file_type().is_file())
            .map(|entry| entry.into_path())
            .collect();
        entries.sort();
        for path in entries {
            match extension(&path).as_deref() {
                Some("zip") | Some("json") => match load_one(&path) {
                    Ok(mut conversations) => all.append(&mut conversations),
                    Err(err) => log::warn!("skip {}: {err:#}", path.display()),
                },
                _ => {}
            }
        }
        Ok(all)
    } else {
        load_one(input)
    }
}

fn load_one(path: &Path) -> Result<Vec<RawConversation>> {
    match extension(path).as_deref() {
        Some("zip") => load_zip(path),
        Some("json") => {
            let text = fs::read_to_string(path)
                .with_context(|| format!("failed to read {}", path.display()))?;
            parse_conversations(&text, &path.display().to_string())
        }
        _ => bail!(
            "unsupported input type: {} (expect .zip, .json, or a directory)",
            path.display()
        ),
    }
}

fn load_zip(path: &Path) -> Result<Vec<RawConversation>> {
    let file = File::open(path).with_context(|| format!("failed to open {}", path.display()))?;
    let mut archive = zip::ZipArchive::new(file)
        .with_context(|| format!("failed to read zip {}", path.display()))?;

    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .with_context(|| format!("failed to read zip entry #{index} in {}", path.display()))?;
        if entry.name().rsplit('/').next() != Some(ZIP_ARCHIVE_NAME) {
            continue;
        }
        let mut text = String::new();
        entry
            .read_to_string(&mut text)
            .with_context(|| format!("failed to read {ZIP_ARCHIVE_NAME} in {}", path.display()))?;
        return parse_conversations(
            &text,
            &format!("{ZIP_ARCHIVE_NAME} in {}", path.display()),
        );
    }

    bail!("{ZIP_ARCHIVE_NAME} not found in {}", path.display())
}

fn parse_conversations(text: &str, source: &str) -> Result<Vec<RawConversation>> {
    // ChatFormat 允许 UTF-8 BOM，serde_json 不接受，先剥掉。
    let text = text.trim_start_matches('\u{feff}');
    if let Ok(list) = serde_json::from_str::<Vec<RawConversation>>(text) {
        return Ok(list);
    }
    if let Ok(single) = serde_json::from_str::<RawConversation>(text) {
        return Ok(vec![single]);
    }
    bail!("failed to parse Claude conversations from {source}")
}

fn extension(path: &Path) -> Option<String> {
    path.extension()
        .and_then(|value| value.to_str())
        .map(str::to_ascii_lowercase)
}

// ═══════════════════════════════════════════════════════════
//  映射
// ═══════════════════════════════════════════════════════════

fn to_conversation(raw: &RawConversation) -> Conversation {
    let time_secs = raw.created_at.as_deref().and_then(time::rfc3339_to_secs);
    let sort_ms = raw
        .updated_at
        .as_deref()
        .and_then(time::rfc3339_to_secs)
        .or(time_secs)
        .map(|secs| secs * 1000);

    let uuid = raw
        .uuid
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let mut extra = Vec::new();
    if let Some(uuid) = uuid {
        extra.push(MetadataLine::code("Conversation ID", uuid));
    }

    Conversation {
        title: raw.name.clone().unwrap_or_default(),
        model: UNKNOWN_MODEL.to_string(),
        time_secs,
        sort_ms,
        url: uuid.map(|uuid| format!("{URL_BASE}/{uuid}")),
        extra,
        group: None,
        id: uuid.map(str::to_string),
        messages: raw.chat_messages.iter().filter_map(to_message).collect(),
    }
}

fn to_message(raw: &RawMessage) -> Option<Message> {
    let mut body: Vec<String> = Vec::new();

    let text = raw.text.as_deref().map(str::trim).unwrap_or("");
    if text.is_empty() {
        // `text` 为空时回退到 content 里的文本片段；tool_use / tool_result / token_budget 一律忽略。
        for part in &raw.content {
            if part.kind.as_deref() != Some("text") {
                continue;
            }
            let Some(text) = part.text.as_deref().map(str::trim).filter(|t| !t.is_empty()) else {
                continue;
            };
            body.push(text.to_string());
        }
    } else {
        body.push(text.to_string());
    }

    // 附件没有 URL，按占位引用输出。
    for name in raw.file_names() {
        body.push(format!("[{name}](attachment)"));
    }

    if body.iter().all(|part| part.trim().is_empty()) {
        return None;
    }

    let role = match raw.sender.as_deref().map(str::trim) {
        Some(sender) if sender.eq_ignore_ascii_case("human") || sender.eq_ignore_ascii_case("user") => {
            Role::User
        }
        // assistant 与一切无法识别的角色都兜底成 Assistant（契约 §4.2）
        _ => Role::Assistant,
    };

    Some(Message {
        role,
        thinking: Vec::new(),
        body,
    })
}

fn display_title(title: &str) -> String {
    chatformat::sanitize_filename_with(title, chatformat::SINGLE_NAME_MAX, "Untitled_Conversation")
}

fn resolve_target(input: &Path, output: Option<&Path>, zip_name: String) -> Result<PathBuf> {
    match output {
        Some(path) if is_zip(path) => Ok(path.to_path_buf()),
        Some(dir) => {
            fs::create_dir_all(dir)
                .with_context(|| format!("failed to create output dir {}", dir.display()))?;
            Ok(dir.join(zip_name))
        }
        None => {
            let parent = input
                .parent()
                .filter(|path| !path.as_os_str().is_empty())
                .unwrap_or_else(|| Path::new("."));
            Ok(parent.join(zip_name))
        }
    }
}

fn is_zip(path: &Path) -> bool {
    extension(path).as_deref() == Some("zip")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> Vec<RawConversation> {
        parse_conversations(text, "test").expect("should parse")
    }

    #[test]
    fn maps_roles_and_ignores_tool_parts() {
        let raws = parse(
            r##"[{
                "uuid": "u1",
                "name": "Greeting / test",
                "created_at": "2024-09-09T14:22:31.424169Z",
                "updated_at": "2024-09-10T14:22:31.000000Z",
                "chat_messages": [
                    {"sender": "human", "text": "# 你好"},
                    {"sender": "assistant", "text": "hi", "content": [{"type": "tool_use", "text": "x"}]},
                    {"sender": "weird", "text": "fallback"}
                ]
            }]"##,
        );
        let conversation = to_conversation(&raws[0]);
        assert_eq!(conversation.model, "Unknown");
        assert_eq!(conversation.time_secs, Some(1_725_891_751));
        assert_eq!(conversation.url.as_deref(), Some("https://claude.ai/chat/u1"));
        assert_eq!(conversation.messages.len(), 3);
        assert_eq!(conversation.messages[0].role, Role::User);
        assert_eq!(conversation.messages[1].role, Role::Assistant);
        assert_eq!(conversation.messages[2].role, Role::Assistant);

        let markdown = conversation.render();
        assert!(markdown.starts_with("## Metadata\n"), "{markdown}");
        assert!(markdown.contains("- **Model:** `Unknown`\n"), "{markdown}");
        assert!(markdown.contains("- **Conversation ID:** `u1`\n"), "{markdown}");
        assert!(markdown.contains("### 🧑‍💻 User\n\n**你好**\n"), "{markdown}");
        assert!(!markdown.contains("tool_use"), "{markdown}");
    }

    #[test]
    fn renders_attachments_as_placeholders() {
        let raws = parse(
            r#"[{
                "name": "files",
                "chat_messages": [{
                    "sender": "human",
                    "text": "see attached",
                    "attachments": [{"file_name": "notes.txt", "file_size": 3, "file_type": "txt"}],
                    "files": [{"file_name": "notes.txt"}, {"file_name": "extra.txt"}]
                }]
            }]"#,
        );
        let conversation = to_conversation(&raws[0]);
        let markdown = conversation.render();
        assert!(markdown.contains("[notes.txt](attachment)"), "{markdown}");
        assert!(markdown.contains("[extra.txt](attachment)"), "{markdown}");
        // 去重：notes.txt 只出现一次
        assert_eq!(markdown.matches("[notes.txt](attachment)").count(), 1);
    }

    #[test]
    fn skips_conversations_without_messages() {
        let raws = parse(r#"[{"name": "empty", "chat_messages": []}]"#);
        let conversation = to_conversation(&raws[0]);
        assert!(conversation.messages.is_empty());
    }

    #[test]
    fn tolerates_null_fields() {
        let raws = parse(r#"[{"name": null, "chat_messages": null, "uuid": null}]"#);
        let conversation = to_conversation(&raws[0]);
        assert!(conversation.messages.is_empty());
        assert_eq!(conversation.title, "");
    }

    #[test]
    fn strips_bom() {
        let raws = parse("\u{feff}[{\"name\":\"a\",\"chat_messages\":[]}]");
        assert_eq!(raws.len(), 1);
    }
}
