use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use tempfile::tempdir;

fn run_qwen(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_qwen"))
        .args(args)
        .output()
        .expect("failed to run qwen binary")
}

fn as_str(path: &Path) -> &str {
    path.to_str().expect("path should be valid utf-8")
}

fn read_markdown(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_else(|err| panic!("read {}: {err}", path.display()))
}

fn single_export_json() -> String {
    serde_json::json!([
        {
            "id": "conv-1",
            "title": "Demo Chat",
            "created_at": 1_700_000_000,
            "chat": {
                "messages": [
                    { "role": "user", "content": "hello?", "models": ["qwen3.5-plus"] },
                    {
                        "role": "assistant",
                        "content": "",
                        "modelName": "Qwen3.5-Plus",
                        "content_list": [
                            { "phase": "think", "content": "thinking hard" },
                            { "phase": "answer", "content": "world!" }
                        ]
                    }
                ]
            }
        }
    ])
    .to_string()
}

fn all_export_json(items: serde_json::Value) -> String {
    serde_json::json!({
        "success": true,
        "request_id": "req-xyz",
        "data": items
    })
    .to_string()
}

fn session(id: &str, title: &str, ts: i64, answer: &str) -> serde_json::Value {
    serde_json::json!({
        "id": id,
        "title": title,
        "created_at": ts,
        "updated_at": ts,
        "chat": {
            "messages": [
                { "role": "user", "content": "q", "models": ["qwen3.5-plus"] },
                {
                    "role": "assistant",
                    "content": "",
                    "modelName": "Qwen3.5-Plus",
                    "content_list": [{ "phase": "answer", "content": answer }]
                }
            ]
        }
    })
}

fn find_zip(dir: &Path) -> PathBuf {
    fs::read_dir(dir)
        .expect("read dir")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| path.extension().and_then(|s| s.to_str()) == Some("zip"))
        .expect("expected a .zip output")
}

fn zip_entry_names(path: &Path) -> Vec<String> {
    let file = fs::File::open(path).expect("open zip");
    let mut archive = zip::ZipArchive::new(file).expect("read zip");
    (0..archive.len())
        .map(|index| {
            archive
                .by_index(index)
                .expect("zip entry")
                .name()
                .to_string()
        })
        .collect()
}

#[test]
fn single_export_writes_markdown_next_to_input() {
    let tmp = tempdir().expect("tempdir");
    let source = tmp.path().join("single.json");
    fs::write(&source, single_export_json()).expect("write input");

    run_qwen(&[as_str(&source)]).assert_success();

    let output = tmp.path().join("Demo Chat.md");
    assert!(output.exists(), "expected {}", output.display());

    let md = read_markdown(&output);
    assert!(md.starts_with("## Metadata\n\n"), "{md}");
    assert!(md.contains("- **Model:** `qwen3.5-plus`"));
    assert!(md.contains("- **Time:** "));
    assert!(md.contains("- **URL:** https://chat.qwen.ai/c/conv-1"));
    assert!(md.contains("## Conversation"));
    assert!(md.contains("### 🧑‍💻 User"));
    assert!(md.contains("### 🤖 Assistant"));
    assert!(md.contains("#### 🤔 Thought Process"));
    assert!(md.contains("#### 💡 Response"));
    assert!(md.contains("world!"));

    // 旧格式的痕迹必须消失
    assert!(!md.contains("### Run Settings"));
    assert!(!md.contains("models/Qwen"));
    assert!(!md.contains("Tags"));
    assert!(!md.contains("Generated At"));
}

#[test]
fn all_export_writes_zip_with_time_prefix_and_order() {
    let tmp = tempdir().expect("tempdir");
    let source = tmp.path().join("all.json");
    fs::write(
        &source,
        all_export_json(serde_json::json!([
            session("a", "Alpha", 1_700_000_000, "alpha answer"),
            session("b", "Beta", 1_700_000_100, "beta answer")
        ])),
    )
    .expect("write input");

    run_qwen(&[as_str(&source)]).assert_success();

    let zip_path = find_zip(tmp.path());
    assert!(zip_path.exists());

    let names = zip_entry_names(&zip_path);
    assert_eq!(names.len(), 2, "{names:?}");
    // 时间降序：较新的 Beta 在前
    assert!(names[0].ends_with("-Beta.md"), "{names:?}");
    assert!(names[1].ends_with("-Alpha.md"), "{names:?}");
    // 前缀应为本地时间 YYYYMMDD-HHMMSS-
    let prefix = names[0].split('-').next().expect("prefix");
    assert_eq!(prefix.len(), 8, "expected YYYYMMDD prefix in {names:?}");
}

#[test]
fn all_export_reports_skipped_items() {
    let tmp = tempdir().expect("tempdir");
    let source = tmp.path().join("all.json");
    fs::write(
        &source,
        all_export_json(serde_json::json!([
            session("a", "Alpha", 1_700_000_000, "alpha answer"),
            { "not": "a session" }
        ])),
    )
    .expect("write input");

    run_qwen(&[as_str(&source)]).assert_success();

    let names = zip_entry_names(&find_zip(tmp.path()));
    assert!(names.iter().any(|n| n == "export-failures.md"), "{names:?}");
    assert_eq!(
        names.iter().filter(|n| n.ends_with(".md")).count(),
        2,
        "{names:?}"
    );
}

#[test]
fn output_directory_flag_places_files() {
    let tmp = tempdir().expect("tempdir");
    let source = tmp.path().join("single.json");
    let out_dir = tmp.path().join("out");
    fs::write(&source, single_export_json()).expect("write input");

    run_qwen(&[as_str(&source), "-o", as_str(&out_dir)]).assert_success();

    assert!(out_dir.join("Demo Chat.md").exists());
}

#[test]
fn output_file_flag_is_honored_for_single_input() {
    let tmp = tempdir().expect("tempdir");
    let source = tmp.path().join("single.json");
    let out_file = tmp.path().join("nested").join("renamed.md");
    fs::write(&source, single_export_json()).expect("write input");

    run_qwen(&[as_str(&source), "-o", as_str(&out_file)]).assert_success();

    assert!(out_file.exists(), "expected {}", out_file.display());
}

#[test]
fn multiple_inputs_are_each_converted() {
    let tmp = tempdir().expect("tempdir");
    let single = tmp.path().join("single.json");
    let all = tmp.path().join("all.json");
    fs::write(&single, single_export_json()).expect("write single");
    fs::write(
        &all,
        all_export_json(serde_json::json!([session(
            "b",
            "Beta",
            1_700_000_100,
            "beta"
        )])),
    )
    .expect("write all");

    run_qwen(&[as_str(&single), as_str(&all)]).assert_success();

    assert!(tmp.path().join("Demo Chat.md").exists());
    assert!(find_zip(tmp.path()).exists());
}

#[test]
fn markdown_headings_in_body_become_bold() {
    let tmp = tempdir().expect("tempdir");
    let source = tmp.path().join("single.json");
    fs::write(
        &source,
        serde_json::json!([
            {
                "id": "conv-hash",
                "title": "Hash",
                "chat": {
                    "messages": [
                        { "role": "user", "content": "# Heading\nsome body" },
                        { "role": "assistant", "content": "## Answer\nbody" }
                    ]
                }
            }
        ])
        .to_string(),
    )
    .expect("write input");

    run_qwen(&[as_str(&source)]).assert_success();

    let md = read_markdown(&tmp.path().join("Hash.md"));
    assert!(md.contains("**Heading**"));
    assert!(!md.contains("# Heading"));
    assert!(!md.contains("## Answer"));
}

#[test]
fn utf8_bom_input_is_accepted() {
    let tmp = tempdir().expect("tempdir");
    let source = tmp.path().join("bom.json");
    fs::write(&source, format!("\u{feff}{}", single_export_json())).expect("write input");

    run_qwen(&[as_str(&source)]).assert_success();

    assert!(tmp.path().join("Demo Chat.md").exists());
}

#[test]
fn code_fences_keep_hash_comments() {
    let tmp = tempdir().expect("tempdir");
    let source = tmp.path().join("fence.json");
    fs::write(
        &source,
        serde_json::json!([
            {
                "id": "conv-fence",
                "title": "Fence",
                "chat": {
                    "messages": [
                        {
                            "role": "user",
                            "content": "# Top Heading\n```python\n# a comment\n```\n## After"
                        }
                    ]
                }
            }
        ])
        .to_string(),
    )
    .expect("write input");

    run_qwen(&[as_str(&source)]).assert_success();

    let md = read_markdown(&tmp.path().join("Fence.md"));
    assert!(md.contains("**Top Heading**"), "{md}");
    assert!(md.contains("**After**"), "{md}");
    assert!(md.contains("# a comment"), "{md}");
    assert!(!md.contains("**a comment**"), "{md}");
}

#[test]
fn rejects_json_that_is_not_a_qwen_session() {
    let tmp = tempdir().expect("tempdir");
    let source = tmp.path().join("tauri.conf.json");
    fs::write(
        &source,
        serde_json::json!({
            "$schema": "../node_modules/@tauri-apps/cli/config.schema.json",
            "package": { "productName": "AfterChat Converter" },
            "tauri": { "windows": [{ "title": "main" }] }
        })
        .to_string(),
    )
    .expect("write input");

    let output = run_qwen(&[as_str(&source)]);
    assert!(!output.status.success(), "should fail on non-session json");
    assert!(!tmp.path().join("untitled.md").exists());
}

trait AssertSuccess {
    fn assert_success(self);
}

impl AssertSuccess for Output {
    fn assert_success(self) {
        assert!(
            self.status.success(),
            "qwen exited with {:?}\nstdout:\n{}\nstderr:\n{}",
            self.status.code(),
            String::from_utf8_lossy(&self.stdout),
            String::from_utf8_lossy(&self.stderr)
        );
    }
}
