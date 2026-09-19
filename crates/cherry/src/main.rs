use anyhow::{Context, Result, bail};
use chrono::{DateTime, Datelike, FixedOffset, Local, TimeZone, Timelike};
use clap::Parser;
use rayon::prelude::*;
use regex::Regex;
use serde::Deserialize;
use serde_json::Value;
use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use zip::CompressionMethod;
use zip::ZipArchive;
use zip::write::{SimpleFileOptions, ZipWriter};

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Input Cherry backup file (JSON or ZIP)
    #[arg(required = true)]
    input_file: PathBuf,

    /// Output directory, or an explicit `.zip` path
    #[arg(short = 'o', long = "output")]
    output: Option<PathBuf>,
}

#[derive(Debug, Deserialize)]
struct Root {
    #[serde(default, rename = "localStorage")]
    local_storage: HashMap<String, String>,
    #[serde(default, rename = "indexedDB")]
    indexed_db: IndexedDB,
}

#[derive(Debug, Deserialize, Default)]
struct IndexedDB {
    #[serde(default)]
    message_blocks: Vec<MessageBlock>,
    #[serde(default)]
    topics: Vec<Topic>,
}

#[derive(Debug, Deserialize, Clone)]
struct MessageBlock {
    #[serde(default)]
    id: String,
    #[serde(default, rename = "messageId")]
    message_id: Option<String>,
    #[serde(rename = "type", default)]
    type_: String,
    #[serde(default)]
    content: String,
    #[serde(default, rename = "createdAt")]
    created_at: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct Topic {
    #[serde(default)]
    id: String,
    #[serde(default)]
    messages: Vec<Message>,
    #[serde(default, rename = "createdAt")]
    created_at: Option<Value>, // Can be string or number
}

#[derive(Debug, Deserialize)]
struct Message {
    #[serde(default)]
    id: String,
    #[serde(default)]
    role: String,
    #[serde(default)]
    model: Option<Value>,
    #[serde(default)]
    blocks: Vec<String>,
    #[serde(default, rename = "createdAt")]
    created_at: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct PersistData {
    #[serde(default)]
    assistants: String,
}

#[derive(Debug, Deserialize)]
struct AssistantsStore {
    #[serde(default, rename = "defaultAssistant")]
    default_assistant: Option<Assistant>,
    #[serde(default)]
    assistants: Vec<Assistant>,
}

#[derive(Debug, Deserialize, Clone)]
struct Assistant {
    #[serde(default)]
    id: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    prompt: String,
    #[serde(default)]
    topics: Vec<TopicMeta>,
}

#[derive(Debug, Deserialize, Clone)]
struct TopicMeta {
    #[serde(default)]
    id: String,
    #[serde(default)]
    name: Option<String>,
    /// 可能是 RFC3339 字符串，也可能是 epoch 秒 / 毫秒数字。
    /// 用 `Value` 兼容两者：若写死 `Option<String>`，一个数字就会让整棵助手树解析失败，
    /// 结果是**全部**主题退化成 `Untitled` / `Assistant` / `00000000-000000`。
    #[serde(default, rename = "createdAt")]
    created_at: Option<Value>,
}

struct AssistantInfo {
    name: String,
    prompt: String,
}

struct TopicMetadata {
    name: String,
    assistant_id: String,
    created_at: Option<Value>,
}

fn sanitize_filename(filename: &str) -> String {
    let re = Regex::new(r#"[\\/*?:"<>|\r\n]"#).unwrap();
    re.replace_all(filename, "").to_string()
}

fn sanitize_path_component(name: &str, fallback: &str) -> String {
    let sanitized = sanitize_filename(name).trim().to_string();
    if sanitized.is_empty() || sanitized == "." || sanitized == ".." {
        fallback.to_string()
    } else {
        sanitized
    }
}

fn get_created_at_f64(v: &Option<Value>) -> f64 {
    match v {
        Some(Value::Number(n)) => n.as_f64().unwrap_or(0.0),
        Some(Value::String(s)) => {
            if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(s) {
                dt.timestamp() as f64
            } else {
                0.0
            }
        }
        _ => 0.0,
    }
}

/// 解析 cherry 的 `createdAt`（RFC3339 字符串 / epoch 秒或毫秒数字）
fn parse_created_at_value(value: &Value) -> Option<DateTime<FixedOffset>> {
    match value {
        Value::String(text) => DateTime::parse_from_rfc3339(text.trim()).ok(),
        Value::Number(number) => {
            let raw = number.as_f64()?;
            let secs = if raw.abs() >= 1e11 { raw / 1000.0 } else { raw };
            Local
                .timestamp_opt(secs as i64, 0)
                .single()
                .map(|dt| dt.fixed_offset())
        }
        _ => None,
    }
}

/// `^#{1,6}\s+(.+)$`（多行）→ `**$1**`：不保留井号标题，但保留强调。
///
/// 1. **代码围栏内不动**（``` / ~~~），否则会把 Python / Shell 的 `# 注释` 误改成加粗。
/// 2. **整条标题加粗**：标题内原有的 `**` 会被吸收。否则外层 `**` 与内层 `**` 同级交错，
///    CommonMark 会错配定界符，导致整条标题强调不全、甚至残留可见的 `**`。
///    但行内代码（`` ` ``）里的 `**` 不是强调（如 glob `**/*.js`），必须原样保留。
fn strip_hashes(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut fence: Option<&'static str> = None;

    for (idx, line) in text.split('\n').enumerate() {
        if idx > 0 {
            out.push('\n');
        }

        let trimmed = line.trim_start();
        let marker = if trimmed.starts_with("```") {
            Some("```")
        } else if trimmed.starts_with("~~~") {
            Some("~~~")
        } else {
            None
        };

        match fence {
            Some(open) => {
                out.push_str(line);
                if marker == Some(open) {
                    fence = None;
                }
            }
            None => {
                if let Some(open) = marker {
                    fence = Some(open);
                    out.push_str(line);
                } else {
                    out.push_str(&strip_hashes_line(line));
                }
            }
        }
    }

    out
}

fn strip_hashes_line(line: &str) -> Cow<'_, str> {
    let hashes = line.bytes().take_while(|b| *b == b'#').count();
    if hashes == 0 || hashes > 6 {
        return Cow::Borrowed(line);
    }

    let rest = &line[hashes..];
    let trimmed = rest.trim_start_matches(char::is_whitespace);
    if trimmed.len() == rest.len() || trimmed.is_empty() {
        return Cow::Borrowed(line);
    }

    // 吸收标题内的 `**`，使整条标题落在一个加粗里（行内代码段除外）
    let merged = remove_bold_outside_code(trimmed);
    let inner = merged.trim();
    if inner.is_empty() {
        return Cow::Borrowed(line);
    }

    Cow::Owned(format!("**{inner}**"))
}

/// 去掉不在行内代码段里的 `**`。
///
/// 行内代码由反引号界定（CommonMark 的 code span 规则：N 个反引号开始，
/// 同样 N 个反引号结束），其中的 `**` 属于代码内容（如 glob `**/*.js`），原样保留。
fn remove_bold_outside_code(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut index = 0;
    // 当前代码段的定界反引号个数；0 表示不在代码段内
    let mut open_ticks = 0;

    while index < bytes.len() {
        if bytes[index] == b'`' {
            let start = index;
            while index < bytes.len() && bytes[index] == b'`' {
                index += 1;
            }
            let run = index - start;
            if open_ticks == 0 {
                open_ticks = run;
            } else if open_ticks == run {
                open_ticks = 0;
            }
            out.push_str(&text[start..index]);
        } else if open_ticks == 0 && bytes[index] == b'*' && bytes.get(index + 1) == Some(&b'*') {
            index += 2;
        } else {
            let ch = text[index..]
                .chars()
                .next()
                .expect("index is on a char boundary");
            out.push(ch);
            index += ch.len_utf8();
        }
    }

    out
}

fn is_zip_input(path: &Path) -> bool {
    path.extension()
        .and_then(|value| value.to_str())
        .is_some_and(|value| value.eq_ignore_ascii_case("zip"))
}

fn load_root_from_json(path: &Path) -> Result<Root> {
    let raw = fs::read(path).with_context(|| format!("打开失败 {}", path.display()))?;
    parse_root_bytes(&raw).with_context(|| format!("解析 JSON 失败 {}", path.display()))
}

/// ChatFormat 允许 UTF-8 BOM，但 serde_json 不接受，先剥掉
fn parse_root_bytes(raw: &[u8]) -> Result<Root> {
    let raw = raw.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(raw);
    serde_json::from_slice(raw).context("JSON 格式不正确")
}

/// 直接在压缩包里找 `data.json` 读出来解析。
///
/// 不落地解压：既不用调 powershell / unzip / ditto，也没有 zip 路径穿越风险。
fn load_root_from_zip(path: &Path) -> Result<Root> {
    let file = File::open(path).with_context(|| format!("打开压缩包失败 {}", path.display()))?;
    let mut archive =
        ZipArchive::new(file).with_context(|| format!("读取压缩包失败 {}", path.display()))?;

    let index = find_data_json_entry(&mut archive)
        .with_context(|| format!("{} 里没有 data.json", path.display()))?;

    let mut entry = archive
        .by_index(index)
        .with_context(|| format!("读取 data.json 失败 {}", path.display()))?;
    let mut raw = Vec::with_capacity(entry.size() as usize);
    entry
        .read_to_end(&mut raw)
        .with_context(|| format!("读取 data.json 失败 {}", path.display()))?;
    drop(entry);

    parse_root_bytes(&raw).with_context(|| format!("解析 {} 内 data.json 失败", path.display()))
}

/// 找压缩包里的 `data.json`：任意层级、文件名大小写不敏感，取层级最浅的那个
fn find_data_json_entry(archive: &mut ZipArchive<File>) -> Result<usize> {
    let total = archive.len();
    let mut best: Option<(usize, usize)> = None;

    for index in 0..total {
        let entry = archive.by_index(index)?;
        if entry.is_dir() {
            continue;
        }

        let name = entry.name();
        let is_data_json = Path::new(name)
            .file_name()
            .and_then(|file| file.to_str())
            .is_some_and(|file| file.eq_ignore_ascii_case("data.json"));
        if !is_data_json {
            continue;
        }

        let depth = Path::new(name).components().count();
        if best.is_none_or(|(best_depth, _)| depth < best_depth) {
            best = Some((depth, index));
        }
    }

    best.map(|(_, index)| index)
        .ok_or_else(|| anyhow::anyhow!("压缩包里没有 data.json"))
}

fn load_root(input_path: &Path) -> Result<Root> {
    if is_zip_input(input_path) {
        return load_root_from_zip(input_path);
    }

    load_root_from_json(input_path)
}

// ═══════════════════════════════════════════════════════════
//  渲染与打包
// ═══════════════════════════════════════════════════════════

/// 输出 zip 名前缀，遵循 docs/SPEC.md §9：`chat-export-{platform}-all-{timestamp}.zip`
const EXPORT_PREFIX: &str = "chat-export-cherry-all";
/// zip 条目名里标题部分的最大字符数
const ENTRY_TITLE_MAX: usize = 80;

struct RenderContext<'a> {
    assistants: &'a HashMap<String, AssistantInfo>,
    topic_metadata: &'a HashMap<String, TopicMetadata>,
    blocks: &'a HashMap<&'a String, &'a MessageBlock>,
    blocks_by_message_id: &'a HashMap<&'a String, Vec<&'a MessageBlock>>,
}

/// 一个待写入 zip 的对话
struct RenderedTopic {
    /// zip 内路径：`<助手名>/<YYYYMMDD-HHmmss>-<标题>.md`
    entry_name: String,
    markdown: String,
    /// 对话时间（epoch 秒）→ 写进 zip 条目的修改时间
    epoch: Option<i64>,
    /// 排序键（毫秒）
    sort_ms: Option<i64>,
}

/// 被跳过的主题（写进 zip 内的 `export-failures.md`）
struct TopicFailure {
    id: String,
    title: String,
    reason: String,
}

enum TopicOutcome {
    Rendered(Box<RenderedTopic>),
    Skipped(TopicFailure),
}

fn format_local_compact(ms: i64) -> String {
    Local
        .timestamp_opt(ms / 1000, 0)
        .single()
        .map(|dt| dt.format("%Y%m%d-%H%M%S").to_string())
        .unwrap_or_else(|| "00000000-000000".to_string())
}

/// epoch 秒 → zip 的 DOS 时间（本地时间，2 秒精度）
fn zip_datetime(secs: i64) -> Option<zip::DateTime> {
    let dt = Local.timestamp_opt(secs, 0).single()?;
    zip::DateTime::from_date_and_time(
        dt.year() as u16,
        dt.month() as u8,
        dt.day() as u8,
        dt.hour() as u8,
        dt.minute() as u8,
        dt.second() as u8,
    )
    .ok()
}

fn truncate_chars(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    text.chars().take(max).collect()
}

/// 把单个主题渲染成契约格式的 Markdown，并算好它在 zip 里的路径与时间
fn render_topic(ctx: &RenderContext<'_>, topic: &Topic) -> TopicOutcome {
    let topic_id = topic.id.as_str();
    let meta = ctx.topic_metadata.get(topic_id);
    let topic_name = meta
        .map(|m| m.name.clone())
        .unwrap_or_else(|| "Untitled".to_string());
    let assistant_id = meta.map(|m| m.assistant_id.as_str()).unwrap_or("default");

    // 元数据里的时间优先，其次用 topic 自身的；两者都可能是字符串或数字
    let created_at_dt = meta
        .and_then(|m| m.created_at.as_ref())
        .and_then(parse_created_at_value)
        .or_else(|| topic.created_at.as_ref().and_then(parse_created_at_value));

    // 契约 §2：`- **Time:** <time>`，与本项目其它转换器同一口径（本地时间）
    let time_str = created_at_dt
        .map(|dt| {
            dt.with_timezone(&Local)
                .format("%Y-%m-%d %H:%M:%S %:z")
                .to_string()
        })
        .unwrap_or_else(|| "unknown".to_string());

    let assistant_info = ctx.assistants.get(assistant_id);
    let assistant_name = assistant_info
        .map(|a| a.name.as_str())
        .unwrap_or("Assistant");
    let system_instruction = assistant_info.map(|a| a.prompt.as_str()).unwrap_or("");

    let mut topic_messages = topic.messages.iter().collect::<Vec<_>>();
    if topic_messages.is_empty() {
        return TopicOutcome::Skipped(TopicFailure {
            id: topic_id.to_string(),
            title: topic_name,
            reason: "没有消息".to_string(),
        });
    }

    topic_messages.sort_by(|a, b| {
        let ta = get_created_at_f64(&a.created_at);
        let tb = get_created_at_f64(&b.created_at);
        ta.partial_cmp(&tb).unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut md_content = String::new();
    md_content.push_str("## Metadata\n\n");
    md_content.push_str("### Run Settings\n\n");

    // First model
    let mut first_model = "Unknown".to_string();
    for m in &topic_messages {
        if m.role == "assistant"
            && let Some(model_info) = &m.model
        {
            match model_info {
                Value::Object(map) => {
                    if let Some(id) = map.get("id")
                        && let Some(s) = id.as_str()
                    {
                        first_model = s.to_string();
                    }
                }
                Value::String(s) => first_model = s.clone(),
                _ => {}
            }
            if first_model != "Unknown" {
                break;
            }
        }
    }

    // 契约 §2 推荐的键排在前面，cherry 特有的键随后（附加键不违规）
    md_content.push_str(&format!("- **Model:** `{first_model}`\n"));
    md_content.push_str(&format!("- **Time:** {time_str}\n"));
    md_content.push_str(&format!("- **Topic ID:** `{topic_id}`\n"));
    md_content.push_str(&format!("- **Assistant:** `{assistant_name}`\n\n"));

    md_content.push_str("## Conversation\n\n");

    // 契约 §3：系统提示是对话的第一条消息
    if !system_instruction.trim().is_empty() {
        md_content.push_str("### ⚙️ System\n\n");
        md_content.push_str(&strip_hashes(system_instruction));
        md_content.push_str("\n\n");
    }

    for msg in &topic_messages {
        let role = msg.role.as_str();

        let mut msg_blocks_content: Vec<&MessageBlock> = Vec::new();
        for bid in &msg.blocks {
            if let Some(block) = ctx.blocks.get(bid) {
                msg_blocks_content.push(block);
            }
        }

        if msg_blocks_content.is_empty()
            && let Some(blocks) = ctx.blocks_by_message_id.get(&msg.id)
        {
            msg_blocks_content.extend(blocks);
        }

        // Sort blocks
        msg_blocks_content.sort_by(|a, b| {
            let ta = get_created_at_f64(&a.created_at);
            let tb = get_created_at_f64(&b.created_at);
            ta.partial_cmp(&tb).unwrap_or(std::cmp::Ordering::Equal)
        });

        // 契约 §3：把助手消息拆成「思考」与「回复」两段，各只出一个标题
        let mut thoughts: Vec<&str> = Vec::new();
        let mut responses: Vec<&str> = Vec::new();
        for block in &msg_blocks_content {
            if block.content.trim().is_empty() {
                continue;
            }
            if block.type_ == "thinking" {
                thoughts.push(block.content.as_str());
            } else {
                responses.push(block.content.as_str());
            }
        }

        if thoughts.is_empty() && responses.is_empty() {
            continue;
        }

        let header = match role {
            "user" => "### 🧑‍💻 User",
            "system" => "### ⚙️ System",
            _ => "### 🤖 Assistant",
        };
        md_content.push_str(header);
        md_content.push_str("\n\n");

        if !thoughts.is_empty() {
            md_content.push_str("#### 🤔 Thought Process\n\n");
            md_content.push_str(&strip_hashes(&thoughts.join("\n\n")));
            md_content.push_str("\n\n");
            if role != "user" && !responses.is_empty() {
                md_content.push_str("#### 💡 Response\n\n");
            }
        }

        if !responses.is_empty() {
            md_content.push_str(&strip_hashes(&responses.join("\n\n")));
            md_content.push_str("\n\n");
        }
    }

    let mut safe_name = sanitize_filename(&topic_name);
    if safe_name.trim().is_empty() {
        safe_name = "Untitled_Conversation".to_string();
    }
    let prefix = created_at_dt
        .map(|dt| format_local_compact(dt.timestamp_millis()))
        .unwrap_or_else(|| "00000000-000000".to_string());

    TopicOutcome::Rendered(Box::new(RenderedTopic {
        entry_name: format!(
            "{}/{}-{}.md",
            sanitize_path_component(assistant_name, "Assistant"),
            prefix,
            truncate_chars(safe_name.trim(), ENTRY_TITLE_MAX)
        ),
        markdown: md_content,
        epoch: created_at_dt.map(|dt| dt.timestamp()),
        sort_ms: created_at_dt.map(|dt| dt.timestamp_millis()),
    }))
}

/// 同一条目名重复时追加 `-2` / `-3`（扩展名保持在末尾）
fn unique_entry_name(name: &str, used: &mut HashSet<String>) -> String {
    if used.insert(name.to_string()) {
        return name.to_string();
    }

    let (stem, ext) = match name.rsplit_once('.') {
        Some((stem, ext)) => (stem.to_string(), format!(".{ext}")),
        None => (name.to_string(), String::new()),
    };

    let mut counter = 2;
    loop {
        let candidate = format!("{stem}-{counter}{ext}");
        if used.insert(candidate.clone()) {
            return candidate;
        }
        counter += 1;
    }
}

fn build_failure_markdown(source: &Path, failures: &[TopicFailure]) -> String {
    let mut lines: Vec<String> = Vec::new();
    lines.push("# Export Failures".to_string());
    lines.push(String::new());
    lines.push("## Metadata".to_string());
    lines.push(String::new());
    lines.push("- **Platform:** `cherry-studio`".to_string());
    lines.push(format!("- **Source:** `{}`", source.display()));
    lines.push(format!("- **Skipped:** {}", failures.len()));
    lines.push(String::new());
    lines.push("这些主题没有任何可导出的消息，因此未生成 Markdown。".to_string());
    lines.push(String::new());

    for (index, failure) in failures.iter().enumerate() {
        lines.push(format!("## {}. {}", index + 1, failure.title));
        lines.push(String::new());
        lines.push(format!("- **Topic ID:** `{}`", failure.id));
        lines.push(format!("- **Reason:** {}", failure.reason));
        lines.push(String::new());
    }

    lines.join("\n")
}

fn write_zip_export(
    source: &Path,
    topics: &[RenderedTopic],
    failures: &[TopicFailure],
    zip_path: &Path,
) -> Result<()> {
    let file =
        File::create(zip_path).with_context(|| format!("创建压缩包失败 {}", zip_path.display()))?;
    let mut zip = ZipWriter::new(file);
    let base = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);

    for topic in topics {
        let options = match topic.epoch.and_then(zip_datetime) {
            Some(datetime) => base.last_modified_time(datetime),
            None => base,
        };
        zip.start_file(topic.entry_name.clone(), options)
            .with_context(|| format!("写入 {} 失败", topic.entry_name))?;
        zip.write_all(topic.markdown.as_bytes())
            .with_context(|| format!("写入 {} 失败", topic.entry_name))?;
    }

    if !failures.is_empty() {
        let report = build_failure_markdown(source, failures);
        // 失败报告是「刚生成的」，不是某个对话，用当前时间（不设的话会退成 1980-01-01）
        let options = zip_datetime(Local::now().timestamp())
            .map(|datetime| base.last_modified_time(datetime))
            .unwrap_or(base);
        zip.start_file("export-failures.md", options)
            .context("写入 export-failures.md 失败")?;
        zip.write_all(report.as_bytes())
            .context("写入 export-failures.md 失败")?;
    }

    zip.finish()
        .with_context(|| format!("收尾压缩包失败 {}", zip_path.display()))?;

    Ok(())
}

fn is_output_file(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|s| s.to_str()),
        Some("zip") | Some("md")
    )
}

/// `-o` 指向 .zip 文件就当作目标文件，否则当作输出目录（省略则与输入同目录）
fn resolve_zip_target(input: &Path, output: Option<&Path>, zip_name: String) -> Result<PathBuf> {
    match output {
        Some(path) if is_output_file(path) => Ok(path.to_path_buf()),
        Some(dir) => {
            fs::create_dir_all(dir)
                .with_context(|| format!("创建输出目录失败 {}", dir.display()))?;
            Ok(dir.join(zip_name))
        }
        None => {
            let parent = input.parent().unwrap_or_else(|| Path::new("."));
            Ok(parent.join(zip_name))
        }
    }
}

fn main() -> Result<()> {
    let args = Args::parse();
    let input_path = args.input_file;

    if !input_path.exists() {
        bail!("找不到文件: {}", input_path.display());
    }

    let root = load_root(&input_path)?;

    if root.indexed_db.topics.is_empty()
        && root.local_storage.is_empty()
        && root.indexed_db.message_blocks.is_empty()
    {
        bail!("Not a Cherry Studio export: no topics, localStorage, or message_blocks found");
    }

    // 1. Parse assistants
    let mut assistants_map: HashMap<String, AssistantInfo> = HashMap::new();
    let mut topic_metadata_map: HashMap<String, TopicMetadata> = HashMap::new();

    if let Some(persist_str) = root.local_storage.get("persist:cherry-studio")
        && let Ok(persist_data) = serde_json::from_str::<PersistData>(persist_str)
        && let Ok(assistants_store) =
            serde_json::from_str::<AssistantsStore>(&persist_data.assistants)
    {
        let mut all_assistants = Vec::new();
        if let Some(da) = assistants_store.default_assistant {
            all_assistants.push(da);
        }
        all_assistants.extend(assistants_store.assistants);

        for assistant in all_assistants {
            assistants_map.insert(
                assistant.id.clone(),
                AssistantInfo {
                    name: assistant.name.clone(),
                    prompt: assistant.prompt.clone(),
                },
            );

            for t in assistant.topics {
                topic_metadata_map.insert(
                    t.id.clone(),
                    TopicMetadata {
                        name: t.name.unwrap_or_else(|| "Untitled".to_string()),
                        assistant_id: assistant.id.clone(),
                        created_at: t.created_at,
                    },
                );
            }
        }
    }

    // 2. Prepare blocks maps (by ID and by MessageID)
    let mut blocks_map: HashMap<&String, &MessageBlock> = HashMap::new();
    let mut blocks_by_message_id: HashMap<&String, Vec<&MessageBlock>> = HashMap::new();

    for block in &root.indexed_db.message_blocks {
        blocks_map.insert(&block.id, block);
        if let Some(mid) = &block.message_id {
            blocks_by_message_id.entry(mid).or_default().push(block);
        }
    }

    // 3. Render topics in parallel
    let topics = &root.indexed_db.topics;

    if topics.is_empty() {
        bail!("未找到任何对话主题 (topics)");
    }

    println!("找到 {} 个对话主题，开始转换...", topics.len());

    let ctx = RenderContext {
        assistants: &assistants_map,
        topic_metadata: &topic_metadata_map,
        blocks: &blocks_map,
        blocks_by_message_id: &blocks_by_message_id,
    };

    let outcomes: Vec<TopicOutcome> = topics.par_iter().map(|t| render_topic(&ctx, t)).collect();

    let mut rendered: Vec<RenderedTopic> = Vec::new();
    let mut failures: Vec<TopicFailure> = Vec::new();
    for outcome in outcomes {
        match outcome {
            TopicOutcome::Rendered(topic) => rendered.push(*topic),
            TopicOutcome::Skipped(failure) => failures.push(failure),
        }
    }

    // zip 内按对话时间从旧到新
    rendered.sort_by(|a, b| match (a.sort_ms, b.sort_ms) {
        (None, None) => a.entry_name.cmp(&b.entry_name),
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (Some(_), None) => std::cmp::Ordering::Less,
        (Some(left), Some(right)) => left
            .cmp(&right)
            .then_with(|| a.entry_name.cmp(&b.entry_name)),
    });

    let mut used_names: HashSet<String> = HashSet::new();
    for topic in &mut rendered {
        topic.entry_name = unique_entry_name(&topic.entry_name, &mut used_names);
    }

    let zip_name = format!(
        "{EXPORT_PREFIX}-{}.zip",
        chrono::Utc::now().timestamp_millis()
    );
    let target = resolve_zip_target(&input_path, args.output.as_deref(), zip_name)?;

    if let Some(parent) = target.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)
            .with_context(|| format!("创建输出目录失败 {}", parent.display()))?;
    }

    write_zip_export(&input_path, &rendered, &failures, &target)?;

    println!("已打包 {} 个对话 → {}", rendered.len(), target.display());
    if !failures.is_empty() {
        println!(
            "跳过 {} 个空主题（详见压缩包内 export-failures.md）",
            failures.len()
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headings_become_bold() {
        assert_eq!(strip_hashes("# Title"), "**Title**");
        assert_eq!(strip_hashes("### Deep"), "**Deep**");
        assert_eq!(strip_hashes("####### too many"), "####### too many");
        assert_eq!(strip_hashes("#nospace"), "#nospace");
        assert_eq!(strip_hashes("plain\ntext"), "plain\ntext");
        assert_eq!(strip_hashes("a\n## b\nc"), "a\n**b**\nc");
    }

    /// 整条标题必须落在一个加粗里：外层一对 `**` 之内不得再出现 `**`，
    /// 否则 CommonMark 会错配定界符（表现为只有某个词被强调，或残留可见星号）。
    #[test]
    fn heading_is_bold_as_a_whole() {
        for (input, expected) in [
            ("# 1. **多分辨率策略不能丢**", "**1. 多分辨率策略不能丢**"),
            (
                "## 方案一：**“水珠”——像吃水果**",
                "**方案一：“水珠”——像吃水果**",
            ),
            ("# 🌅 **早晨的第一瞬间**", "**🌅 早晨的第一瞬间**"),
            ("# 1. **A** 2. **B**", "**1. A 2. B**"),
            ("# **Bold**", "**Bold**"),
        ] {
            let out = strip_hashes(input);
            assert_eq!(out, expected, "input: {input}");
            let inner = &out[2..out.len() - 2];
            assert!(!inner.contains("**"), "内层仍有 **（未整条加粗）: {out}");
        }
    }

    #[test]
    fn heading_keeps_bold_inside_inline_code() {
        // glob 里的 `**` 不是强调，不能当成内层加粗吸收掉
        assert_eq!(
            strip_hashes("# 匹配 `**/*.js` 的路径"),
            "**匹配 `**/*.js` 的路径**"
        );
        assert_eq!(strip_hashes("# a ``**x**`` b"), "**a ``**x**`` b**");
    }

    #[test]
    fn code_fences_are_untouched() {
        // 否则 Python / Shell 的 `# 注释` 会被误改成加粗
        for input in [
            "```python\n# comment\n```",
            "~~~bash\n# comment\n~~~",
            "# before\n```\n# inside\n```\n# after",
        ] {
            assert_eq!(
                strip_hashes(input),
                input
                    .replace("# before", "**before**")
                    .replace("# after", "**after**")
            );
        }
    }

    #[test]
    fn duplicate_entry_names_get_suffix() {
        let mut used = HashSet::new();
        let name = "A/20240101-000000-T.md";
        assert_eq!(unique_entry_name(name, &mut used), name);
        assert_eq!(
            unique_entry_name(name, &mut used),
            "A/20240101-000000-T-2.md"
        );
        assert_eq!(
            unique_entry_name(name, &mut used),
            "A/20240101-000000-T-3.md"
        );
        // 不同助手目录下同名互不影响
        assert_eq!(
            unique_entry_name("B/20240101-000000-T.md", &mut used),
            "B/20240101-000000-T.md"
        );
    }

    #[test]
    fn created_at_parses_rfc3339_and_epoch() {
        let iso = parse_created_at_value(&serde_json::json!("2024-03-28T13:31:51.887Z"));
        assert_eq!(iso.map(|dt| dt.timestamp()), Some(1711632711));

        let secs = parse_created_at_value(&serde_json::json!(1761203267));
        let millis = parse_created_at_value(&serde_json::json!(1761203267000i64));
        assert_eq!(secs.map(|dt| dt.timestamp()), Some(1761203267));
        assert_eq!(millis.map(|dt| dt.timestamp()), Some(1761203267));

        assert!(parse_created_at_value(&serde_json::json!(null)).is_none());
        assert!(parse_created_at_value(&serde_json::json!("not a date")).is_none());
    }
}
