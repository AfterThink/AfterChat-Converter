//! 把 Qwen 导出的 JSON 会话转换为 AfterChat 对话 Markdown / ZIP。
//!
//! 输出契约见仓库 `docs/CHATFORMAT.md`，渲染/命名/打包一律走 `chatformat`。
//!
//! 输入只支持两种形态：
//! - **单体导出**：顶层数组（通常只有 1 个会话）→ 输出单个 `.md`
//! - **全部导出**：`{ success, request_id, data: [session, ...] }` → 输出单个 `.zip`
//!
//! 兼容形态：顶层对象里 `data` 是单个对象、或顶层本身就是会话对象 → 按单体处理。

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use chatformat::Message as ChatMessage;
use chatformat::{
    Conversation, ExportFailure, NameStyle, Role, SINGLE_NAME_MAX, ZipExport, default_zip_name,
    time,
};
use log::{debug, warn};
use serde::Deserialize;
use serde_json::Value;

const URL_BASE: &str = "https://chat.qwen.ai";
const PLATFORM_ID: &str = "qwen";

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
        match convert_one(
            input,
            options.output.as_deref(),
            exact_output.is_some(),
            options.show_progress,
        ) {
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

    let raw =
        fs::read_to_string(input).with_context(|| format!("failed to read {}", input.display()))?;
    // ChatFormat 允许 UTF-8 BOM，但 serde_json 不接受，这里先剥掉。
    let value: Value = serde_json::from_str(raw.trim_start_matches('\u{feff}'))
        .with_context(|| format!("failed to parse json in {}", input.display()))?;
    let parsed = parse_input_payload(value)
        .with_context(|| format!("unsupported json shape in {}", input.display()))?;

    match parsed {
        ParsedInput::Single(session) => {
            let conversation = to_conversation(&session);
            let default_name = format!(
                "{}.md",
                NameStyle::Js.sanitize(&conversation.title, SINGLE_NAME_MAX)
            );
            let target = resolve_target(input, output, output_is_file, default_name)?;
            ensure_parent_dir(&target)?;
            fs::write(&target, chatformat::render(&conversation))
                .with_context(|| format!("failed to write {}", target.display()))?;
            Ok(target)
        }
        ParsedInput::All { sessions, failures } => {
            let conversations: Vec<Conversation> = sessions.iter().map(to_conversation).collect();
            let failures: Vec<ExportFailure> = failures
                .into_iter()
                .map(|failure| ExportFailure {
                    title: failure.label,
                    id: failure.id,
                    reason: failure.reason,
                })
                .collect();
            let target =
                resolve_target(input, output, output_is_file, default_zip_name(PLATFORM_ID))?;
            ensure_parent_dir(&target)?;
            chatformat::write_zip(&ZipExport {
                platform: PLATFORM_ID,
                conversations: &conversations,
                failures: &failures,
                output: &target,
                source: Some(input),
                name_style: NameStyle::Js,
                show_progress,
            })?;
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
//  映射到 chatformat
// ═══════════════════════════════════════════════════════════

fn to_conversation(session: &Session) -> Conversation {
    let messages = session_messages(session);
    let model = detect_model(&messages);
    let time_secs = time::value_to_secs(session.created_at.as_ref());
    let sort_ms = time::value_to_secs(session.updated_at.as_ref())
        .or(time_secs)
        .map(|secs| secs * 1000);

    let mapped = messages
        .iter()
        .filter_map(|message| {
            match message
                .role
                .as_deref()
                .map(str::to_ascii_lowercase)
                .as_deref()
            {
                Some("user") => {
                    let text = message.content.as_deref().map(str::trim).unwrap_or("");
                    if text.is_empty() {
                        return None;
                    }
                    Some(ChatMessage {
                        role: Role::User,
                        thinking: Vec::new(),
                        body: vec![text.to_string()],
                    })
                }
                Some("assistant") => {
                    let (thinking, body) = split_assistant(message);
                    if thinking.is_empty() && body.is_empty() {
                        return None;
                    }
                    Some(ChatMessage {
                        role: Role::Assistant,
                        thinking,
                        body,
                    })
                }
                _ => None,
            }
        })
        .collect();

    Conversation {
        title: session_display_title(session),
        model,
        time_secs,
        sort_ms,
        url: Some(session_url(session)),
        extra: Vec::new(),
        group: None,
        id: session.id.clone(),
        messages: mapped,
    }
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

    if let Some(titles) = extra
        .pointer("/summary_title/content")
        .and_then(Value::as_array)
    {
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

    if let Some(items) = extra
        .pointer("/summary_thought/content")
        .and_then(Value::as_array)
    {
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
    chatformat::UNKNOWN_MODEL.to_string()
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

#[cfg(test)]
mod tests {
    use super::*;

    fn session_from(value: Value) -> Session {
        serde_json::from_value(value).expect("session should parse")
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

        let md = chatformat::render(&to_conversation(&session));
        assert!(md.starts_with("## Metadata\n"), "{md}");
        assert!(md.contains("- **Model:** `qwen3.5-plus`"), "{md}");
        assert!(md.contains("- **Time:** "), "{md}");
        assert!(
            md.contains("- **URL:** https://chat.qwen.ai/c/conv-1"),
            "{md}"
        );
        assert!(!md.contains("### Run Settings"), "{md}");
        assert!(!md.contains("models/Qwen"), "{md}");
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

        let md = chatformat::render(&to_conversation(&session));
        assert!(md.contains("#### 🤔 Thought Process"), "{md}");
        assert!(md.contains("let me think"), "{md}");
        assert!(md.contains("#### 💡 Response"), "{md}");
        assert!(md.contains("final answer"), "{md}");

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

        let md = chatformat::render(&to_conversation(&session));
        assert!(md.contains("**Planning**"), "{md}");
        assert!(md.contains("- step one"), "{md}");
        assert!(md.contains("- step two"), "{md}");
        assert!(md.contains("#### 💡 Response"), "{md}");
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

        let md = chatformat::render(&to_conversation(&session));
        assert!(md.contains("clean answer"), "{md}");
        assert!(!md.contains("search noise"), "{md}");
        assert!(!md.contains("tool noise"), "{md}");
        // 没有思考时不应出现 Thought / Response 标题
        assert!(!md.contains("#### 🤔 Thought Process"), "{md}");
        assert!(!md.contains("#### 💡 Response"), "{md}");
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
        assert_eq!(detect_model(&session_messages(&session)), "qwen3.5-plus");
    }
}
