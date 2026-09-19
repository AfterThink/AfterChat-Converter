use std::collections::BTreeMap;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use assert_cmd::cargo::cargo_bin_cmd;
use zip::ZipArchive;
use zip::write::SimpleFileOptions;

/// 产物应该只有一个 `chat-export-cherry-all-<ms>.zip`
fn find_output_zip(dir: &Path) -> PathBuf {
    let mut found: Vec<PathBuf> = fs::read_dir(dir)
        .expect("read output dir")
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| {
                    name.starts_with("chat-export-cherry-all-") && name.ends_with(".zip")
                })
        })
        .collect();
    found.sort();
    assert_eq!(found.len(), 1, "输出目录里应恰好有一个导出 zip: {dir:?}");
    found.remove(0)
}

/// 读出 zip 里所有条目（名字 → 内容）
fn read_zip(zip_path: &Path) -> BTreeMap<String, String> {
    let file = fs::File::open(zip_path).expect("open zip");
    let mut archive = ZipArchive::new(file).expect("read zip");
    let mut entries = BTreeMap::new();
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).expect("zip entry");
        let name = entry.name().to_string();
        let mut text = String::new();
        entry.read_to_string(&mut text).expect("read zip entry");
        entries.insert(name, text);
    }
    entries
}

/// 条目名前 15 个字符应是 `YYYYMMDD-HHmmss`
fn assert_time_prefixed(entry_name: &str, assistant_dir: &str, title: &str) {
    let stem = entry_name
        .strip_prefix(&format!("{assistant_dir}/"))
        .unwrap_or_else(|| panic!("条目应位于助手目录下: {entry_name}"));
    let (prefix, rest) = stem.split_at(15);
    assert!(
        prefix.chars().all(|c| c.is_ascii_digit() || c == '-'),
        "时间前缀格式不对: {prefix}"
    );
    assert_eq!(&prefix[8..9], "-", "时间前缀应为 YYYYMMDD-HHmmss: {prefix}");
    assert_eq!(rest, format!("-{title}.md"), "条目名不对: {entry_name}");
}

fn sample_cherry_export_json() -> String {
    let assistants = serde_json::json!({
        "defaultAssistant": {
            "id": "assistant-1",
            "name": "Unit Assistant",
            "prompt": "Be helpful.",
            "topics": [
                {
                    "id": "topic-1",
                    "name": "Unit Topic",
                    "createdAt": "2024-01-01T00:00:00Z"
                }
            ]
        },
        "assistants": []
    });

    let persist = serde_json::json!({
        "assistants": assistants.to_string()
    });

    serde_json::json!({
        "localStorage": {
            "persist:cherry-studio": persist.to_string()
        },
        "indexedDB": {
            "topics": [
                {
                    "id": "topic-1",
                    "createdAt": "2024-01-01T00:00:00Z",
                    "messages": [
                        {
                            "id": "message-1",
                            "role": "user",
                            "blocks": ["block-1"],
                            "createdAt": "2024-01-01T00:00:01Z"
                        },
                        {
                            "id": "message-2",
                            "role": "assistant",
                            "model": "Unit Model",
                            "blocks": ["block-2"],
                            "createdAt": "2024-01-01T00:00:02Z"
                        }
                    ]
                }
            ],
            "message_blocks": [
                {
                    "id": "block-1",
                    "messageId": "message-1",
                    "type": "text",
                    "content": "hello cherry",
                    "createdAt": "2024-01-01T00:00:01Z"
                },
                {
                    "id": "block-2",
                    "messageId": "message-2",
                    "type": "text",
                    "content": "hello back",
                    "createdAt": "2024-01-01T00:00:02Z"
                }
            ]
        }
    })
    .to_string()
}

fn format_sample_json() -> String {
    let assistants = serde_json::json!({
        "defaultAssistant": {
            "id": "assistant-1",
            "name": "Format Assistant",
            "prompt": "Follow rules.\n\n# 语气\n专业。\n\n```python\n# code comment\npass\n```",
            "topics": [
                { "id": "topic-1", "name": "Format Topic", "createdAt": "2025-10-23T12:27:47.010Z" }
            ]
        },
        "assistants": []
    });

    let persist = serde_json::json!({ "assistants": assistants.to_string() });

    serde_json::json!({
        "localStorage": { "persist:cherry-studio": persist.to_string() },
        "indexedDB": {
            "topics": [
                {
                    "id": "topic-1",
                    "createdAt": "2025-10-23T12:27:47.010Z",
                    "messages": [
                        { "id": "m1", "role": "user", "blocks": ["b1"], "createdAt": "2025-10-23T12:27:48Z" },
                        {
                            "id": "m2",
                            "role": "assistant",
                            "model": { "id": "test-model" },
                            "blocks": ["b2", "b3"],
                            "createdAt": "2025-10-23T12:27:50Z"
                        }
                    ]
                }
            ],
            "message_blocks": [
                {
                    "id": "b1",
                    "messageId": "m1",
                    "type": "text",
                    "content": "# 一级标题\n## 二级：**重点**\n匹配 `**/*.js` 的路径\n\n```python\n# comment stays\n```",
                    "createdAt": "2025-10-23T12:27:48Z"
                },
                {
                    "id": "b2",
                    "messageId": "m2",
                    "type": "thinking",
                    "content": "先思考。",
                    "createdAt": "2025-10-23T12:27:49Z"
                },
                {
                    "id": "b3",
                    "messageId": "m2",
                    "type": "text",
                    "content": "## 结论\n这是回复。",
                    "createdAt": "2025-10-23T12:27:50Z"
                }
            ]
        }
    })
    .to_string()
}

fn numeric_created_at_json() -> String {
    // cherry 的 createdAt 可能是 epoch 毫秒数字而非 RFC3339 字符串
    let assistants = serde_json::json!({
        "defaultAssistant": {
            "id": "assistant-1",
            "name": "Numeric Assistant",
            "prompt": "",
            "topics": [
                { "id": "topic-1", "name": "Numeric Topic", "createdAt": 1735689600000i64 }
            ]
        },
        "assistants": []
    });
    let persist = serde_json::json!({ "assistants": assistants.to_string() });

    serde_json::json!({
        "localStorage": { "persist:cherry-studio": persist.to_string() },
        "indexedDB": {
            "topics": [
                {
                    "id": "topic-1",
                    "createdAt": null,
                    "messages": [
                        { "id": "m1", "role": "user", "blocks": ["b1"], "createdAt": 1735689600000i64 }
                    ]
                }
            ],
            "message_blocks": [
                {
                    "id": "b1",
                    "messageId": "m1",
                    "type": "text",
                    "content": "hi",
                    "createdAt": 1735689600000i64
                }
            ]
        }
    })
    .to_string()
}

fn create_test_dir() -> std::path::PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let path =
        std::env::temp_dir().join(format!("cherry-cli-test-{}-{}", std::process::id(), unique));
    fs::create_dir_all(&path).expect("create temp test dir");
    path
}

/// 用 zip 库造一个测试用压缩包（不依赖 powershell / Compress-Archive）
fn create_zip_backup(entries: &[(&str, String)], archive_path: &Path) {
    let file = fs::File::create(archive_path).expect("create archive");
    let mut writer = zip::ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for (name, content) in entries {
        writer.start_file(*name, options).expect("start entry");
        writer.write_all(content.as_bytes()).expect("write entry");
    }
    writer.finish().expect("finish archive");
}

#[test]
fn converts_zip_backup_by_extracting_data_json() {
    let tmp = create_test_dir();
    let input_zip = tmp.join("my-cherry-backup.zip");
    let output_dir = tmp.join("out");
    create_zip_backup(&[("data.json", sample_cherry_export_json())], &input_zip);

    cargo_bin_cmd!("cherry")
        .args([
            input_zip.to_string_lossy().as_ref(),
            "-o",
            output_dir.to_string_lossy().as_ref(),
        ])
        .assert()
        .success();

    let zip_path = find_output_zip(&output_dir);
    let entries = read_zip(&zip_path);

    assert_eq!(entries.len(), 1, "一个主题应产出恰好一个条目");
    let (entry_name, text) = entries.iter().next().expect("one entry");
    assert_time_prefixed(entry_name, "Unit Assistant", "Unit Topic");

    assert!(text.contains("- **Assistant:** `Unit Assistant`"));
    assert!(text.contains("hello cherry"));
    assert!(text.contains("hello back"));

    let _ = fs::remove_dir_all(tmp);
}

#[test]
fn zip_data_json_is_found_in_nested_dir_despite_bom() {
    let tmp = create_test_dir();
    let input_zip = tmp.join("nested.zip");
    let output_dir = tmp.join("out");
    // data.json 在子目录里，而且带 UTF-8 BOM
    let with_bom = format!("\u{feff}{}", sample_cherry_export_json());
    create_zip_backup(
        &[
            ("readme.txt", "not the payload".to_string()),
            ("Data/data.json", with_bom),
        ],
        &input_zip,
    );

    cargo_bin_cmd!("cherry")
        .args([
            input_zip.to_string_lossy().as_ref(),
            "-o",
            output_dir.to_string_lossy().as_ref(),
        ])
        .assert()
        .success();

    assert_eq!(read_zip(&find_output_zip(&output_dir)).len(), 1);

    let _ = fs::remove_dir_all(tmp);
}

#[test]
fn zip_without_data_json_fails_without_writing_output() {
    let tmp = create_test_dir();
    let input_zip = tmp.join("not-a-backup.zip");
    let output_dir = tmp.join("out");
    create_zip_backup(&[("readme.txt", "hello".to_string())], &input_zip);

    cargo_bin_cmd!("cherry")
        .args([
            input_zip.to_string_lossy().as_ref(),
            "-o",
            output_dir.to_string_lossy().as_ref(),
        ])
        .assert()
        .failure();

    assert!(!output_dir.exists());

    let _ = fs::remove_dir_all(tmp);
}

#[test]
fn explicit_zip_output_path_is_used() {
    let tmp = create_test_dir();
    let input_json = tmp.join("backup.json");
    let target = tmp.join("custom-name.zip");
    fs::write(&input_json, sample_cherry_export_json()).expect("write backup");

    cargo_bin_cmd!("cherry")
        .args([
            input_json.to_string_lossy().as_ref(),
            "-o",
            target.to_string_lossy().as_ref(),
        ])
        .assert()
        .success();

    assert!(target.exists(), "-o 指向 .zip 时应直接写该文件");
    assert_eq!(read_zip(&target).len(), 1);

    let _ = fs::remove_dir_all(tmp);
}

#[test]
fn markdown_matches_chatformat() {
    let tmp = create_test_dir();
    let input_json = tmp.join("backup.json");
    let output_dir = tmp.join("out");
    fs::write(&input_json, format_sample_json()).expect("write backup");

    cargo_bin_cmd!("cherry")
        .args([
            input_json.to_string_lossy().as_ref(),
            "-o",
            output_dir.to_string_lossy().as_ref(),
        ])
        .assert()
        .success();

    let zip_path = find_output_zip(&output_dir);
    let entries = read_zip(&zip_path);
    assert_eq!(entries.len(), 1);
    let (entry_name, text) = entries.iter().next().expect("one entry");
    assert_time_prefixed(entry_name, "Format Assistant", "Format Topic");

    // §2 Metadata：推荐键，Time 用本地时间；不再有 Conversation Transcript 首行
    assert!(text.starts_with("## Metadata\n"));
    assert!(text.contains("- **Model:** `test-model`"));
    assert!(text.contains("- **Time:** "));
    assert!(!text.contains("Created At"));
    assert!(text.contains("- **Topic ID:** `topic-1`"));
    assert!(text.contains("- **Assistant:** `Format Assistant`"));

    // §3 正文：井号标题转加粗，且整条加粗
    assert!(text.contains("**一级标题**"));
    assert!(text.contains("**二级：重点**"));
    assert!(!text.contains("# 一级标题"));
    assert!(text.contains("**结论**"));

    // §3 代码围栏与行内代码不动
    assert!(text.contains("# comment stays"));
    assert!(!text.contains("**comment stays**"));
    assert!(text.contains("`**/*.js`"));

    // §3 思考 / 回复两段，标题独占一行
    assert!(text.contains("#### 🤔 Thought Process\n\n先思考。\n"));
    assert!(text.contains("#### 💡 Response\n\n**结论**"));

    // §3 系统提示是对话的第一条消息，且排在首个角色消息之前
    assert!(text.contains("## Conversation\n\n### ⚙️ System\n\nFollow rules."));
    assert!(!text.contains("### System Instruction"));
    assert!(text.contains("**语气**"));
    assert!(text.contains("# code comment"));
    let system_pos = text.find("### ⚙️ System").expect("system header");
    let user_pos = text.find("### 🧑‍💻 User").expect("user header");
    assert!(system_pos < user_pos, "系统消息应排在首个用户消息之前");

    let _ = fs::remove_dir_all(tmp);
}

#[test]
fn numeric_created_at_does_not_break_assistant_metadata() {
    let tmp = create_test_dir();
    let input_json = tmp.join("numeric.json");
    let output_dir = tmp.join("out");
    fs::write(&input_json, numeric_created_at_json()).expect("write backup");

    cargo_bin_cmd!("cherry")
        .args([
            input_json.to_string_lossy().as_ref(),
            "-o",
            output_dir.to_string_lossy().as_ref(),
        ])
        .assert()
        .success();

    let entries = read_zip(&find_output_zip(&output_dir));
    let (entry_name, text) = entries.iter().next().expect("one entry");

    // 曾经 `TopicMeta.created_at` 写死 Option<String>，一个数字就让整棵助手树解析失败，
    // 结果是全部主题退化成 Assistant/00000000-000000-Untitled.md
    assert!(
        !entry_name.contains("00000000-000000"),
        "数字 createdAt 不该退化成默认时间: {entry_name}"
    );
    assert!(
        !entry_name.contains("Untitled"),
        "数字 createdAt 不该退化成 Untitled: {entry_name}"
    );
    assert_time_prefixed(entry_name, "Numeric Assistant", "Numeric Topic");
    assert!(text.contains("- **Assistant:** `Numeric Assistant`"));
    assert!(!text.contains("- **Time:** unknown"));

    let _ = fs::remove_dir_all(tmp);
}

#[test]
fn invalid_json_does_not_create_output_dir() {
    let tmp = create_test_dir();
    let input_json = tmp.join("tauri.conf.json");
    let output_dir = tmp.join("cherry-studio-export");

    fs::write(
        &input_json,
        serde_json::json!({
            "$schema": "../node_modules/@tauri-apps/cli/config.schema.json",
            "package": {
                "productName": "AfterChat Converter"
            },
            "tauri": {
                "windows": [
                    {
                        "title": "main"
                    }
                ]
            }
        })
        .to_string(),
    )
    .expect("write arbitrary json");

    cargo_bin_cmd!("cherry")
        .args([
            input_json.to_string_lossy().as_ref(),
            "-o",
            output_dir.to_string_lossy().as_ref(),
        ])
        .assert()
        .failure();

    assert!(!output_dir.exists());

    let _ = fs::remove_dir_all(tmp);
}
