//! CLI 端到端：单文件输出 `.md`，目录输入输出一棵 `.md` 树。

use std::fs;
use std::process::Command;

use assert_cmd::prelude::*;
use tempfile::TempDir;

const CONVERSATION: &str = r##"{
    "runSettings": {"model": "gemini-2.5-pro"},
    "systemInstruction": {"text": "# System"},
    "chunkedPrompt": {"chunks": [
        {"role": "user", "text": "# Hello"},
        {"role": "model", "text": "thinking", "isThought": true},
        {"role": "model", "text": "world"}
    ]}
}"##;

#[test]
fn converts_single_json_next_to_source() {
    let dir = TempDir::new().unwrap();
    let input = dir.path().join("sample.json");
    fs::write(&input, CONVERSATION).unwrap();

    Command::cargo_bin("ai-studio")
        .unwrap()
        .arg(&input)
        .assert()
        .success();

    let output = dir.path().join("sample.md");
    let markdown = fs::read_to_string(&output).expect("output md should exist");
    assert!(markdown.starts_with("## Metadata\n"), "{markdown}");
    assert!(markdown.contains("- **Model:** `gemini-2.5-pro`\n"), "{markdown}");
    assert!(markdown.contains("### ⚙️ System\n\n**System**\n"), "{markdown}");
    assert!(markdown.contains("### 🧑‍💻 User\n\n**Hello**\n"), "{markdown}");
    assert!(
        markdown.contains("#### 🤔 Thought Process\n\nthinking\n"),
        "{markdown}"
    );
    assert!(markdown.contains("#### 💡 Response\n\nworld\n"), "{markdown}");
}

#[test]
fn converts_directory_into_markdown_tree() {
    let dir = TempDir::new().unwrap();
    let input = dir.path().join("in");
    fs::create_dir_all(input.join("nested")).unwrap();
    fs::write(input.join("a.json"), CONVERSATION).unwrap();
    fs::write(input.join("nested/b.json"), CONVERSATION).unwrap();
    fs::write(input.join("ignore.txt"), "not json").unwrap();

    let out = dir.path().join("out");
    Command::cargo_bin("ai-studio")
        .unwrap()
        .arg(&input)
        .arg("-o")
        .arg(&out)
        .assert()
        .success();

    assert!(out.join("a.md").is_file(), "a.md missing");
    assert!(out.join("nested/b.md").is_file(), "nested/b.md missing");
    assert!(!out.join("ignore.md").exists(), "non-json should be skipped");
}
