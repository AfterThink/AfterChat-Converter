use std::fs;

use assert_cmd::Command;
use tempfile::tempdir;

fn small_session_json(question: &str, answer: &str) -> String {
    format!(
        r#"[{{
  "id": "conv-1",
  "title": "demo",
  "meta": {{"tags": ["topic/demo"]}},
  "chat_type": "t2t",
  "chat": {{
    "history": {{
      "messages": {{
        "u1": {{
          "id": "u1",
          "role": "user",
          "content": "{question}",
          "childrenIds": ["a1"],
          "timestamp": 1700000000
        }},
        "a1": {{
          "id": "a1",
          "role": "assistant",
          "content": "{answer}",
          "parentId": "u1",
          "timestamp": 1700000001,
          "modelName": "Qwen-Unit"
        }}
      }}
    }}
  }}
}}]"#
    )
}

#[test]
fn converts_single_json_to_markdown() {
    let tmp = tempdir().expect("create temp dir");
    let in_dir = tmp.path().join("in");
    let out_dir = tmp.path().join("out");
    fs::create_dir_all(&in_dir).expect("create input dir");
    fs::write(
        in_dir.join("small.json"),
        small_session_json("hello?", "world!"),
    )
    .expect("write small json");

    Command::cargo_bin("qwen")
        .expect("binary")
        .args([
            in_dir.join("small.json").to_string_lossy().as_ref(),
            "-o",
            out_dir.to_string_lossy().as_ref(),
        ])
        .assert()
        .success();

    let output_md = out_dir.join("demo.md");
    assert!(output_md.exists());
    let text = fs::read_to_string(output_md).expect("read output markdown");
    assert!(text.contains("## Conversation"));
    assert!(text.contains("### 🧑‍💻 User"));
    assert!(text.contains("hello?"));
    assert!(text.contains("world!"));
}

#[test]
fn wrapped_large_json_splits_into_many_files() {
    let tmp = tempdir().expect("create temp dir");
    let in_dir = tmp.path().join("in");
    let out_dir = tmp.path().join("out");
    fs::create_dir_all(&in_dir).expect("create input dir");

    let item1 = serde_json::from_str::<serde_json::Value>(&small_session_json("q1", "a1"))
        .expect("valid json array")[0]
        .clone();
    let item2 = serde_json::from_str::<serde_json::Value>(&small_session_json("q2", "a2"))
        .expect("valid json array")[0]
        .clone();
    let item3 = serde_json::from_str::<serde_json::Value>(&small_session_json("q3", "a3"))
        .expect("valid json array")[0]
        .clone();

    let large = serde_json::json!({
        "success": true,
        "request_id": "req-xyz",
        "data": [item1, item2, item3]
    });
    fs::write(
        in_dir.join("large.json"),
        serde_json::to_string_pretty(&large).expect("serialize large json"),
    )
    .expect("write large json");

    Command::cargo_bin("qwen")
        .expect("binary")
        .args([
            in_dir.join("large.json").to_string_lossy().as_ref(),
            "-o",
            out_dir.to_string_lossy().as_ref(),
        ])
        .assert()
        .success();

    let split_dir = out_dir.join("large");
    assert!(split_dir.exists());

    let md_count = fs::read_dir(split_dir)
        .expect("read split dir")
        .filter_map(Result::ok)
        .filter(|entry| entry.path().extension().and_then(|s| s.to_str()) == Some("md"))
        .count();

    assert_eq!(md_count, 3);
}

#[test]
fn directory_mode_preserves_output_tree() {
    let tmp = tempdir().expect("create temp dir");
    let in_dir = tmp.path().join("in");
    let out_dir = tmp.path().join("out");
    let a_dir = in_dir.join("a");
    let sub_dir = in_dir.join("sub");
    fs::create_dir_all(&a_dir).expect("create a dir");
    fs::create_dir_all(&sub_dir).expect("create sub dir");
    fs::write(
        a_dir.join("one.json"),
        small_session_json("question one", "answer one"),
    )
    .expect("write one.json");
    fs::write(
        sub_dir.join("two.json"),
        small_session_json("question two", "answer two"),
    )
    .expect("write two.json");

    Command::cargo_bin("qwen")
        .expect("binary")
        .args([
            in_dir.to_string_lossy().as_ref(),
            "-o",
            out_dir.to_string_lossy().as_ref(),
        ])
        .assert()
        .success();

    assert!(out_dir.join("a").join("demo.md").exists());
    assert!(out_dir.join("sub").join("demo.md").exists());
}

#[test]
fn drag_drop_style_invocation_works() {
    let tmp = tempdir().expect("create temp dir");
    let in_dir = tmp.path().join("in");
    fs::create_dir_all(&in_dir).expect("create input dir");
    let source = in_dir.join("drag.json");
    fs::write(&source, small_session_json("drag q", "drag a")).expect("write drag json");

    Command::cargo_bin("qwen")
        .expect("binary")
        .arg(source.to_string_lossy().as_ref())
        .assert()
        .success();

    assert!(in_dir.join("demo.md").exists());
}
