//! CLI 端到端：合成一份 ChatGPT 导出 ZIP，跑 `chatgpt`，检查产出的 ZIP 结构与 Markdown 格式。

use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::Command;

use assert_cmd::prelude::*;
use tempfile::TempDir;

fn build_fixture(dir: &TempDir) -> std::path::PathBuf {
    let zip_path = dir.path().join("chatgpt-export.zip");
    let file = fs::File::create(&zip_path).expect("create fixture zip");
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);

    let conversations = r##"[
        {
            "id": "c-1",
            "title": "Hello ChatGPT",
            "create_time": 1716346557.0,
            "update_time": 1716346561.0,
            "default_model_slug": "auto",
            "current_node": "node-assistant",
            "mapping": {
                "node-root": {
                    "id": "node-root",
                    "parent": null,
                    "children": ["node-user"],
                    "message": null
                },
                "node-user": {
                    "id": "node-user",
                    "parent": "node-root",
                    "children": ["node-thought"],
                    "message": {
                        "id": "m-user",
                        "author": { "role": "user" },
                        "content": {
                            "content_type": "text",
                            "parts": ["# User Question"]
                        }
                    }
                },
                "node-thought": {
                    "id": "node-thought",
                    "parent": "node-user",
                    "children": ["node-assistant"],
                    "message": {
                        "id": "m-thought",
                        "author": { "role": "assistant" },
                        "content": {
                            "content_type": "thoughts",
                            "thoughts": [
                                { "summary": "Thinking step", "content": "I am thinking carefully." }
                            ]
                        }
                    }
                },
                "node-assistant": {
                    "id": "node-assistant",
                    "parent": "node-thought",
                    "children": [],
                    "message": {
                        "id": "m-assistant",
                        "author": { "role": "assistant" },
                        "content": {
                            "content_type": "text",
                            "parts": ["Assistant Response with citation\ue200cite\ue202turn0search1\ue201"]
                        },
                        "metadata": {
                            "model_slug": "gpt-4o",
                            "content_references": [
                                {
                                    "alt": "([OpenAI](https://openai.com))",
                                    "items": [
                                        {
                                            "title": "OpenAI",
                                            "url": "https://openai.com",
                                            "refs": [{ "turn_index": 0, "ref_index": 1 }]
                                        }
                                    ]
                                }
                            ]
                        }
                    }
                }
            }
        },
        {
            "id": "c-2",
            "title": "Empty Conv",
            "create_time": 1716346557.0,
            "current_node": "empty-root",
            "mapping": {
                "empty-root": {
                    "id": "empty-root",
                    "parent": null,
                    "children": [],
                    "message": null
                }
            }
        }
    ]"##;

    zip.start_file("conversations.json", options).unwrap();
    zip.write_all(conversations.as_bytes()).unwrap();
    zip.finish().unwrap();

    zip_path
}

#[test]
fn converts_chatgpt_zip_into_directory() {
    let dir = TempDir::new().unwrap();
    let input = build_fixture(&dir);
    let out_dir = dir.path().join("vault");

    Command::cargo_bin("chatgpt")
        .unwrap()
        .arg(&input)
        .arg("-o")
        .arg(&out_dir)
        .assert()
        .success();

    assert!(out_dir.exists());
    assert!(out_dir.join("export-failures.md").exists());

    let files: Vec<PathBuf> = fs::read_dir(&out_dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();

    let entry_path = files
        .iter()
        .find(|path| {
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .ends_with("-Hello ChatGPT.md")
        })
        .expect("conversation file should be `{time}-Hello ChatGPT.md`");

    let markdown = fs::read_to_string(entry_path).unwrap();
    assert!(markdown.starts_with("## Metadata\n"), "{markdown}");
    assert!(markdown.contains("- **Model:** `gpt-4o`\n"), "{markdown}");
    assert!(markdown.contains("- **Time:** 2024-05-22 "), "{markdown}");
    assert!(
        markdown.contains("- **URL:** https://chatgpt.com/c/c-1\n"),
        "{markdown}"
    );
    assert!(
        markdown.contains("- **Conversation ID:** `c-1`\n"),
        "{markdown}"
    );
    assert!(
        markdown.contains("### 🧑‍💻 User\n\n**User Question**\n"),
        "{markdown}"
    );
    assert!(
        markdown.contains("#### 🤔 Thought Process\n\nI am thinking carefully.\n"),
        "{markdown}"
    );
    assert!(
        markdown.contains(
            "#### 💡 Response\n\nAssistant Response with citation ([OpenAI](https://openai.com))\n"
        ),
        "{markdown}"
    );
}

#[test]
fn converts_chatgpt_zip_into_explicit_zip() {
    let dir = TempDir::new().unwrap();
    let input = build_fixture(&dir);
    let out_zip = dir.path().join("out").join("exported.zip");

    Command::cargo_bin("chatgpt")
        .unwrap()
        .arg(&input)
        .arg("-o")
        .arg(&out_zip)
        .assert()
        .success();

    assert!(out_zip.exists());

    let file = fs::File::open(&out_zip).unwrap();
    let mut archive = zip::ZipArchive::new(file).unwrap();
    let names: Vec<String> = (0..archive.len())
        .map(|index| archive.by_index(index).unwrap().name().to_string())
        .collect();

    assert!(names.iter().any(|name| name == "export-failures.md"));
    assert!(names.iter().any(|name| name.ends_with("-Hello ChatGPT.md")));
}

#[test]
fn unpacks_afterchat_zip_directly() {
    let dir = TempDir::new().unwrap();
    let afterchat_zip = dir.path().join("afterchat_vault.zip");

    {
        let file = fs::File::create(&afterchat_zip).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        zip.start_file("sample.md", zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(b"# Hello AfterChat").unwrap();
        zip.finish().unwrap();
    }

    let out_dir = dir.path().join("unpacked");

    Command::cargo_bin("chatgpt")
        .unwrap()
        .arg(&afterchat_zip)
        .arg("-o")
        .arg(&out_dir)
        .assert()
        .success();

    assert!(out_dir.join("sample.md").exists());
    assert_eq!(
        fs::read_to_string(out_dir.join("sample.md")).unwrap(),
        "# Hello AfterChat"
    );
}
