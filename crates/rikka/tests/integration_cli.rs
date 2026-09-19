use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

use rusqlite::Connection;
use tempfile::tempdir;
use zip::CompressionMethod;
use zip::write::{SimpleFileOptions, ZipWriter};

const SETTINGS: &str = r#"{
  "providers": [
    { "models": [
      { "id": "m1", "modelId": "gemini-3-flash", "displayName": "Gemini 3 Flash" }
    ]}
  ],
  "assistants": [
    { "id": "a1", "name": "Gemini", "systemPrompt": "你是助手",
      "allowConversationSystemPrompt": false }
  ]
}"#;

struct DbConversation {
    id: String,
    assistant_id: String,
    title: String,
    create_at: i64,
    custom_system_prompt: String,
    /// (messages JSON, select_index)，按 node_index 顺序
    nodes: Vec<(String, i64)>,
}

fn create_schema(conn: &Connection) {
    conn.execute_batch(
        "CREATE TABLE ConversationEntity (
            id TEXT PRIMARY KEY,
            assistant_id TEXT NOT NULL DEFAULT '',
            title TEXT NOT NULL,
            nodes TEXT NOT NULL DEFAULT '[]',
            create_at INTEGER NOT NULL,
            update_at INTEGER NOT NULL,
            custom_system_prompt TEXT NOT NULL DEFAULT ''
        );
        CREATE TABLE message_node (
            id TEXT PRIMARY KEY,
            conversation_id TEXT NOT NULL,
            node_index INTEGER NOT NULL,
            messages TEXT NOT NULL,
            select_index INTEGER NOT NULL
        );",
    )
    .expect("create schema");
}

fn insert_conversation(conn: &Connection, conversation: &DbConversation) {
    conn.execute(
        "INSERT INTO ConversationEntity
            (id, assistant_id, title, nodes, create_at, update_at, custom_system_prompt)
         VALUES (?1, ?2, ?3, '[]', ?4, ?4, ?5)",
        rusqlite::params![
            conversation.id,
            conversation.assistant_id,
            conversation.title,
            conversation.create_at,
            conversation.custom_system_prompt,
        ],
    )
    .expect("insert conversation");

    for (index, (messages, select_index)) in conversation.nodes.iter().enumerate() {
        // 真实库里 `messages` 是「同一位置的候选消息数组」（List<UIMessage>）
        let messages_array = format!("[{messages}]");
        conn.execute(
            "INSERT INTO message_node (id, conversation_id, node_index, messages, select_index)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![
                format!("{}-node-{index}", conversation.id),
                conversation.id,
                index as i64,
                messages_array,
                select_index,
            ],
        )
        .expect("insert node");
    }
}

fn user_message(text: &str) -> String {
    serde_json::json!({ "role": "user", "parts": [{ "type": "text", "text": text }] }).to_string()
}

fn assistant_message(text: &str, reasoning: Option<&str>, model_id: &str) -> String {
    let mut parts = Vec::new();
    if let Some(reasoning) = reasoning {
        parts.push(serde_json::json!({ "type": "reasoning", "reasoning": reasoning }));
    }
    parts.push(serde_json::json!({ "type": "text", "text": text }));
    serde_json::json!({ "role": "assistant", "modelId": model_id, "parts": parts }).to_string()
}

fn build_zip(zip_path: &Path, entries: &[(String, Vec<u8>)]) {
    let file = File::create(zip_path).expect("create zip");
    let mut zip = ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    for (name, bytes) in entries {
        zip.start_file(name.clone(), options).expect("start file");
        zip.write_all(bytes).expect("write entry");
    }
    zip.finish().expect("finish zip");
}

/// 普通（非 WAL）备份：合成 db + settings.json
fn make_plain_backup(dir: &Path, conversations: &[DbConversation]) -> PathBuf {
    let db_path = dir.join("rikka_hub.db");
    {
        let conn = Connection::open(&db_path).expect("open db");
        create_schema(&conn);
        for conversation in conversations {
            insert_conversation(&conn, conversation);
        }
    }
    let backup = dir.join("backup.zip");
    build_zip(
        &backup,
        &[
            ("rikka_hub.db".to_string(), fs::read(&db_path).unwrap()),
            ("settings.json".to_string(), SETTINGS.as_bytes().to_vec()),
        ],
    );
    backup
}

fn run_rikka(input: &Path) {
    let output = Command::new(env!("CARGO_BIN_EXE_rikka"))
        .arg(input)
        .output()
        .expect("run rikka");
    assert!(
        output.status.success(),
        "rikka failed: {}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn find_output_zip(dir: &Path) -> PathBuf {
    fs::read_dir(dir)
        .expect("read dir")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("chat-export-rikka-all-"))
        })
        .expect("expected output zip")
}

fn read_entries(path: &Path) -> Vec<(String, String)> {
    let file = File::open(path).expect("open output zip");
    let mut archive = zip::ZipArchive::new(file).expect("read output zip");
    (0..archive.len())
        .map(|index| {
            let mut entry = archive.by_index(index).expect("entry");
            let name = entry.name().to_string();
            let mut content = String::new();
            std::io::Read::read_to_string(&mut entry, &mut content).expect("read entry");
            (name, content)
        })
        .collect()
}

#[test]
fn converts_backup_zip_newest_first() {
    let dir = tempdir().expect("tempdir");
    let backup = make_plain_backup(
        dir.path(),
        &[
            DbConversation {
                id: "conv-1".into(),
                assistant_id: "a1".into(),
                title: "First".into(),
                create_at: 1_700_000_000_000,
                custom_system_prompt: String::new(),
                nodes: vec![
                    (user_message("请给我一个标题"), 0),
                    (assistant_message("这是回答", Some("先思考"), "m1"), 0),
                ],
            },
            DbConversation {
                id: "conv-2".into(),
                assistant_id: "a1".into(),
                title: "Second".into(),
                create_at: 1_700_000_100_000,
                custom_system_prompt: String::new(),
                nodes: vec![(user_message("只有用户消息"), 0)],
            },
        ],
    );

    run_rikka(&backup);
    let output = find_output_zip(dir.path());
    let entries = read_entries(&output);

    assert_eq!(entries.len(), 2, "entries: {entries:#?}");
    // 从新到旧：Second 在前
    assert!(entries[0].0.starts_with("Gemini/"), "{}", entries[0].0);
    assert!(entries[0].0.ends_with("-Second.md"), "{}", entries[0].0);
    assert!(entries[1].0.ends_with("-First.md"), "{}", entries[1].0);

    let markdown = &entries[1].1;
    assert!(markdown.starts_with("## Metadata\n\n"), "{markdown}");
    assert!(
        markdown.contains("- **Model:** `Gemini 3 Flash`"),
        "{markdown}"
    );
    assert!(markdown.contains("- **Time:** 20"), "{markdown}");
    assert!(
        markdown.contains("- **Conversation ID:** `conv-1`"),
        "{markdown}"
    );
    assert!(markdown.contains("- **Assistant:** `Gemini`"), "{markdown}");
    assert!(markdown.contains("### ⚙️ System\n\n你是助手"), "{markdown}");
    assert!(markdown.contains("### 🧑‍💻 User"), "{markdown}");
    assert!(
        markdown.contains("#### 🤔 Thought Process\n\n先思考"),
        "{markdown}"
    );
    assert!(
        markdown.contains("#### 💡 Response\n\n这是回答"),
        "{markdown}"
    );
}

fn make_wal_backup(dir: &Path) -> PathBuf {
    let db_path = dir.join("rikka_hub.db");
    let conn = Connection::open(&db_path).expect("open db");
    create_schema(&conn);
    conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0;")
        .expect("enable wal");
    insert_conversation(
        &conn,
        &DbConversation {
            id: "conv-wal".into(),
            assistant_id: "a1".into(),
            title: "FromWal".into(),
            create_at: 1_700_000_200_000,
            custom_system_prompt: String::new(),
            nodes: vec![(user_message("wal 里的消息"), 0)],
        },
    );

    // 连接仍打开时复制 db + wal，确保 WAL 里确有未 checkpoint 的数据
    let db_bytes = fs::read(&db_path).expect("read db");
    let wal_path = db_path.with_file_name("rikka_hub.db-wal");
    let wal_bytes = fs::read(&wal_path).expect("read wal");
    assert!(!wal_bytes.is_empty(), "wal should be non-empty");

    let backup = dir.join("backup.zip");
    build_zip(
        &backup,
        &[
            ("rikka_hub.db".to_string(), db_bytes),
            ("rikka_hub.db-wal".to_string(), wal_bytes),
            ("settings.json".to_string(), SETTINGS.as_bytes().to_vec()),
        ],
    );
    drop(conn);
    backup
}

#[test]
fn replays_wal_contents() {
    let dir = tempdir().expect("tempdir");
    let backup = make_wal_backup(dir.path());

    run_rikka(&backup);
    let output = find_output_zip(dir.path());
    let entries = read_entries(&output);

    assert_eq!(entries.len(), 1, "entries: {entries:#?}");
    assert!(entries[0].0.ends_with("-FromWal.md"), "{}", entries[0].0);
    assert!(entries[0].1.contains("wal 里的消息"), "{}", entries[0].1);
}

#[test]
fn empty_conversation_is_reported() {
    let dir = tempdir().expect("tempdir");
    let backup = make_plain_backup(
        dir.path(),
        &[
            DbConversation {
                id: "conv-ok".into(),
                assistant_id: "a1".into(),
                title: "Good".into(),
                create_at: 1_700_000_000_000,
                custom_system_prompt: String::new(),
                nodes: vec![(user_message("hi"), 0)],
            },
            DbConversation {
                id: "conv-empty".into(),
                assistant_id: "a1".into(),
                title: "Empty".into(),
                create_at: 1_700_000_050_000,
                custom_system_prompt: String::new(),
                nodes: Vec::new(),
            },
        ],
    );

    run_rikka(&backup);
    let output = find_output_zip(dir.path());
    let entries = read_entries(&output);

    assert_eq!(entries.len(), 2, "entries: {entries:#?}");
    let failures = entries
        .iter()
        .find(|(name, _)| name == "export-failures.md")
        .expect("expected export-failures.md");
    assert!(
        failures.1.contains("- **Platform:** `rikka`"),
        "{}",
        failures.1
    );
    assert!(failures.1.contains("conv-empty"), "{}", failures.1);
    assert!(failures.1.contains("Empty"), "{}", failures.1);
}

#[test]
fn duplicated_titles_get_suffix() {
    let dir = tempdir().expect("tempdir");
    let backup = make_plain_backup(
        dir.path(),
        &[
            DbConversation {
                id: "conv-a".into(),
                assistant_id: "a1".into(),
                title: "Same".into(),
                create_at: 1_700_000_000_000,
                custom_system_prompt: String::new(),
                nodes: vec![(user_message("a"), 0)],
            },
            DbConversation {
                id: "conv-b".into(),
                assistant_id: "a1".into(),
                title: "Same".into(),
                create_at: 1_700_000_000_000,
                custom_system_prompt: String::new(),
                nodes: vec![(user_message("b"), 0)],
            },
        ],
    );

    run_rikka(&backup);
    let output = find_output_zip(dir.path());
    let names: Vec<String> = read_entries(&output)
        .into_iter()
        .map(|(name, _)| name)
        .collect();

    assert_eq!(names.len(), 2, "names: {names:#?}");
    assert!(
        names.iter().any(|name| name.ends_with("-Same.md")),
        "{names:#?}"
    );
    assert!(
        names.iter().any(|name| name.ends_with("-Same-2.md")),
        "{names:#?}"
    );
}
