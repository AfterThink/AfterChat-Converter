//! CLI 端到端：合成一份 DeepSeek 导出 ZIP，跑 `deepseek`，检查产出的 ZIP 结构与 Markdown 格式。

use std::fs;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::Command;

use assert_cmd::prelude::*;
use tempfile::TempDir;

fn build_fixture(dir: &TempDir) -> PathBuf {
    let zip_path = dir.path().join("deepseek_data.zip");
    let file = fs::File::create(&zip_path).expect("create fixture zip");
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);

    // manifest.json
    zip.start_file("manifest.json", options).unwrap();
    zip.write_all(br#"{"export_format_version": 3}"#).unwrap();

    // user.json
    zip.start_file("user.json", options).unwrap();
    zip.write_all(br#"{"user_id": "u-123", "email": "test@example.com"}"#)
        .unwrap();

    // chat_sessions/session-1.json
    let session_1 = r##"{
        "id": "11111111-1111-1111-1111-111111111111",
        "title": "Hello DeepSeek",
        "current_message_id": 2,
        "inserted_at": "2026-11-20T10:00:00.000Z",
        "updated_at": "2026-11-20T10:05:00.000Z",
        "messages": [
            {
                "id": 1,
                "parent_id": null,
                "role": "USER",
                "fragments": [
                    { "type": "REQUEST", "content": "# User Question" }
                ]
            },
            {
                "id": 2,
                "parent_id": 1,
                "role": "ASSISTANT",
                "thinking_enabled": true,
                "fragments": [
                    { "type": "THINK", "content": "I am thinking carefully." },
                    { "type": "RESPONSE", "content": "DeepSeek response text" },
                    { "type": "CONTENT_RISK", "content": "disclaimer banner", "style": "WARNING" }
                ]
            }
        ]
    }"##;
    zip.start_file(
        "chat_sessions/11111111-1111-1111-1111-111111111111.json",
        options,
    )
    .unwrap();
    zip.write_all(session_1.as_bytes()).unwrap();

    // chat_sessions/empty-session.json
    let session_empty = r##"{
        "id": "22222222-2222-2222-2222-222222222222",
        "title": "Empty Session",
        "current_message_id": null,
        "messages": []
    }"##;
    zip.start_file(
        "chat_sessions/22222222-2222-2222-2222-222222222222.json",
        options,
    )
    .unwrap();
    zip.write_all(session_empty.as_bytes()).unwrap();

    zip.finish().unwrap();
    zip_path
}

#[test]
fn cli_converts_fixture_into_zip() {
    let temp = TempDir::new().unwrap();
    let zip_path = build_fixture(&temp);

    let mut cmd = Command::cargo_bin("deepseek").unwrap();
    cmd.arg(&zip_path);
    cmd.assert().success();

    let mut found_out_zip = None;
    for entry in fs::read_dir(temp.path()).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path != zip_path && path.extension().is_some_and(|ext| ext == "zip") {
            found_out_zip = Some(path);
            break;
        }
    }

    let out_zip = found_out_zip.expect("expected output zip next to input zip");
    let file = fs::File::open(&out_zip).unwrap();
    let mut archive = zip::ZipArchive::new(file).unwrap();

    let mut entry_names = Vec::new();
    for i in 0..archive.len() {
        entry_names.push(archive.by_index(i).unwrap().name().to_string());
    }

    assert!(
        entry_names.iter().any(|n| n.contains("Hello DeepSeek.md")),
        "missing rendered markdown: {entry_names:?}"
    );
    assert!(
        entry_names.contains(&"export-failures.md".to_string()),
        "missing export-failures.md: {entry_names:?}"
    );

    let mut md = String::new();
    let entry_name = entry_names
        .iter()
        .find(|n| n.contains("Hello DeepSeek.md"))
        .unwrap();
    archive
        .by_name(entry_name)
        .unwrap()
        .read_to_string(&mut md)
        .unwrap();

    assert!(md.contains("## Metadata"), "{md}");
    assert!(md.contains("- **Model:** `DeepSeek`"), "{md}");
    assert!(
        md.contains(
            "- **URL:** https://chat.deepseek.com/a/chat/s/11111111-1111-1111-1111-111111111111"
        ),
        "{md}"
    );
    assert!(md.contains("## Conversation"), "{md}");
    assert!(md.contains("### 🧑‍💻 User"), "{md}");
    assert!(md.contains("**User Question**"), "{md}"); // # converted to **
    assert!(md.contains("### 🤖 Assistant"), "{md}");
    assert!(md.contains("#### 🤔 Thought Process"), "{md}");
    assert!(md.contains("I am thinking carefully."), "{md}");
    assert!(md.contains("#### 💡 Response"), "{md}");
    assert!(md.contains("DeepSeek response text"), "{md}");
    assert!(!md.contains("disclaimer banner"), "{md}"); // CONTENT_RISK ignored
}

#[test]
fn cli_outputs_directory_when_specified() {
    let temp = TempDir::new().unwrap();
    let zip_path = build_fixture(&temp);
    let out_dir = temp.path().join("exported_markdown");

    let mut cmd = Command::cargo_bin("deepseek").unwrap();
    cmd.arg(&zip_path).arg("-o").arg(&out_dir);
    cmd.assert().success();

    assert!(out_dir.is_dir());
    let files: Vec<PathBuf> = fs::read_dir(&out_dir)
        .unwrap()
        .map(|r| r.unwrap().path())
        .collect();

    assert!(
        files
            .iter()
            .any(|p| p.to_string_lossy().contains("Hello DeepSeek.md")),
        "missing markdown in out_dir: {files:?}"
    );
}

#[test]
fn cli_converts_single_json_into_md() {
    let temp = TempDir::new().unwrap();
    let json_path = temp.path().join("single.json");
    let json_content = r#"{
        "id": "single-1",
        "title": "Single Question",
        "current_message_id": 1,
        "messages": [
            {
                "id": 1,
                "parent_id": null,
                "role": "USER",
                "fragments": [{ "type": "REQUEST", "content": "Just a test" }]
            }
        ]
    }"#;
    fs::write(&json_path, json_content).unwrap();

    let mut cmd = Command::cargo_bin("deepseek").unwrap();
    cmd.arg(&json_path);
    cmd.assert().success();

    let expected_md = temp.path().join("Single Question.md");
    assert!(
        expected_md.is_file(),
        "expected Single Question.md to exist"
    );
    let content = fs::read_to_string(&expected_md).unwrap();
    assert!(content.contains("- **Model:** `DeepSeek`"));
    assert!(content.contains("Just a test"));
}
