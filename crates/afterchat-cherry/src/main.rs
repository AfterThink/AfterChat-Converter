use anyhow::{Context, Result, bail};
use chatformat::Message as ChatMessage;
use chatformat::{
    Conversation, ExportFailure, NameStyle, Role, UNKNOWN_MODEL, ZipExport, default_zip_name, time,
};
use clap::Parser;
use rayon::prelude::*;
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use zip::ZipArchive;

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

/// 解析 cherry 的 `createdAt`（RFC3339 字符串 / epoch 秒或毫秒数字）→ epoch 秒
fn parse_created_at_secs(value: &Value) -> Option<i64> {
    time::value_to_secs_any(Some(value))
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

/// 输出 zip 名前缀，遵循 docs/CHATFORMAT.md §6：`chat-export-{platform}-all-{timestamp}.zip`
const PLATFORM_ID: &str = "cherry";

struct RenderContext<'a> {
    assistants: &'a HashMap<String, AssistantInfo>,
    topic_metadata: &'a HashMap<String, TopicMetadata>,
    blocks: &'a HashMap<&'a String, &'a MessageBlock>,
    blocks_by_message_id: &'a HashMap<&'a String, Vec<&'a MessageBlock>>,
}

/// 被跳过的主题（写进 zip 内的 `export-failures.md`）
struct TopicFailure {
    id: String,
    title: String,
    reason: String,
}

enum TopicOutcome {
    Rendered(Box<Conversation>),
    Skipped(TopicFailure),
}

/// 把单个主题映射成 `chatformat::Conversation`（渲染 / 命名 / 打包交给公共库）
fn render_topic(ctx: &RenderContext<'_>, topic: &Topic) -> TopicOutcome {
    let topic_id = topic.id.as_str();
    let meta = ctx.topic_metadata.get(topic_id);
    let topic_name = meta
        .map(|m| m.name.clone())
        .unwrap_or_else(|| "Untitled".to_string());
    let assistant_id = meta.map(|m| m.assistant_id.as_str()).unwrap_or("default");

    // 元数据里的时间优先，其次用 topic 自身的；两者都可能是字符串或数字
    let time_secs = meta
        .and_then(|m| m.created_at.as_ref())
        .and_then(parse_created_at_secs)
        .or_else(|| topic.created_at.as_ref().and_then(parse_created_at_secs));

    let assistant_info = ctx.assistants.get(assistant_id);
    let assistant_name = assistant_info.map(|a| a.name.as_str()).unwrap_or("Assistant");
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

    // 第一条带 model 的助手消息 → Model（契约 §2 必须有 Model 键）
    let mut first_model = UNKNOWN_MODEL.to_string();
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
            if first_model != UNKNOWN_MODEL {
                break;
            }
        }
    }

    let mut messages: Vec<ChatMessage> = Vec::new();
    // 契约 §3：系统提示是对话的第一条消息
    if !system_instruction.trim().is_empty() {
        messages.push(ChatMessage::system(system_instruction));
    }

    for msg in &topic_messages {
        let mut blocks: Vec<&MessageBlock> = Vec::new();
        for bid in &msg.blocks {
            if let Some(block) = ctx.blocks.get(bid) {
                blocks.push(block);
            }
        }
        if blocks.is_empty()
            && let Some(by_message) = ctx.blocks_by_message_id.get(&msg.id)
        {
            blocks.extend(by_message.iter().copied());
        }
        blocks.sort_by(|a, b| {
            let ta = get_created_at_f64(&a.created_at);
            let tb = get_created_at_f64(&b.created_at);
            ta.partial_cmp(&tb).unwrap_or(std::cmp::Ordering::Equal)
        });

        // 契约 §3：助手消息拆成「思考」与「回复」；其它块类型算正文
        let mut thinking = Vec::new();
        let mut body = Vec::new();
        for block in blocks {
            let content = block.content.trim();
            if content.is_empty() {
                continue;
            }
            if block.type_ == "thinking" {
                thinking.push(content.to_string());
            } else {
                body.push(content.to_string());
            }
        }
        if thinking.is_empty() && body.is_empty() {
            continue;
        }

        let role = match msg.role.as_str() {
            "user" => Role::User,
            "system" => Role::System,
            _ => Role::Assistant,
        };
        messages.push(ChatMessage {
            role,
            thinking,
            body,
        });
    }

    TopicOutcome::Rendered(Box::new(Conversation {
        title: topic_name,
        model: first_model,
        time_secs,
        sort_ms: time_secs.map(|secs| secs * 1000),
        url: None,
        extra: vec![
            chatformat::MetadataLine::code("Topic ID", topic_id),
            chatformat::MetadataLine::code("Assistant", assistant_name),
        ],
        group: Some(assistant_name.to_string()),
        id: Some(topic_id.to_string()),
        messages,
    }))
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

    let mut conversations: Vec<Conversation> = Vec::new();
    let mut failures: Vec<ExportFailure> = Vec::new();
    for outcome in outcomes {
        match outcome {
            TopicOutcome::Rendered(conversation) => conversations.push(*conversation),
            TopicOutcome::Skipped(failure) => failures.push(ExportFailure {
                title: failure.title,
                id: failure.id,
                reason: failure.reason,
            }),
        }
    }

    let target = resolve_zip_target(
        &input_path,
        args.output.as_deref(),
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
        source: Some(&input_path),
        name_style: NameStyle::Spec,
        show_progress: false,
    })?;

    println!("已打包 {} 个对话 → {}", conversations.len(), target.display());
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
    fn created_at_parses_rfc3339_and_epoch() {
        let iso = parse_created_at_secs(&serde_json::json!("2024-03-28T13:31:51.887Z"));
        assert_eq!(iso, Some(1711632711));

        let secs = parse_created_at_secs(&serde_json::json!(1761203267));
        let millis = parse_created_at_secs(&serde_json::json!(1761203267000i64));
        assert_eq!(secs, Some(1761203267));
        assert_eq!(millis, Some(1761203267));

        assert_eq!(parse_created_at_secs(&serde_json::json!(null)), None);
        assert_eq!(parse_created_at_secs(&serde_json::json!("not a date")), None);
    }
}
