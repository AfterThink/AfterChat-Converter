//! CLI 端到端：合成一份 Claude 导出 ZIP，跑 `claude`，检查产出的 ZIP 结构。

use std::fs;
use std::io::Write;
use std::process::Command;

use assert_cmd::prelude::*;
use tempfile::TempDir;

fn build_fixture(dir: &TempDir) -> std::path::PathBuf {
    let zip_path = dir.path().join("data-test-batch-0000.zip");
    let file = fs::File::create(&zip_path).expect("create fixture zip");
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);

    let conversations = r##"[
        {
            "uuid": "u-1",
            "name": "Hello World",
            "created_at": "2024-09-09T14:22:31.424169Z",
            "updated_at": "2024-09-09T15:00:00.000000Z",
            "chat_messages": [
                {"sender": "human", "text": "# Question", "attachments": [], "files": []},
                {"sender": "assistant", "text": "Answer", "attachments": [], "files": []}
            ]
        },
        {
            "uuid": "u-2",
            "name": "Empty",
            "created_at": "2024-09-10T14:22:31.424169Z",
            "chat_messages": []
        }
    ]"##;

    zip.start_file("conversations.json", options).unwrap();
    zip.write_all(conversations.as_bytes()).unwrap();
    zip.start_file("users.json", options).unwrap();
    zip.write_all(b"[]").unwrap();
    zip.finish().unwrap();

    zip_path
}

#[test]
fn converts_claude_zip_into_contract_zip() {
    let dir = TempDir::new().unwrap();
    let input = build_fixture(&dir);
    let out_dir = dir.path().join("out");
    fs::create_dir_all(&out_dir).unwrap();

    Command::cargo_bin("claude")
        .unwrap()
        .arg(&input)
        .arg("-o")
        .arg(&out_dir)
        .assert()
        .success();

    let produced: Vec<_> = fs::read_dir(&out_dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().and_then(|e| e.to_str()) == Some("zip"))
        .collect();
    assert_eq!(produced.len(), 1, "expected exactly one zip: {produced:?}");

    let file = fs::File::open(&produced[0]).unwrap();
    let mut archive = zip::ZipArchive::new(file).unwrap();
    let names: Vec<String> = (0..archive.len())
        .map(|index| archive.by_index(index).unwrap().name().to_string())
        .collect();

    assert!(
        names.iter().any(|name| name == "export-failures.md"),
        "empty conversation should be reported: {names:?}"
    );

    let entry_name = names
        .iter()
        .find(|name| name.ends_with("-Hello World.md"))
        .expect("conversation entry should be `{time}-Hello World.md`")
        .clone();

    let mut entry = archive.by_name(&entry_name).unwrap();
    let mut markdown = String::new();
    std::io::Read::read_to_string(&mut entry, &mut markdown).unwrap();
    drop(entry);

    assert!(markdown.starts_with("## Metadata\n"), "{markdown}");
    assert!(markdown.contains("- **Model:** `Unknown`\n"), "{markdown}");
    assert!(markdown.contains("- **Time:** 2024-09-09 "), "{markdown}");
    assert!(
        markdown.contains("- **URL:** https://claude.ai/chat/u-1\n"),
        "{markdown}"
    );
    assert!(
        markdown.contains("### 🧑‍💻 User\n\n**Question**\n"),
        "{markdown}"
    );
    assert!(
        markdown.contains("### 🤖 Assistant\n\nAnswer\n"),
        "{markdown}"
    );
}
