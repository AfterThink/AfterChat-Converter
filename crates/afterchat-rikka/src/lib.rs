//! 把 RikkaHub 的备份（内含 SQLite 数据库）转换为 AfterChat 对话 Markdown / ZIP。
//!
//! 输出契约见仓库 `docs/CHATFORMAT.md`。
//! 实现思路对齐 `cherry-studio-backup-json-converter`：按助手分目录、附加 Metadata 键、兜底命名。

use std::collections::HashMap;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use chatformat::{
    Conversation as ChatConversation, ExportFailure, Message as ChatMessage, MetadataLine,
    NameStyle, Role, ZipExport, default_zip_name,
};
use rayon::prelude::*;
use rusqlite::Connection;
use serde::Deserialize;
use tempfile::TempDir;
use zip::ZipArchive;

/// 失败报告里的平台标识
const PLATFORM_ID: &str = "rikka";

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

    let mut conversations: Vec<ChatConversation> = Vec::new();
    let mut failures: Vec<ExportFailure> = Vec::new();
    for outcome in outcomes {
        match outcome {
            Outcome::Rendered(conversation) => conversations.push(*conversation),
            Outcome::Failed(failure) => failures.push(ExportFailure {
                title: failure.title,
                id: failure.id,
                reason: failure.reason,
            }),
        }
    }

    let target = resolve_zip_target(
        input,
        options.output.as_deref(),
        default_zip_name(PLATFORM_ID),
    )?;
    if let Some(parent) = target.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)
            .with_context(|| format!("创建输出目录失败 {}", parent.display()))?;
    }

    chatformat::write_zip(&ZipExport {
        platform: PLATFORM_ID,
        conversations: &conversations,
        failures: &failures,
        output: &target,
        source: Some(input),
        name_style: NameStyle::Spec,
        show_progress: true,
    })?;

    Ok(RunSummary {
        output: target,
        exported: conversations.len(),
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

struct ConversationFailure {
    id: String,
    title: String,
    reason: String,
}

enum Outcome {
    Rendered(Box<ChatConversation>),
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
    let assistant_name =
        non_empty(&assistant.map(|a| a.name.as_str()).unwrap_or_default()).unwrap_or("Assistant");
    let system_prompt = effective_system_prompt(conversation, assistant);
    let model = resolve_model(conversation, settings);

    let mut messages: Vec<ChatMessage> = Vec::new();
    // 契约 §3：系统提示是对话的第一条消息
    if !system_prompt.trim().is_empty() {
        messages.push(ChatMessage::system(system_prompt.trim()));
    }
    for message in &conversation.messages {
        if let Some(rendered) = render_message(message) {
            messages.push(rendered);
        }
    }

    Outcome::Rendered(Box::new(ChatConversation {
        title: conversation.title.clone(),
        model,
        time_secs: conversation.create_at_ms.map(|ms| ms / 1000),
        sort_ms: conversation.create_at_ms,
        url: None,
        extra: vec![
            MetadataLine::code("Conversation ID", conversation.id.clone()),
            MetadataLine::code("Assistant", assistant_name),
        ],
        group: Some(assistant_name.to_string()),
        id: Some(conversation.id.clone()),
        messages,
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

fn render_message(message: &RawMessage) -> Option<ChatMessage> {
    let mut thinking: Vec<String> = Vec::new();
    let mut body: Vec<String> = Vec::new();

    for part in &message.parts {
        match part.kind.as_str() {
            "reasoning" => {
                if let Some(text) = &part.reasoning
                    && !text.trim().is_empty()
                {
                    thinking.push(text.clone());
                }
            }
            "text" => {
                if let Some(text) = &part.text
                    && !text.trim().is_empty()
                {
                    body.push(text.clone());
                }
            }
            "image" => push_media(&mut body, "image", part, None),
            "video" => push_media(&mut body, "video", part, None),
            "audio" => push_media(&mut body, "audio", part, None),
            "document" => push_media(
                &mut body,
                "document",
                part,
                Some(part.file_name.as_deref().unwrap_or_default()),
            ),
            // 工具类 part 与未知类型：不进正文
            _ => {}
        }
    }

    if thinking.is_empty() && body.is_empty() {
        return None;
    }

    // 无法识别的角色按契约 §4.2 兜底成 Assistant
    let role = match message.role.as_deref() {
        Some("user") => Role::User,
        Some("system") => Role::System,
        _ => Role::Assistant,
    };
    Some(ChatMessage {
        role,
        thinking,
        body,
    })
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
    chatformat::sanitize_filename_with(title, 100, "Untitled_Conversation")
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
        let markdown = rendered.render();

        assert!(markdown.starts_with("## Metadata\n\n"));
        assert!(markdown.contains("- **Model:** `m1`"));
        assert!(markdown.contains("- **Conversation ID:** `conv-1`"));
        assert!(markdown.contains("- **Assistant:** `Assistant`"));
        assert!(markdown.contains("### 🧑‍💻 User"));
        assert!(markdown.contains("**标题**\n正文"));
        assert!(markdown.contains("#### 🤔 Thought Process\n\n思考中"));
        assert!(markdown.contains("#### 💡 Response\n\n回答"));
        assert!(markdown.contains("![image](file:///data/a.png)"));
        assert_eq!(rendered.group.as_deref(), Some("Assistant"));
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
        let markdown = rendered.render();
        assert!(!markdown.contains("### 🧑‍💻 User"));
        assert!(markdown.contains("#### 🤔 Thought Process"));
        assert!(!markdown.contains("#### 💡 Response"));
        assert!(!markdown.contains("- **Time:**"));
        // 空标题由 chatformat 兜底成 Untitled_Conversation
        assert_eq!(
            chatformat::sanitize_filename_with(&rendered.title, 100, "Untitled_Conversation"),
            "Untitled_Conversation"
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
