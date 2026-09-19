//! 把 Google AI Studio 导出的 JSON 转换为 AfterChat 对话 Markdown。
//!
//! 输出契约见仓库 `docs/CHATFORMAT.md`，渲染走 `chatformat`。
//!
//! 形态（沿用既有实现）：
//! - 输入**单个 JSON** → 输出**单个 `.md``**
//! - 输入**目录**（递归找 `.json`）→ 输出**一棵平行的 `.md` 目录树**
//!
//! 时间取**输入文件的 mtime**（AI Studio 导出里没有对话时间字段）。

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::UNIX_EPOCH;

use anyhow::{Context, Result, bail};
use chatformat::{
    Conversation, Message, MetadataLine, Role, SINGLE_NAME_MAX, UNKNOWN_MODEL,
    sanitize_filename_with,
};
use clap::Parser;
use filetime::{FileTime, set_file_times};
use log::{error, info, warn};
use rayon::prelude::*;
use serde::Deserialize;
use walkdir::WalkDir;

#[derive(Debug, Parser)]
#[command(
    name = "ai-studio",
    version,
    about = "Convert Google AI Studio export JSON into AfterChat Markdown"
)]
struct Cli {
    /// Input JSON file, or a directory containing JSON files
    input: PathBuf,

    /// Output `.md` file (single input) or output directory (directory input)
    #[arg(short, long)]
    output: Option<PathBuf>,
}

// ═══════════════════════════════════════════════════════════
//  输入结构
// ═══════════════════════════════════════════════════════════

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct RunSettings {
    model: Option<String>,
    temperature: Option<f64>,
    top_p: Option<f64>,
    top_k: Option<u32>,
    max_output_tokens: Option<u32>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct SystemInstruction {
    text: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Chunk {
    #[serde(default)]
    text: Option<String>,
    role: String,
    #[serde(default)]
    is_thought: bool,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct ChunkedPrompt {
    chunks: Vec<Chunk>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct Root {
    run_settings: Option<RunSettings>,
    system_instruction: Option<SystemInstruction>,
    chunked_prompt: Option<ChunkedPrompt>,
}

impl Root {
    fn looks_like_export(&self) -> bool {
        self.run_settings.is_some()
            || self.system_instruction.is_some()
            || self.chunked_prompt.is_some()
    }
}

// ═══════════════════════════════════════════════════════════
//  映射
// ═══════════════════════════════════════════════════════════

fn to_conversation(root: &Root, title: &str, time_secs: Option<i64>) -> Conversation {
    let model = root
        .run_settings
        .as_ref()
        .and_then(|settings| settings.model.as_deref())
        .map(str::trim)
        .filter(|model| !model.is_empty())
        .unwrap_or(UNKNOWN_MODEL)
        .to_string();

    let mut extra = Vec::new();
    if let Some(settings) = &root.run_settings {
        if let Some(value) = settings.temperature {
            extra.push(MetadataLine::code("Temperature", value.to_string()));
        }
        if let Some(value) = settings.top_p {
            extra.push(MetadataLine::code("Top P", value.to_string()));
        }
        if let Some(value) = settings.top_k {
            extra.push(MetadataLine::code("Top K", value.to_string()));
        }
        if let Some(value) = settings.max_output_tokens {
            extra.push(MetadataLine::code("Max Output Tokens", value.to_string()));
        }
    }

    let mut messages = Vec::new();
    if let Some(text) = root
        .system_instruction
        .as_ref()
        .and_then(|instruction| instruction.text.as_deref())
        .map(str::trim)
        .filter(|text| !text.is_empty())
    {
        messages.push(Message::system(text));
    }
    let chunks = root
        .chunked_prompt
        .as_ref()
        .map(|prompt| prompt.chunks.as_slice())
        .unwrap_or(&[]);
    messages.extend(build_messages(chunks));

    Conversation {
        title: title.to_string(),
        model,
        time_secs,
        sort_ms: None,
        url: None,
        extra,
        group: None,
        id: None,
        messages,
    }
}

/// 相邻同角色 chunk 合并成一条消息：`isThought` 归思维链，其余归正文。
/// 未知角色一律兜底成 Assistant（契约 §4.2）。
fn build_messages(chunks: &[Chunk]) -> Vec<Message> {
    let mut messages: Vec<Message> = Vec::new();
    let mut cursor = 0;

    while cursor < chunks.len() {
        let role = normalize_role(&chunks[cursor].role);
        let mut thinking: Vec<String> = Vec::new();
        let mut body: Vec<String> = Vec::new();

        while cursor < chunks.len() && normalize_role(&chunks[cursor].role) == role {
            let chunk = &chunks[cursor];
            if let Some(text) = chunk
                .text
                .as_deref()
                .map(str::trim)
                .filter(|text| !text.is_empty())
            {
                if role == Role::Assistant && chunk.is_thought {
                    thinking.push(text.to_string());
                } else {
                    body.push(text.to_string());
                }
            }
            cursor += 1;
        }

        if thinking.is_empty() && body.is_empty() {
            continue;
        }
        messages.push(Message {
            role,
            thinking,
            body,
        });
    }

    messages
}

fn normalize_role(role: &str) -> Role {
    if role.eq_ignore_ascii_case("user") {
        Role::User
    } else {
        Role::Assistant
    }
}

// ═══════════════════════════════════════════════════════════
//  IO
// ═══════════════════════════════════════════════════════════

fn load_json(path: &Path) -> Result<Root> {
    let raw =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    // ChatFormat 允许 UTF-8 BOM，serde_json 不接受，先剥掉。
    let root: Root = serde_json::from_str(raw.trim_start_matches('\u{feff}'))
        .with_context(|| format!("failed to parse JSON in {}", path.display()))?;
    if !root.looks_like_export() {
        bail!(
            "not a Google AI Studio export (no runSettings / systemInstruction / chunkedPrompt): {}",
            path.display()
        );
    }
    Ok(root)
}

fn file_mtime_secs(path: &Path) -> Option<i64> {
    let modified = fs::metadata(path).ok()?.modified().ok()?;
    modified
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|delta| delta.as_secs() as i64)
}

fn copy_times(target: &Path, source: &Path) {
    let Ok(metadata) = fs::metadata(source) else {
        return;
    };
    let atime = FileTime::from_last_access_time(&metadata);
    let mtime = FileTime::from_last_modification_time(&metadata);
    if let Err(err) = set_file_times(target, atime, mtime) {
        warn!("failed to set timestamps on {}: {err}", target.display());
    }
}

fn ensure_parent(path: &Path) -> Result<()> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    Ok(())
}

fn is_markdown(path: &Path) -> bool {
    path.extension()
        .and_then(|value| value.to_str())
        .is_some_and(|value| value.eq_ignore_ascii_case("md"))
}

fn is_json(path: &Path) -> bool {
    path.extension()
        .and_then(|value| value.to_str())
        .is_some_and(|value| value.eq_ignore_ascii_case("json"))
}

fn default_stem(path: &Path) -> String {
    path.file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .filter(|stem| !stem.trim().is_empty())
        .unwrap_or_else(|| "conversation".to_string())
}

fn convert_file(input: &Path) -> Result<Root> {
    load_json(input)
}

fn render_to(root: &Root, source: &Path) -> String {
    let title = default_stem(source);
    let conversation = to_conversation(root, &title, file_mtime_secs(source));
    conversation.render()
}

fn output_for_single(input: &Path, output: Option<&Path>) -> Result<PathBuf> {
    match output {
        Some(path) if is_markdown(path) => Ok(path.to_path_buf()),
        Some(dir) => {
            fs::create_dir_all(dir)
                .with_context(|| format!("failed to create output dir {}", dir.display()))?;
            let name = sanitize_filename_with(
                &default_stem(input),
                SINGLE_NAME_MAX,
                "Untitled_Conversation",
            );
            Ok(dir.join(format!("{name}.md")))
        }
        None => Ok(input.with_extension("md")),
    }
}

fn output_for_tree_member(
    input_root: &Path,
    file: &Path,
    output: Option<&Path>,
) -> Result<PathBuf> {
    match output {
        Some(base) => {
            let relative = file.strip_prefix(input_root).with_context(|| {
                format!("{} is not under {}", file.display(), input_root.display())
            })?;
            let mut target = base.join(relative);
            target.set_extension("md");
            Ok(target)
        }
        None => Ok(file.with_extension("md")),
    }
}

fn run(input: &Path, output: Option<&Path>) -> Result<usize> {
    if !input.exists() {
        bail!("input path does not exist: {}", input.display());
    }

    if input.is_file() {
        let root = convert_file(input)?;
        let target = output_for_single(input, output)?;
        ensure_parent(&target)?;
        fs::write(&target, render_to(&root, input))
            .with_context(|| format!("failed to write {}", target.display()))?;
        copy_times(&target, input);
        info!("wrote {}", target.display());
        return Ok(1);
    }

    if !input.is_dir() {
        bail!(
            "input must be a JSON file or a directory: {}",
            input.display()
        );
    }

    let mut files: Vec<PathBuf> = WalkDir::new(input)
        .into_iter()
        .filter_map(|entry| match entry {
            Ok(entry) => Some(entry),
            Err(err) => {
                warn!("error walking directory: {err}");
                None
            }
        })
        .filter(|entry| entry.file_type().is_file())
        .map(|entry| entry.into_path())
        .filter(|path| is_json(path))
        .collect();
    files.sort();

    let results: Vec<Result<PathBuf>> = files
        .par_iter()
        .map(|source| {
            let root = load_json(source)?;
            let target = output_for_tree_member(input, source, output)?;
            ensure_parent(&target)?;
            fs::write(&target, render_to(&root, source))
                .with_context(|| format!("failed to write {}", target.display()))?;
            copy_times(&target, source);
            Ok(target)
        })
        .collect();

    let mut written = 0;
    for result in results {
        match result {
            Ok(target) => {
                written += 1;
                info!("wrote {}", target.display());
            }
            Err(err) => error!("{err:#}"),
        }
    }

    info!(
        "converted {written}/{} files under {}",
        files.len(),
        input.display()
    );
    Ok(written)
}

fn main() -> ExitCode {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let cli = Cli::parse();
    match run(&cli.input, cli.output.as_deref()) {
        Ok(_) => ExitCode::SUCCESS,
        Err(err) => {
            error!("{err:#}");
            ExitCode::from(1)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> Root {
        serde_json::from_str(text).expect("should parse")
    }

    #[test]
    fn renders_contract_skeleton() {
        let root = parse(
            r##"{
                "runSettings": {"model": "gemini-2.5-pro", "temperature": 1, "topP": 0.95},
                "systemInstruction": {"text": "# 你是助手"},
                "chunkedPrompt": {"chunks": [
                    {"role": "user", "text": "# 问题"},
                    {"role": "model", "text": "想想", "isThought": true},
                    {"role": "model", "text": "答案"}
                ]}
            }"##,
        );
        let conversation = to_conversation(&root, "sample", Some(1_725_891_751));
        let markdown = conversation.render();

        assert!(markdown.starts_with("## Metadata\n"), "{markdown}");
        assert!(!markdown.contains("Conversation Transcript"), "{markdown}");
        assert!(
            markdown.contains("- **Model:** `gemini-2.5-pro`\n"),
            "{markdown}"
        );
        assert!(markdown.contains("- **Time:** 2024-"), "{markdown}");
        assert!(markdown.contains("- **Temperature:** `1`\n"), "{markdown}");
        assert!(markdown.contains("- **Top P:** `0.95`\n"), "{markdown}");
        assert!(
            markdown.contains("### ⚙️ System\n\n**你是助手**\n"),
            "{markdown}"
        );
        assert!(markdown.contains("### 🧑‍💻 User\n\n**问题**\n"), "{markdown}");
        assert!(
            markdown.contains("#### 🤔 Thought Process\n\n想想\n"),
            "{markdown}"
        );
        assert!(
            markdown.contains("#### 💡 Response\n\n答案\n"),
            "{markdown}"
        );
    }

    #[test]
    fn merges_consecutive_blocks_and_skips_empty() {
        let chunks = vec![
            Chunk {
                text: None,
                role: "user".into(),
                is_thought: false,
            },
            Chunk {
                text: Some("a".into()),
                role: "model".into(),
                is_thought: true,
            },
            Chunk {
                text: Some("b".into()),
                role: "model".into(),
                is_thought: true,
            },
            Chunk {
                text: Some("c".into()),
                role: "model".into(),
                is_thought: false,
            },
        ];
        let messages = build_messages(&chunks);
        assert_eq!(messages.len(), 1, "{messages:?}");
        assert_eq!(messages[0].thinking, vec!["a", "b"]);
        assert_eq!(messages[0].body, vec!["c"]);
    }

    #[test]
    fn unknown_role_falls_back_to_assistant() {
        let chunks = vec![Chunk {
            text: Some("x".into()),
            role: "system".into(),
            is_thought: false,
        }];
        let messages = build_messages(&chunks);
        assert_eq!(messages[0].role, Role::Assistant);
    }

    #[test]
    fn no_thinking_means_no_response_heading() {
        let root = parse(
            r#"{"chunkedPrompt": {"chunks": [
                {"role": "user", "text": "q"},
                {"role": "model", "text": "a"}
            ]}}"#,
        );
        let markdown = to_conversation(&root, "t", None).render();
        assert!(!markdown.contains("#### 💡 Response"), "{markdown}");
        assert!(!markdown.contains("#### 🤔 Thought Process"), "{markdown}");
        assert!(markdown.contains("### 🤖 Assistant\n\na\n"), "{markdown}");
        assert!(!markdown.contains("- **Time:**"), "{markdown}");
    }
}
