//! 目录展开导出与 AfterChat ZIP 解包。

use std::collections::HashSet;
use std::fs::{self, File};
use std::path::Path;
use std::time::{Duration, UNIX_EPOCH};

use anyhow::{Context, Result};
use rayon::prelude::*;

use crate::naming::NameStyle;
use crate::zip::{ExportFailure, entry_name, failure_markdown_content, order_conversations};
use crate::{Conversation, render};

/// 一次目录展开导出请求（直接把各会话落地为 Markdown 文件树）。
#[derive(Debug)]
pub struct DirExport<'a> {
    pub platform: &'a str,
    pub conversations: &'a [Conversation],
    pub failures: &'a [ExportFailure],
    pub output: &'a Path,
    pub source: Option<&'a Path>,
    pub name_style: NameStyle,
    pub show_progress: bool,
}

/// 渲染全部对话并直接写入指定目录（不打包为 ZIP）。
pub fn write_directory(export: &DirExport<'_>) -> Result<()> {
    fs::create_dir_all(export.output).with_context(|| {
        format!(
            "failed to create output directory {}",
            export.output.display()
        )
    })?;

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

    let mut used: HashSet<String> = HashSet::new();

    for (position, markdown) in rendered.iter().enumerate() {
        let conversation = &export.conversations[order[position]];
        let entry = entry_name(conversation, position, total, export.name_style, &mut used);
        let target_file = export.output.join(&entry);

        if let Some(parent) = target_file.parent() {
            if !parent.exists() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("failed to create directory {}", parent.display()))?;
            }
        }

        fs::write(&target_file, markdown.as_bytes())
            .with_context(|| format!("failed to write {}", target_file.display()))?;

        // 设置文件 mtime 为会话发生时间，方便 Obsidian / AfterChat 按时间排序索引
        if let Some(secs) = conversation.time_secs {
            let mtime = UNIX_EPOCH + Duration::from_secs(secs as u64);
            let ft = std::fs::FileTimes::new().set_modified(mtime);
            if let Ok(file) = File::options().write(true).open(&target_file) {
                let _ = file.set_times(ft);
            }
        }
    }

    if !export.failures.is_empty() {
        let report = failure_markdown_content(
            export.platform,
            export.conversations.len(),
            export.failures,
            export.source,
        );
        let failure_path = export.output.join("export-failures.md");
        let _ = fs::write(failure_path, report.as_bytes());
    }

    log::info!(
        "wrote {total} markdown files into {}",
        export.output.display()
    );

    Ok(())
}

/// 检查给定 ZIP 是否为已符合 AfterChat 规范的 ZIP（包含 Markdown 文件且无原始平台标记）。
pub fn is_afterchat_zip(zip_path: &Path) -> bool {
    let Ok(file) = File::open(zip_path) else {
        return false;
    };
    let Ok(mut archive) = zip::ZipArchive::new(file) else {
        return false;
    };

    let mut has_md = false;
    for i in 0..archive.len() {
        let Ok(entry) = archive.by_index(i) else {
            return false;
        };
        let name = entry.name().to_ascii_lowercase();
        if name.ends_with(".md") {
            has_md = true;
        }
        // 若包含其他平台特有的原始文件，交由对应的平台转换器解析，不当作普通解压
        if name.ends_with(".json")
            || name.ends_with(".db")
            || name.ends_with(".sqlite")
            || name.ends_with(".sqlite3")
        {
            return false;
        }
    }
    has_md
}

/// 把 AfterChat 规范的 ZIP 归档包解压提取到目标目录。
pub fn unpack_zip(zip_path: &Path, output_dir: &Path) -> Result<usize> {
    fs::create_dir_all(output_dir)
        .with_context(|| format!("failed to create output dir {}", output_dir.display()))?;

    let file = File::open(zip_path)
        .with_context(|| format!("failed to open zip {}", zip_path.display()))?;
    let mut archive = zip::ZipArchive::new(file)
        .with_context(|| format!("failed to read zip {}", zip_path.display()))?;

    let mut count = 0;
    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .with_context(|| format!("failed to read zip entry #{i}"))?;
        let name = entry.name().to_string();

        // 安全检查：防路径穿越
        if name.contains("..") || name.starts_with('/') || name.starts_with('\\') {
            continue;
        }

        let target_path = output_dir.join(&name);
        if entry.is_dir() {
            fs::create_dir_all(&target_path)?;
            continue;
        }

        if let Some(parent) = target_path.parent() {
            fs::create_dir_all(parent)?;
        }

        let mut outfile = File::create(&target_path)
            .with_context(|| format!("failed to create {}", target_path.display()))?;
        std::io::copy(&mut entry, &mut outfile)?;
        count += 1;
    }

    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Message;
    use std::io::Write;
    use zip::write::SimpleFileOptions;

    #[test]
    fn write_directory_creates_markdown_files_and_failures() {
        let temp = tempfile::tempdir().unwrap();
        let out_dir = temp.path().join("vault");

        let convs = vec![
            Conversation {
                title: "Topic 1".into(),
                model: "gpt-4o".into(),
                time_secs: Some(1_700_000_000),
                sort_ms: Some(1_700_000_000_000),
                url: None,
                extra: Vec::new(),
                group: Some("AssistantA".into()),
                id: Some("id-1".into()),
                messages: vec![Message::user("hello")],
            },
            Conversation {
                title: "Topic 2".into(),
                model: "claude-3-5".into(),
                time_secs: Some(1_700_001_000),
                sort_ms: Some(1_700_001_000_000),
                url: None,
                extra: Vec::new(),
                group: None,
                id: Some("id-2".into()),
                messages: vec![Message::user("world")],
            },
        ];

        let failures = vec![ExportFailure {
            title: "Empty Topic".into(),
            id: "id-3".into(),
            reason: "empty message".into(),
        }];

        let export = DirExport {
            platform: "test",
            conversations: &convs,
            failures: &failures,
            output: &out_dir,
            source: None,
            name_style: NameStyle::Spec,
            show_progress: false,
        };

        write_directory(&export).unwrap();

        assert!(out_dir.exists());
        assert!(out_dir.join("export-failures.md").exists());
        assert!(out_dir.join("AssistantA").is_dir());

        let files: Vec<String> = walkdir::WalkDir::new(&out_dir)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();

        assert!(files.iter().any(|f| f.ends_with("-Topic 1.md")));
        assert!(files.iter().any(|f| f.ends_with("-Topic 2.md")));
        assert!(files.iter().any(|f| f == "export-failures.md"));
    }

    #[test]
    fn detects_afterchat_zip_and_unpacks() {
        let temp = tempfile::tempdir().unwrap();
        let afterchat_zip = temp.path().join("afterchat.zip");
        let raw_zip = temp.path().join("chatgpt_raw.zip");

        // 1. 创建符合 AfterChat 规范的 ZIP（仅含 .md）
        {
            let file = File::create(&afterchat_zip).unwrap();
            let mut writer = zip::ZipWriter::new(file);
            writer
                .start_file("sample.md", SimpleFileOptions::default())
                .unwrap();
            writer.write_all(b"# Hello").unwrap();
            writer.finish().unwrap();
        }

        // 2. 创建包含原始平台标记的 ZIP（含 conversations.json）
        {
            let file = File::create(&raw_zip).unwrap();
            let mut writer = zip::ZipWriter::new(file);
            writer
                .start_file("conversations.json", SimpleFileOptions::default())
                .unwrap();
            writer.write_all(b"[]").unwrap();
            writer.finish().unwrap();
        }

        assert!(is_afterchat_zip(&afterchat_zip));
        assert!(!is_afterchat_zip(&raw_zip));

        // 3. 测试解压
        let extract_dir = temp.path().join("extracted");
        let count = unpack_zip(&afterchat_zip, &extract_dir).unwrap();
        assert_eq!(count, 1);
        assert!(extract_dir.join("sample.md").exists());
        assert_eq!(
            fs::read_to_string(extract_dir.join("sample.md")).unwrap(),
            "# Hello"
        );
    }
}
