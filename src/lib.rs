use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result, anyhow, bail};
use chrono::{DateTime, Utc};
use filetime::{FileTime, set_file_times};
use indicatif::{ProgressBar, ProgressStyle};
use log::{debug, info, warn};
use rayon::prelude::*;
use serde::{Deserialize, Deserializer};
use serde_json::Value;
use walkdir::WalkDir;

const LARGE_BATCH_SIZE: usize = 1000;

#[derive(Debug, Clone)]
pub struct ConvertOptions {
    pub input: PathBuf,
    pub output: Option<PathBuf>,
    pub show_progress: bool,
}

#[derive(Debug, Default, Clone)]
pub struct RunSummary {
    pub input_files: usize,
    pub generated_files: usize,
    pub failed_files: usize,
    pub error_log: Option<PathBuf>,
}

#[derive(Debug, Clone)]
struct FileTask {
    source: PathBuf,
    relative_parent: PathBuf,
    is_primary_input: bool,
}

#[derive(Debug, Clone)]
struct FileFailure {
    source: PathBuf,
    reason: String,
}

#[derive(Debug, Clone)]
struct RenderedSession {
    markdown: String,
    title_hint: String,
    file_timestamp: Option<i64>,
}

#[derive(Debug, Clone)]
struct ConversationMessage {
    role: Role,
    content: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Role {
    User,
    Assistant,
}

#[derive(Debug, Clone)]
enum ParsedInput {
    Single(Session),
    Sessions(Vec<Session>),
    Wrapped {
        request_id: Option<String>,
        sessions: Vec<Session>,
    },
}

#[derive(Debug, Clone, Deserialize)]
struct Session {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    user_id: Option<String>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    meta: Value,
    #[serde(default)]
    chat_type: Option<String>,
    #[serde(default)]
    sub_chat_type: Option<String>,
    #[serde(default)]
    created_at: Option<i64>,
    #[serde(default)]
    updated_at: Option<i64>,
    #[serde(default)]
    chat: Option<SessionChat>,
    #[serde(default, deserialize_with = "de_vec_or_default")]
    messages: Vec<MessageNode>,
}

#[derive(Debug, Clone, Deserialize)]
struct SessionChat {
    #[serde(default)]
    history: Option<SessionHistory>,
}

#[derive(Debug, Clone, Deserialize)]
struct SessionHistory {
    #[serde(default, deserialize_with = "de_map_or_default")]
    messages: HashMap<String, MessageNode>,
}

#[derive(Debug, Clone, Deserialize)]
struct MessageNode {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    role: Option<String>,
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    reasoning_content: Option<Value>,
    #[serde(default, deserialize_with = "de_vec_or_default")]
    content_list: Vec<MessageContent>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default, rename = "modelName")]
    model_name: Option<String>,
    #[serde(default, rename = "parentId")]
    parent_id: Option<String>,
    #[serde(
        default,
        rename = "childrenIds",
        deserialize_with = "de_vec_or_default"
    )]
    children_ids: Vec<String>,
    #[serde(default)]
    timestamp: Option<i64>,
    #[serde(default)]
    chat_type: Option<String>,
    #[serde(default)]
    sub_chat_type: Option<String>,
}

fn session_has_message_payload(session: &Session) -> bool {
    !session.messages.is_empty()
        || session
            .chat
            .as_ref()
            .and_then(|chat| chat.history.as_ref())
            .is_some_and(|history| !history.messages.is_empty())
}

#[derive(Debug, Clone, Deserialize)]
struct MessageContent {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    phase: Option<String>,
    #[serde(default)]
    extra: Option<Value>,
}

pub fn run_conversion(options: ConvertOptions) -> Result<RunSummary> {
    let input = options.input.clone();
    if !input.exists() {
        bail!("input path does not exist: {}", input.display());
    }

    let input_is_dir = input.is_dir();
    let output_root = determine_output_root(&input, options.output.as_deref())?;
    if !output_root.exists() {
        fs::create_dir_all(&output_root)
            .with_context(|| format!("failed to create output dir: {}", output_root.display()))?;
    }

    let tasks = collect_tasks(&input)?;
    if tasks.is_empty() {
        bail!("no JSON files found in input path: {}", input.display());
    }

    info!(
        "using {} worker threads",
        std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1)
    );

    let progress = if options.show_progress {
        Some(make_progress_bar(tasks.len() as u64, "files"))
    } else {
        None
    };

    let generated = Arc::new(AtomicUsize::new(0));
    let failures: Arc<Mutex<Vec<FileFailure>>> = Arc::new(Mutex::new(Vec::new()));

    let output_is_md_file = options
        .output
        .as_ref()
        .map(|p| p.extension().and_then(|s| s.to_str()) == Some("md"))
        .unwrap_or(false);

    let thread_count = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1);
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(thread_count)
        .build()
        .context("failed to build rayon thread pool")?;

    let input_file_count = tasks.len();
    let primary_source = input.canonicalize().unwrap_or(input.clone());

    pool.install(|| {
        tasks.par_iter().for_each(|task| {
            let single_output_file = if !input_is_dir
                && task.is_primary_input
                && output_is_md_file
                && input_file_count == 1
            {
                options.output.clone()
            } else {
                None
            };

            match process_one_json_file(
                task,
                &output_root,
                single_output_file,
                &primary_source,
                options.show_progress && input_file_count == 1,
            ) {
                Ok(count) => {
                    generated.fetch_add(count, Ordering::SeqCst);
                }
                Err(err) => {
                    let mut guard = failures.lock().expect("poisoned lock");
                    guard.push(FileFailure {
                        source: task.source.clone(),
                        reason: format!("{err:#}"),
                    });
                }
            }

            if let Some(pb) = &progress {
                pb.inc(1);
            }
        });
    });

    if let Some(pb) = progress {
        pb.finish_and_clear();
    }

    let failed_files = failures.lock().expect("poisoned lock").len();
    let generated_files = generated.load(Ordering::SeqCst);

    let mut summary = RunSummary {
        input_files: input_file_count,
        generated_files,
        failed_files,
        error_log: None,
    };

    if failed_files > 0 {
        let error_path = output_root.join("error.log");
        write_error_log(&error_path, &failures.lock().expect("poisoned lock"))?;
        summary.error_log = Some(error_path);
    }

    Ok(summary)
}

fn determine_output_root(input: &Path, output: Option<&Path>) -> Result<PathBuf> {
    if let Some(path) = output {
        if path.extension().and_then(|s| s.to_str()) == Some("md") {
            let parent = path
                .parent()
                .ok_or_else(|| anyhow!("output markdown path requires a parent directory"))?;
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create parent dir {}", parent.display()))?;
            return Ok(parent.to_path_buf());
        }

        return Ok(path.to_path_buf());
    }

    if input.is_dir() {
        return Ok(input.to_path_buf());
    }

    input
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| anyhow!("failed to infer output directory from input"))
}

fn collect_tasks(input: &Path) -> Result<Vec<FileTask>> {
    if input.is_file() {
        return Ok(vec![FileTask {
            source: input.to_path_buf(),
            relative_parent: PathBuf::new(),
            is_primary_input: true,
        }]);
    }

    let mut tasks = Vec::new();
    for entry in WalkDir::new(input) {
        let entry = match entry {
            Ok(e) => e,
            Err(err) => {
                warn!("skip unreadable directory entry: {err}");
                continue;
            }
        };

        if !entry.file_type().is_file() {
            continue;
        }

        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }

        let rel = path
            .strip_prefix(input)
            .unwrap_or(path)
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default();

        tasks.push(FileTask {
            source: path.to_path_buf(),
            relative_parent: rel,
            is_primary_input: false,
        });
    }

    tasks.sort_by(|a, b| a.source.cmp(&b.source));
    Ok(tasks)
}

fn process_one_json_file(
    task: &FileTask,
    output_root: &Path,
    single_output_file: Option<PathBuf>,
    primary_source: &Path,
    enable_inner_progress: bool,
) -> Result<usize> {
    let raw = fs::read_to_string(&task.source)
        .with_context(|| format!("failed to read {}", task.source.display()))?;
    let parsed_json: Value = serde_json::from_str(&raw)
        .with_context(|| format!("failed to parse json in {}", task.source.display()))?;

    let parsed_input = parse_input_payload(parsed_json)
        .with_context(|| format!("unsupported json shape in {}", task.source.display()))?;

    let source_stem = task
        .source
        .file_stem()
        .and_then(|s| s.to_str())
        .map(sanitize_file_name)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "session".to_string());

    let parent_dir = output_root.join(&task.relative_parent);
    if !parent_dir.exists() {
        fs::create_dir_all(&parent_dir)
            .with_context(|| format!("failed to create {}", parent_dir.display()))?;
    }

    match parsed_input {
        ParsedInput::Single(session) => {
            let rendered = render_session_markdown(&session, None, &task.source);
            let target_file = single_output_file.unwrap_or_else(|| {
                let name = build_session_file_name(
                    &rendered.title_hint,
                    session.id.as_deref(),
                    &source_stem,
                );
                parent_dir.join(format!("{name}.md"))
            });
            write_rendered_session(&target_file, &rendered)?;
            Ok(1)
        }
        ParsedInput::Sessions(sessions) => {
            if sessions.len() == 1 {
                let rendered = render_session_markdown(&sessions[0], None, &task.source);
                let target_file = single_output_file.unwrap_or_else(|| {
                    let name = build_session_file_name(
                        &rendered.title_hint,
                        sessions[0].id.as_deref(),
                        &source_stem,
                    );
                    parent_dir.join(format!("{name}.md"))
                });
                write_rendered_session(&target_file, &rendered)?;
                return Ok(1);
            }

            if single_output_file.is_some() {
                bail!(
                    "output path points to a markdown file, but {} contains multiple sessions",
                    task.source.display()
                );
            }

            let output_dir = parent_dir.join(&source_stem);
            fs::create_dir_all(&output_dir)
                .with_context(|| format!("failed to create {}", output_dir.display()))?;
            write_session_batch(
                &sessions,
                None,
                &output_dir,
                &task.source,
                enable_inner_progress,
            )
        }
        ParsedInput::Wrapped {
            request_id,
            sessions,
        } => {
            if single_output_file.is_some() {
                bail!(
                    "output path points to a markdown file, but wrapped payload in {} expands to multiple sessions",
                    task.source.display()
                );
            }

            let output_dir = parent_dir.join(&source_stem);
            fs::create_dir_all(&output_dir)
                .with_context(|| format!("failed to create {}", output_dir.display()))?;

            let request_ref = request_id.as_deref();
            let count = write_session_batch(
                &sessions,
                request_ref,
                &output_dir,
                &task.source,
                enable_inner_progress,
            )?;

            if let Ok(metadata) = fs::metadata(primary_source) {
                if let Ok(modified) = metadata.modified() {
                    let mtime = FileTime::from_system_time(modified);
                    let _ = set_file_times(&output_dir, mtime, mtime);
                }
            }

            Ok(count)
        }
    }
}

fn write_session_batch(
    sessions: &[Session],
    request_id: Option<&str>,
    output_dir: &Path,
    source_path: &Path,
    enable_progress: bool,
) -> Result<usize> {
    if sessions.is_empty() {
        return Ok(0);
    }

    let progress = if enable_progress {
        Some(make_progress_bar(sessions.len() as u64, "sessions"))
    } else {
        None
    };

    let failures: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let generated = Arc::new(AtomicUsize::new(0));
    let mut name_counters: HashMap<String, usize> = HashMap::new();

    for batch in sessions.chunks(LARGE_BATCH_SIZE) {
        let planned_names = build_batch_file_names(batch, &mut name_counters);

        batch.par_iter().enumerate().for_each(|(offset, session)| {
            let rendered = render_session_markdown(session, request_id, source_path);
            let file_name = planned_names
                .get(offset)
                .cloned()
                .unwrap_or_else(|| "session.md".to_string());
            let target = output_dir.join(file_name);

            if let Err(err) = write_rendered_session(&target, &rendered) {
                let mut guard = failures.lock().expect("poisoned lock");
                guard.push(format!("{}: {err:#}", target.display()));
            } else {
                generated.fetch_add(1, Ordering::SeqCst);
            }

            if let Some(pb) = &progress {
                pb.inc(1);
            }
        });
    }

    if let Some(pb) = progress {
        pb.finish_and_clear();
    }

    let errors = failures.lock().expect("poisoned lock");
    if !errors.is_empty() {
        bail!(
            "failed to write {} sessions from {}. first error: {}",
            errors.len(),
            source_path.display(),
            errors[0]
        );
    }

    Ok(generated.load(Ordering::SeqCst))
}

fn build_batch_file_names(batch: &[Session], counters: &mut HashMap<String, usize>) -> Vec<String> {
    let mut names = Vec::with_capacity(batch.len());

    for session in batch {
        let base = build_session_file_name(
            session.title.as_deref().unwrap_or_default(),
            session.id.as_deref(),
            "session",
        );

        let count = counters.entry(base.clone()).or_insert(0);
        *count += 1;
        if *count == 1 {
            names.push(format!("{base}.md"));
        } else {
            names.push(format!("{base}-{}.md", *count));
        }
    }

    names
}

fn write_rendered_session(path: &Path, rendered: &RenderedSession) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create parent dir {}", parent.display()))?;
    }

    fs::write(path, &rendered.markdown)
        .with_context(|| format!("failed to write markdown {}", path.display()))?;

    if let Some(ts) = rendered.file_timestamp {
        let normalized = normalize_timestamp(ts);
        let ft = FileTime::from_unix_time(normalized, 0);
        let _ = set_file_times(path, ft, ft);
    }

    Ok(())
}

fn parse_input_payload(value: Value) -> Result<ParsedInput> {
    if let Some(obj) = value.as_object() {
        if let Some(data_val) = obj.get("data") {
            if let Some(arr) = data_val.as_array() {
                let sessions = parse_sessions_from_array(arr)?;
                let request_id = obj
                    .get("request_id")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned);
                return Ok(ParsedInput::Wrapped {
                    request_id,
                    sessions,
                });
            }
        }

        let session: Session = serde_json::from_value(Value::Object(obj.clone()))
            .context("failed to parse session object")?;
        if !session_has_message_payload(&session) {
            bail!("object does not look like a Qwen session: no message payload found");
        }
        return Ok(ParsedInput::Single(session));
    }

    if let Some(arr) = value.as_array() {
        let sessions = parse_sessions_from_array(arr)?;
        if sessions.len() == 1 {
            return Ok(ParsedInput::Single(
                sessions.into_iter().next().expect("one session"),
            ));
        }
        return Ok(ParsedInput::Sessions(sessions));
    }

    bail!("json root must be an object or array")
}

fn parse_sessions_from_array(arr: &[Value]) -> Result<Vec<Session>> {
    let mut sessions = Vec::with_capacity(arr.len());
    let mut skipped = 0usize;

    for (idx, item) in arr.iter().enumerate() {
        if item.is_null() {
            skipped += 1;
            debug!("skip null session item at index {}", idx);
            continue;
        }

        if !item.is_object() {
            skipped += 1;
            warn!(
                "skip non-object session item at index {} with type {}",
                idx,
                json_type_name(item)
            );
            continue;
        }

        match serde_json::from_value::<Session>(item.clone()) {
            Ok(session) => {
                if session_has_message_payload(&session) {
                    sessions.push(session);
                } else {
                    skipped += 1;
                    warn!(
                        "skip object at index {} because it has no Qwen message payload",
                        idx
                    );
                }
            }
            Err(err) => {
                skipped += 1;
                warn!("skip invalid session item at index {}: {}", idx, err);
            }
        }
    }

    if sessions.is_empty() {
        bail!(
            "array contains no valid session objects (total: {}, skipped: {})",
            arr.len(),
            skipped
        );
    }

    if skipped > 0 {
        warn!(
            "parsed {} valid sessions and skipped {} invalid items in array",
            sessions.len(),
            skipped
        );
    }

    Ok(sessions)
}

fn de_vec_or_default<'de, D, T>(deserializer: D) -> std::result::Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<Vec<T>>::deserialize(deserializer).map(Option::unwrap_or_default)
}

fn de_map_or_default<'de, D, K, V>(deserializer: D) -> std::result::Result<HashMap<K, V>, D::Error>
where
    D: Deserializer<'de>,
    K: std::hash::Hash + Eq + Deserialize<'de>,
    V: Deserialize<'de>,
{
    Option::<HashMap<K, V>>::deserialize(deserializer).map(Option::unwrap_or_default)
}

fn json_type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

fn render_session_markdown(
    session: &Session,
    request_id: Option<&str>,
    source_path: &Path,
) -> RenderedSession {
    let message_map = collect_messages(session);
    let dialogue = extract_dialogue(&message_map);

    let model_raw = detect_model(&message_map).unwrap_or_else(|| "unknown".to_string());
    let model = normalize_model_namespace(&model_raw);
    let tags = detect_tags(session);
    debug!("meta tags for {:?}: {:?}", session.id, tags);

    let title_hint = session
        .title
        .clone()
        .or_else(|| session.id.clone())
        .unwrap_or_else(|| "session".to_string());

    let mut metadata = BTreeMap::new();
    metadata.insert("Model".to_string(), format!("`{}`", model));
    metadata.insert("Tags".to_string(), format!("`{}`", tags.join(", ")));

    if let Some(id) = session.id.as_deref() {
        metadata.insert("Conversation ID".to_string(), format!("`{}`", id));
    }
    if let Some(uid) = session.user_id.as_deref() {
        metadata.insert("User ID".to_string(), format!("`{}`", uid));
    }
    if let Some(request_id) = request_id {
        metadata.insert("Request ID".to_string(), format!("`{}`", request_id));
    }

    let chat_type = session
        .chat_type
        .clone()
        .or_else(|| first_non_empty_message_field(&message_map, |m| m.chat_type.clone()));
    if let Some(v) = chat_type {
        metadata.insert("Chat Type".to_string(), format!("`{}`", v));
    }

    let sub_chat_type = session
        .sub_chat_type
        .clone()
        .or_else(|| first_non_empty_message_field(&message_map, |m| m.sub_chat_type.clone()));
    if let Some(v) = sub_chat_type {
        metadata.insert("Sub Chat Type".to_string(), format!("`{}`", v));
    }

    metadata.insert(
        "Source".to_string(),
        format!(
            "`{}`",
            source_path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("unknown")
        ),
    );

    let generated_at: DateTime<Utc> = Utc::now();
    metadata.insert(
        "Generated At (UTC)".to_string(),
        format!("`{}`", generated_at.to_rfc3339()),
    );

    let mut markdown = String::new();
    markdown.push_str("## Metadata\n\n### Run Settings\n");
    for (key, value) in metadata {
        markdown.push_str(&format!("- **{}:** {}\n", key, value));
    }

    markdown.push_str("\n## Conversation\n\n");

    if dialogue.is_empty() {
        markdown.push_str("### 🧑‍💻 User\n(无可用用户消息)\n\n");
        markdown.push_str("### 🤖 Assistant\n(无可用助手消息)\n");
    } else {
        for message in dialogue {
            match message.role {
                Role::User => markdown.push_str("### 🧑‍💻 User\n"),
                Role::Assistant => markdown.push_str("### 🤖 Assistant\n"),
            }
            markdown.push_str(message.content.trim());
            markdown.push_str("\n\n");
        }
    }

    let file_timestamp = detect_file_timestamp(session, &message_map);

    RenderedSession {
        markdown,
        title_hint,
        file_timestamp,
    }
}

fn detect_file_timestamp(
    session: &Session,
    message_map: &HashMap<String, MessageNode>,
) -> Option<i64> {
    let mut max_ts = None;

    for message in message_map.values() {
        if let Some(ts) = message.timestamp {
            max_ts = Some(max_ts.map_or(ts, |cur: i64| cur.max(ts)));
        }
    }

    if max_ts.is_none() {
        max_ts = session.updated_at.or(session.created_at);
    }

    if max_ts.is_none() {
        max_ts = session
            .meta
            .get("timestamp")
            .and_then(Value::as_i64)
            .or_else(|| session.meta.get("updated_at").and_then(Value::as_i64));
    }

    max_ts
}

fn collect_messages(session: &Session) -> HashMap<String, MessageNode> {
    let mut map = HashMap::new();

    if let Some(history) = session.chat.as_ref().and_then(|c| c.history.as_ref()) {
        for (id, message) in &history.messages {
            let mut cloned = message.clone();
            if cloned.id.is_none() {
                cloned.id = Some(id.clone());
            }
            map.insert(id.clone(), cloned);
        }
    }

    for message in &session.messages {
        let id = message
            .id
            .clone()
            .unwrap_or_else(|| format!("anon-{}", map.len() + 1));
        let mut cloned = message.clone();
        cloned.id = Some(id.clone());
        map.insert(id, cloned);
    }

    map
}

fn extract_dialogue(message_map: &HashMap<String, MessageNode>) -> Vec<ConversationMessage> {
    if message_map.is_empty() {
        return Vec::new();
    }

    let mut edges: HashMap<String, Vec<String>> = HashMap::new();

    for (id, message) in message_map {
        if let Some(parent) = &message.parent_id {
            edges.entry(parent.clone()).or_default().push(id.clone());
        }

        for child in &message.children_ids {
            edges.entry(id.clone()).or_default().push(child.clone());
        }
    }

    for children in edges.values_mut() {
        children.sort();
        children.dedup();
    }

    let mut users: Vec<&MessageNode> = message_map
        .values()
        .filter(|m| role_eq(m.role.as_deref(), "user"))
        .collect();

    users.sort_by(|a, b| {
        let a_ts = normalize_timestamp(a.timestamp.unwrap_or_default());
        let b_ts = normalize_timestamp(b.timestamp.unwrap_or_default());
        a_ts.cmp(&b_ts).then_with(|| a.id.cmp(&b.id))
    });

    let mut out = Vec::new();

    for user in users {
        let user_id = match &user.id {
            Some(id) => id,
            None => continue,
        };

        let user_text = extract_plain_content(user);
        if user_text.trim().is_empty() {
            continue;
        }

        let mut assistant_children = edges.get(user_id).cloned().unwrap_or_default();
        assistant_children.retain(|child_id| {
            message_map
                .get(child_id)
                .map(|m| role_eq(m.role.as_deref(), "assistant"))
                .unwrap_or(false)
        });

        assistant_children.sort_by(|a, b| {
            let am = message_map.get(a);
            let bm = message_map.get(b);
            let a_ts = normalize_timestamp(am.and_then(|m| m.timestamp).unwrap_or_default());
            let b_ts = normalize_timestamp(bm.and_then(|m| m.timestamp).unwrap_or_default());
            a_ts.cmp(&b_ts).then_with(|| a.cmp(b))
        });
        assistant_children.dedup();

        for child_id in assistant_children {
            let assistant = match message_map.get(&child_id) {
                Some(v) => v,
                None => continue,
            };

            let assistant_text = extract_assistant_content(assistant);
            if assistant_text.trim().is_empty() {
                continue;
            }

            out.push(ConversationMessage {
                role: Role::User,
                content: user_text.clone(),
            });
            out.push(ConversationMessage {
                role: Role::Assistant,
                content: assistant_text,
            });
        }
    }

    if out.is_empty() {
        return fallback_linear_dialogue(message_map);
    }

    out
}

fn fallback_linear_dialogue(
    message_map: &HashMap<String, MessageNode>,
) -> Vec<ConversationMessage> {
    let mut messages: Vec<&MessageNode> = message_map
        .values()
        .filter(|m| role_eq(m.role.as_deref(), "user") || role_eq(m.role.as_deref(), "assistant"))
        .collect();

    messages.sort_by(|a, b| {
        normalize_timestamp(a.timestamp.unwrap_or_default())
            .cmp(&normalize_timestamp(b.timestamp.unwrap_or_default()))
            .then_with(|| a.id.cmp(&b.id))
    });

    let mut out = Vec::new();
    for message in messages {
        let role = if role_eq(message.role.as_deref(), "user") {
            Role::User
        } else {
            Role::Assistant
        };

        let content = if role == Role::User {
            extract_plain_content(message)
        } else {
            extract_assistant_content(message)
        };

        if content.trim().is_empty() {
            continue;
        }

        out.push(ConversationMessage { role, content });
    }

    out
}

fn detect_model(message_map: &HashMap<String, MessageNode>) -> Option<String> {
    let mut assistants: Vec<&MessageNode> = message_map
        .values()
        .filter(|m| role_eq(m.role.as_deref(), "assistant"))
        .collect();

    assistants.sort_by(|a, b| {
        normalize_timestamp(a.timestamp.unwrap_or_default())
            .cmp(&normalize_timestamp(b.timestamp.unwrap_or_default()))
    });

    for assistant in assistants {
        if let Some(name) = assistant
            .model_name
            .as_deref()
            .filter(|s| !s.trim().is_empty())
        {
            return Some(name.to_string());
        }
        if let Some(model) = assistant.model.as_deref().filter(|s| !s.trim().is_empty()) {
            return Some(model.to_string());
        }
    }

    None
}

fn normalize_model_namespace(model_name: &str) -> String {
    let cleaned = model_name.trim().trim_matches('`');
    if cleaned.is_empty() {
        return "models/unknown".to_string();
    }

    if cleaned.starts_with("models/") {
        cleaned.to_string()
    } else {
        format!("models/{}", cleaned)
    }
}

fn detect_tags(session: &Session) -> Vec<String> {
    if let Some(tags) = extract_tags_from_value(session.meta.get("tags")) {
        if !tags.is_empty() {
            return tags;
        }
    }

    let mut fallback = Vec::new();

    if let Some(chat_type) = session
        .chat_type
        .as_deref()
        .filter(|s| !s.trim().is_empty())
    {
        fallback.push(format!("chat/{}", chat_type));
    }

    if let Some(sub_chat_type) = session
        .sub_chat_type
        .as_deref()
        .filter(|s| !s.trim().is_empty())
    {
        fallback.push(format!("sub/{}", sub_chat_type));
    }

    if fallback.is_empty() {
        fallback.push("untagged".to_string());
    }

    normalize_tags(fallback)
}

fn extract_tags_from_value(value: Option<&Value>) -> Option<Vec<String>> {
    let value = value?;

    if let Some(arr) = value.as_array() {
        let tags = arr
            .iter()
            .filter_map(Value::as_str)
            .map(ToOwned::to_owned)
            .collect::<Vec<_>>();
        if looks_like_garbled_tags(&tags) {
            warn!("ignore garbled meta.tags array payload");
            return None;
        }
        return Some(normalize_tags(tags));
    }

    if let Some(text) = value.as_str() {
        let cleaned = text.trim().trim_matches('`').trim();
        if cleaned.is_empty() {
            return None;
        }

        if cleaned.starts_with('[') && cleaned.ends_with(']') {
            if let Ok(parsed) = serde_json::from_str::<Vec<String>>(cleaned) {
                if looks_like_garbled_tags(&parsed) {
                    warn!("ignore garbled meta.tags string payload");
                    return None;
                }
                return Some(normalize_tags(parsed));
            }
        }

        let tags = split_tag_text(cleaned);
        if looks_like_garbled_tags(&tags) {
            warn!("ignore garbled split tags payload");
            return None;
        }
        return Some(normalize_tags(tags));
    }

    None
}

fn looks_like_garbled_tags(tags: &[String]) -> bool {
    if tags.len() < 8 {
        return false;
    }

    let mut non_empty = 0usize;
    let mut single_char = 0usize;
    let mut punctuation_or_ctrl = 0usize;
    let mut has_brace = false;
    let mut has_quote = false;

    for raw in tags {
        let token = raw.trim();
        if token.is_empty() {
            continue;
        }
        non_empty += 1;
        if token.chars().count() <= 1 {
            single_char += 1;
        }
        if token == "{" || token == "}" || token == "[" || token == "]" {
            has_brace = true;
        }
        if token == "\"" || token == "'" || token == "`" {
            has_quote = true;
        }
        if token
            .chars()
            .all(|c| c.is_ascii_punctuation() || c.is_control() || c.is_whitespace())
        {
            punctuation_or_ctrl += 1;
        }
    }

    if non_empty == 0 {
        return true;
    }

    let single_ratio = single_char as f64 / non_empty as f64;
    let punct_ratio = punctuation_or_ctrl as f64 / non_empty as f64;

    (single_ratio >= 0.7 && punct_ratio >= 0.2)
        || (single_ratio >= 0.85)
        || (single_ratio >= 0.6 && has_brace && has_quote)
}

fn split_tag_text(text: &str) -> Vec<String> {
    if text.contains(',') {
        return text.split(',').map(ToOwned::to_owned).collect();
    }

    if text.contains(';') {
        return text.split(';').map(ToOwned::to_owned).collect();
    }

    text.split_whitespace().map(ToOwned::to_owned).collect()
}

fn normalize_tags(tags: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();

    for raw in tags {
        let mut candidate = raw
            .trim()
            .trim_matches('`')
            .trim_matches('"')
            .trim()
            .to_string();
        while let Some(stripped) = candidate.strip_prefix('#') {
            candidate = stripped.trim().to_string();
        }

        if candidate.is_empty() {
            continue;
        }

        let key = candidate.to_lowercase();
        if seen.insert(key) {
            out.push(candidate);
        }
    }

    out
}

fn extract_plain_content(message: &MessageNode) -> String {
    if let Some(content) = message.content.as_deref() {
        if !content.trim().is_empty() {
            return content.to_string();
        }
    }

    let mut chunks = Vec::new();
    for block in &message.content_list {
        if let Some(content) = block.content.as_deref().filter(|s| !s.trim().is_empty()) {
            chunks.push(content.to_string());
        }
    }

    chunks.join("\n\n")
}

fn extract_assistant_content(message: &MessageNode) -> String {
    let mut thoughts = Vec::new();
    let mut responses = Vec::new();

    if let Some(reasoning) = &message.reasoning_content {
        collect_strings_from_value(reasoning, &mut thoughts);
    }

    for block in &message.content_list {
        let phase = block.phase.as_deref().unwrap_or_default().to_lowercase();
        let content = block.content.as_deref().filter(|s| !s.trim().is_empty());

        if phase.contains("thinking") {
            if let Some(value) = content {
                push_unique_text(&mut thoughts, value.to_string());
            }
            if let Some(extra) = &block.extra {
                for thought in extract_thoughts_from_extra(extra) {
                    push_unique_text(&mut thoughts, thought);
                }
            }
        } else if let Some(value) = content {
            push_unique_text(&mut responses, value.to_string());
        }
    }

    if let Some(content) = message.content.as_deref().filter(|s| !s.trim().is_empty()) {
        push_unique_text(&mut responses, content.to_string());
    }

    match (thoughts.is_empty(), responses.is_empty()) {
        (false, false) => format!(
            "#### 🤔 Thought Process\n{}\n\n#### 💡 Response\n{}",
            thoughts.join("\n\n"),
            responses.join("\n\n")
        ),
        (false, true) => format!("#### 🤔 Thought Process\n{}", thoughts.join("\n\n")),
        (true, false) => responses.join("\n\n"),
        (true, true) => String::new(),
    }
}

fn extract_thoughts_from_extra(extra: &Value) -> Vec<String> {
    let mut thoughts = Vec::new();

    if let Some(value) = extra
        .get("summary_thought")
        .and_then(|v| v.get("content"))
        .or_else(|| extra.get("summary_thought"))
    {
        collect_strings_from_value(value, &mut thoughts);
    }

    if let Some(value) = extra.get("thinking").or_else(|| extra.get("thought")) {
        collect_strings_from_value(value, &mut thoughts);
    }

    thoughts
}

fn collect_strings_from_value(value: &Value, out: &mut Vec<String>) {
    match value {
        Value::String(text) => {
            push_unique_text(out, text.to_string());
        }
        Value::Array(items) => {
            for item in items {
                collect_strings_from_value(item, out);
            }
        }
        Value::Object(map) => {
            for value in map.values() {
                collect_strings_from_value(value, out);
            }
        }
        _ => {}
    }
}

fn push_unique_text(out: &mut Vec<String>, candidate: String) {
    let normalized = candidate.trim();
    if normalized.is_empty() {
        return;
    }

    if out.iter().any(|line| line.trim() == normalized) {
        return;
    }

    out.push(normalized.to_string());
}

fn first_non_empty_message_field<F>(
    message_map: &HashMap<String, MessageNode>,
    picker: F,
) -> Option<String>
where
    F: Fn(&MessageNode) -> Option<String>,
{
    let mut messages: Vec<&MessageNode> = message_map.values().collect();
    messages.sort_by(|a, b| {
        normalize_timestamp(a.timestamp.unwrap_or_default())
            .cmp(&normalize_timestamp(b.timestamp.unwrap_or_default()))
    });

    for message in messages {
        if let Some(value) = picker(message).filter(|s| !s.trim().is_empty()) {
            return Some(value);
        }
    }

    None
}

fn build_session_file_name(
    title_hint: &str,
    fallback_id: Option<&str>,
    fallback_name: &str,
) -> String {
    let sanitized = sanitize_file_name(title_hint);
    if !sanitized.is_empty() {
        return sanitized;
    }

    if let Some(id) = fallback_id {
        let id_name = sanitize_file_name(id);
        if !id_name.is_empty() {
            return id_name;
        }
    }

    let fallback = sanitize_file_name(fallback_name);
    if fallback.is_empty() {
        "session".to_string()
    } else {
        fallback
    }
}

fn sanitize_file_name(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        let is_invalid =
            matches!(ch, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') || ch.is_control();
        if is_invalid {
            out.push('_');
        } else if ch.is_whitespace() {
            out.push('_');
        } else {
            out.push(ch);
        }
    }

    while out.contains("__") {
        out = out.replace("__", "_");
    }

    out.trim_matches(&['_', '.', ' '][..]).to_string()
}

fn role_eq(role: Option<&str>, expected: &str) -> bool {
    role.map(|v| v.eq_ignore_ascii_case(expected))
        .unwrap_or(false)
}

fn normalize_timestamp(ts: i64) -> i64 {
    if ts > 1_000_000_000_000 {
        ts / 1000
    } else {
        ts
    }
}

fn write_error_log(path: &Path, failures: &[FileFailure]) -> Result<()> {
    let mut body = String::new();
    for failure in failures {
        body.push_str(&format!(
            "{}\n{}\n\n",
            failure.source.display(),
            failure.reason
        ));
    }

    fs::write(path, body).with_context(|| format!("failed to write {}", path.display()))
}

fn make_progress_bar(total: u64, unit: &str) -> ProgressBar {
    let pb = ProgressBar::new(total);
    let template = "{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} {msg} ({per_sec}, ETA {eta})";
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

    #[test]
    fn normalize_tags_handles_hash_and_case() {
        let tags = normalize_tags(vec![
            "#Topic/A".to_string(),
            "topic/a".to_string(),
            " `Topic/B` ".to_string(),
        ]);
        assert_eq!(tags, vec!["Topic/A", "Topic/B"]);
    }

    #[test]
    fn parse_wrapped_payload() {
        let value = serde_json::json!({
            "success": true,
            "request_id": "req-1",
            "data": [
                {
                    "id": "a",
                    "title": "A",
                    "chat": {
                        "history": {
                            "messages": {
                                "u1": {
                                    "id": "u1",
                                    "role": "user",
                                    "content": "hello"
                                }
                            }
                        }
                    }
                }
            ]
        });

        let parsed = parse_input_payload(value).expect("payload parse should succeed");
        match parsed {
            ParsedInput::Wrapped {
                request_id,
                sessions,
            } => {
                assert_eq!(request_id.as_deref(), Some("req-1"));
                assert_eq!(sessions.len(), 1);
            }
            _ => panic!("expected wrapped payload"),
        }
    }

    #[test]
    fn extract_dialogue_duplicates_user_for_regen_branch() {
        let mut map = HashMap::new();
        map.insert(
            "u1".to_string(),
            MessageNode {
                id: Some("u1".to_string()),
                role: Some("user".to_string()),
                content: Some("question".to_string()),
                reasoning_content: None,
                content_list: vec![],
                model: None,
                model_name: None,
                parent_id: None,
                children_ids: vec!["a1".to_string(), "a2".to_string()],
                timestamp: Some(100),
                chat_type: None,
                sub_chat_type: None,
            },
        );

        for id in ["a1", "a2"] {
            map.insert(
                id.to_string(),
                MessageNode {
                    id: Some(id.to_string()),
                    role: Some("assistant".to_string()),
                    content: Some(format!("answer-{id}")),
                    reasoning_content: None,
                    content_list: vec![],
                    model: None,
                    model_name: None,
                    parent_id: Some("u1".to_string()),
                    children_ids: vec![],
                    timestamp: Some(101),
                    chat_type: None,
                    sub_chat_type: None,
                },
            );
        }

        let dialogue = extract_dialogue(&map);
        assert_eq!(dialogue.len(), 4);
        assert_eq!(dialogue[0].content, "question");
        assert_eq!(dialogue[1].content, "answer-a1");
        assert_eq!(dialogue[2].content, "question");
        assert_eq!(dialogue[3].content, "answer-a2");
    }

    #[test]
    fn tags_fallback_when_meta_missing() {
        let session = Session {
            id: Some("id-1".to_string()),
            user_id: None,
            title: None,
            meta: Value::Null,
            chat_type: Some("t2t".to_string()),
            sub_chat_type: None,
            created_at: None,
            updated_at: None,
            chat: None,
            messages: Vec::new(),
        };

        assert_eq!(detect_tags(&session), vec!["chat/t2t"]);
    }

    #[test]
    fn assistant_thinking_is_rendered_into_thought_section() {
        let assistant = MessageNode {
            id: Some("a1".to_string()),
            role: Some("assistant".to_string()),
            content: Some("final answer".to_string()),
            reasoning_content: Some(serde_json::json!({
                "summary_thought": { "content": ["first think", "second think"] }
            })),
            content_list: vec![],
            model: None,
            model_name: None,
            parent_id: None,
            children_ids: vec![],
            timestamp: None,
            chat_type: None,
            sub_chat_type: None,
        };

        let rendered = extract_assistant_content(&assistant);
        assert!(rendered.contains("#### 🤔 Thought Process"));
        assert!(rendered.contains("first think"));
        assert!(rendered.contains("#### 💡 Response"));
        assert!(rendered.contains("final answer"));
    }

    #[test]
    fn model_namespace_is_normalized() {
        assert_eq!(
            normalize_model_namespace("Qwen3.5-397B-A17B"),
            "models/Qwen3.5-397B-A17B"
        );
        assert_eq!(
            normalize_model_namespace("models/openai/gpt-4.1"),
            "models/openai/gpt-4.1"
        );
    }

    #[test]
    fn garbled_char_bag_tags_are_rejected() {
        let value = serde_json::json!([",", "\n", "e", "}", "c", ":", "a", "{", "\"", "[", "]"]);
        let parsed = extract_tags_from_value(Some(&value));
        assert!(parsed.is_none());
    }
}
