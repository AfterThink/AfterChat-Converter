use std::io::IsTerminal;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;
use env_logger::Env;
use log::{error, info, warn};
use afterchat_claude::{ConvertOptions, run_conversion};

#[derive(Debug, Parser)]
#[command(
    name = "claude",
    version,
    about = "Convert Claude export (ZIP/JSON) into an AfterChat conversation ZIP"
)]
struct Cli {
    /// Input ZIP, JSON, or directory
    input: PathBuf,

    /// Output directory, or an explicit `.zip` path
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
        "converted {} -> {} conversations ({} skipped)",
        cli.input.display(),
        summary.conversations,
        summary.failed
    );

    if let Some(output) = &summary.output {
        info!("wrote {}", output.display());
    }

    if summary.failed > 0 {
        warn!("{0} conversation(s) were skipped; see export-failures.md", summary.failed);
    }

    Ok(ExitCode::SUCCESS)
}
