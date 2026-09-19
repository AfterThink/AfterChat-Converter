use anyhow::{Result, bail};
use chrono::DateTime;
use filetime::{FileTime, set_file_times};
use indicatif::{ProgressBar, ProgressStyle};
use log::{info, warn};
use rayon::prelude::*;
use regex::Regex;
use serde::Deserialize;
use serde_json::Value;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

#[derive(Debug, Clone)]
pub struct ConvertOptions {
    pub input: PathBuf,
    pub output: Option<PathBuf>,
    pub show_progress: bool,
}

#[derive(Debug, Default, Clone)]
pub struct RunSummary {
    pub input_files: usize,
    pub generated_files: usize,
    pub failed_files: usize,
    pub error_log: Option<PathBuf>,
}

#[derive(Debug, Clone, Deserialize)]
struct Conversation {
    name: Option<String>,
    created_at: Option<String>,
    updated_at: Option<String>,
    #[serde(alias = "model_name", alias = "current_model", alias = "model_type")]
    model: Option<String>,
    metadata: Option<Value>,
    chat_messages: Vec<Message>,
}

#[derive(Debug, Clone, Deserialize)]
struct Message {
    sender: String,
    text: Option<String>,
    // created_at: Option<String>,
    #[serde(alias = "model_name")]
    model: Option<String>,
    metadata: Option<Value>,
    content: Option<Vec<MessageContent>>,
}

#[derive(Debug, Clone, Deserialize)]
struct MessageContent {
    #[serde(rename = "type")]
    content_type: String,
    text: Option<String>,
}

pub fn run_conversion(options: ConvertOptions) -> Result<RunSummary> {
    let mut summary = RunSummary::default();
    let input = &options.input;

    if !input.exists() {
        bail!("Input path does not exist: {}", input.display());
    }

    // Determine output base directory
    let output_is_md_file = options.output.as_ref()
        .map(|p| p.extension().and_then(|s| s.to_str()) == Some("md"))
        .unwrap_or(false);

    let output_root = if let Some(path) = &options.output {
        if output_is_md_file {
            path.parent().unwrap_or(Path::new(".")).to_path_buf()
        } else {
            path.clone()
        }
    } else {
        input.parent().unwrap_or(Path::new(".")).to_path_buf()
    };

    if !output_root.exists() {
        fs::create_dir_all(&output_root)?;
    }

    let conversations = if input.is_dir() {
        let mut all_convs = Vec::new();
        for entry in WalkDir::new(input).into_iter().filter_map(|e| e.ok()) {
            if entry.path().extension().map_or(false, |ext| ext == "json") {
                if let Ok(json_convs) = load_from_json(entry.path()) {
                    all_convs.extend(json_convs);
                    summary.input_files += 1;
                }
            } else if entry.path().extension().map_or(false, |ext| ext == "zip") {
                if let Ok(zip_convs) = load_from_zip(entry.path()) {
                    all_convs.extend(zip_convs);
                    summary.input_files += 1;
                }
            }
        }
        all_convs
    } else if input.extension().map_or(false, |ext| ext == "json") {
        summary.input_files = 1;
        load_from_json(input)?
    } else if input.extension().map_or(false, |ext| ext == "zip") {
        summary.input_files = 1;
        load_from_zip(input)?
    } else {
        bail!("Unsupported file type: {}", input.display());
    };

    if conversations.is_empty() {
        info!("No conversations found to process.");
        return Ok(summary);
    }

    let pb = if options.show_progress {
        let pb = ProgressBar::new(conversations.len() as u64);
        pb.set_style(
            ProgressStyle::default_bar()
                .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} ({eta})")
                .unwrap()
                .progress_chars("#>-"),
        );
        Some(pb)
    } else {
        None
    };

    // If we have multiple conversations and output is a single .md file, it's problematic
    // Let's create a directory for the batch if there's more than one conversation
    let output_dir = if conversations.len() > 1 && output_is_md_file {
        let dir = options.output.as_ref().unwrap().with_extension("");
        if !dir.exists() { fs::create_dir_all(&dir)?; }
        dir
    } else if conversations.len() > 1 && options.output.is_none() {
        let dir = output_root.join(format!("{}_markdowns", input.file_stem().unwrap_or_default().to_string_lossy()));
        if !dir.exists() { fs::create_dir_all(&dir)?; }
        dir
    } else {
        output_root
    };

    let conv_count = conversations.len();
    let results: Vec<Result<()>> = conversations
        .into_par_iter()
        .map(|conv| {
            let res = convert_conversation(&conv, &output_dir, output_is_md_file && conv_count == 1, options.output.as_ref());
            if let Some(ref pb) = pb {
                pb.inc(1);
            }
            res
        })
        .collect();

    if let Some(pb) = pb {
        pb.finish_with_message("done");
    }

    for res in results {
        match res {
            Ok(_) => summary.generated_files += 1,
            Err(e) => {
                warn!("Failed to convert conversation: {}", e);
                summary.failed_files += 1;
            }
        }
    }

    Ok(summary)
}

fn load_from_json(path: &Path) -> Result<Vec<Conversation>> {
    let content = fs::read_to_string(path)?;
    if let Ok(convs) = serde_json::from_str::<Vec<Conversation>>(&content) {
        Ok(convs)
    } else if let Ok(conv) = serde_json::from_str::<Conversation>(&content) {
        Ok(vec![conv])
    } else {
        bail!("Failed to parse Claude JSON from {}", path.display());
    }
}

fn load_from_zip(path: &Path) -> Result<Vec<Conversation>> {
    let file = fs::File::open(path)?;
    let mut archive = zip::ZipArchive::new(file)?;
    
    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        if file.name() == "conversations.json" {
            let mut content = String::new();
            file.read_to_string(&mut content)?;
            if let Ok(convs) = serde_json::from_str::<Vec<Conversation>>(&content) {
                return Ok(convs);
            } else if let Ok(conv) = serde_json::from_str::<Conversation>(&content) {
                return Ok(vec![conv]);
            } else {
                bail!("Failed to parse Claude JSON from zip's conversations.json");
            }
        }
    }
    
    bail!("Could not find conversations.json in {}", path.display());
}

fn convert_conversation(conv: &Conversation, output_dir: &Path, use_explicit_output: bool, explicit_output: Option<&PathBuf>) -> Result<()> {
    let title = conv.name.as_deref().unwrap_or("Untitled");
    let safe_title = sanitize_filename(title);
    
    let mut markdown_lines = Vec::new();

    let mut model_name = conv.model.clone();
    
    // Check conversation metadata
    if model_name.is_none() {
        if let Some(metadata) = &conv.metadata {
            if let Some(m) = metadata.get("model").and_then(|v| v.as_str()) {
                model_name = Some(m.to_string());
            } else if let Some(m) = metadata.get("model_name").and_then(|v| v.as_str()) {
                model_name = Some(m.to_string());
            }
        }
    }

    // Check message model/metadata
    if model_name.is_none() {
        for msg in &conv.chat_messages {
            if let Some(m) = &msg.model {
                model_name = Some(m.clone());
                break;
            }
            if let Some(metadata) = &msg.metadata {
                if let Some(m) = metadata.get("model").and_then(|v| v.as_str()) {
                    model_name = Some(m.to_string());
                    break;
                } else if let Some(m) = metadata.get("model_name").and_then(|v| v.as_str()) {
                    model_name = Some(m.to_string());
                    break;
                }
            }
        }
    }

    // 1. Add Title (standard: Conversation Transcript: [Name])
    markdown_lines.push(format!("Conversation Transcript: {}", title));
    markdown_lines.push("".to_string());

    // 2. Add Metadata Section (standard: ## Metadata)
    markdown_lines.push("## Metadata".to_string());
    markdown_lines.push("".to_string());
    if let Some(model) = model_name {
        markdown_lines.push(format!("- **Model:** `{}`", model));
    }
    if let Some(created_at) = &conv.created_at {
        markdown_lines.push(format!("- **Created at:** `{}`", created_at));
    }
    if let Some(updated_at) = &conv.updated_at {
        markdown_lines.push(format!("- **Updated at:** `{}`", updated_at));
    }
    markdown_lines.push("".to_string());

    // 3. Add Conversation Section (standard: ## Conversation)
    markdown_lines.push("## Conversation".to_string());
    markdown_lines.push("".to_string());

    for msg in &conv.chat_messages {
        let (role_header, emoji) = match msg.sender.as_str() {
            "human" => ("User", "🧑‍💻"),
            "assistant" => ("Assistant", "🤖"),
            other => (other, "👤"),
        };
        
        let content = if let Some(text) = &msg.text {
            text.trim().to_string()
        } else if let Some(contents) = &msg.content {
            contents.iter()
                .filter_map(|c| if c.content_type == "text" { c.text.as_deref().map(str::trim) } else { None })
                .collect::<Vec<_>>()
                .join("\n\n")
        } else {
            String::new()
        };

        if !content.is_empty() {
            markdown_lines.push(format!("### {} {}", emoji, role_header));
            markdown_lines.push("".to_string());
            markdown_lines.push(content);
            markdown_lines.push("".to_string());
        }
    }

    // 4. Clean up consecutive blank lines (standard: max 2)
    let mut final_content = markdown_lines.join("\n");
    if let Ok(re) = Regex::new(r"\n{3,}") {
        final_content = re.replace_all(&final_content, "\n\n").to_string();
    }
    final_content = final_content.trim().to_string() + "\n";

    // 5. Determine Final Path
    let final_path = if use_explicit_output && explicit_output.is_some() {
        explicit_output.unwrap().clone()
    } else {
        let filename = format!("{}.md", safe_title);
        let mut path = output_dir.join(filename);
        let mut count = 1;
        while path.exists() {
            path = output_dir.join(format!("{} ({}).md", safe_title, count));
            count += 1;
        }
        path
    };

    // 6. Write File
    fs::write(&final_path, final_content)?;

    // 7. Set File Timestamps (standard)
    if let Some(created_str) = &conv.created_at {
        if let Ok(dt) = DateTime::parse_from_rfc3339(created_str) {
            let ft = FileTime::from_unix_time(dt.timestamp(), 0);
            let _ = set_file_times(&final_path, ft, ft);
        }
    }

    Ok(())
}

fn sanitize_filename(name: &str) -> String {
    let mut name = name.chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            _ => c,
        })
        .collect::<String>();
    
    if name.is_empty() {
        name = "Untitled".to_string();
    }
    
    if name.len() > 200 {
        name.truncate(200);
    }
    
    name.trim().to_string()
}
