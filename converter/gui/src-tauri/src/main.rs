#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;

use serde::{Deserialize, Serialize};
use tauri::Manager;
use tauri::api::process::{Command, CommandEvent};

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct LaunchConfig {
    output_path: Option<String>,
    lang: Option<String>,
}

fn parse_launch_config() -> LaunchConfig {
    let args: Vec<String> = std::env::args().collect();
    let mut output_path = None;
    let mut lang = None;
    let mut i = 1;
    while i < args.len() {
        if (args[i] == "-o" || args[i] == "--output") && i + 1 < args.len() {
            output_path = Some(args[i + 1].clone());
            i += 2;
            continue;
        }
        if (args[i] == "-l" || args[i] == "--lang") && i + 1 < args.len() {
            lang = Some(args[i + 1].clone());
            i += 2;
            continue;
        }
        i += 1;
    }
    LaunchConfig { output_path, lang }
}

#[tauri::command]
fn get_launch_config(config: tauri::State<LaunchConfig>) -> LaunchConfig {
    config.inner().clone()
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
enum ConverterKind {
    AiStudio,
    Cherry,
    Qwen,
    Claude,
}

const ALL_CONVERTERS: [ConverterKind; 4] = [
    ConverterKind::AiStudio,
    ConverterKind::Cherry,
    ConverterKind::Qwen,
    ConverterKind::Claude,
];

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConvertRequest {
    converter: Option<ConverterKind>,
    input_path: String,
    output_path: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct InputInfo {
    path: String,
    name: String,
    is_dir: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ConvertResponse {
    exit_code: i32,
    stdout: String,
    stderr: String,
    output_path: String,
}

#[tauri::command]
fn inspect_input(path: String) -> Result<InputInfo, String> {
    let input_path = PathBuf::from(&path);
    let metadata = fs::metadata(&input_path)
        .map_err(|error| format!("无法读取输入路径 {}: {error}", input_path.display()))?;

    let name = input_path
        .file_name()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| input_path.display().to_string());

    if metadata.is_dir() {
        if !directory_contains_json(&input_path)
            .map_err(|error| format!("无法读取目录 {}: {error}", input_path.display()))?
        {
            return Err(format!(
                "目录中没有可转换的 JSON 文件：{}",
                input_path.display()
            ));
        }
    }

    Ok(InputInfo {
        path,
        name,
        is_dir: metadata.is_dir(),
    })
}

#[tauri::command]
async fn run_conversion(request: ConvertRequest) -> Result<ConvertResponse, String> {
    let input_path = PathBuf::from(&request.input_path);
    let output_path = request.output_path.as_ref().map(PathBuf::from);
    let args = build_args(&input_path, output_path.as_ref());

    if let Some(converter) = request.converter {
        let result = try_sidecar(converter, &args).await?;
        let predicted_output =
            predict_output_path(Some(converter), &input_path, output_path.as_ref());
        return Ok(ConvertResponse {
            exit_code: result.exit_code,
            stdout: result.stdout,
            stderr: result.stderr,
            output_path: predicted_output.display().to_string(),
        });
    }

    // Try all converters, first exit 0 wins
    let mut last_error = String::new();
    for kind in ALL_CONVERTERS {
        let result = try_sidecar(kind, &args).await?;
        if result.exit_code == 0 {
            let predicted_output =
                predict_output_path(Some(kind), &input_path, output_path.as_ref());
            return Ok(ConvertResponse {
                exit_code: result.exit_code,
                stdout: result.stdout,
                stderr: result.stderr,
                output_path: predicted_output.display().to_string(),
            });
        }
        last_error = [result.stderr, result.stdout]
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("\n");
    }

    let predicted_output = predict_output_path(None, &input_path, output_path.as_ref());
    Ok(ConvertResponse {
        exit_code: 1,
        stdout: String::new(),
        stderr: if last_error.is_empty() {
            "所有转换器均无法处理此文件".to_string()
        } else {
            last_error
        },
        output_path: predicted_output.display().to_string(),
    })
}

struct SidecarResult {
    exit_code: i32,
    stdout: String,
    stderr: String,
}

async fn try_sidecar(converter: ConverterKind, args: &[String]) -> Result<SidecarResult, String> {
    let sidecar_name = match converter {
        ConverterKind::AiStudio => "ai-studio",
        ConverterKind::Cherry => "cherry",
        ConverterKind::Qwen => "qwen",
        ConverterKind::Claude => "claude",
    };

    let (mut receiver, _child) = Command::new_sidecar(sidecar_name)
        .map_err(|error| format!("无法创建 sidecar 命令 {sidecar_name}: {error}"))?
        .args(args)
        .spawn()
        .map_err(|error| format!("无法启动转换器 {sidecar_name}: {error}"))?;

    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let mut exit_code = -1;

    while let Some(event) = receiver.recv().await {
        match event {
            CommandEvent::Stdout(line) => stdout.push(line),
            CommandEvent::Stderr(line) => stderr.push(line),
            CommandEvent::Error(line) => stderr.push(line),
            CommandEvent::Terminated(payload) => {
                exit_code = payload.code.unwrap_or_default();
                break;
            }
            _ => {}
        }
    }

    Ok(SidecarResult {
        exit_code,
        stdout: stdout.join("\n").trim().to_string(),
        stderr: stderr.join("\n").trim().to_string(),
    })
}

fn predict_output_path(
    converter: Option<ConverterKind>,
    input_path: &Path,
    output_path: Option<&PathBuf>,
) -> PathBuf {
    if let Some(path) = output_path {
        return path.clone();
    }

    match converter {
        Some(ConverterKind::Cherry) => input_path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."))
            .join("cherry-studio-export"),
        Some(ConverterKind::Claude) => input_path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."))
            .join("claude_markdowns"),
        _ => {
            if input_path.is_dir() {
                input_path.to_path_buf()
            } else {
                input_path
                    .parent()
                    .map(Path::to_path_buf)
                    .unwrap_or_else(|| PathBuf::from("."))
            }
        }
    }
}

#[tauri::command]
fn reveal_in_file_manager(path: String) -> Result<(), String> {
    let target = PathBuf::from(&path);
    if !target.exists() {
        return Err(format!("输出路径不存在：{}", target.display()));
    }

    reveal_path(&target).map_err(|error| format!("无法打开输出位置 {}: {error}", target.display()))
}

fn build_args(input_path: &Path, output_path: Option<&PathBuf>) -> Vec<String> {
    let mut args = vec![input_path.display().to_string()];
    if let Some(path) = output_path {
        args.extend(["-o".to_string(), path.display().to_string()]);
    }
    args
}

fn directory_contains_json(path: &Path) -> std::io::Result<bool> {
    let mut stack = vec![path.to_path_buf()];

    while let Some(current) = stack.pop() {
        for entry in fs::read_dir(current)? {
            let entry = entry?;
            let entry_path = entry.path();

            if entry.file_type()?.is_dir() {
                stack.push(entry_path);
                continue;
            }

            if entry_path
                .extension()
                .and_then(|value| value.to_str())
                .is_some_and(|value| value.eq_ignore_ascii_case("json"))
            {
                return Ok(true);
            }
        }
    }

    Ok(false)
}

fn reveal_path(target: &Path) -> std::io::Result<()> {
    #[cfg(target_os = "windows")]
    {
        let canonical = target
            .canonicalize()
            .unwrap_or_else(|_| target.to_path_buf());
        if canonical.is_file() {
            ProcessCommand::new("explorer")
                .arg(format!("/select,{}", canonical.display()))
                .spawn()?;
        } else {
            ProcessCommand::new("explorer").arg(canonical).spawn()?;
        }
        return Ok(());
    }

    #[cfg(target_os = "macos")]
    {
        ProcessCommand::new("open").arg("-R").arg(target).spawn()?;
        return Ok(());
    }

    #[cfg(target_os = "linux")]
    {
        let directory = if target.is_dir() {
            target.to_path_buf()
        } else {
            target
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| target.to_path_buf())
        };
        ProcessCommand::new("xdg-open").arg(directory).spawn()?;
        return Ok(());
    }

    #[allow(unreachable_code)]
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "unsupported platform",
    ))
}

fn main() {
    let config = parse_launch_config();
    tauri::Builder::default()
        .manage(config)
        .setup(|app| {
            if let Some(window) = app.get_window("main") {
                let _ = window.center();
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_launch_config,
            inspect_input,
            run_conversion,
            reveal_in_file_manager
        ])
        .run(tauri::generate_context!())
        .expect("error while running converters gui");
}
