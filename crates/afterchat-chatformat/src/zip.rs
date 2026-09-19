//! ZIP 打包（契约 §6）。

use std::collections::HashSet;
use std::fs::File;
use std::io::Write;
use std::path::Path;

use anyhow::{Context, Result};
use rayon::prelude::*;
use zip::CompressionMethod;
use zip::write::SimpleFileOptions;

use crate::naming::{NameStyle, sanitize_path_component};
use crate::time;
use crate::{Conversation, render};

/// ZIP 内条目文件名上限（对齐 JS `makeMarkdownZipFilename`）。
pub const ZIP_NAME_MAX: usize = 100;

/// 一条无法导出的对话（写进 `export-failures.md`，契约 §6.3）。
#[derive(Debug, Clone)]
pub struct ExportFailure {
    pub title: String,
    pub id: String,
    pub reason: String,
}

/// 一次 ZIP 导出请求。
#[derive(Debug)]
pub struct ZipExport<'a> {
    /// 平台短名（小写），用于默认包名与失败报告，如 `qwen` / `cherry`。
    pub platform: &'a str,
    pub conversations: &'a [Conversation],
    pub failures: &'a [ExportFailure],
    pub output: &'a Path,
    /// 失败报告里记录的来源文件（可选）。
    pub source: Option<&'a Path>,
    pub name_style: NameStyle,
    pub show_progress: bool,
}

/// `chat-export-{platform}-all-{毫秒时间戳}.zip`（契约 §6.2）。
pub fn default_zip_name(platform: &str) -> String {
    format!(
        "chat-export-{platform}-all-{}.zip",
        chrono::Utc::now().timestamp_millis()
    )
}

/// 渲染全部对话并按契约 §6.2 打包。
pub fn write_zip(export: &ZipExport<'_>) -> Result<()> {
    let order = order_conversations(export.conversations);
    let total = order.len();

    let progress = export
        .show_progress
        .then(|| crate::make_progress_bar(total as u64, "conversations"));

    let rendered: Vec<String> = order
        .par_iter()
        .map(|&index| {
            let markdown = render(&export.conversations[index]);
            if let Some(pb) = &progress {
                pb.inc(1);
            }
            markdown
        })
        .collect();

    if let Some(pb) = progress {
        pb.finish_and_clear();
    }

    let file = File::create(export.output)
        .with_context(|| format!("failed to create {}", export.output.display()))?;
    let mut zip = zip::ZipWriter::new(file);
    let base = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    let mut used: HashSet<String> = HashSet::new();

    for (position, markdown) in rendered.iter().enumerate() {
        let conversation = &export.conversations[order[position]];
        let entry = entry_name(conversation, position, total, export.name_style, &mut used);
        let options = match conversation.time_secs.and_then(time::zip_datetime) {
            Some(datetime) => base.last_modified_time(datetime),
            None => base,
        };
        zip.start_file(entry.clone(), options)
            .with_context(|| format!("failed to start zip entry {entry}"))?;
        zip.write_all(markdown.as_bytes())
            .with_context(|| format!("failed to write zip entry {entry}"))?;
    }

    if !export.failures.is_empty() {
        let report = failure_markdown(export);
        // 失败报告是「刚生成的」，用当前时间（不设会退成 1980-01-01）
        let options = time::zip_datetime(chrono::Local::now().timestamp())
            .map(|datetime| base.last_modified_time(datetime))
            .unwrap_or(base);
        zip.start_file("export-failures.md", options)
            .context("failed to add export-failures.md")?;
        zip.write_all(report.as_bytes())
            .context("failed to write export-failures.md")?;
    }

    zip.finish()
        .with_context(|| format!("failed to finalize {}", export.output.display()))?;

    if export.failures.is_empty() {
        log::info!(
            "packed {total} conversations into {}",
            export.output.display()
        );
    } else {
        log::warn!(
            "packed {total} conversations ({} skipped) into {}",
            export.failures.len(),
            export.output.display()
        );
    }

    Ok(())
}

/// 时间降序（最新在前，用户约定）；无时间的垫底并保持原有相对顺序。
fn order_conversations(conversations: &[Conversation]) -> Vec<usize> {
    let mut order: Vec<usize> = (0..conversations.len()).collect();
    order.sort_by(
        |&a, &b| match (conversations[a].sort_ms(), conversations[b].sort_ms()) {
            (None, None) => a.cmp(&b),
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (Some(_), None) => std::cmp::Ordering::Less,
            (Some(left), Some(right)) => right.cmp(&left).then_with(|| a.cmp(&b)),
        },
    );
    order
}

fn entry_name(
    conversation: &Conversation,
    index: usize,
    total: usize,
    style: NameStyle,
    used: &mut HashSet<String>,
) -> String {
    let title = style.sanitize(&conversation.title, ZIP_NAME_MAX);
    let prefix = match conversation.sort_ms() {
        Some(ms) => time::format_local_compact(ms),
        None => {
            let width = total.to_string().len().max(3);
            format!("{:0width$}", index + 1, width = width)
        }
    };
    let base = format!("{prefix}-{title}.md");
    let name = match conversation
        .group
        .as_deref()
        .map(str::trim)
        .filter(|group| !group.is_empty())
    {
        Some(group) => format!("{}/{base}", sanitize_path_component(group, "Assistant")),
        None => base,
    };
    unique_entry_name(&name, used)
}

/// 同一条目名重复时追加 `-2` / `-3`（扩展名保持在末尾）。
fn unique_entry_name(name: &str, used: &mut HashSet<String>) -> String {
    if used.insert(name.to_string()) {
        return name.to_string();
    }

    let (stem, extension) = match name.rsplit_once('.') {
        Some((stem, extension)) => (stem.to_string(), format!(".{extension}")),
        None => (name.to_string(), String::new()),
    };

    let mut counter = 2;
    loop {
        let candidate = format!("{stem}-{counter}{extension}");
        if used.insert(candidate.clone()) {
            return candidate;
        }
        counter += 1;
    }
}

fn failure_markdown(export: &ZipExport<'_>) -> String {
    let failures = export.failures;
    let mut lines: Vec<String> = Vec::new();
    lines.push("# Export Failures".to_string());
    lines.push(String::new());
    lines.push("## Metadata".to_string());
    lines.push(String::new());
    lines.push(format!("- **Platform:** `{}`", export.platform));
    if let Some(source) = export.source {
        lines.push(format!("- **Source:** `{}`", source.display()));
    }
    lines.push(format!(
        "- **Total Conversations:** {}",
        export.conversations.len() + failures.len()
    ));
    lines.push(format!("- **Exported:** {}", export.conversations.len()));
    lines.push(format!("- **Failed:** {}", failures.len()));
    lines.push(String::new());
    lines.push("## Failed Conversations".to_string());
    lines.push(String::new());

    for failure in failures {
        lines.push(format!("- **{}**", failure.title));
        lines.push(format!("  - ID: `{}`", failure.id));
        lines.push(format!("  - Error: {}", failure.reason));
    }
    lines.push(String::new());

    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Message;

    fn conversation(title: &str, secs: Option<i64>, group: Option<&str>) -> Conversation {
        Conversation {
            title: title.to_string(),
            model: "test-model".to_string(),
            time_secs: secs,
            sort_ms: secs.map(|s| s * 1000),
            url: None,
            extra: Vec::new(),
            group: group.map(str::to_string),
            id: None,
            messages: vec![Message::user("hi")],
        }
    }

    #[test]
    fn orders_newest_first_and_none_last() {
        let convs = vec![
            conversation("old", Some(100), None),
            conversation("none", None, None),
            conversation("new", Some(300), None),
        ];
        let order = order_conversations(&convs);
        assert_eq!(order, vec![2, 0, 1]);
    }

    #[test]
    fn entry_names_include_group_and_dedupe() {
        let mut used = HashSet::new();
        let conv = conversation("A/B", Some(0), Some("Assistant"));
        let first = entry_name(&conv, 0, 2, NameStyle::Spec, &mut used);
        let second = entry_name(&conv, 1, 2, NameStyle::Spec, &mut used);
        assert!(first.starts_with("Assistant/"), "{first}");
        assert!(first.ends_with("-AB.md"), "{first}");
        assert!(second.ends_with("-AB-2.md"), "{second}");
    }

    #[test]
    fn entry_names_fall_back_to_index_without_time() {
        let mut used = HashSet::new();
        let conv = conversation("no time", None, None);
        assert_eq!(
            entry_name(&conv, 4, 10, NameStyle::Spec, &mut used),
            "005-no time.md"
        );
    }
}
