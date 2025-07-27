// Writen by Gemini 2.5 Pro Experimental 03-25, 2025/03/30

use anyhow::{Context, Result}; // Use anyhow for easy error handling
use clap::Parser;
use filetime::{set_file_times, FileTime};
use log::{error, info, warn}; // Logging macros
use rayon::prelude::*;
use regex::Regex;
use serde::Deserialize; // Trait for deserialization
use std::{fs, path::PathBuf};
use walkdir::WalkDir;
// --- Data Structures Mirroring JSON ---
// Use Option<T> for fields that might be missing or null
// Use #[serde(default)] for booleans that might be missing (defaults to false)
// Use #[serde(rename_all = "camelCase")] to map JSON keys to Rust fields

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct SafetySetting {
    // Keep fields even if unused for complete deserialization
    #[allow(dead_code)]
    category: String,
    #[allow(dead_code)]
    threshold: String,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct RunSettings {
    temperature: Option<f64>,
    model: Option<String>,
    top_p: Option<f64>,
    top_k: Option<u32>,
    max_output_tokens: Option<u32>,
    // We don't strictly need all fields unless we use them
    // safety_settings: Option<Vec<SafetySetting>>,
    // response_mime_type: Option<String>,
    // ... other fields
}

#[derive(Deserialize, Debug)]
struct SystemInstruction {
    // Text might be null or the whole object might be missing
    text: Option<String>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct Chunk {
    // Text might be null or missing
    text: Option<String>,
    // Role is expected to be present
    role: String,
    // isThought might be missing, default to false
    #[serde(default)]
    is_thought: bool,
    // token_count: Option<u32>, // ignore if not needed
    // finish_reason: Option<String>, // ignore if not needed
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct ChunkedPrompt {
    // Chunks list is expected, but might be empty
    chunks: Vec<Chunk>,
    // pending_inputs: Option<Vec<PendingInput>>, // ignore if not needed
}

// #[derive(Deserialize, Debug)]
// struct PendingInput { // ignore if not needed
//     text: Option<String>,
//     role: Option<String>,
// }

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct Root {
    run_settings: Option<RunSettings>,
    system_instruction: Option<SystemInstruction>,
    chunked_prompt: Option<ChunkedPrompt>,
}

// --- CLI Argument Parsing ---
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Path to the input JSON file.
    #[arg(required = true)] // Make it mandatory
    json_path: PathBuf,

    /// Path to the output Markdown file.
    /// If not provided, defaults to '[input_filename].md' in the same directory.
    #[arg(short, long)]
    output: Option<PathBuf>,
}

// --- Formatting Logic ---

/// Formats the metadata section from the parsed JSON data.
fn format_metadata(root: &Root) -> Vec<String> {
    let mut md_lines = Vec::new();
    let mut has_metadata = false; // Track if the "## Metadata" header was added

    // --- Run Settings ---
    // Check if run_settings exists and is Some
    if let Some(settings) = &root.run_settings {
        if !has_metadata {
            md_lines.push("## Metadata".to_string());
            md_lines.push("".to_string()); // Blank line
            has_metadata = true;
        }
        md_lines.push("### Run Settings".to_string());
        // Selectively add settings if they exist (are Some)
        if let Some(val) = &settings.model {
            md_lines.push(format!("- **Model:** `{}`", val));
        }
        if let Some(val) = settings.temperature {
            md_lines.push(format!("- **Temperature:** `{}`", val));
        }
        if let Some(val) = settings.top_p {
            md_lines.push(format!("- **Top P:** `{}`", val));
        }
        if let Some(val) = settings.top_k {
            md_lines.push(format!("- **Top K:** `{}`", val));
        }
        if let Some(val) = settings.max_output_tokens {
            md_lines.push(format!("- **Max Output Tokens:** `{}`", val));
        }
        md_lines.push("".to_string()); // Blank line after settings
    }

    // --- System Instruction ---
    // Check if system_instruction is Some and its text field is Some and not empty
    if let Some(instruction) = &root.system_instruction {
        if let Some(text) = &instruction.text {
            if !text.trim().is_empty() {
                if !has_metadata {
                    md_lines.push("## Metadata".to_string());
                    md_lines.push("".to_string());
                    // has_metadata = true; // No need to set again if already set
                }
                md_lines.push("### System Instruction".to_string());
                md_lines.push(text.trim().to_string());
                md_lines.push("".to_string());
            }
        }
    }

    md_lines
}

/// Formats the conversation turns from the parsed JSON data.
fn format_conversation(root: &Root) -> Vec<String> {
    let mut md_lines = Vec::new();

    // Check if chunked_prompt and chunks exist
    let chunks = match &root.chunked_prompt {
        Some(prompt) => &prompt.chunks,
        None => {
            warn!("No 'chunkedPrompt' found in JSON.");
            // Return only the header if no chunks
            md_lines.push("## Conversation".to_string());
            md_lines.push("".to_string());
            md_lines.push("*No conversation turns found in the JSON data.*".to_string());
            return md_lines;
        }
    };

    if chunks.is_empty() {
        warn!("'chunks' array is empty.");
        // Return only the header if chunks is empty
        md_lines.push("## Conversation".to_string());
        md_lines.push("".to_string());
        md_lines.push("*Conversation turns array is empty.*".to_string());
        return md_lines;
    }

    md_lines.push("## Conversation".to_string());
    md_lines.push("".to_string()); // Blank line

    let mut last_role: Option<String> = None;
    let mut has_thought_pending = false; // Track if the last model output was a thought

    for (i, chunk) in chunks.iter().enumerate() {
        // Borrow role string for comparisons
        let current_role = &chunk.role;
        // Get text, trim, handle None or empty
        let text = chunk
            .text
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty());

        match current_role.as_str() {
            "user" => {
                // Add space before new user turn if not the first turn
                if last_role.is_some() {
                    md_lines.push("".to_string());
                }
                md_lines.push("### 🧑‍💻 User".to_string());
                if let Some(t) = text {
                    md_lines.push(t.to_string());
                }
                last_role = Some(current_role.clone());
                has_thought_pending = false; // Reset flag on user turn
            }
            "model" => {
                // Print Assistant header only if role changes from user or it's the first model chunk
                if last_role.as_deref() != Some("model") {
                    // Add space before new assistant turn if needed
                    if last_role.is_some() {
                        md_lines.push("".to_string());
                    }
                    md_lines.push("### 🤖 Assistant".to_string());
                    // Add extra space if subheadings might follow
                    if chunk.is_thought {
                        md_lines.push("".to_string());
                    }
                }

                if chunk.is_thought {
                    // Add space before thought if previous was a response
                    if last_role.as_deref() == Some("model") && !has_thought_pending {
                        md_lines.push("".to_string());
                    }
                    md_lines.push("#### 🤔 Thought Process".to_string());
                    if let Some(t) = text {
                        md_lines.push(t.to_string());
                    }
                    has_thought_pending = true; // Mark that a thought was just processed
                } else {
                    // This chunk is a response
                    // Only add the "Response" sub-heading if it follows a thought
                    if has_thought_pending {
                        md_lines.push("".to_string()); // Space before subheading
                        md_lines.push("#### 💡 Response".to_string());
                    }
                    // If has_thought_pending is False, no sub-heading needed.
                    if let Some(t) = text {
                        md_lines.push(t.to_string());
                    }
                    has_thought_pending = false; // Reset flag as this is a response
                }
                last_role = Some(current_role.clone());
            }
            _ => {
                warn!("Chunk {} has unknown role '{}'. Skipping.", i, current_role);
                // Reset state just in case
                has_thought_pending = false;
                last_role = Some("unknown".to_string()); // Track unknown roles if needed
            }
        }
    }

    md_lines
}

fn converter(input_json_path: &PathBuf, output_md_path: &PathBuf) -> Result<()> {
    // --- Read JSON File ---
    let json_content = fs::read_to_string(&input_json_path)
        .with_context(|| format!("Failed to read JSON file: {}", input_json_path.display()))?;
    info!("JSON file loaded successfully.");

    // --- Parse JSON Content ---
    let root: Root = serde_json::from_str(&json_content).with_context(|| {
        format!(
            "Failed to parse JSON content from: {}",
            input_json_path.display()
        )
    })?;
    info!("JSON content parsed successfully.");

    // --- Generate Markdown Content ---
    let mut markdown_lines = Vec::new();

    // Add Title
    let title = format!(
        "Conversation Transcript: {}",
        input_json_path.file_stem().map_or_else(
            || "Unknown".to_string(), // Fallback if no file stem
            |stem| stem.to_string_lossy().into_owned()
        )
    );
    markdown_lines.push(title);
    markdown_lines.push("".to_string());

    // Add Metadata Section
    let metadata_md = format_metadata(&root);
    if !metadata_md.is_empty() {
        markdown_lines.extend(metadata_md);
        // Add extra space only if conversation follows and metadata was added
        if root
            .chunked_prompt
            .as_ref()
            .map_or(false, |p| !p.chunks.is_empty())
        {
            markdown_lines.push("".to_string());
        }
    }

    // Add Conversation Section
    let conversation_md = format_conversation(&root);
    markdown_lines.extend(conversation_md);

    // --- Prepare Final Output String ---
    let mut final_content = markdown_lines.join("\n");

    // Optional: Clean up multiple consecutive blank lines using regex
    // This regex replaces 3 or more newlines with exactly 2 newlines
    match Regex::new(r"\n{3,}") {
        Ok(re) => {
            final_content = re.replace_all(&final_content, "\n\n").to_string();
            final_content = final_content.trim().to_string(); // Trim leading/trailing whitespace
        }
        Err(e) => {
            error!("Failed to compile regex for cleaning newlines: {}", e);
            // Proceed without regex cleaning if it fails
            final_content = final_content.trim().to_string();
        }
    };

    // --- Write Markdown File ---
    fs::write(&output_md_path, final_content + "\n") // Ensure trailing newline
        .with_context(|| {
            format!(
                "Failed to write Markdown file: {}",
                output_md_path.display()
            )
        })?;

    info!(
        "Markdown file successfully generated: {}",
        output_md_path.display()
    );

    let input_metadata = fs::metadata(&input_json_path).with_context(|| {
        format!(
            "Failed to read metadata for source file: {}",
            input_json_path.display()
        )
    })?;

    let atime = FileTime::from_last_access_time(&input_metadata);
    let mtime = FileTime::from_last_modification_time(&input_metadata);

    if let Err(e) = set_file_times(&output_md_path, atime, mtime) {
        warn!(
            "Failed to set timestamps for {}: {}. The file was created successfully.",
            output_md_path.display(),
            e
        );
    }

    info!(
        "Markdown file successfully generated: {}",
        output_md_path.display()
    );
    Ok(())
}

// --- Main Application Logic ---
fn main() -> Result<()> {
    // Using anyhow::Result for easy error propagation
    // Initialize logger - RUST_LOG=info cargo run ...
    env_logger::init();

    // Parse command line arguments using clap
    let args = Args::parse();

    let input_path = &args.json_path; // Use a shorter name for clarity
    let input_metadata: fs::Metadata = fs::metadata(input_path).with_context(|| {
        format!(
            "Failed to read metadata for input: {}",
            input_path.display()
        )
    })?;

    // --- Handle based on Input Type (File or Directory) ---

    if input_metadata.is_file() {
        // --- Determine Output Path for Single File ---
        let output_path = match args.output {
            Some(path) => path,
            None => {
                // Default to same directory with .md extension
                let mut default_path = input_path.clone();
                if !default_path.set_extension("md") {
                    // Handle case where input path might not have a filename or extension
                    let filename = input_path
                        .file_name()
                        .map(|name| name.to_string_lossy().into_owned() + ".md")
                        .unwrap_or_else(|| "output.md".to_string());
                    default_path = input_path.join(filename);
                }
                info!(
                    "Output path not specified. Using default: {}",
                    default_path.display()
                );
                default_path
            }
        };

        // --- Ensure Output Directory Exists ---
        if let Some(parent_dir) = output_path.parent() {
            // Check if it exists *and* is a directory
            if !parent_dir.is_dir() {
                fs::create_dir_all(parent_dir).with_context(|| {
                    format!("Could not create output directory {}", parent_dir.display())
                })?;
            }
        } else {
            info!("Output path has no parent directory, assuming current directory.");
        }

        converter(input_path, &output_path).with_context(|| {
            format!(
                "Failed to convert single file: {} -> {}. Maybe something error.", // Kept your original message hint
                input_path.display(),
                output_path.display()
            )
        })?;
    } else if input_metadata.is_dir() {
        // --- Determine Output Base Directory ---
        let output_base_dir: Option<PathBuf> = match args.output {
            Some(path) => {
                if !path.is_dir() {
                    // Check if it exists *and* is a directory
                    fs::create_dir_all(&path).with_context(|| {
                        format!("Could not create output directory {}", path.display())
                    })?;
                }
                info!("Outputting to directory: {}", path.display());
                Some(path.clone())
            }
            None => {
                // Output alongside original files
                info!("Output directory not specified. Files will be generated alongside originals with .md extension.");
                None
            }
        };

        // --- Collect Files Recursively (using walkdir) ---
        // We collect paths first to easily feed them into Rayon.
        let files_to_process: Vec<PathBuf> = WalkDir::new(input_path)
            .into_iter()
            .filter_map(|entry_result| {
                // Log errors accessing directory entries, but skip them
                match entry_result {
                    Ok(entry) => Some(entry),
                    Err(e) => {
                        warn!("Error accessing entry during directory walk: {}", e);
                        None
                    }
                }
            })
            .filter(|entry| entry.file_type().is_file()) // Only process files
            .map(|entry| entry.into_path())
            .collect();

        info!(
            "Found {} potential files to process in directory.",
            files_to_process.len()
        );

        // --- Process Files in Parallel (using rayon) ---
        files_to_process
            .par_iter() // Use parallel iterator
            .for_each(|input_file_path| {
                // Use a closure to handle errors for individual files cleanly
                let result: Result<()> = (|| {
                    // --- Calculate Output Path for Each File ---
                    let output_md_path = match &output_base_dir {
                        Some(base_dir) => {
                            // Get relative path from input base dir
                            let relative_path = input_file_path.strip_prefix(input_path).expect(
                                "Internal error: file path should always be prefixed by input dir",
                            ); // Should not fail if walkdir works correctly

                            // Join with output base dir and set extension
                            let mut target_path = base_dir.join(relative_path);
                            target_path.set_extension("md"); // Handles replacing or adding extension
                            target_path
                        }
                        None => {
                            // Output alongside: Clone input path and set extension
                            let mut target_path = input_file_path.clone();
                            target_path.set_extension("md");
                            target_path
                        }
                    };

                    // --- Ensure Specific Output Directory Exists (for this file) ---
                    // This is crucial for the directory output structure.
                    if let Some(parent_dir) = output_md_path.parent() {
                        if !parent_dir.is_dir() {
                            // Avoid redundant calls if dir already exists
                            // Note: Potential race condition if multiple threads try to create the same dir.
                            // `create_dir_all` is generally idempotent, so it's usually okay.
                            fs::create_dir_all(parent_dir).with_context(|| {
                                format!(
                                    "Could not create output directory for file: {}",
                                    parent_dir.display()
                                )
                            })?;
                        }
                    }

                    // --- Call Converter for This File ---
                    converter(input_file_path, &output_md_path).with_context(|| {
                        format!(
                            "Error during conversion: {} -> {}",
                            input_file_path.display(),
                            output_md_path.display()
                        )
                    })?;
                    // Optional: Log success per file (can be verbose)
                    // info!("Successfully converted {} -> {}", input_file_path.display(), output_md_path.display());
                    Ok(())
                })(); // Immediately invoke the closure

                // --- Log Individual File Errors ---
                // Don't stop the whole process for one file error, just log it.
                if let Err(e) = result {
                    error!("Failed processing {}: {:?}", input_file_path.display(), e);
                }
            });

        info!("Finished processing directory.");
    }

    Ok(())
}
