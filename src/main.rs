use anyhow::{Context, Result};
use clap::Parser;
use rayon::prelude::*;
use regex::Regex;
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{BufReader, Write};
use std::path::{Path, PathBuf};

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Input JSON file
    #[arg(required = true)]
    input_file: PathBuf,

    /// Output directory
    #[arg(short = 'o', long = "output")]
    output_dir: Option<PathBuf>,
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
    #[serde(default, rename = "createdAt")]
    created_at: Option<String>,
}

struct AssistantInfo {
    name: String,
    prompt: String,
}

struct TopicMetadata {
    name: String,
    assistant_id: String,
    created_at: Option<String>,
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

fn main() -> Result<()> {
    let args = Args::parse();
    let input_path = args.input_file;

    if !input_path.exists() {
        println!("找不到文件: {}", input_path.display());
        return Ok(());
    }

    let output_dir = if let Some(dir) = args.output_dir {
        dir
    } else {
        let parent = input_path.parent().unwrap_or_else(|| Path::new("."));
        parent.join("cherry-studio-export")
    };

    if !output_dir.exists() {
        fs::create_dir_all(&output_dir)?;
    }

    let file = File::open(&input_path).context(format!("Failed to open {}", input_path.display()))?;
    let reader = BufReader::new(file);
    
    let root: Root = serde_json::from_reader(reader).context("Failed to parse JSON")?;

    // 1. Parse assistants
    let mut assistants_map: HashMap<String, AssistantInfo> = HashMap::new();
    let mut topic_metadata_map: HashMap<String, TopicMetadata> = HashMap::new();

    if let Some(persist_str) = root.local_storage.get("persist:cherry-studio") {
        if let Ok(persist_data) = serde_json::from_str::<PersistData>(persist_str) {
            if let Ok(assistants_store) = serde_json::from_str::<AssistantsStore>(&persist_data.assistants) {
                let mut all_assistants = Vec::new();
                if let Some(da) = assistants_store.default_assistant {
                    all_assistants.push(da);
                }
                all_assistants.extend(assistants_store.assistants);

                for assistant in all_assistants {
                    assistants_map.insert(assistant.id.clone(), AssistantInfo {
                        name: assistant.name.clone(),
                        prompt: assistant.prompt.clone(),
                    });

                    for t in assistant.topics {
                        topic_metadata_map.insert(t.id.clone(), TopicMetadata {
                            name: t.name.unwrap_or_else(|| "Untitled".to_string()),
                            assistant_id: assistant.id.clone(),
                            created_at: t.created_at,
                        });
                    }
                }
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

    // 3. Process topics
    let topics = &root.indexed_db.topics;
    
    if topics.is_empty() {
        println!("未找到任何对话主题 (topics)");
        return Ok(());
    }

    println!("找到 {} 个对话主题，开始转换...", topics.len());

    // Use Rayon for parallel processing
    topics.par_iter().for_each(|topic| {
        let topic_id = &topic.id;
        
        // Metadata
        let meta = topic_metadata_map.get(topic_id);
        let topic_name = meta.map(|m| m.name.as_str()).unwrap_or("Untitled");
        let assistant_id = meta.map(|m| m.assistant_id.as_str()).unwrap_or("default");
        
        let created_at_str = if let Some(m) = meta {
            if let Some(c) = &m.created_at {
                c.clone()
            } else {
                match &topic.created_at {
                    Some(Value::String(s)) => s.clone(),
                    Some(Value::Number(n)) => n.to_string(),
                    _ => "Unknown".to_string(),
                }
            }
        } else {
             match &topic.created_at {
                Some(Value::String(s)) => s.clone(),
                Some(Value::Number(n)) => n.to_string(),
                _ => "Unknown".to_string(),
            }
        };

        // Assistant info
        let assistant_info = assistants_map.get(assistant_id);
        let assistant_name = assistant_info.map(|a| a.name.as_str()).unwrap_or("Assistant");
        let system_instruction = assistant_info.map(|a| a.prompt.as_str()).unwrap_or("");

        // Messages
        let mut topic_messages = topic.messages.iter().collect::<Vec<_>>();
        if topic_messages.is_empty() {
            return;
        }

        // Sort messages
        topic_messages.sort_by(|a, b| {
            let ta = get_created_at_f64(&a.created_at);
            let tb = get_created_at_f64(&b.created_at);
            ta.partial_cmp(&tb).unwrap_or(std::cmp::Ordering::Equal)
        });

        // Build Markdown
        let mut safe_name = sanitize_filename(topic_name);
        if safe_name.trim().is_empty() {
            safe_name = "Untitled_Conversation".to_string();
        }

        let mut md_content = String::new();
        md_content.push_str(&format!("Conversation Transcript: {}\n\n", safe_name));
        
        md_content.push_str("## Metadata\n\n");
        md_content.push_str("### Run Settings\n\n");

        // First model
        let mut first_model = "Unknown".to_string();
        for m in &topic_messages {
            if m.role == "assistant" {
                if let Some(model_info) = &m.model {
                    match model_info {
                        Value::Object(map) => {
                            if let Some(id) = map.get("id") {
                                if let Some(s) = id.as_str() {
                                    first_model = s.to_string();
                                }
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
        }

        md_content.push_str(&format!("- **Topic ID:** `{}`\n", topic_id));
        md_content.push_str(&format!("- **Assistant:** `{}`\n", assistant_name));
        md_content.push_str(&format!("- **Created At:** `{}`\n", created_at_str));
        md_content.push_str(&format!("- **Model:** `{}`\n\n", first_model));

        if !system_instruction.is_empty() {
            md_content.push_str("### System Instruction\n\n");
            md_content.push_str(system_instruction);
            md_content.push_str("\n\n");
        }

        md_content.push_str("## Conversation\n\n");

        for msg in &topic_messages {
            let role = &msg.role;
            if role == "user" {
                md_content.push_str("### 🧑‍💻 User\n\n");
            } else {
                md_content.push_str("### 🤖 Assistant\n\n");
            }

            let mut msg_blocks_content: Vec<&MessageBlock> = Vec::new();
            for bid in &msg.blocks {
                if let Some(block) = blocks_map.get(bid) {
                    msg_blocks_content.push(block);
                }
            }

            if msg_blocks_content.is_empty() {
                if let Some(blocks) = blocks_by_message_id.get(&msg.id) {
                    msg_blocks_content.extend(blocks);
                }
            }

            // Sort blocks
            msg_blocks_content.sort_by(|a, b| {
                let ta = get_created_at_f64(&a.created_at);
                let tb = get_created_at_f64(&b.created_at);
                ta.partial_cmp(&tb).unwrap_or(std::cmp::Ordering::Equal)
            });
            
            let has_thought = msg_blocks_content.iter().any(|b| b.type_ == "thinking");

            for b in msg_blocks_content {
                if b.content.is_empty() {
                    continue;
                }

                if b.type_ == "thinking" {
                    md_content.push_str(&format!("#### 🤔 Thought Process\n{}\n", b.content));
                } else {
                     if has_thought && role != "user" {
                         md_content.push_str(&format!("#### 💡 Response{}\n\n", b.content));
                     } else {
                         md_content.push_str(&format!("{}\n\n", b.content));
                     }
                }
            }
        }

        let safe_assistant_name = sanitize_path_component(assistant_name, "Assistant");
        let assistant_dir = output_dir.join(&safe_assistant_name);
        if let Err(e) = fs::create_dir_all(&assistant_dir) {
            println!("创建助手目录失败 {:?}: {}", assistant_dir, e);
            return;
        }

        // 处理同名文件：添加时间戳后缀
        let mut file_path = assistant_dir.join(format!("{}.md", safe_name));
        let mut counter = 1;
        while file_path.exists() {
            let timestamp_suffix = if created_at_str != "Unknown" {
                created_at_str.chars().take(19).collect::<String>().replace(['T', ':'], "-")
            } else {
                format!("copy{}", counter)
            };
            let new_filename = format!("{}_{}.md", safe_name, timestamp_suffix);
            file_path = assistant_dir.join(&new_filename);
            
            // 如果加了时间戳还是同名，继续添加序号
            if file_path.exists() {
                let numbered_filename = format!("{}_{}-{}.md", safe_name, timestamp_suffix, counter);
                file_path = assistant_dir.join(&numbered_filename);
                counter += 1;
            } else {
                break;
            }
        }
        
        if let Ok(mut f) = File::create(&file_path) {
            if let Err(e) = f.write_all(md_content.as_bytes()) {
                println!("写入文件失败 {:?}: {}", file_path, e);
            } else {
                 // Update file time
                 if created_at_str != "Unknown" {
                     if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(&created_at_str.replace("Z", "+00:00")) {
                         let timestamp = dt.timestamp();
                         if timestamp > 0 {
                             let filetime = filetime::FileTime::from_unix_time(timestamp, 0);
                             let _ = filetime::set_file_times(&file_path, filetime, filetime);
                             println!("已生成: {} (时间已重置为 {})", safe_name, created_at_str);
                         } else {
                             println!("已生成: {} (时间无效)", safe_name);
                         }
                     } else {
                         println!("已生成: {} (时间解析失败)", safe_name);
                     }
                 } else {
                     println!("已生成: {}", safe_name);
                 }
            }
        } else {
             println!("创建文件失败: {:?}", file_path);
        }
    });

    Ok(())
}
