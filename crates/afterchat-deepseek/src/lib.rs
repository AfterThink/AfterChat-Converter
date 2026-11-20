//! 把 DeepSeek 官方导出（ZIP，内含 `chat_sessions/*.json`）转换为 AfterChat 对话 ZIP 或 Markdown。
//!
//! 输出契约见仓库 `docs/CHATFORMAT.md`，全部渲染/命名/打包都走 `chatformat`。
//!
//! DeepSeek 导出特征：
//! - 包含 `manifest.json` 与 `chat_sessions/<id>.json`
//! - 每个会话 JSON 含有 `current_message_id`、`messages`
//! - 消息包含 `fragments`（`REQUEST`、`RESPONSE`、`THINK`、`FILE`、`TOOL_*` 等）
//! - 官方导出未提供模型代际 slug，统一填为 `DeepSeek`。

use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use chatformat::{
    Conversation, ExportFailure, Message, MetadataLine, NameStyle, Role, ZipExport,
    default_zip_name, time,
};
use serde::{Deserialize, Deserializer};
use walkdir::WalkDir;

const PLATFORM: &str = "deepseek";
const URL_BASE: &str = "https://chat.deepseek.com/a/chat/s";
const DEFAULT_MODEL: &str = "DeepSeek";

#[derive(Debug, Clone)]
pub struct ConvertOptions {
    /// 输入：ZIP / JSON / 目录
    pub input: PathBuf,
    /// 输出目录，或显式的 `.zip`/`.md` 路径
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

    if chatformat::is_afterchat_zip(&options.input) {
        let out_dir = match options.output.as_deref() {
            Some(out) if !is_zip(out) => out.to_path_buf(),
            Some(out) => out.parent().unwrap_or(out).to_path_buf(),
            None => {
                let stem = options
                    .input
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("deepseek_extracted");
                options.input.parent().unwrap_or(Path::new(".")).join(stem)
            }
        };
        let count = chatformat::unpack_zip(&options.input, &out_dir)?;
        return Ok(RunSummary {
            conversations: count,
            failed: 0,
            output: Some(out_dir),
        });
    }

    let is_single_json_input =
        options.input.is_file() && extension(&options.input).as_deref() == Some("json");
    let raws = load(&options.input)?;
    if raws.is_empty() {
        bail!(
            "no DeepSeek conversations found in {}",
            options.input.display()
        );
    }

    let mut conversations = Vec::new();
    let mut failures = Vec::new();
    for raw in &raws {
        let conversation = to_conversation(raw);
        if conversation.messages.is_empty() {
            failures.push(ExportFailure {
                title: display_title(&conversation.title),
                id: conversation.id.clone().unwrap_or_else(|| "-".to_string()),
                reason: "对话没有任何可导出的消息".to_string(),
            });
        } else {
            conversations.push(conversation);
        }
    }

    if conversations.is_empty() {
        bail!(
            "no exportable DeepSeek conversations in {}",
            options.input.display()
        );
    }

    // 若输入本身是单个 JSON 文件，且仅解析出 1 个有效会话，且未显式指定 .zip 输出：
    // 直接渲染为单个 Markdown 文件输出
    let output_is_zip = options.output.as_deref().is_some_and(is_zip);
    if is_single_json_input && conversations.len() == 1 && !output_is_zip {
        let conversation = &conversations[0];
        let target = if let Some(out) = &options.output {
            if is_dir_or_trailing_slash(out) {
                let filename = format!(
                    "{}.md",
                    NameStyle::Spec.sanitize(&conversation.title, chatformat::SINGLE_NAME_MAX)
                );
                out.join(filename)
            } else {
                out.clone()
            }
        } else {
            let filename = format!(
                "{}.md",
                NameStyle::Spec.sanitize(&conversation.title, chatformat::SINGLE_NAME_MAX)
            );
            options
                .input
                .parent()
                .unwrap_or(Path::new("."))
                .join(filename)
        };

        if let Some(parent) = target.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent).with_context(|| {
                    format!("failed to create output directory {}", parent.display())
                })?;
            }
        }

        fs::write(&target, conversation.render())
            .with_context(|| format!("failed to write {}", target.display()))?;

        return Ok(RunSummary {
            conversations: 1,
            failed: failures.len(),
            output: Some(target),
        });
    }

    // 若指定了输出目录（非 .zip 文件），直接把每个会话以 Markdown 文件形式写入该目录
    if let Some(out_dir) = options.output.as_deref().filter(|p| !is_zip(p)) {
        chatformat::write_directory(&chatformat::DirExport {
            platform: PLATFORM,
            conversations: &conversations,
            failures: &failures,
            output: out_dir,
            source: Some(&options.input),
            name_style: NameStyle::Spec,
            show_progress: options.show_progress,
        })?;

        return Ok(RunSummary {
            conversations: conversations.len(),
            failed: failures.len(),
            output: Some(out_dir.to_path_buf()),
        });
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
//  数据结构
// ═══════════════════════════════════════════════════════════

#[derive(Debug, Deserialize)]
pub struct RawSession {
    #[serde(default, deserialize_with = "null_to_default")]
    pub id: Option<String>,
    #[serde(default, deserialize_with = "null_to_default")]
    pub title: Option<String>,
    #[serde(default, deserialize_with = "null_to_default")]
    pub pinned: Option<bool>,
    #[serde(default, deserialize_with = "null_to_default")]
    pub current_message_id: Option<i64>,
    #[serde(default, deserialize_with = "null_to_default")]
    pub inserted_at: Option<String>,
    #[serde(default, deserialize_with = "null_to_default")]
    pub updated_at: Option<String>,
    #[serde(default, deserialize_with = "null_to_default")]
    pub messages: Vec<RawMessage>,
}

#[derive(Debug, Deserialize)]
pub struct RawMessage {
    #[serde(default, deserialize_with = "null_to_default")]
    pub id: Option<i64>,
    #[serde(default, deserialize_with = "null_to_default")]
    pub parent_id: Option<i64>,
    #[serde(default, deserialize_with = "null_to_default")]
    pub role: Option<String>,
    #[serde(default, deserialize_with = "null_to_default")]
    pub status: Option<String>,
    #[serde(default, deserialize_with = "null_to_default")]
    pub thinking_enabled: Option<bool>,
    #[serde(default, deserialize_with = "null_to_default")]
    pub search_enabled: Option<bool>,
    #[serde(default, deserialize_with = "null_to_default")]
    pub accumulated_token_usage: Option<i64>,
    #[serde(default, deserialize_with = "null_to_default")]
    pub inserted_at: Option<String>,
    #[serde(default, deserialize_with = "null_to_default")]
    pub fragments: Vec<RawFragment>,
}

#[derive(Debug, Deserialize)]
pub struct RawFragment {
    #[serde(default, rename = "type", deserialize_with = "null_to_default")]
    pub fragment_type: Option<String>,
    #[serde(default, deserialize_with = "null_to_default")]
    pub content: Option<String>,
    #[serde(default, deserialize_with = "null_to_default")]
    pub elapsed_secs: Option<f64>,
    #[serde(default, deserialize_with = "null_to_default")]
    pub files: Vec<RawFile>,
}

#[derive(Debug, Deserialize)]
pub struct RawFile {
    #[serde(default, deserialize_with = "null_to_default")]
    pub id: Option<String>,
    #[serde(default, deserialize_with = "null_to_default")]
    pub file_name: Option<String>,
    #[serde(default, deserialize_with = "null_to_default")]
    pub export_path: Option<String>,
    #[serde(default, deserialize_with = "null_to_default")]
    pub file_size: Option<i64>,
    #[serde(default, deserialize_with = "null_to_default")]
    pub is_image: Option<bool>,
}

fn null_to_default<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Default + Deserialize<'de>,
{
    Ok(Option::<T>::deserialize(deserializer)?.unwrap_or_default())
}

// ═══════════════════════════════════════════════════════════
//  输入解析与加载
// ═══════════════════════════════════════════════════════════

fn load(input: &Path) -> Result<Vec<RawSession>> {
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

fn load_one(path: &Path) -> Result<Vec<RawSession>> {
    match extension(path).as_deref() {
        Some("zip") => load_zip(path),
        Some("json") => {
            let text = fs::read_to_string(path)
                .with_context(|| format!("failed to read {}", path.display()))?;
            parse_sessions(&text, &path.display().to_string())
        }
        _ => bail!(
            "unsupported input type: {} (expect .zip, .json, or a directory)",
            path.display()
        ),
    }
}

fn load_zip(path: &Path) -> Result<Vec<RawSession>> {
    let file = File::open(path).with_context(|| format!("failed to open {}", path.display()))?;
    let mut archive = zip::ZipArchive::new(file)
        .with_context(|| format!("failed to read zip {}", path.display()))?;

    let mut sessions = Vec::new();
    let mut session_names = Vec::new();

    for index in 0..archive.len() {
        let entry = archive
            .by_index(index)
            .with_context(|| format!("failed to read zip entry #{index} in {}", path.display()))?;
        let name = entry.name().replace('\\', "/");
        if is_session_json_path(&name) {
            session_names.push(entry.name().to_string());
        }
    }

    if session_names.is_empty() {
        bail!(
            "no DeepSeek chat_sessions/*.json found in {}",
            path.display()
        );
    }

    session_names.sort();
    for entry_name in session_names {
        let mut entry = archive.by_name(&entry_name).with_context(|| {
            format!(
                "failed to open zip entry {entry_name} in {}",
                path.display()
            )
        })?;
        let mut text = String::new();
        entry
            .read_to_string(&mut text)
            .with_context(|| format!("failed to read {entry_name} in {}", path.display()))?;
        match parse_sessions(&text, &entry_name) {
            Ok(mut s) => sessions.append(&mut s),
            Err(err) => log::warn!("skip {entry_name}: {err:#}"),
        }
    }

    if sessions.is_empty() {
        bail!(
            "failed to parse any DeepSeek sessions from {}",
            path.display()
        );
    }

    Ok(sessions)
}

fn is_session_json_path(path: &str) -> bool {
    let normalized = path.replace('\\', "/");
    let parts: Vec<&str> = normalized.split('/').collect();
    if parts.len() == 2 && parts[0] == "chat_sessions" && parts[1].ends_with(".json") {
        return true;
    }
    // Also accept root-level uuid.json if in an unzipped sessions directory
    if parts.len() == 1
        && parts[0].ends_with(".json")
        && !parts[0].eq_ignore_ascii_case("manifest.json")
        && !parts[0].eq_ignore_ascii_case("user.json")
        && !parts[0].eq_ignore_ascii_case("chat_session_index.json")
    {
        return true;
    }
    false
}

fn parse_sessions(text: &str, source: &str) -> Result<Vec<RawSession>> {
    let text = text.trim_start_matches('\u{feff}');

    // 1. 单个会话对象
    if let Ok(single) = serde_json::from_str::<RawSession>(text) {
        let is_in_chat_sessions = source.replace('\\', "/").contains("chat_sessions/");
        if is_in_chat_sessions || single.current_message_id.is_some() || !single.messages.is_empty()
        {
            return Ok(vec![single]);
        }
    }

    // 2. 数组（多个会话对象）
    if let Ok(list) = serde_json::from_str::<Vec<RawSession>>(text) {
        if list
            .iter()
            .any(|s| s.current_message_id.is_some() || !s.messages.is_empty())
        {
            return Ok(list);
        }
    }

    bail!("failed to parse DeepSeek session from {source} (no matching structure)")
}

fn extension(path: &Path) -> Option<String> {
    path.extension()
        .and_then(|value| value.to_str())
        .map(str::to_ascii_lowercase)
}

fn is_dir_or_trailing_slash(path: &Path) -> bool {
    if path.is_dir() {
        return true;
    }
    let s = path.to_string_lossy();
    s.ends_with('/') || s.ends_with('\\')
}

// ═══════════════════════════════════════════════════════════
//  映射
// ═══════════════════════════════════════════════════════════

pub fn to_conversation(raw: &RawSession) -> Conversation {
    let conv_id = raw.id.as_deref().map(str::trim).filter(|v| !v.is_empty());

    let time_secs = raw.inserted_at.as_deref().and_then(time::rfc3339_to_secs);
    let sort_ms = raw
        .updated_at
        .as_deref()
        .and_then(parse_rfc3339_millis)
        .or_else(|| raw.inserted_at.as_deref().and_then(parse_rfc3339_millis))
        .or_else(|| time_secs.map(|s| s * 1000));

    let mut extra = Vec::new();
    if let Some(id) = conv_id {
        extra.push(MetadataLine::code("Conversation ID", id));
    }

    let active_node_ids = trace_active_path(raw);
    let messages = process_active_messages(&active_node_ids, raw);

    Conversation {
        title: raw.title.clone().unwrap_or_default(),
        model: DEFAULT_MODEL.to_string(),
        time_secs,
        sort_ms,
        url: conv_id.map(|id| format!("{URL_BASE}/{id}")),
        extra,
        group: None,
        id: conv_id.map(str::to_string),
        messages,
    }
}

fn parse_rfc3339_millis(text: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(text.trim())
        .ok()
        .map(|dt| dt.timestamp_millis())
}

/// 从 `current_message_id` 开始沿 `parent_id` 向上回溯到根节点，翻转得到按时序排列的活跃分支。
fn trace_active_path(raw: &RawSession) -> Vec<i64> {
    let msg_map: HashMap<i64, &RawMessage> = raw
        .messages
        .iter()
        .filter_map(|m| m.id.map(|id| (id, m)))
        .collect();

    if let Some(curr) = raw.current_message_id {
        if msg_map.contains_key(&curr) {
            let mut path = Vec::new();
            let mut visited = HashSet::new();
            let mut node_id = Some(curr);

            while let Some(nid) = node_id {
                if !visited.insert(nid) {
                    break;
                }
                path.push(nid);
                node_id = msg_map.get(&nid).and_then(|node| node.parent_id);
            }

            path.reverse();
            return path;
        }
    }

    // 回退策略：按 messages 顺序提取有效 id
    raw.messages.iter().filter_map(|m| m.id).collect()
}

fn process_active_messages(active_ids: &[i64], raw: &RawSession) -> Vec<Message> {
    let msg_map: HashMap<i64, &RawMessage> = raw
        .messages
        .iter()
        .filter_map(|m| m.id.map(|id| (id, m)))
        .collect();

    let mut messages = Vec::new();

    for &id in active_ids {
        let Some(msg) = msg_map.get(&id) else {
            continue;
        };

        let role_str = msg.role.as_deref().unwrap_or("ASSISTANT").trim();
        let is_user = role_str.eq_ignore_ascii_case("USER");

        if is_user {
            let mut body_blocks = Vec::new();

            for fragment in &msg.fragments {
                let ftype = fragment.fragment_type.as_deref().unwrap_or("").trim();
                match ftype {
                    "FILE" => {
                        for file in &fragment.files {
                            let is_img = file.is_image.unwrap_or(false);
                            let default_label = if is_img { "image" } else { "attachment" };
                            let label = file
                                .file_name
                                .as_deref()
                                .map(str::trim)
                                .filter(|s| !s.is_empty())
                                .unwrap_or(default_label);

                            if is_img {
                                body_blocks.push(format!("![{label}](attachment)"));
                            } else {
                                body_blocks.push(format!("[{label}](attachment)"));
                            }
                        }
                    }
                    "REQUEST" => {
                        if let Some(content) = fragment
                            .content
                            .as_deref()
                            .map(str::trim)
                            .filter(|s| !s.is_empty())
                        {
                            body_blocks.push(content.to_string());
                        }
                    }
                    _ => {
                        // 其它 fragment 若有非空 content 且非工具噪声亦保留
                        if let Some(content) = fragment
                            .content
                            .as_deref()
                            .map(str::trim)
                            .filter(|s| !s.is_empty())
                        {
                            body_blocks.push(content.to_string());
                        }
                    }
                }
            }

            if !body_blocks.is_empty() {
                messages.push(Message {
                    role: Role::User,
                    thinking: Vec::new(),
                    body: body_blocks,
                });
            }
        } else {
            // Assistant
            let mut thinking_blocks = Vec::new();
            let mut response_blocks = Vec::new();

            for fragment in &msg.fragments {
                let ftype = fragment.fragment_type.as_deref().unwrap_or("").trim();
                match ftype {
                    "THINK" => {
                        if let Some(content) = fragment
                            .content
                            .as_deref()
                            .map(str::trim)
                            .filter(|s| !s.is_empty())
                        {
                            thinking_blocks.push(content.to_string());
                        }
                    }
                    "RESPONSE" => {
                        if let Some(content) = fragment
                            .content
                            .as_deref()
                            .map(str::trim)
                            .filter(|s| !s.is_empty())
                        {
                            response_blocks.push(content.to_string());
                        }
                    }
                    // TOOL_SEARCH, TOOL_OPEN, TOOL_FIND, TOOL_IMAGE_SEARCH, CONTENT_RISK: 工具过程与提示横幅，忽略
                    _ => {}
                }
            }

            if !thinking_blocks.is_empty() || !response_blocks.is_empty() {
                messages.push(Message {
                    role: Role::Assistant,
                    thinking: thinking_blocks,
                    body: response_blocks,
                });
            }
        }
    }

    messages
}

fn display_title(title: &str) -> String {
    chatformat::sanitize_filename_with(title, chatformat::SINGLE_NAME_MAX, "Untitled_Conversation")
}

fn resolve_target(input: &Path, output: Option<&Path>, zip_name: String) -> Result<PathBuf> {
    match output {
        Some(path) => {
            if let Some(parent) = path.parent() {
                if !parent.as_os_str().is_empty() {
                    fs::create_dir_all(parent).with_context(|| {
                        format!("failed to create output directory {}", parent.display())
                    })?;
                }
            }
            Ok(path.to_path_buf())
        }
        None => {
            let parent = input.parent().unwrap_or_else(|| Path::new("."));
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

    #[test]
    fn parses_single_session() {
        let json = r#"{
            "id": "11111111-2222-3333-4444-555555555555",
            "title": "测试对话",
            "current_message_id": 2,
            "inserted_at": "2026-11-20T10:00:00.000Z",
            "updated_at": "2026-11-20T10:05:00.000Z",
            "messages": [
                {
                    "id": 1,
                    "parent_id": null,
                    "role": "USER",
                    "fragments": [
                        { "type": "REQUEST", "content": "你好，请写一首诗" }
                    ]
                },
                {
                    "id": 2,
                    "parent_id": 1,
                    "role": "ASSISTANT",
                    "thinking_enabled": true,
                    "fragments": [
                        { "type": "THINK", "content": "思考诗的押韵" },
                        { "type": "RESPONSE", "content": "春水初生，春林初盛。" }
                    ]
                }
            ]
        }"#;

        let sessions = parse_sessions(json, "test.json").expect("parse session");
        assert_eq!(sessions.len(), 1);

        let conv = to_conversation(&sessions[0]);
        assert_eq!(conv.title, "测试对话");
        assert_eq!(conv.model, "DeepSeek");
        assert_eq!(conv.messages.len(), 2);
        assert_eq!(conv.messages[0].role, Role::User);
        assert_eq!(conv.messages[0].body, vec!["你好，请写一首诗"]);
        assert_eq!(conv.messages[1].role, Role::Assistant);
        assert_eq!(conv.messages[1].thinking, vec!["思考诗的押韵"]);
        assert_eq!(conv.messages[1].body, vec!["春水初生，春林初盛。"]);

        let md = conv.render();
        assert!(md.contains("## Metadata"));
        assert!(md.contains("- **Model:** `DeepSeek`"));
        assert!(md.contains("### 🧑‍💻 User"));
        assert!(md.contains("### 🤖 Assistant"));
        assert!(md.contains("#### 🤔 Thought Process"));
        assert!(md.contains("#### 💡 Response"));
    }

    #[test]
    fn parses_files_in_user_message() {
        let json = r#"{
            "id": "file-conv",
            "title": "文件测试",
            "current_message_id": 1,
            "messages": [
                {
                    "id": 1,
                    "parent_id": null,
                    "role": "USER",
                    "fragments": [
                        {
                            "type": "FILE",
                            "files": [
                                { "file_name": "photo.png", "is_image": true },
                                { "file_name": "doc.pdf", "is_image": false }
                            ]
                        },
                        { "type": "REQUEST", "content": "请分析以上文件" }
                    ]
                }
            ]
        }"#;

        let sessions = parse_sessions(json, "file_test.json").expect("parse session");
        let conv = to_conversation(&sessions[0]);
        assert_eq!(conv.messages.len(), 1);
        assert_eq!(
            conv.messages[0].body,
            vec![
                "![photo.png](attachment)",
                "[doc.pdf](attachment)",
                "请分析以上文件"
            ]
        );
    }
}
