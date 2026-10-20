//! 把 ChatGPT 官方导出（ZIP，内含 `conversations.json`）转换为 AfterChat 对话 ZIP。
//!
//! 输出契约见仓库 `docs/CHATFORMAT.md`，全部渲染/命名/打包都走 `chatformat`。
//!
//! 输入支持 ZIP（如官方导出的哈希命名 ZIP）、单个 JSON、以及包含它们的目录；
//! 输出始终是一个符合规范的 ZIP。

use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use chatformat::{
    Conversation, ExportFailure, Message, MetadataLine, NameStyle, Role, UNKNOWN_MODEL, ZipExport,
    default_zip_name,
};
use regex::Regex;
use serde::{Deserialize, Deserializer};
use walkdir::WalkDir;

const PLATFORM: &str = "chatgpt";
const URL_BASE: &str = "https://chatgpt.com/c";
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

    if chatformat::is_afterchat_zip(&options.input) {
        let out_dir = match options.output.as_deref() {
            Some(out) if !is_zip(out) => out.to_path_buf(),
            Some(out) => out.parent().unwrap_or(out).to_path_buf(),
            None => {
                let stem = options
                    .input
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("chatgpt_extracted");
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

    let raws = load(&options.input)?;
    if raws.is_empty() {
        bail!(
            "no ChatGPT conversations found in {}",
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
            "no exportable ChatGPT conversations in {}",
            options.input.display()
        );
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
//  输入解析
// ═══════════════════════════════════════════════════════════

#[derive(Debug, Deserialize)]
struct RawConversation {
    #[serde(default, deserialize_with = "null_to_default")]
    id: Option<String>,
    #[serde(default, deserialize_with = "null_to_default")]
    conversation_id: Option<String>,
    #[serde(default, deserialize_with = "null_to_default")]
    title: Option<String>,
    #[serde(default, deserialize_with = "null_to_default")]
    create_time: Option<f64>,
    #[serde(default, deserialize_with = "null_to_default")]
    update_time: Option<f64>,
    #[serde(default, deserialize_with = "null_to_default")]
    default_model_slug: Option<String>,
    #[serde(default, deserialize_with = "null_to_default")]
    current_node: Option<String>,
    #[serde(default, deserialize_with = "null_to_default")]
    mapping: HashMap<String, RawNode>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct RawNode {
    #[serde(default, deserialize_with = "null_to_default")]
    id: Option<String>,
    #[serde(default, deserialize_with = "null_to_default")]
    parent: Option<String>,
    #[serde(default, deserialize_with = "null_to_default")]
    children: Vec<String>,
    #[serde(default, deserialize_with = "null_to_default")]
    message: Option<RawMessage>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct RawMessage {
    #[serde(default, deserialize_with = "null_to_default")]
    id: Option<String>,
    #[serde(default, deserialize_with = "null_to_default")]
    author: Option<RawAuthor>,
    #[serde(default, deserialize_with = "null_to_default")]
    create_time: Option<f64>,
    #[serde(default, deserialize_with = "null_to_default")]
    content: Option<RawContent>,
    #[serde(default, deserialize_with = "null_to_default")]
    metadata: Option<RawMetadata>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct RawAuthor {
    #[serde(default, deserialize_with = "null_to_default")]
    role: Option<String>,
    #[serde(default, deserialize_with = "null_to_default")]
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct RawContent {
    #[serde(default, deserialize_with = "null_to_default")]
    content_type: Option<String>,
    #[serde(default, deserialize_with = "null_to_default")]
    parts: Vec<serde_json::Value>,
    #[serde(default, deserialize_with = "null_to_default")]
    text: Option<String>,
    #[serde(default, deserialize_with = "null_to_default")]
    thoughts: Vec<RawThought>,
    #[serde(default, deserialize_with = "null_to_default")]
    content: Option<String>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct RawThought {
    #[serde(default, deserialize_with = "null_to_default")]
    summary: Option<String>,
    #[serde(default, deserialize_with = "null_to_default")]
    content: Option<String>,
    #[serde(default, deserialize_with = "null_to_default")]
    finished: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct RawMetadata {
    #[serde(default, deserialize_with = "null_to_default")]
    model_slug: Option<String>,
    #[serde(default, deserialize_with = "null_to_default")]
    content_references: Vec<RawContentReference>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct RawContentReference {
    #[serde(default, deserialize_with = "null_to_default")]
    alt: Option<String>,
    #[serde(default, deserialize_with = "null_to_default")]
    items: Vec<RawContentRefItem>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct RawContentRefItem {
    #[serde(default, deserialize_with = "null_to_default")]
    title: Option<String>,
    #[serde(default, deserialize_with = "null_to_default")]
    url: Option<String>,
    #[serde(default, deserialize_with = "null_to_default")]
    attribution: Option<String>,
    #[serde(default, deserialize_with = "null_to_default")]
    refs: Vec<RawRef>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct RawRef {
    #[serde(default, deserialize_with = "null_to_default")]
    turn_index: Option<usize>,
    #[serde(default, deserialize_with = "null_to_default")]
    ref_index: Option<usize>,
    #[serde(default, deserialize_with = "null_to_default")]
    ref_type: Option<String>,
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
        return parse_conversations(&text, &format!("{ZIP_ARCHIVE_NAME} in {}", path.display()));
    }

    bail!("{ZIP_ARCHIVE_NAME} not found in {}", path.display())
}

fn parse_conversations(text: &str, source: &str) -> Result<Vec<RawConversation>> {
    let text = text.trim_start_matches('\u{feff}');

    if let Ok(list) = serde_json::from_str::<Vec<RawConversation>>(text) {
        // 确保反序列化出的对象确实含有 ChatGPT 特征的 mapping 树，避免混淆 Claude 导出的 conversations.json
        if list.iter().any(|c| !c.mapping.is_empty()) {
            return Ok(list);
        }
    }
    if let Ok(single) = serde_json::from_str::<RawConversation>(text) {
        if !single.mapping.is_empty() {
            return Ok(vec![single]);
        }
    }
    bail!("failed to parse ChatGPT conversations from {source} (no mapping found)")
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
    let conv_id = raw
        .id
        .as_deref()
        .or(raw.conversation_id.as_deref())
        .map(str::trim)
        .filter(|v| !v.is_empty());

    let time_secs = raw.create_time.map(|t| t as i64);
    let sort_ms = raw
        .update_time
        .or(raw.create_time)
        .map(|t| (t * 1000.0) as i64);

    let mut extra = Vec::new();
    if let Some(id) = conv_id {
        extra.push(MetadataLine::code("Conversation ID", id));
    }

    let active_node_ids = trace_active_path(raw);
    let (messages, resolved_model) = process_active_path(&active_node_ids, &raw.mapping);

    let model = resolved_model
        .or_else(|| {
            raw.default_model_slug
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty() && !s.eq_ignore_ascii_case("auto"))
                .map(str::to_string)
        })
        .unwrap_or_else(|| UNKNOWN_MODEL.to_string());

    Conversation {
        title: raw.title.clone().unwrap_or_default(),
        model,
        time_secs,
        sort_ms,
        url: conv_id.map(|id| format!("{URL_BASE}/{id}")),
        extra,
        group: None,
        id: conv_id.map(str::to_string),
        messages,
    }
}

/// 从 `current_node` 开始沿 `parent` 向上回溯到根节点，翻转得到按时序排列的活跃分支。
fn trace_active_path(raw: &RawConversation) -> Vec<String> {
    if let Some(curr) = raw.current_node.as_deref() {
        if raw.mapping.contains_key(curr) {
            let mut path = Vec::new();
            let mut visited = HashSet::new();
            let mut node_id = Some(curr.to_string());

            while let Some(nid) = node_id {
                if !visited.insert(nid.clone()) {
                    break;
                }
                path.push(nid.clone());
                node_id = raw.mapping.get(&nid).and_then(|node| node.parent.clone());
            }

            path.reverse();
            return path;
        }
    }

    // 回退策略：若缺失 current_node，从根节点出发走每一步的最末子节点
    let root_id = raw.mapping.iter().find_map(|(id, node)| {
        let is_root = node
            .parent
            .as_ref()
            .map_or(true, |p| !raw.mapping.contains_key(p));
        if is_root { Some(id.clone()) } else { None }
    });

    let mut path = Vec::new();
    let mut curr_id = root_id;
    let mut visited = HashSet::new();
    while let Some(nid) = curr_id {
        if !visited.insert(nid.clone()) {
            break;
        }
        path.push(nid.clone());
        curr_id = raw
            .mapping
            .get(&nid)
            .and_then(|node| node.children.last().cloned());
    }

    path
}

/// 遍历活跃路径，把连续的 Assistant 节点（思考、检索、正文）聚合成单个 Assistant 消息。
fn process_active_path(
    path: &[String],
    mapping: &HashMap<String, RawNode>,
) -> (Vec<Message>, Option<String>) {
    let mut messages = Vec::new();
    let mut resolved_model: Option<String> = None;

    let mut pending_thinking: Vec<String> = Vec::new();
    let mut pending_body: Vec<String> = Vec::new();

    let flush_assistant = |pending_thinking: &mut Vec<String>,
                           pending_body: &mut Vec<String>,
                           messages: &mut Vec<Message>| {
        if pending_thinking.is_empty() && pending_body.is_empty() {
            return;
        }
        let thinking = std::mem::take(pending_thinking);
        let mut body = std::mem::take(pending_body);

        if body.is_empty() && !thinking.is_empty() {
            // 兜底：若只有思维链而无回复正文，以占位正文保证消息体完整
            body.push("*(无正文回复)*".to_string());
        }

        messages.push(Message::assistant_blocks(thinking, body));
    };

    for nid in path {
        let Some(node) = mapping.get(nid) else {
            continue;
        };
        let Some(msg) = &node.message else { continue };

        let role_str = msg
            .author
            .as_ref()
            .and_then(|a| a.role.as_deref())
            .unwrap_or("assistant");

        // 收集模型名（优先取活跃路径中最晚出现的具体助手模型）
        if let Some(slug) = msg.metadata.as_ref().and_then(|m| m.model_slug.as_deref()) {
            let slug = slug.trim();
            if !slug.is_empty() && !slug.eq_ignore_ascii_case("auto") {
                resolved_model = Some(slug.to_string());
            }
        }

        match role_str.to_ascii_lowercase().as_str() {
            "user" => {
                flush_assistant(&mut pending_thinking, &mut pending_body, &mut messages);
                let body = extract_user_body(msg);
                if !body.is_empty() {
                    messages.push(Message {
                        role: Role::User,
                        thinking: Vec::new(),
                        body,
                    });
                }
            }
            "system" => {
                flush_assistant(&mut pending_thinking, &mut pending_body, &mut messages);
                let body = extract_text_from_content(msg.content.as_ref(), msg.metadata.as_ref());
                if !body.is_empty() {
                    messages.push(Message {
                        role: Role::System,
                        thinking: Vec::new(),
                        body,
                    });
                }
            }
            _ => {
                // Assistant 节点（包含 thoughts / reasoning_recap / text）
                let content_type = msg
                    .content
                    .as_ref()
                    .and_then(|c| c.content_type.as_deref())
                    .unwrap_or("text");

                match content_type {
                    "thoughts" => {
                        if let Some(content) = &msg.content {
                            for thought in &content.thoughts {
                                if let Some(t_content) = thought.content.as_deref().map(str::trim) {
                                    if !t_content.is_empty() {
                                        pending_thinking.push(t_content.to_string());
                                        continue;
                                    }
                                }
                                if let Some(summary) = thought.summary.as_deref().map(str::trim) {
                                    if !summary.is_empty() {
                                        pending_thinking.push(summary.to_string());
                                    }
                                }
                            }
                        }
                    }
                    "reasoning_recap" => {
                        if let Some(recap) = msg
                            .content
                            .as_ref()
                            .and_then(|c| c.content.as_deref())
                            .map(str::trim)
                        {
                            if !recap.is_empty() {
                                pending_thinking.push(recap.to_string());
                            }
                        }
                    }
                    _ => {
                        let text_parts =
                            extract_text_from_content(msg.content.as_ref(), msg.metadata.as_ref());
                        for part in text_parts {
                            if !part.trim().is_empty() {
                                pending_body.push(part);
                            }
                        }
                    }
                }
            }
        }
    }

    flush_assistant(&mut pending_thinking, &mut pending_body, &mut messages);

    (messages, resolved_model)
}

fn extract_user_body(msg: &RawMessage) -> Vec<String> {
    let mut body = Vec::new();
    let Some(content) = &msg.content else {
        return body;
    };

    if let Some(text) = content.text.as_deref().map(str::trim) {
        if !text.is_empty() {
            body.push(text.to_string());
        }
    }

    for part in &content.parts {
        match part {
            serde_json::Value::String(s) => {
                let trimmed = s.trim();
                if !trimmed.is_empty() {
                    body.push(trimmed.to_string());
                }
            }
            serde_json::Value::Object(map) => {
                let ctype = map.get("content_type").and_then(|v| v.as_str());
                if ctype == Some("image_asset_pointer") || map.contains_key("asset_pointer") {
                    body.push("[image](attachment)".to_string());
                }
            }
            _ => {}
        }
    }

    body
}

fn extract_text_from_content(
    content: Option<&RawContent>,
    metadata: Option<&RawMetadata>,
) -> Vec<String> {
    let mut parts = Vec::new();
    let Some(content) = content else {
        return parts;
    };

    let ref_map = build_citation_map(metadata);

    let process_str = |text: &str| -> String { replace_citations(text, &ref_map) };

    if let Some(text) = content.text.as_deref() {
        let processed = process_str(text);
        if !processed.trim().is_empty() {
            parts.push(processed);
        }
    }

    for part in &content.parts {
        if let serde_json::Value::String(s) = part {
            let processed = process_str(s);
            if !processed.trim().is_empty() {
                parts.push(processed);
            }
        }
    }

    if let Some(text) = content.content.as_deref() {
        let processed = process_str(text);
        if !processed.trim().is_empty() {
            parts.push(processed);
        }
    }

    parts
}

/// 从 metadata.content_references 提取 (turn_index, ref_index) -> markdown link
fn build_citation_map(metadata: Option<&RawMetadata>) -> HashMap<(usize, usize), String> {
    let mut map = HashMap::new();
    let Some(metadata) = metadata else {
        return map;
    };

    for r in &metadata.content_references {
        let alt = r.alt.as_deref().map(str::trim).filter(|s| !s.is_empty());
        for item in &r.items {
            let link = if let Some(alt) = alt {
                alt.to_string()
            } else if let Some(url) = item.url.as_deref() {
                let title = item
                    .attribution
                    .as_deref()
                    .or(item.title.as_deref())
                    .unwrap_or("source");
                format!("[{title}]({url})")
            } else {
                continue;
            };

            for r_idx in &item.refs {
                if let (Some(t_idx), Some(ref_i)) = (r_idx.turn_index, r_idx.ref_index) {
                    map.insert((t_idx, ref_i), link.clone());
                }
            }
        }
    }

    map
}

/// 替换正文中的 `\ue200cite\ue202turn0search1\ue201` 为 Markdown 链接，并剥离残留的私有区控制符。
fn replace_citations(text: &str, ref_map: &HashMap<(usize, usize), String>) -> String {
    // 匹配 \ue200cite\ue202([^\ue201]+)\ue201
    let cite_regex = Regex::new(r"\u{e200}cite\u{e202}([^\u{e201}]+)\u{e201}").unwrap();
    let tag_regex = Regex::new(r"turn(\d+)search(\d+)").unwrap();

    let replaced = cite_regex.replace_all(text, |caps: &regex::Captures| {
        let tag = &caps[1];
        if let Some(t_caps) = tag_regex.captures(tag) {
            let t_idx: usize = t_caps[1].parse().unwrap_or(0);
            let r_idx: usize = t_caps[2].parse().unwrap_or(0);
            if let Some(link) = ref_map.get(&(t_idx, r_idx)) {
                return format!(" {link}");
            }
        }
        String::new()
    });

    // 清洗剩余的 Unicode Private Use Area 控制标记
    let clean_regex = Regex::new(r"[\u{e200}-\u{e202}]").unwrap();
    clean_regex.replace_all(&replaced, "").to_string()
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
