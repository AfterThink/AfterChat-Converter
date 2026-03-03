use std::path::PathBuf;
use std::process::ExitCode;

use clap::{ArgAction, Args, Parser, Subcommand};
use env_logger::Env;
use log::{error, info, warn};
use qwen_json_converter::{ConvertOptions, run_conversion};

#[derive(Debug, Parser)]
#[command(
    name = "qwen-json-converter",
    version,
    about = "Convert Qwen export JSON into markdown conversations"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
    #[arg(value_name = "PATH", num_args = 0..)]
    drag_paths: Vec<PathBuf>,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Convert(ConvertArgs),
}

#[derive(Debug, Clone, Args)]
struct ConvertArgs {
    #[arg(short, long)]
    input: Option<PathBuf>,
    #[arg(short, long)]
    output: Option<PathBuf>,
    #[arg(long, action = ArgAction::Set, default_value_t = true)]
    progress: bool,
    #[arg(value_name = "PATH", num_args = 0..)]
    paths: Vec<PathBuf>,
}

fn main() -> ExitCode {
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();

    let cli = Cli::parse();
    match run(cli) {
        Ok(code) => code,
        Err(err) => {
            error!("{err:#}");
            ExitCode::from(1)
        }
    }
}

fn run(cli: Cli) -> anyhow::Result<ExitCode> {
    match cli.command {
        Some(Commands::Convert(args)) => run_convert(args),
        None => {
            if cli.drag_paths.is_empty() {
                anyhow::bail!(
                    "no input path provided. Use convert -i <path> or drag files to the executable."
                );
            }
            let args = ConvertArgs {
                input: None,
                output: None,
                progress: true,
                paths: cli.drag_paths,
            };
            run_convert(args)
        }
    }
}

fn run_convert(args: ConvertArgs) -> anyhow::Result<ExitCode> {
    let mut inputs = args.paths.clone();
    if let Some(path) = args.input.clone() {
        inputs.insert(0, path);
    }

    if inputs.is_empty() {
        anyhow::bail!("convert requires at least one input path");
    }

    if inputs.len() > 1 {
        if let Some(out) = args.output.as_ref() {
            if out.extension().and_then(|v| v.to_str()) == Some("md") {
                anyhow::bail!("when converting multiple inputs, output must be a directory path");
            }
        }
    }

    let mut had_failure = false;
    let shared_output = args.output.clone();

    for input in inputs {
        let summary = run_conversion(ConvertOptions {
            input: input.clone(),
            output: shared_output.clone(),
            show_progress: args.progress,
        })?;

        info!(
            "converted {} -> {} markdown files ({} failed files)",
            input.display(),
            summary.generated_files,
            summary.failed_files
        );

        if let Some(error_log) = summary.error_log {
            warn!("errors were logged to {}", error_log.display());
        }

        if summary.failed_files > 0 {
            had_failure = true;
        }
    }

    if had_failure {
        Ok(ExitCode::from(1))
    } else {
        Ok(ExitCode::SUCCESS)
    }
}
