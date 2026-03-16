use std::fs;
use std::path::Path;
use std::process::Command as ProcessCommand;
use std::time::{SystemTime, UNIX_EPOCH};

use assert_cmd::Command;

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

#[cfg(target_os = "windows")]
fn powershell_literal(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

#[cfg(target_os = "windows")]
fn create_zip_backup(source_dir: &Path, archive_path: &Path) {
    let source = source_dir.to_string_lossy().into_owned();
    let archive = archive_path.to_string_lossy().into_owned();
    let command = format!(
        "Compress-Archive -LiteralPath {} -DestinationPath {} -Force",
        powershell_literal(&source),
        powershell_literal(&archive)
    );
    let output = ProcessCommand::new("powershell")
        .args(["-NoProfile", "-Command", command.as_str()])
        .output()
        .expect("run Compress-Archive");

    if output.status.success() {
        return;
    }

    panic!(
        "Compress-Archive failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[cfg(target_os = "windows")]
#[test]
fn converts_zip_backup_by_extracting_data_json() {
    let tmp = create_test_dir();
    let input_zip = tmp.join("my-cherry-backup.zip");
    let output_dir = tmp.join("out");
    let extracted_dir = tmp.join("backup");
    fs::create_dir_all(&extracted_dir).expect("create extracted dir");
    fs::write(extracted_dir.join("data.json"), sample_cherry_export_json())
        .expect("write data.json");
    create_zip_backup(&extracted_dir, &input_zip);

    Command::cargo_bin("cherry")
        .expect("binary")
        .args([
            input_zip.to_string_lossy().as_ref(),
            "-o",
            output_dir.to_string_lossy().as_ref(),
        ])
        .assert()
        .success();

    let assistant_dir = output_dir.join("Unit Assistant");
    let markdown = assistant_dir.join("Unit Topic.md");
    assert!(assistant_dir.exists());
    assert!(markdown.exists());

    let text = fs::read_to_string(markdown).expect("read markdown");
    assert!(text.contains("Unit Assistant"));
    assert!(text.contains("hello cherry"));
    assert!(text.contains("hello back"));

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

    Command::cargo_bin("cherry")
        .expect("binary")
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
