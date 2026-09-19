//! 把 Qwen 导出的 JSON 会话转换为 AfterChat 对话 Markdown / ZIP。
//!
//! 输出契约见仓库根目录 `CHATFORMAT.md`（同步自 AfterChat-Script-Dev）。
//!
//! 输入只支持两种形态：
//! - **单体导出**：顶层数组（通常只有 1 个会话）→ 输出单个 `.md`
//! - **全部导出**：`{ success, request_id, data: [session, ...] }` → 输出单个 `.zip`
//!
//! 兼容形态：顶层对象里 `data` 是单个对象、或顶层本身就是会话对象 → 按单体处理。

use std::borrow::Cow;
use std::cmp::Ordering;
use std::collections::HashSet;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use chrono::{Local, TimeZone};
use indicatif::{ProgressBar, ProgressStyle};
use log::{debug, warn};
use rayon::prelude::*;
use serde::Deserialize;
use serde_json::Value;
use zip::CompressionMethod;
use zip::write::SimpleFileOptions;

const URL_BASE: &str = "https://chat.qwen.ai";
const EXPORT_PREFIX: &str = "chat-export-qwen-all";
const PLATFORM_ID: &str = "qwen";
/// 单条导出文件名上限（对齐 JS `sanitizeFilename` 默认值）
const SINGLE_NAME_MAX: usize = 60;
/// ZIP 内条目文件名上限（对齐 JS `makeMarkdownZipFilename`）
const ZIP_NAME_MAX: usize = 100;
/// 判定「毫秒时间戳」的下限：秒级时间戳不会大于它
const MILLIS_THRESHOLD: i64 = 10_000_000_000;

// ═══════════════════════════════════════════════════════════
//  对外 API
// ═══════════════════════════════════════════════════════════

#[derive(Debug, Clone)]
pub struct ConvertOptions {
    /// 一个或多个输入 JSON 文件
    pub inputs: Vec<PathBuf>,
    /// 输出文件或目录；`None` 表示输出到各自源文件同目录
    pub output: Option<PathBuf>,
    /// 是否显示进度条
    pub show_progress: bool,
}

#[derive(Debug, Default, Clone)]
pub struct RunSummary {
    pub inputs: usize,
    pub outputs: Vec<PathBuf>,
    pub failed: usize,
}

pub fn run_conversion(options: ConvertOptions) -> Result<RunSummary> {
    if options.inputs.is_empty() {
        bail!("no input files provided");
    }

    let exact_output = options.output.as_deref().filter(|p| is_output_file(p));
    if exact_output.is_some() && options.inputs.len() > 1 {
        bail!(
            "output points to a single file, but {} inputs were provided",
            options.inputs.len()
        );
    }

    let mut summary = RunSummary::default();

    for input in &options.inputs {
        summary.inputs += 1;
        match convert_one(input, options.output.as_deref(), exact_output.is_some(), options.show_progress)
        {
            Ok(path) => summary.outputs.push(path),
            Err(err) => {
                warn!("failed to convert {}: {err:#}", input.display());
                summary.failed += 1;
            }
        }
    }

    Ok(summary)
}

fn convert_one(
    input: &Path,
    output: Option<&Path>,
    output_is_file: bool,
    show_progress: bool,
) -> Result<PathBuf> {
    if !input.exists() {
        bail!("input path does not exist: {}", input.display());
    }
    if !input.is_file() {
        bail!("input must be a JSON file: {}", input.display());
    }

    let raw = fs::read_to_string(input)
        .with_context(|| format!("failed to read {}", input.display()))?;
    // ChatFormat 允许 UTF-8 BOM，但 serde_json 不接受，这里先剥掉。
    let value: Value = serde_json::from_str(raw.trim_start_matches('\u{feff}'))
        .with_context(|| format!("failed to parse json in {}", input.display()))?;
    let parsed = parse_input_payload(value)
        .with_context(|| format!("unsupported json shape in {}", input.display()))?;

    match parsed {
        ParsedInput::Single(session) => {
            let default_name = format!("{}.md", session_file_stem(&session, SINGLE_NAME_MAX));
            let target = resolve_target(input, output, output_is_file, default_name)?;
            let markdown = render_session_markdown(&session);
            ensure_parent_dir(&target)?;
            fs::write(&target, markdown)
                .with_context(|| format!("failed to write {}", target.display()))?;
            Ok(target)
        }
        ParsedInput::All { sessions, failures } => {
            let zip_name = format!(
                "{EXPORT_PREFIX}-{}.zip",
                chrono::Utc::now().timestamp_millis()
            );
            let target = resolve_target(input, output, output_is_file, zip_name)?;
            ensure_parent_dir(&target)?;
            write_zip_export(&sessions, &failures, input, &target, show_progress)?;
            Ok(target)
        }
    }
}

fn resolve_target(
    input: &Path,
    output: Option<&Path>,
    output_is_file: bool,
    default_name: String,
) -> Result<PathBuf> {
    match output {
        Some(path) if output_is_file => Ok(path.to_path_buf()),
        Some(dir) => {
            fs::create_dir_all(dir)
                .with_context(|| format!("failed to create output dir {}", dir.display()))?;
            Ok(dir.join(default_name))
        }
        None => Ok(input_dir(input).join(default_name)),
    }
}

fn input_dir(input: &Path) -> PathBuf {
    match input.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.to_path_buf(),
        _ => PathBuf::from("."),
    }
}

fn ensure_parent_dir(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create parent dir {}", parent.display()))?;
    }
    Ok(())
}

fn is_output_file(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|s| s.to_str()),
        Some("md") | Some("zip")
    )
}

// ═══════════════════════════════════════════════════════════
//  输入解析
// ═══════════════════════════════════════════════════════════

#[derive(Debug, Clone)]
enum ParsedInput {
    /// 单条会话 → 输出 `.md`
    Single(Box<Session>),
    /// 多条会话 → 输出 `.zip`
    All {
        sessions: Vec<Session>,
        failures: Vec<SessionFailure>,
    },
}

#[derive(Debug, Clone)]
struct SessionFailure {
    label: String,
    id: String,
    reason: String,
}

#[derive(Debug, Clone, Deserialize)]
struct Session {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    created_at: Option<Value>,
    #[serde(default)]
    updated_at: Option<Value>,
    #[serde(default)]
    chat: Option<Chat>,
}

#[derive(Debug, Clone, Deserialize)]
struct Chat {
    #[serde(default)]
    messages: Option<Value>,
}

#[derive(Debug, Clone, Deserialize)]
struct Message {
    #[serde(default)]
    role: Option<String>,
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    reasoning_content: Option<Value>,
    #[serde(default, rename = "modelName")]
    model_name: Option<String>,
    #[serde(default)]
    models: Option<Value>,
    #[serde(default)]
    content_list: Option<Value>,
}

fn parse_input_payload(value: Value) -> Result<ParsedInput> {
    match value {
        // 单体导出：顶层数组 → 单条输出 .md（多条时退化为 .zip）
        Value::Array(items) => {
            let (sessions, failures) = parse_sessions(&items);
            if sessions.is_empty() {
                bail!("array contains no valid Qwen sessions");
            }
            if sessions.len() == 1 && failures.is_empty() {
                Ok(ParsedInput::Single(Box::new(
                    sessions.into_iter().next().expect("one session"),
                )))
            } else {
                Ok(ParsedInput::All { sessions, failures })
            }
        }

        Value::Object(obj) => match obj.get("data") {
            // 全部导出：{ success, request_id, data: [...] } → 始终打包成 .zip
            Some(Value::Array(items)) => {
                let (sessions, failures) = parse_sessions(items);
                if sessions.is_empty() {
                    bail!("`data` array contains no valid Qwen sessions");
                }
                Ok(ParsedInput::All { sessions, failures })
            }
            Some(Value::Object(_)) => {
                if obj["data"].get("chat").is_none() {
                    bail!("`data` object does not look like a Qwen session (missing `chat`)");
                }
                let session: Session = serde_json::from_value(obj["data"].clone())
                    .context("failed to parse single session object")?;
                Ok(ParsedInput::Single(Box::new(session)))
            }
            Some(_) => bail!("`data` must be an object or an array"),
            None => {
                if !obj.contains_key("chat") {
                    bail!("object does not look like a Qwen session (missing `chat`)");
                }
                let session: Session = serde_json::from_value(Value::Object(obj))
                    .context("failed to parse session object")?;
                Ok(ParsedInput::Single(Box::new(session)))
            }
        },

        _ => bail!("json root must be an object or an array"),
    }
}

/// 逐项解析会话数组；坏项记入 failures，不中断整体。
fn parse_sessions(items: &[Value]) -> (Vec<Session>, Vec<SessionFailure>) {
    let mut sessions = Vec::with_capacity(items.len());
    let mut failures = Vec::new();

    for (idx, item) in items.iter().enumerate() {
        if item.is_null() {
            debug!("skip null session item at index {idx}");
            continue;
        }

        if item.get("chat").is_none() {
            let label = format!("item #{}", idx + 1);
            warn!("skip invalid session {label}: not a Qwen session (missing `chat`)");
            failures.push(SessionFailure {
                label,
                id: "-".to_string(),
                reason: "not a Qwen session (missing `chat` field)".to_string(),
            });
            continue;
        }

        match serde_json::from_value::<Session>(item.clone()) {
            Ok(session) => sessions.push(session),
            Err(err) => {
                let label = format!("item #{}", idx + 1);
                warn!("skip invalid session {label}: {err}");
                failures.push(SessionFailure {
                    label,
                    id: "-".to_string(),
                    reason: err.to_string(),
                });
            }
        }
    }

    (sessions, failures)
}

// ═══════════════════════════════════════════════════════════
//  渲染
// ═══════════════════════════════════════════════════════════

fn render_session_markdown(session: &Session) -> String {
    let messages = session_messages(session);
    let model = detect_model(&messages);
    let time = value_to_secs(session.created_at.as_ref())
        .map(format_local_time)
        .unwrap_or_else(|| "unknown".to_string());
    let url = session_url(session);

    let mut lines: Vec<String> = Vec::new();
    lines.push("## Metadata".to_string());
    lines.push(String::new());
    lines.push(format!("- **Model:** `{model}`"));
    lines.push(format!("- **Time:** {time}"));
    lines.push(format!("- **URL:** {url}"));
    lines.push(String::new());
    lines.push("## Conversation".to_string());
    lines.push(String::new());
    lines.push(render_conversation(&messages));

    lines.join("\n")
}

fn render_conversation(messages: &[Message]) -> String {
    let mut lines: Vec<String> = Vec::new();

    for message in messages {
        match message.role.as_deref().map(str::to_ascii_lowercase).as_deref() {
            Some("user") => {
                let text = message.content.as_deref().unwrap_or("");
                if text.is_empty() {
                    continue;
                }
                lines.push("### 🧑‍💻 User".to_string());
                lines.push(String::new());
                lines.push(strip_hashes(text));
                lines.push(String::new());
            }
            Some("assistant") => {
                let (thoughts, responses) = split_assistant(message);
                if thoughts.is_empty() && responses.is_empty() {
                    continue;
                }

                lines.push("### 🤖 Assistant".to_string());
                lines.push(String::new());

                if !thoughts.is_empty() {
                    lines.push("#### 🤔 Thought Process".to_string());
                    lines.push(String::new());
                    lines.push(strip_hashes(&thoughts.join("\n\n")));
                    lines.push(String::new());
                    if !responses.is_empty() {
                        lines.push("#### 💡 Response".to_string());
                        lines.push(String::new());
                    }
                }

                if !responses.is_empty() {
                    lines.push(strip_hashes(&responses.join("\n\n")));
                }
                lines.push(String::new());
            }
            _ => {}
        }
    }

    lines.join("\n")
}

/// 按 `content_list[].phase` 把助手消息拆成「思考」与「回复」两段。
///
/// - `think` / `thinking_summary` → 思考
/// - `answer` → 回复
/// - 其它（`web_search` / `image_gen_tool` / 空）→ 工具过程，忽略
/// - 没有 `content_list` 时回退到 `content`
fn split_assistant(message: &Message) -> (Vec<String>, Vec<String>) {
    let mut thoughts = Vec::new();
    let mut responses = Vec::new();

    if let Some(reasoning) = &message.reasoning_content {
        collect_strings(reasoning, &mut thoughts);
    }

    match message.content_list.as_ref().and_then(Value::as_array) {
        Some(items) => {
            for item in items {
                let phase = item
                    .get("phase")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_ascii_lowercase();
                let content = item.get("content").and_then(Value::as_str).unwrap_or("");

                match phase.as_str() {
                    "think" => push_unique(&mut thoughts, content.to_string()),
                    "thinking_summary" => {
                        if let Some(text) = render_thinking_summary(item) {
                            push_unique(&mut thoughts, text);
                        }
                    }
                    "answer" => push_unique(&mut responses, content.to_string()),
                    _ => {}
                }
            }
        }
        None => {
            if let Some(content) = message.content.as_deref() {
                push_unique(&mut responses, content.to_string());
            }
        }
    }

    (thoughts, responses)
}

/// `thinking_summary` 的正文是空串，真内容在 `extra.summary_title` / `extra.summary_thought`。
fn render_thinking_summary(item: &Value) -> Option<String> {
    let extra = item.get("extra")?;
    let mut parts: Vec<String> = Vec::new();

    if let Some(titles) = extra.pointer("/summary_title/content").and_then(Value::as_array) {
        let joined: Vec<&str> = titles
            .iter()
            .filter_map(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .collect();
        if !joined.is_empty() {
            parts.push(format!("**{}**", joined.join(" ")));
        }
    }

    if let Some(items) = extra.pointer("/summary_thought/content").and_then(Value::as_array) {
        for text in items.iter().filter_map(Value::as_str) {
            let text = text.trim();
            if !text.is_empty() {
                parts.push(format!("- {text}"));
            }
        }
    }

    if parts.is_empty() {
        None
    } else {
        Some(parts.join("\n"))
    }
}

fn detect_model(messages: &[Message]) -> String {
    for message in messages {
        if let Some(name) = message
            .model_name
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            return name.to_string();
        }
        if let Some(name) = first_string(message.models.as_ref())
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            return name.to_string();
        }
    }
    "unknown".to_string()
}

fn session_url(session: &Session) -> String {
    match session
        .id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        Some(id) => format!("{URL_BASE}/c/{id}"),
        None => URL_BASE.to_string(),
    }
}

fn session_messages(session: &Session) -> Vec<Message> {
    let Some(items) = session
        .chat
        .as_ref()
        .and_then(|chat| chat.messages.as_ref())
        .and_then(Value::as_array)
    else {
        return Vec::new();
    };

    items
        .iter()
        .filter(|item| item.is_object())
        .filter_map(|item| serde_json::from_value::<Message>(item.clone()).ok())
        .collect()
}

/// `^#{1,6}\s+(.+)$`（多行）→ `**$1**`：不保留井号标题，但保留强调。
///
/// **代码围栏内不动**（``` / ~~~），否则会把 Python / Shell 的 `# 注释` 误改成加粗。
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

    Cow::Owned(format!("**{trimmed}**"))
}

fn collect_strings(value: &Value, out: &mut Vec<String>) {
    match value {
        Value::String(text) => push_unique(out, text.clone()),
        Value::Array(items) => items.iter().for_each(|item| collect_strings(item, out)),
        Value::Object(map) => map.values().for_each(|item| collect_strings(item, out)),
        _ => {}
    }
}

fn push_unique(out: &mut Vec<String>, candidate: String) {
    let candidate = candidate.trim();
    if candidate.is_empty() {
        return;
    }
    if out.iter().any(|existing| existing == candidate) {
        return;
    }
    out.push(candidate.to_string());
}

fn first_string(value: Option<&Value>) -> Option<&str> {
    match value? {
        Value::String(text) => Some(text.as_str()),
        Value::Array(items) => items.iter().find_map(Value::as_str),
        _ => None,
    }
}

// ═══════════════════════════════════════════════════════════
//  命名 / 时间
// ═══════════════════════════════════════════════════════════

fn session_display_title(session: &Session) -> String {
    session
        .title
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .or_else(|| {
            session
                .id
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
        })
        .unwrap_or("")
        .to_string()
}

fn session_file_stem(session: &Session, max_len: usize) -> String {
    sanitize_filename(&session_display_title(session), max_len)
}

/// 对齐 JS `sanitizeFilename`：非法字符→`_`，控制符→空格，空白折叠为单空格，
/// 超长按字符截断后去掉尾部 `\s._-`，空则 `untitled`。
fn sanitize_filename(name: &str, max_len: usize) -> String {
    let mut replaced = String::with_capacity(name.len());
    for ch in name.chars() {
        if matches!(ch, '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|') {
            replaced.push('_');
        } else if ch.is_control() {
            replaced.push(' ');
        } else {
            replaced.push(ch);
        }
    }

    let mut collapsed = String::with_capacity(replaced.len());
    let mut prev_whitespace = false;
    for ch in replaced.chars() {
        if ch.is_whitespace() {
            if !prev_whitespace {
                collapsed.push(' ');
                prev_whitespace = true;
            }
        } else {
            collapsed.push(ch);
            prev_whitespace = false;
        }
    }

    let trimmed = collapsed.trim();
    let truncated = if trimmed.chars().count() > max_len {
        let head: String = trimmed.chars().take(max_len).collect();
        head.trim_end_matches(|ch: char| ch.is_whitespace() || matches!(ch, '.' | '_' | '-'))
            .to_string()
    } else {
        trimmed.to_string()
    };

    if truncated.is_empty() {
        "untitled".to_string()
    } else {
        truncated
    }
}

fn value_to_secs(value: Option<&Value>) -> Option<i64> {
    let raw = match value? {
        Value::Number(number) => number
            .as_i64()
            .or_else(|| number.as_f64().map(|f| f as i64))?,
        Value::String(text) => text.trim().parse::<f64>().ok().map(|f| f as i64)?,
        _ => return None,
    };

    Some(if raw >= MILLIS_THRESHOLD {
        raw / 1000
    } else {
        raw
    })
}

fn session_sort_ms(session: &Session) -> Option<i64> {
    value_to_secs(session.updated_at.as_ref())
        .or_else(|| value_to_secs(session.created_at.as_ref()))
        .map(|secs| secs * 1000)
}

fn format_local_time(secs: i64) -> String {
    Local
        .timestamp_opt(secs, 0)
        .single()
        .map(|dt| dt.format("%Y-%m-%d %H:%M:%S %:z").to_string())
        .unwrap_or_else(|| "unknown".to_string())
}

fn format_local_compact(ms: i64) -> String {
    Local
        .timestamp_opt(ms / 1000, 0)
        .single()
        .map(|dt| dt.format("%Y%m%d-%H%M%S").to_string())
        .unwrap_or_else(|| "00000000-000000".to_string())
}

// ═══════════════════════════════════════════════════════════
//  ZIP 导出
// ═══════════════════════════════════════════════════════════

fn write_zip_export(
    sessions: &[Session],
    failures: &[SessionFailure],
    source: &Path,
    zip_path: &Path,
    show_progress: bool,
) -> Result<()> {
    let order = order_sessions(sessions);
    let total = order.len();

    let progress = if show_progress {
        Some(make_progress_bar(total as u64, "sessions"))
    } else {
        None
    };

    let rendered: Vec<String> = order
        .par_iter()
        .map(|&index| {
            let markdown = render_session_markdown(&sessions[index]);
            if let Some(pb) = &progress {
                pb.inc(1);
            }
            markdown
        })
        .collect();

    if let Some(pb) = progress {
        pb.finish_and_clear();
    }

    let file = File::create(zip_path)
        .with_context(|| format!("failed to create {}", zip_path.display()))?;
    let mut zip = zip::ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    let mut used_names: HashSet<String> = HashSet::new();

    for (position, markdown) in rendered.iter().enumerate() {
        let entry = zip_entry_name(&sessions[order[position]], position, total, &mut used_names);
        zip.start_file(entry, options)
            .with_context(|| format!("failed to start zip entry in {}", zip_path.display()))?;
        zip.write_all(markdown.as_bytes())
            .with_context(|| format!("failed to write zip entry in {}", zip_path.display()))?;
    }

    if !failures.is_empty() {
        let report = build_failure_markdown(total, failures);
        zip.start_file("export-failures.md", options)
            .context("failed to add export-failures.md")?;
        zip.write_all(report.as_bytes())
            .context("failed to write export-failures.md")?;
    }

    zip.finish()
        .with_context(|| format!("failed to finalize {}", zip_path.display()))?;

    if failures.is_empty() {
        log::info!(
            "packed {} sessions from {} into {}",
            total,
            source.display(),
            zip_path.display()
        );
    } else {
        log::warn!(
            "packed {} sessions ({} skipped) from {} into {}",
            total,
            failures.len(),
            source.display(),
            zip_path.display()
        );
    }

    Ok(())
}

/// 时间降序（最新在前），无时间的垫底并保持原有相对顺序。
fn order_sessions(sessions: &[Session]) -> Vec<usize> {
    let mut order: Vec<usize> = (0..sessions.len()).collect();
    order.sort_by(|&a, &b| match (session_sort_ms(&sessions[a]), session_sort_ms(&sessions[b])) {
        (None, None) => a.cmp(&b),
        (None, Some(_)) => Ordering::Greater,
        (Some(_), None) => Ordering::Less,
        (Some(left), Some(right)) => right.cmp(&left).then_with(|| a.cmp(&b)),
    });
    order
}

fn zip_entry_name(
    session: &Session,
    index: usize,
    total: usize,
    used: &mut HashSet<String>,
) -> String {
    let title = sanitize_filename(&session_display_title(session), ZIP_NAME_MAX);
    let prefix = match session_sort_ms(session) {
        Some(ms) => format_local_compact(ms),
        None => {
            let width = total.to_string().len().max(3);
            format!("{:0width$}", index + 1, width = width)
        }
    };

    let base = format!("{prefix}-{title}");
    let mut name = format!("{base}.md");
    let mut counter = 2;
    while used.contains(&name) {
        name = format!("{base}-{counter}.md");
        counter += 1;
    }

    used.insert(name.clone());
    name
}

fn build_failure_markdown(total: usize, failures: &[SessionFailure]) -> String {
    let mut lines: Vec<String> = Vec::new();
    lines.push("# Export Failures".to_string());
    lines.push(String::new());
    lines.push("## Metadata".to_string());
    lines.push(String::new());
    lines.push(format!("- **Platform:** `{PLATFORM_ID}`"));
    lines.push(format!(
        "- **Export Time:** {}",
        format_local_time(chrono::Utc::now().timestamp())
    ));
    lines.push(format!("- **Total Conversations:** {}", total + failures.len()));
    lines.push(format!("- **Exported:** {total}"));
    lines.push(format!("- **Failed:** {}", failures.len()));
    lines.push(String::new());
    lines.push("## Failed Conversations".to_string());
    lines.push(String::new());

    for failure in failures {
        lines.push(format!("- **{}**", failure.label));
        lines.push(format!("  - ID: `{}`", failure.id));
        lines.push(format!("  - Error: {}", failure.reason));
    }
    lines.push(String::new());

    lines.join("\n")
}

fn make_progress_bar(total: u64, unit: &str) -> ProgressBar {
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

    fn session_from(value: Value) -> Session {
        serde_json::from_value(value).expect("session should parse")
    }

    fn messages_of(session: &Session) -> Vec<Message> {
        session_messages(session)
    }

    #[test]
    fn parses_wrapped_payload_as_all() {
        let value = serde_json::json!({
            "success": true,
            "request_id": "req-1",
            "data": [
                { "id": "a", "title": "A", "chat": { "messages": [] } },
                { "id": "b", "title": "B", "chat": { "messages": [] } }
            ]
        });

        match parse_input_payload(value).expect("parse") {
            ParsedInput::All { sessions, failures } => {
                assert_eq!(sessions.len(), 2);
                assert!(failures.is_empty());
            }
            _ => panic!("expected All"),
        }
    }

    #[test]
    fn parses_top_level_array_single_as_single() {
        let value = serde_json::json!([
            { "id": "a", "title": "A", "chat": { "messages": [] } }
        ]);

        assert!(matches!(
            parse_input_payload(value).expect("parse"),
            ParsedInput::Single(_)
        ));
    }

    #[test]
    fn wrapped_single_session_still_packs_zip() {
        let value = serde_json::json!({
            "success": true,
            "request_id": "req-1",
            "data": [{ "id": "a", "title": "A", "chat": { "messages": [] } }]
        });

        assert!(matches!(
            parse_input_payload(value).expect("parse"),
            ParsedInput::All { .. }
        ));
    }

    #[test]
    fn metadata_uses_model_time_url() {
        let session = session_from(serde_json::json!({
            "id": "conv-1",
            "title": "Demo",
            "created_at": 1_700_000_000,
            "chat": { "messages": [
                { "role": "user", "content": "hi", "models": ["qwen3.5-plus"] },
                { "role": "assistant", "content": "", "modelName": "Qwen3.5-Plus",
                  "content_list": [{ "phase": "answer", "content": "hello" }] }
            ]}
        }));

        let md = render_session_markdown(&session);
        assert!(md.contains("## Metadata"));
        assert!(md.contains("- **Model:** `qwen3.5-plus`"));
        assert!(md.contains("- **Time:** "));
        assert!(md.contains("- **URL:** https://chat.qwen.ai/c/conv-1"));
        assert!(!md.contains("### Run Settings"));
        assert!(!md.contains("models/Qwen"));
    }

    #[test]
    fn think_phase_renders_thought_and_response() {
        let session = session_from(serde_json::json!({
            "id": "conv-2",
            "chat": { "messages": [
                { "role": "user", "content": "q" },
                { "role": "assistant", "content": "",
                  "content_list": [
                    { "phase": "think", "content": "let me think" },
                    { "phase": "answer", "content": "final answer" }
                  ] }
            ]}
        }));

        let md = render_session_markdown(&session);
        assert!(md.contains("#### 🤔 Thought Process"));
        assert!(md.contains("let me think"));
        assert!(md.contains("#### 💡 Response"));
        assert!(md.contains("final answer"));

        let think_pos = md.find("let me think").expect("thought");
        let answer_pos = md.find("final answer").expect("answer");
        assert!(think_pos < answer_pos);
    }

    #[test]
    fn thinking_summary_reads_extra_payload() {
        let session = session_from(serde_json::json!({
            "id": "conv-3",
            "chat": { "messages": [
                { "role": "assistant", "content": "",
                  "content_list": [
                    { "phase": "thinking_summary", "content": "",
                      "extra": {
                        "summary_title": { "content": ["Planning"] },
                        "summary_thought": { "content": ["step one", "step two"] }
                      } },
                    { "phase": "answer", "content": "done" }
                  ] }
            ]}
        }));

        let md = render_session_markdown(&session);
        assert!(md.contains("**Planning**"));
        assert!(md.contains("- step one"));
        assert!(md.contains("- step two"));
        assert!(md.contains("#### 💡 Response"));
    }

    #[test]
    fn tool_phases_are_ignored() {
        let session = session_from(serde_json::json!({
            "id": "conv-4",
            "chat": { "messages": [
                { "role": "assistant", "content": "",
                  "content_list": [
                    { "phase": "web_search", "content": "search noise" },
                    { "phase": "image_gen_tool", "content": "tool noise" },
                    { "phase": "answer", "content": "clean answer" }
                  ] }
            ]}
        }));

        let md = render_session_markdown(&session);
        assert!(md.contains("clean answer"));
        assert!(!md.contains("search noise"));
        assert!(!md.contains("tool noise"));
        // 没有思考时不应出现 Thought / Response 标题
        assert!(!md.contains("#### 🤔 Thought Process"));
        assert!(!md.contains("#### 💡 Response"));
    }

    #[test]
    fn markdown_headings_are_converted_to_bold() {
        assert_eq!(strip_hashes("# Title"), "**Title**");
        assert_eq!(strip_hashes("### Deep"), "**Deep**");
        assert_eq!(strip_hashes("####### too many"), "####### too many");
        assert_eq!(strip_hashes("#nospace"), "#nospace");
        assert_eq!(strip_hashes("plain\ntext"), "plain\ntext");
        assert_eq!(strip_hashes("a\n## b\nc"), "a\n**b**\nc");
    }

    #[test]
    fn strip_hashes_preserves_code_fences() {
        let text = "# Title\n```python\n# a comment\n## another\n```\n## Real Heading\n~~~\n# tilde comment\n~~~";
        let out = strip_hashes(text);

        assert!(out.contains("**Title**"));
        assert!(out.contains("**Real Heading**"));
        assert!(out.contains("# a comment"), "{out}");
        assert!(out.contains("## another"), "{out}");
        assert!(out.contains("# tilde comment"), "{out}");
        assert!(!out.contains("**a comment**"));
        assert!(!out.contains("**another**"));
    }

    #[test]
    fn sanitize_matches_js_rules() {
        assert_eq!(sanitize_filename("a/b:c*d?e\"f<g>h|i", 60), "a_b_c_d_e_f_g_h_i");
        assert_eq!(sanitize_filename("  many   spaces  ", 60), "many spaces");
        assert_eq!(sanitize_filename("", 60), "untitled");
        // JS trim 只去空白，不去点号
        assert_eq!(sanitize_filename("....", 60), "....");
        let long = "字".repeat(80);
        assert_eq!(sanitize_filename(&long, 10).chars().count(), 10);
    }

    #[test]
    fn model_detection_prefers_model_name_then_models() {
        let session = session_from(serde_json::json!({
            "id": "m",
            "chat": { "messages": [
                { "role": "user", "content": "q", "models": ["qwen3.5-plus"] },
                { "role": "assistant", "modelName": "Qwen3.5-Plus" }
            ]}
        }));
        // JS 口径：按消息顺序，先看 modelName，再看 models[0] —— 第一条 user 命中 models[0]
        assert_eq!(detect_model(&messages_of(&session)), "qwen3.5-plus");
    }

    #[test]
    fn zip_entries_are_time_descending() {
        let older = session_from(serde_json::json!({
            "id": "old", "title": "Alpha", "created_at": 1_700_000_000,
            "updated_at": 1_700_000_000, "chat": { "messages": [] }
        }));
        let newer = session_from(serde_json::json!({
            "id": "new", "title": "Beta", "created_at": 1_700_000_100,
            "updated_at": 1_700_000_100, "chat": { "messages": [] }
        }));
        let sessions = vec![older, newer];

        assert_eq!(order_sessions(&sessions), vec![1, 0], "newest first");
    }

    #[test]
    fn zip_entries_with_same_time_get_suffixes() {
        let first_session = session_from(serde_json::json!({
            "id": "a", "title": "Same", "created_at": 1_700_000_000,
            "updated_at": 1_700_000_000, "chat": { "messages": [] }
        }));
        let second_session = session_from(serde_json::json!({
            "id": "b", "title": "Same", "created_at": 1_700_000_000,
            "updated_at": 1_700_000_000, "chat": { "messages": [] }
        }));

        let mut used = HashSet::new();
        let first = zip_entry_name(&first_session, 0, 2, &mut used);
        let second = zip_entry_name(&second_session, 1, 2, &mut used);
        assert!(first.ends_with("-Same.md"), "{first}");
        assert!(second.ends_with("-Same-2.md"), "{second}");
        assert_ne!(first, second);
    }

    #[test]
    fn zip_entries_fall_back_to_index_without_time() {
        let session = session_from(serde_json::json!({
            "id": "x", "title": "NoTime", "chat": { "messages": [] }
        }));
        let mut used = HashSet::new();
        assert_eq!(zip_entry_name(&session, 0, 4, &mut used), "001-NoTime.md");
    }
}
