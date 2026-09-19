use std::io::IsTerminal;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;
use env_logger::Env;
use log::{error, info, warn};
use claude_json_converter::{ConvertOptions, run_conversion};

#[derive(Debug, Parser)]
#[command(
    name = "claude-json-converter",
    version,
    about = "Convert Claude export JSON into markdown conversations"
)]
struct Cli {
    /// Input file (JSON or ZIP) or directory
    input: PathBuf,

    /// Output path (Markdown file or directory)
    #[arg(short, long)]
    output: Option<PathBuf>,
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
    let show_progress = std::io::stdout().is_terminal();

    let summary = run_conversion(ConvertOptions {
        input: cli.input.clone(),
        output: cli.output,
        show_progress,
    })?;

    info!(
        "converted {} -> {} markdown files ({} failed files)",
        cli.input.display(),
        summary.generated_files,
        summary.failed_files
    );

    if let Some(error_log) = summary.error_log {
        warn!("errors were logged to {}", error_log.display());
    }

    if summary.failed_files > 0 {
        Ok(ExitCode::from(1))
    } else {
        Ok(ExitCode::SUCCESS)
    }
}
