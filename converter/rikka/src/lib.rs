//! 把 RikkaHub 的备份（内含 SQLite 数据库）转换为 AfterChat 对话 Markdown / ZIP。
//!
//! 输出契约见仓库根目录 `CHATFORMAT-CONVERTER.md`。
//! 实现思路对齐 `cherry-studio-backup-json-converter`：按助手分目录、附加 Metadata 键、兜底命名。

use std::borrow::Cow;
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use chrono::{Datelike, Local, TimeZone, Timelike};
use rayon::prelude::*;
use rusqlite::Connection;
use serde::Deserialize;
use tempfile::TempDir;
use zip::CompressionMethod;
use zip::ZipArchive;
use zip::write::{SimpleFileOptions, ZipWriter};

/// 输出 zip 名前缀：`chat-export-rikka-all-{毫秒时间戳}.zip`
const EXPORT_PREFIX: &str = "chat-export-rikka-all";
/// 失败报告里的平台标识
const PLATFORM_ID: &str = "rikka";
/// zip 条目名里标题部分的最大字符数
const ENTRY_TITLE_MAX: usize = 80;
/// 标题为空时的兜底名
const DEFAULT_TITLE: &str = "Untitled_Conversation";
/// 助手名 / 分组目录为空时的兜底名
const DEFAULT_ASSISTANT: &str = "Assistant";

// ═══════════════════════════════════════════════════════════
//  对外 API
// ═══════════════════════════════════════════════════════════

#[derive(Debug, Clone)]
pub struct ConvertOptions {
    /// 输入：RikkaHub 备份 zip 或裸 `.db`
    pub input: PathBuf,
    /// 输出目录，或显式 `.zip` 路径；`None` 表示写到源文件同目录
    pub output: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct RunSummary {
    pub output: PathBuf,
    pub exported: usize,
    pub skipped: usize,
}

pub fn run_conversion(options: ConvertOptions) -> Result<RunSummary> {
    let input = &options.input;
    if !input.exists() {
        bail!("找不到文件: {}", input.display());
    }
    if !input.is_file() {
        bail!("输入必须是文件: {}", input.display());
    }

    let payload = if is_zip_input(input) {
        read_zip_entries(input)?
    } else {
        read_db_files(input)?
    };
    let database = open_database(&payload)?;
    let settings = SettingsIndex::parse(payload.settings.as_deref());

    let conversations = load_conversations(&database.conn)?;
    if conversations.is_empty() {
        bail!("未找到任何对话（ConversationEntity 为空，可能不是 RikkaHub 备份）");
    }

    let outcomes: Vec<Outcome> = conversations
        .par_iter()
        .map(|conversation| render_conversation(conversation, &settings))
        .collect();

    let mut rendered: Vec<RenderedConversation> = Vec::new();
    let mut failures: Vec<ConversationFailure> = Vec::new();
    for outcome in outcomes {
        match outcome {
            Outcome::Rendered(conversation) => rendered.push(*conversation),
            Outcome::Failed(failure) => failures.push(failure),
        }
    }

    // 包内按对话时间从新到旧（qwen 口径）；无时间的垫底
    rendered.sort_by(|a, b| match (a.sort_ms, b.sort_ms) {
        (None, None) => a.entry_name.cmp(&b.entry_name),
        (None, Some(_)) => Ordering::Greater,
        (Some(_), None) => Ordering::Less,
        (Some(left), Some(right)) => right
            .cmp(&left)
            .then_with(|| a.entry_name.cmp(&b.entry_name)),
    });

    // 重名追加 -2 / -3
    let mut used_names: HashSet<String> = HashSet::new();
    for conversation in &mut rendered {
        conversation.entry_name = unique_entry_name(&conversation.entry_name, &mut used_names);
    }

    let zip_name = format!(
        "{EXPORT_PREFIX}-{}.zip",
        chrono::Utc::now().timestamp_millis()
    );
    let target = resolve_zip_target(input, options.output.as_deref(), zip_name)?;
    if let Some(parent) = target.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)
            .with_context(|| format!("创建输出目录失败 {}", parent.display()))?;
    }

    write_zip_export(input, &rendered, &failures, &target)?;

    Ok(RunSummary {
        output: target,
        exported: rendered.len(),
        skipped: failures.len(),
    })
}

// ═══════════════════════════════════════════════════════════
//  输入读取（zip 内 db + wal，或裸 db）
// ═══════════════════════════════════════════════════════════

struct BackupPayload {
    db_name: String,
    db: Vec<u8>,
    wal: Option<Vec<u8>>,
    settings: Option<Vec<u8>>,
}

struct Database {
    // 字段顺序即析构顺序：先关连接，再删临时目录（Windows 上文件不能被占用）
    conn: Connection,
    _tmp: TempDir,
}

fn is_zip_input(path: &Path) -> bool {
    path.extension()
        .and_then(|value| value.to_str())
        .is_some_and(|value| value.eq_ignore_ascii_case("zip"))
}

fn entry_basename(name: &str) -> &str {
    name.rsplit(['/', '\\']).next().unwrap_or(name)
}

/// 在压缩包里按 basename 找条目：任意层级、大小写不敏感，取层级最浅者（同层按名字典序）
fn find_zip_entry<F>(archive: &mut ZipArchive<File>, mut predicate: F) -> Option<usize>
where
    F: FnMut(&str) -> bool,
{
    let mut best: Option<(usize, String, usize)> = None;
    for index in 0..archive.len() {
        let Ok(entry) = archive.by_index(index) else {
            continue;
        };
        if entry.is_dir() {
            continue;
        }
        let name = entry.name().to_string();
        let base = entry_basename(&name);
        if !predicate(base) {
            continue;
        }
        let depth = Path::new(&name).components().count();
        let replace = match &best {
            None => true,
            Some((best_depth, best_name, _)) => (depth, base) < (*best_depth, best_name.as_str()),
        };
        if replace {
            best = Some((depth, base.to_string(), index));
        }
    }
    best.map(|(_, _, index)| index)
}

fn read_zip_entry(archive: &mut ZipArchive<File>, index: usize) -> Result<Vec<u8>> {
    let mut entry = archive.by_index(index)?;
    let mut buffer = Vec::with_capacity(entry.size() as usize);
    entry
        .read_to_end(&mut buffer)
        .context("读取压缩包内文件失败")?;
    Ok(buffer)
}

fn zip_entry_basename(archive: &mut ZipArchive<File>, index: usize) -> Result<String> {
    let entry = archive.by_index(index)?;
    Ok(entry_basename(entry.name()).to_string())
}

fn read_zip_entries(path: &Path) -> Result<BackupPayload> {
    let file = File::open(path).with_context(|| format!("打开压缩包失败 {}", path.display()))?;
    let mut archive =
        ZipArchive::new(file).with_context(|| format!("读取压缩包失败 {}", path.display()))?;

    let db_index = find_zip_entry(&mut archive, |base| {
        base.eq_ignore_ascii_case("rikka_hub.db")
    })
    .or_else(|| {
        find_zip_entry(&mut archive, |base| {
            base.to_ascii_lowercase().ends_with(".db")
        })
    })
    .with_context(|| format!("{} 里没有找到数据库文件 (*.db)", path.display()))?;

    let db_name = zip_entry_basename(&mut archive, db_index)?;
    let wal_name = format!("{db_name}-wal");
    let wal_index = find_zip_entry(&mut archive, |base| base.eq_ignore_ascii_case(&wal_name));
    let settings_index = find_zip_entry(&mut archive, |base| {
        base.eq_ignore_ascii_case("settings.json")
    });

    let db = read_zip_entry(&mut archive, db_index)?;
    let wal = match wal_index {
        Some(index) => Some(read_zip_entry(&mut archive, index)?),
        None => None,
    };
    let settings = match settings_index {
        Some(index) => Some(read_zip_entry(&mut archive, index)?),
        None => None,
    };

    Ok(BackupPayload {
        db_name,
        db,
        wal,
        settings,
    })
}

fn read_db_files(path: &Path) -> Result<BackupPayload> {
    let db_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("rikka_hub.db")
        .to_string();
    let db = fs::read(path).with_context(|| format!("读取数据库失败 {}", path.display()))?;

    let wal_path = path.with_file_name(format!("{db_name}-wal"));
    let wal = if wal_path.is_file() {
        Some(fs::read(&wal_path).with_context(|| format!("读取 WAL 失败 {}", wal_path.display()))?)
    } else {
        None
    };

    let settings_path = path.with_file_name("settings.json");
    let settings = if settings_path.is_file() {
        Some(
            fs::read(&settings_path)
                .with_context(|| format!("读取 settings.json 失败 {}", settings_path.display()))?,
        )
    } else {
        None
    };

    Ok(BackupPayload {
        db_name,
        db,
        wal,
        settings,
    })
}

/// 把 db（及可选 wal）落到临时目录再打开。
///
/// **不写 `-shm`**：共享内存易变，复制进来会阻止 SQLite 回放 WAL。让 SQLite 自行重建。
fn open_database(payload: &BackupPayload) -> Result<Database> {
    let tmp = tempfile::tempdir().context("创建临时目录失败")?;
    let db_path = tmp.path().join(&payload.db_name);
    fs::write(&db_path, &payload.db).context("写入临时数据库失败")?;
    if let Some(wal) = &payload.wal {
        let wal_path = tmp.path().join(format!("{}-wal", payload.db_name));
        fs::write(&wal_path, wal).context("写入临时 WAL 失败")?;
    }

    let conn = Connection::open(&db_path)
        .with_context(|| format!("打开数据库失败 {}", db_path.display()))?;
    Ok(Database { conn, _tmp: tmp })
}

// ═══════════════════════════════════════════════════════════
//  settings.json
// ═══════════════════════════════════════════════════════════

#[derive(Debug, Default, Deserialize)]
struct Settings {
    #[serde(default)]
    providers: Vec<Provider>,
    #[serde(default)]
    assistants: Vec<AssistantConf>,
}

#[derive(Debug, Default, Deserialize)]
struct Provider {
    #[serde(default)]
    models: Vec<ModelConf>,
}

#[derive(Debug, Default, Deserialize)]
struct ModelConf {
    #[serde(default)]
    id: String,
    #[serde(default, rename = "modelId")]
    model_id: Option<String>,
    #[serde(default, rename = "displayName")]
    display_name: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct AssistantConf {
    #[serde(default)]
    id: String,
    #[serde(default)]
    name: String,
    #[serde(default, rename = "systemPrompt")]
    system_prompt: String,
    #[serde(default, rename = "allowConversationSystemPrompt")]
    allow_conversation_system_prompt: bool,
}

struct SettingsIndex {
    /// model id (uuid) → 展示名
    model_labels: HashMap<String, String>,
    assistants: HashMap<String, AssistantConf>,
}

impl SettingsIndex {
    /// 解析 settings.json；缺失 / 损坏时返回空索引（只影响 Model / System / 助手名）。
    fn parse(raw: Option<&[u8]>) -> Self {
        let Some(raw) = raw else {
            return Self::empty();
        };
        let raw = raw.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(raw);
        let Ok(settings) = serde_json::from_slice::<Settings>(raw) else {
            return Self::empty();
        };

        let mut model_labels = HashMap::new();
        for provider in settings.providers {
            for model in provider.models {
                if model.id.is_empty() {
                    continue;
                }
                let label = first_non_empty([
                    model.display_name.as_deref(),
                    model.model_id.as_deref(),
                    Some(model.id.as_str()),
                ])
                .unwrap_or("Unknown")
                .to_string();
                model_labels.insert(model.id, label);
            }
        }

        let mut assistants = HashMap::new();
        for assistant in settings.assistants {
            if !assistant.id.is_empty() {
                assistants.insert(assistant.id.clone(), assistant);
            }
        }

        SettingsIndex {
            model_labels,
            assistants,
        }
    }

    fn empty() -> Self {
        SettingsIndex {
            model_labels: HashMap::new(),
            assistants: HashMap::new(),
        }
    }
}

fn first_non_empty<'a>(candidates: [Option<&'a str>; 3]) -> Option<&'a str> {
    candidates
        .into_iter()
        .flatten()
        .map(str::trim)
        .find(|value| !value.is_empty())
}

// ═══════════════════════════════════════════════════════════
//  数据库 → 会话
// ═══════════════════════════════════════════════════════════

#[derive(Debug, Clone, Deserialize)]
struct RawMessage {
    #[serde(default)]
    role: Option<String>,
    #[serde(default)]
    parts: Vec<Part>,
    #[serde(default, rename = "modelId")]
    model_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct Part {
    #[serde(default, rename = "type")]
    kind: String,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    reasoning: Option<String>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default, rename = "fileName")]
    file_name: Option<String>,
}

/// 旧版：消息直接存在 `ConversationEntity.nodes` 里（`[{ messages, selectIndex }]`）
#[derive(Debug, Deserialize)]
struct LegacyNode {
    #[serde(default)]
    messages: Vec<RawMessage>,
    #[serde(default, rename = "selectIndex")]
    select_index: i64,
}

struct Conversation {
    id: String,
    assistant_id: String,
    title: String,
    create_at_ms: Option<i64>,
    custom_system_prompt: String,
    messages: Vec<RawMessage>,
}

fn load_conversations(conn: &Connection) -> Result<Vec<Conversation>> {
    // 先把所有 node 读进内存（conversation_id → [(messages JSON, select_index)]）；
    // 老版本可能还没有 message_node 表，此时为空，走 nodes JSON 兜底。
    let mut nodes_by_conversation = load_node_rows(conn);

    // 不同 RikkaHub 版本的 ConversationEntity 列不一致（老版本没有
    // custom_system_prompt / mode_injection_ids 等），因此按列名动态取值。
    let mut stmt = conn
        .prepare("SELECT * FROM ConversationEntity")
        .context("查询 ConversationEntity 失败（不是 RikkaHub 数据库？）")?;

    let (id_idx, assistant_idx, title_idx, create_at_idx, custom_prompt_idx, nodes_idx) = {
        let columns: HashMap<&str, usize> = stmt
            .column_names()
            .iter()
            .enumerate()
            .map(|(index, name)| (*name, index))
            .collect();
        (
            columns.get("id").copied(),
            columns.get("assistant_id").copied(),
            columns.get("title").copied(),
            columns.get("create_at").copied(),
            columns.get("custom_system_prompt").copied(),
            columns.get("nodes").copied(),
        )
    };

    let rows = stmt.query_map([], move |row| {
        let text = |index: Option<usize>| -> rusqlite::Result<String> {
            match index {
                Some(index) => Ok(row.get::<_, Option<String>>(index)?.unwrap_or_default()),
                None => Ok(String::new()),
            }
        };
        let create_at = match create_at_idx {
            Some(index) => row.get::<_, Option<i64>>(index)?,
            None => None,
        };
        Ok((
            text(id_idx)?,
            text(assistant_idx)?,
            text(title_idx)?,
            create_at,
            text(custom_prompt_idx)?,
            text(nodes_idx)?,
        ))
    })?;

    let mut conversations = Vec::new();
    for row in rows {
        let (id, assistant_id, title, create_at, custom_system_prompt, nodes) = row?;
        let node_rows = nodes_by_conversation.remove(&id).unwrap_or_default();
        let messages = build_messages(node_rows, &nodes);
        conversations.push(Conversation {
            id,
            assistant_id,
            title,
            create_at_ms: create_at.filter(|value| *value > 0),
            custom_system_prompt,
            messages,
        });
    }

    Ok(conversations)
}

/// 读 `message_node`：conversation_id → 按 node_index 排好序的 (messages JSON, select_index)。
/// 表不存在 / 查询失败时返回空表（老版本靠 `ConversationEntity.nodes` 兜底）。
fn load_node_rows(conn: &Connection) -> HashMap<String, Vec<(String, i64)>> {
    let mut map: HashMap<String, Vec<(String, i64)>> = HashMap::new();
    let Ok(mut stmt) = conn.prepare(
        "SELECT conversation_id, messages, select_index FROM message_node \
         ORDER BY conversation_id ASC, node_index ASC",
    ) else {
        return map;
    };
    let Ok(rows) = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, i64>(2)?,
        ))
    }) else {
        return map;
    };
    for row in rows.flatten() {
        let (conversation_id, messages, select_index) = row;
        map.entry(conversation_id)
            .or_default()
            .push((messages, select_index));
    }
    map
}

/// 按 node 顺序取每个 node 的 `messages[select_index]`；无新表数据时回退旧 `nodes` JSON。
fn build_messages(node_rows: Vec<(String, i64)>, legacy_nodes: &str) -> Vec<RawMessage> {
    let mut messages = Vec::new();
    for (json, select_index) in node_rows {
        if let Ok(list) = serde_json::from_str::<Vec<RawMessage>>(&json)
            && let Some(message) = pick_message(list, select_index)
        {
            messages.push(message);
        }
    }

    if messages.is_empty()
        && let Ok(nodes) = serde_json::from_str::<Vec<LegacyNode>>(legacy_nodes)
    {
        for node in nodes {
            if let Some(message) = pick_message(node.messages, node.select_index) {
                messages.push(message);
            }
        }
    }

    messages
}

fn pick_message(mut list: Vec<RawMessage>, select_index: i64) -> Option<RawMessage> {
    if list.is_empty() {
        return None;
    }
    let index = select_index.clamp(0, (list.len() - 1) as i64) as usize;
    Some(list.swap_remove(index))
}

// ═══════════════════════════════════════════════════════════
//  渲染
// ═══════════════════════════════════════════════════════════

struct RenderedConversation {
    /// zip 内路径：`<助手名>/<YYYYMMDD-HHmmss>-<标题>.md`
    entry_name: String,
    markdown: String,
    /// 排序键（毫秒）
    sort_ms: Option<i64>,
    /// zip 条目修改时间（epoch 秒）
    epoch_secs: Option<i64>,
}

struct ConversationFailure {
    id: String,
    title: String,
    reason: String,
}

enum Outcome {
    Rendered(Box<RenderedConversation>),
    Failed(ConversationFailure),
}

fn render_conversation(conversation: &Conversation, settings: &SettingsIndex) -> Outcome {
    if conversation.messages.is_empty() {
        return Outcome::Failed(ConversationFailure {
            id: conversation.id.clone(),
            title: display_title(&conversation.title),
            reason: "没有消息".to_string(),
        });
    }

    let assistant = settings.assistants.get(&conversation.assistant_id);
    let assistant_name = non_empty(&assistant.map(|a| a.name.as_str()).unwrap_or_default())
        .unwrap_or(DEFAULT_ASSISTANT);
    let system_prompt = effective_system_prompt(conversation, assistant);
    let model = resolve_model(conversation, settings);
    let time_str = conversation
        .create_at_ms
        .map(format_local_time_ms)
        .unwrap_or_else(|| "unknown".to_string());

    let mut markdown = String::new();
    markdown.push_str("## Metadata\n\n");
    markdown.push_str(&format!("- **Model:** `{model}`\n"));
    markdown.push_str(&format!("- **Time:** {time_str}\n"));
    markdown.push_str(&format!("- **Conversation ID:** `{}`\n", conversation.id));
    markdown.push_str(&format!("- **Assistant:** `{assistant_name}`\n\n"));
    markdown.push_str("## Conversation\n\n");

    // 契约：System 提示是对话的第一条消息
    if !system_prompt.trim().is_empty() {
        markdown.push_str("### ⚙️ System\n\n");
        markdown.push_str(&strip_hashes(&system_prompt));
        markdown.push_str("\n\n");
    }

    for message in &conversation.messages {
        render_message(&mut markdown, message);
    }

    let safe_title = {
        let sanitized = sanitize_filename(&conversation.title);
        let sanitized = if sanitized.is_empty() {
            DEFAULT_TITLE.to_string()
        } else {
            sanitized
        };
        truncate_chars(&sanitized, ENTRY_TITLE_MAX)
    };
    let prefix = conversation
        .create_at_ms
        .map(format_local_compact_ms)
        .unwrap_or_else(|| "00000000-000000".to_string());
    let directory = sanitize_path_component(assistant_name, DEFAULT_ASSISTANT);

    Outcome::Rendered(Box::new(RenderedConversation {
        entry_name: format!("{directory}/{prefix}-{safe_title}.md"),
        markdown,
        sort_ms: conversation.create_at_ms,
        epoch_secs: conversation.create_at_ms.map(|ms| ms / 1000),
    }))
}

fn effective_system_prompt(
    conversation: &Conversation,
    assistant: Option<&AssistantConf>,
) -> String {
    let custom = conversation.custom_system_prompt.trim();
    match assistant {
        // 与 App GenerationLoop 同口径
        Some(conf) if conf.allow_conversation_system_prompt && !custom.is_empty() => {
            custom.to_string()
        }
        Some(conf) => conf.system_prompt.trim().to_string(),
        None => custom.to_string(),
    }
}

fn resolve_model(conversation: &Conversation, settings: &SettingsIndex) -> String {
    for message in &conversation.messages {
        if message.role.as_deref() != Some("assistant") {
            continue;
        }
        let Some(id) = message
            .model_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            continue;
        };
        return settings
            .model_labels
            .get(id)
            .cloned()
            .unwrap_or_else(|| id.to_string());
    }
    "Unknown".to_string()
}

fn render_message(markdown: &mut String, message: &RawMessage) {
    let mut thoughts: Vec<String> = Vec::new();
    let mut responses: Vec<String> = Vec::new();

    for part in &message.parts {
        match part.kind.as_str() {
            "reasoning" => {
                if let Some(text) = &part.reasoning
                    && !text.trim().is_empty()
                {
                    thoughts.push(text.clone());
                }
            }
            "text" => {
                if let Some(text) = &part.text
                    && !text.trim().is_empty()
                {
                    responses.push(text.clone());
                }
            }
            "image" => push_media(&mut responses, "image", part, None),
            "video" => push_media(&mut responses, "video", part, None),
            "audio" => push_media(&mut responses, "audio", part, None),
            "document" => push_media(
                &mut responses,
                "document",
                part,
                Some(part.file_name.as_deref().unwrap_or_default()),
            ),
            // 工具类 part 与未知类型：不进正文
            _ => {}
        }
    }

    if thoughts.is_empty() && responses.is_empty() {
        return;
    }

    let role = message.role.as_deref().unwrap_or_default();
    let header = match role {
        "user" => "### 🧑‍💻 User",
        "system" => "### ⚙️ System",
        _ => "### 🤖 Assistant",
    };
    markdown.push_str(header);
    markdown.push_str("\n\n");

    if !thoughts.is_empty() {
        markdown.push_str("#### 🤔 Thought Process\n\n");
        markdown.push_str(&strip_hashes(&thoughts.join("\n\n")));
        markdown.push_str("\n\n");
        if role != "user" && !responses.is_empty() {
            markdown.push_str("#### 💡 Response\n\n");
        }
    }

    if !responses.is_empty() {
        markdown.push_str(&strip_hashes(&responses.join("\n\n")));
        markdown.push_str("\n\n");
    }
}

fn push_media(out: &mut Vec<String>, kind: &str, part: &Part, file_name: Option<&str>) {
    let Some(url) = part.url.as_deref().map(str::trim).filter(|v| !v.is_empty()) else {
        return;
    };
    match kind {
        "document" => {
            let name = file_name
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .unwrap_or("document");
            out.push(format!("[{name}]({url})"));
        }
        _ => out.push(format!("![{kind}]({url})")),
    }
}

// ═══════════════════════════════════════════════════════════
//  文本处理（契约 §5）
// ═══════════════════════════════════════════════════════════

/// `^#{1,6}\s+(.+)$`（多行）→ `**$1**`：不保留井号标题，但保留强调。
///
/// 1. **代码围栏内不动**（``` / ~~~），否则会把 Python / Shell 的 `# 注释` 误改成加粗。
/// 2. **整条标题加粗**：标题内原有的 `**` 会被吸收，避免同级定界符交错（渲染出可见星号）。
///    但行内代码（`` ` ``）里的 `**` 不是强调（如 glob `**/*.js`），必须保留。
fn strip_hashes(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut fence: Option<&'static str> = None;

    for (index, line) in text.split('\n').enumerate() {
        if index > 0 {
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
    let hashes = line.bytes().take_while(|byte| *byte == b'#').count();
    if hashes == 0 || hashes > 6 {
        return Cow::Borrowed(line);
    }

    let rest = &line[hashes..];
    let trimmed = rest.trim_start_matches(char::is_whitespace);
    if trimmed.len() == rest.len() || trimmed.is_empty() {
        return Cow::Borrowed(line);
    }

    let merged = remove_bold_outside_code(trimmed);
    let inner = merged.trim();
    if inner.is_empty() {
        return Cow::Borrowed(line);
    }

    Cow::Owned(format!("**{inner}**"))
}

/// 去掉不在行内代码段里的 `**`。
///
/// 行内代码由反引号界定（CommonMark：N 个反引号开始、同样 N 个结束），其中的 `**` 属于代码内容。
fn remove_bold_outside_code(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut index = 0;
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

// ═══════════════════════════════════════════════════════════
//  命名 / 时间
// ═══════════════════════════════════════════════════════════

fn non_empty(value: &str) -> Option<&str> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

fn display_title(title: &str) -> String {
    let sanitized = sanitize_filename(title);
    if sanitized.is_empty() {
        DEFAULT_TITLE.to_string()
    } else {
        truncate_chars(&sanitized, ENTRY_TITLE_MAX)
    }
}

/// 删除 Windows 非法字符与换行（对齐 cherry：删除而非替换）。
fn sanitize_filename(name: &str) -> String {
    name.chars()
        .filter(|ch| {
            !matches!(
                ch,
                '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '\r' | '\n' | '\t'
            )
        })
        .collect::<String>()
        .trim()
        .to_string()
}

fn sanitize_path_component(name: &str, fallback: &str) -> String {
    let sanitized = sanitize_filename(name);
    if sanitized.is_empty() || sanitized == "." || sanitized == ".." {
        fallback.to_string()
    } else {
        sanitized
    }
}

fn truncate_chars(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}

fn format_local_time_ms(ms: i64) -> String {
    Local
        .timestamp_millis_opt(ms)
        .single()
        .map(|dt| dt.format("%Y-%m-%d %H:%M:%S %:z").to_string())
        .unwrap_or_else(|| "unknown".to_string())
}

fn format_local_compact_ms(ms: i64) -> String {
    Local
        .timestamp_millis_opt(ms)
        .single()
        .map(|dt| dt.format("%Y%m%d-%H%M%S").to_string())
        .unwrap_or_else(|| "00000000-000000".to_string())
}

/// epoch 秒 → zip DOS 时间（本地时间，2 秒精度）
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

// ═══════════════════════════════════════════════════════════
//  ZIP 打包
// ═══════════════════════════════════════════════════════════

fn is_zip_output(path: &Path) -> bool {
    path.extension()
        .and_then(|value| value.to_str())
        .is_some_and(|value| value.eq_ignore_ascii_case("zip"))
}

fn resolve_zip_target(input: &Path, output: Option<&Path>, zip_name: String) -> Result<PathBuf> {
    match output {
        Some(path) if is_zip_output(path) => Ok(path.to_path_buf()),
        Some(dir) => {
            fs::create_dir_all(dir)
                .with_context(|| format!("创建输出目录失败 {}", dir.display()))?;
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

/// 同一条目名重复时追加 `-2` / `-3`（扩展名保持在末尾）
fn unique_entry_name(name: &str, used: &mut HashSet<String>) -> String {
    if used.insert(name.to_string()) {
        return name.to_string();
    }

    let (stem, extension) = match name.rsplit_once('.') {
        Some((stem, extension)) => (stem.to_string(), format!(".{extension}")),
        None => (name.to_string(), String::new()),
    };

    let mut counter = 2;
    loop {
        let candidate = format!("{stem}-{counter}{extension}");
        if used.insert(candidate.clone()) {
            return candidate;
        }
        counter += 1;
    }
}

fn build_failure_markdown(source: &Path, failures: &[ConversationFailure]) -> String {
    let mut lines: Vec<String> = Vec::new();
    lines.push("# Export Failures".to_string());
    lines.push(String::new());
    lines.push("## Metadata".to_string());
    lines.push(String::new());
    lines.push(format!("- **Platform:** `{PLATFORM_ID}`"));
    lines.push(format!("- **Source:** `{}`", source.display()));
    lines.push(format!("- **Skipped:** {}", failures.len()));
    lines.push(String::new());
    lines.push("这些对话没有任何可导出的消息，因此未生成 Markdown。".to_string());
    lines.push(String::new());

    for (index, failure) in failures.iter().enumerate() {
        lines.push(format!("## {}. {}", index + 1, failure.title));
        lines.push(String::new());
        lines.push(format!("- **Conversation ID:** `{}`", failure.id));
        lines.push(format!("- **Reason:** {}", failure.reason));
        lines.push(String::new());
    }

    lines.join("\n")
}

fn write_zip_export(
    source: &Path,
    conversations: &[RenderedConversation],
    failures: &[ConversationFailure],
    zip_path: &Path,
) -> Result<()> {
    let file =
        File::create(zip_path).with_context(|| format!("创建压缩包失败 {}", zip_path.display()))?;
    let mut zip = ZipWriter::new(file);
    let base = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);

    for conversation in conversations {
        let options = match conversation.epoch_secs.and_then(zip_datetime) {
            Some(datetime) => base.last_modified_time(datetime),
            None => base,
        };
        zip.start_file(conversation.entry_name.clone(), options)
            .with_context(|| format!("写入 {} 失败", conversation.entry_name))?;
        zip.write_all(conversation.markdown.as_bytes())
            .with_context(|| format!("写入 {} 失败", conversation.entry_name))?;
    }

    if !failures.is_empty() {
        let report = build_failure_markdown(source, failures);
        // 失败报告是「刚生成的」，用当前时间（不设的话会退成 1980-01-01）
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

// ═══════════════════════════════════════════════════════════
//  测试
// ═══════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    fn message(value: serde_json::Value) -> RawMessage {
        serde_json::from_value(value).expect("message should parse")
    }

    #[test]
    fn headings_become_bold() {
        assert_eq!(strip_hashes("# Title"), "**Title**");
        assert_eq!(strip_hashes("### Deep"), "**Deep**");
        assert_eq!(strip_hashes("# 1. **重点**"), "**1. 重点**");
        assert_eq!(strip_hashes("####### too many"), "####### too many");
        assert_eq!(strip_hashes("#nospace"), "#nospace");
        assert_eq!(strip_hashes("# "), "# ");
        assert_eq!(strip_hashes("a *b* c"), "a *b* c");
        assert_eq!(strip_hashes("# a *b* c"), "**a *b* c**");
        assert_eq!(strip_hashes("plain\ntext"), "plain\ntext");
        assert_eq!(strip_hashes("a\n## b\nc"), "a\n**b**\nc");
    }

    /// 整条标题必须落在一个加粗里：外层一对 `**` 之内不得再出现 `**`。
    #[test]
    fn heading_is_bold_as_a_whole() {
        for (input, expected) in [
            ("# **Bold**", "**Bold**"),
            ("# 1. **A** 2. **B**", "**1. A 2. B**"),
            ("# 🌅 **早晨**", "**🌅 早晨**"),
            (
                "## 方案一：**“水珠”——像吃水果**",
                "**方案一：“水珠”——像吃水果**",
            ),
        ] {
            let out = strip_hashes(input);
            assert_eq!(out, expected, "input: {input}");
            let inner = &out[2..out.len() - 2];
            assert!(!inner.contains("**"), "内层仍有 **（未整条加粗）: {out}");
        }
    }

    #[test]
    fn heading_keeps_bold_inside_inline_code() {
        assert_eq!(
            strip_hashes("# 匹配 `**/*.js` 的路径"),
            "**匹配 `**/*.js` 的路径**"
        );
        assert_eq!(strip_hashes("# a ``**x**`` b"), "**a ``**x**`` b**");
    }

    #[test]
    fn strip_hashes_preserves_code() {
        let fenced = "```python\n# 注释\n## another\n```";
        assert_eq!(strip_hashes(fenced), fenced);
        let tildes = "~~~\n# 注释\n~~~";
        assert_eq!(strip_hashes(tildes), tildes);
        let indented = "    # 缩进四格";
        assert_eq!(strip_hashes(indented), indented);
        let mixed = "# Title\n```\n# inside\n```\n## Real";
        assert_eq!(
            strip_hashes(mixed),
            "**Title**\n```\n# inside\n```\n**Real**"
        );
    }

    #[test]
    fn sanitize_matches_cherry_rules() {
        assert_eq!(sanitize_filename("a/b:c*d?e\"f<g>h|i"), "abcdefghi");
        assert_eq!(sanitize_filename("  spaced  "), "spaced");
        assert_eq!(sanitize_filename("a\nb\tc"), "abc");
        assert_eq!(sanitize_filename(""), "");
        assert_eq!(sanitize_path_component("", DEFAULT_ASSISTANT), "Assistant");
        assert_eq!(
            sanitize_path_component("..", DEFAULT_ASSISTANT),
            "Assistant"
        );
        assert_eq!(
            truncate_chars(&"字".repeat(100), ENTRY_TITLE_MAX)
                .chars()
                .count(),
            80
        );
    }

    #[test]
    fn settings_resolves_model_and_system() {
        let raw = r#"{
            "providers": [{ "models": [
                { "id": "m1", "modelId": "gemini-3-flash", "displayName": "Gemini 3 Flash" },
                { "id": "m2", "modelId": "deepseek-v4" }
            ]}],
            "assistants": [
                { "id": "a1", "name": "Gemini", "systemPrompt": "你是助手",
                  "allowConversationSystemPrompt": false },
                { "id": "a2", "name": "Override", "systemPrompt": "默认",
                  "allowConversationSystemPrompt": true }
            ]
        }"#
        .as_bytes();
        let index = SettingsIndex::parse(Some(raw));
        assert_eq!(
            index.model_labels.get("m1").map(String::as_str),
            Some("Gemini 3 Flash")
        );
        assert_eq!(
            index.model_labels.get("m2").map(String::as_str),
            Some("deepseek-v4")
        );

        let mut conversation = Conversation {
            id: "c".into(),
            assistant_id: "a1".into(),
            title: "T".into(),
            create_at_ms: Some(1_700_000_000_000),
            custom_system_prompt: "对话覆盖".into(),
            messages: vec![message(serde_json::json!({
                "role": "assistant", "modelId": "m1", "parts": [{ "type": "text", "text": "hi" }]
            }))],
        };
        let assistant = index.assistants.get("a1");
        assert_eq!(
            effective_system_prompt(&conversation, assistant),
            "你是助手"
        );
        assert_eq!(resolve_model(&conversation, &index), "Gemini 3 Flash");

        conversation.assistant_id = "a2".into();
        let assistant = index.assistants.get("a2");
        assert_eq!(
            effective_system_prompt(&conversation, assistant),
            "对话覆盖"
        );

        conversation.messages[0].model_id = Some("m-unknown".into());
        assert_eq!(resolve_model(&conversation, &index), "m-unknown");
    }

    #[test]
    fn pick_message_uses_select_index_and_clamps() {
        let list = || {
            vec![
                message(serde_json::json!({ "role": "user", "parts": [] })),
                message(serde_json::json!({ "role": "assistant", "parts": [] })),
            ]
        };
        assert_eq!(
            pick_message(list(), 1).unwrap().role.as_deref(),
            Some("assistant")
        );
        assert_eq!(
            pick_message(list(), 5).unwrap().role.as_deref(),
            Some("assistant")
        );
        assert_eq!(
            pick_message(list(), -3).unwrap().role.as_deref(),
            Some("user")
        );
        assert!(pick_message(Vec::new(), 0).is_none());
    }

    #[test]
    fn render_splits_thought_and_response() {
        let conversation = Conversation {
            id: "conv-1".into(),
            assistant_id: "a1".into(),
            title: "Demo".into(),
            create_at_ms: Some(1_700_000_000_000),
            custom_system_prompt: String::new(),
            messages: vec![
                message(serde_json::json!({
                    "role": "user",
                    "parts": [{ "type": "text", "text": "# 标题\n正文" }]
                })),
                message(serde_json::json!({
                    "role": "assistant", "modelId": "m1",
                    "parts": [
                        { "type": "reasoning", "reasoning": "思考中" },
                        { "type": "text", "text": "回答" },
                        { "type": "image", "url": "file:///data/a.png" }
                    ]
                })),
            ],
        };
        let settings = SettingsIndex::empty();
        let Outcome::Rendered(rendered) = render_conversation(&conversation, &settings) else {
            panic!("expected rendered");
        };

        assert!(rendered.markdown.starts_with("## Metadata\n\n"));
        assert!(rendered.markdown.contains("- **Model:** `m1`"));
        assert!(
            rendered
                .markdown
                .contains("- **Conversation ID:** `conv-1`")
        );
        assert!(rendered.markdown.contains("- **Assistant:** `Assistant`"));
        assert!(rendered.markdown.contains("### 🧑‍💻 User"));
        assert!(rendered.markdown.contains("**标题**\n正文"));
        assert!(
            rendered
                .markdown
                .contains("#### 🤔 Thought Process\n\n思考中")
        );
        assert!(rendered.markdown.contains("#### 💡 Response\n\n回答"));
        assert!(rendered.markdown.contains("![image](file:///data/a.png)"));
        assert!(rendered.entry_name.ends_with("-Demo.md"));
    }

    #[test]
    fn render_skips_empty_and_reasoning_only_has_no_response_header() {
        let conversation = Conversation {
            id: "conv-2".into(),
            assistant_id: String::new(),
            title: String::new(),
            create_at_ms: None,
            custom_system_prompt: String::new(),
            messages: vec![
                message(
                    serde_json::json!({ "role": "user", "parts": [{ "type": "text", "text": "  " }] }),
                ),
                message(serde_json::json!({
                    "role": "assistant",
                    "parts": [{ "type": "reasoning", "reasoning": "只有思考" }]
                })),
            ],
        };
        let Outcome::Rendered(rendered) =
            render_conversation(&conversation, &SettingsIndex::empty())
        else {
            panic!("expected rendered");
        };
        assert!(!rendered.markdown.contains("### 🧑‍💻 User"));
        assert!(rendered.markdown.contains("#### 🤔 Thought Process"));
        assert!(!rendered.markdown.contains("#### 💡 Response"));
        assert!(
            rendered
                .entry_name
                .contains("/00000000-000000-Untitled_Conversation.md")
        );
    }

    #[test]
    fn resolve_model_falls_back_for_missing_model_id() {
        let conversation = Conversation {
            id: "c".into(),
            assistant_id: String::new(),
            title: "t".into(),
            create_at_ms: Some(1),
            custom_system_prompt: String::new(),
            messages: vec![message(serde_json::json!({
                "role": "user", "parts": [{ "type": "text", "text": "hi" }]
            }))],
        };
        assert_eq!(
            resolve_model(&conversation, &SettingsIndex::empty()),
            "Unknown"
        );
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
    }

    /// 老版本（DB v17）的 ConversationEntity 没有 custom_system_prompt 等列，不能因此报错。
    #[test]
    fn loads_legacy_conversation_schema() {
        let conn = Connection::open_in_memory().expect("in-memory db");
        conn.execute_batch(
            "CREATE TABLE ConversationEntity (
                 id TEXT PRIMARY KEY, assistant_id TEXT NOT NULL DEFAULT '',
                 title TEXT NOT NULL, nodes TEXT NOT NULL DEFAULT '[]',
                 create_at INTEGER NOT NULL, update_at INTEGER NOT NULL);
             CREATE TABLE message_node (
                 id TEXT PRIMARY KEY, conversation_id TEXT NOT NULL, node_index INTEGER NOT NULL,
                 messages TEXT NOT NULL, select_index INTEGER NOT NULL);",
        )
        .expect("schema");
        conn.execute(
            "INSERT INTO ConversationEntity (id, assistant_id, title, nodes, create_at, update_at)
             VALUES ('c1', 'a1', 'Legacy', '[]', 1700000000000, 1700000000000)",
            [],
        )
        .expect("insert conversation");
        conn.execute(
            "INSERT INTO message_node (id, conversation_id, node_index, messages, select_index)
             VALUES ('n1', 'c1', 0, ?1, 0)",
            [r#"[{"role":"user","parts":[{"type":"text","text":"hi"}]}]"#],
        )
        .expect("insert node");

        let conversations = load_conversations(&conn).expect("load");
        assert_eq!(conversations.len(), 1);
        assert_eq!(conversations[0].messages.len(), 1);
        assert!(conversations[0].custom_system_prompt.is_empty());
    }

    /// 极老版本没有 message_node 表，应回退解析 `ConversationEntity.nodes` JSON。
    #[test]
    fn falls_back_to_legacy_nodes_json() {
        let conn = Connection::open_in_memory().expect("in-memory db");
        conn.execute_batch(
            "CREATE TABLE ConversationEntity (
                 id TEXT PRIMARY KEY, assistant_id TEXT NOT NULL DEFAULT '',
                 title TEXT NOT NULL, nodes TEXT NOT NULL, create_at INTEGER NOT NULL);",
        )
        .expect("schema");
        conn.execute(
            "INSERT INTO ConversationEntity (id, assistant_id, title, nodes, create_at)
             VALUES ('c1', 'a1', 'Old', ?1, 1700000000000)",
            [r#"[{"messages":[{"role":"user","parts":[{"type":"text","text":"legacy"}]}],"selectIndex":0}]"#],
        )
        .expect("insert conversation");

        let conversations = load_conversations(&conn).expect("load");
        assert_eq!(conversations.len(), 1);
        assert_eq!(conversations[0].messages.len(), 1);
        assert_eq!(
            conversations[0].messages[0].parts[0].text.as_deref(),
            Some("legacy")
        );
    }
}
