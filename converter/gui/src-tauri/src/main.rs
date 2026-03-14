#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;

use serde::{Deserialize, Serialize};
use tauri::api::process::{Command, CommandEvent};

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum ConverterKind {
    AiStudio,
    Cherry,
    Qwen,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConvertRequest {
    converter: ConverterKind,
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
    let predicted_output =
        predict_output_path(request.converter, &input_path, output_path.as_ref())?;

    let sidecar_name = match request.converter {
        ConverterKind::AiStudio => "google-ai-studio-json-converter",
        ConverterKind::Cherry => "cherry-studio-backup-json-converter",
        ConverterKind::Qwen => "qwen-json-converter",
    };

    let args = build_args(request.converter, &input_path, output_path.as_ref());
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

    Ok(ConvertResponse {
        exit_code,
        stdout: stdout.join("\n").trim().to_string(),
        stderr: stderr.join("\n").trim().to_string(),
        output_path: predicted_output.display().to_string(),
    })
}

#[tauri::command]
fn reveal_in_file_manager(path: String) -> Result<(), String> {
    let target = PathBuf::from(&path);
    if !target.exists() {
        return Err(format!("输出路径不存在：{}", target.display()));
    }

    reveal_path(&target).map_err(|error| format!("无法打开输出位置 {}: {error}", target.display()))
}

fn build_args(
    converter: ConverterKind,
    input_path: &Path,
    output_path: Option<&PathBuf>,
) -> Vec<String> {
    match converter {
        ConverterKind::AiStudio => {
            let mut args = vec![input_path.display().to_string()];
            if let Some(path) = output_path {
                args.push("-o".to_string());
                args.push(path.display().to_string());
            }
            args
        }
        ConverterKind::Cherry => {
            let mut args = vec![input_path.display().to_string()];
            if let Some(path) = output_path {
                args.push("-o".to_string());
                args.push(path.display().to_string());
            }
            args
        }
        ConverterKind::Qwen => {
            let mut args = vec![
                "convert".to_string(),
                "-i".to_string(),
                input_path.display().to_string(),
                "--progress".to_string(),
                "false".to_string(),
            ];
            if let Some(path) = output_path {
                args.push("-o".to_string());
                args.push(path.display().to_string());
            }
            args
        }
    }
}

fn predict_output_path(
    converter: ConverterKind,
    input_path: &Path,
    output_path: Option<&PathBuf>,
) -> Result<PathBuf, String> {
    if let Some(path) = output_path {
        return Ok(path.clone());
    }

    match converter {
        ConverterKind::Cherry => Ok(input_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join("cherry-studio-export")),
        ConverterKind::AiStudio => {
            if input_path.is_dir() {
                return Ok(input_path.to_path_buf());
            }

            let mut default_output = input_path.to_path_buf();
            if default_output.set_extension("md") {
                Ok(default_output)
            } else {
                let parent = input_path.parent().unwrap_or_else(|| Path::new("."));
                let file_name = input_path
                    .file_name()
                    .and_then(|value| value.to_str())
                    .filter(|value| !value.is_empty())
                    .map(|value| format!("{value}.md"))
                    .unwrap_or_else(|| "output.md".to_string());
                Ok(parent.join(file_name))
            }
        }
        ConverterKind::Qwen => {
            if input_path.is_dir() {
                Ok(input_path.to_path_buf())
            } else {
                input_path
                    .parent()
                    .map(Path::to_path_buf)
                    .ok_or_else(|| "无法推断 Qwen 默认输出目录。".to_string())
            }
        }
    }
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
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            inspect_input,
            run_conversion,
            reveal_in_file_manager
        ])
        .run(tauri::generate_context!())
        .expect("error while running converters gui");
}
