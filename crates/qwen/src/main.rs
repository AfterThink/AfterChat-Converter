use std::io::IsTerminal;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;
use env_logger::Env;
use log::{error, info};
use afterchat_qwen::{ConvertOptions, run_conversion};

#[derive(Debug, Parser)]
#[command(
    name = "qwen",
    version,
    about = "把 Qwen 导出的 JSON 会话转换为 AfterChat 对话 Markdown / ZIP"
)]
struct Cli {
    /// 输入 JSON 文件（可直接拖拽；单体导出→.md，全部导出→.zip）
    #[arg(value_name = "JSON", required = true)]
    input: Vec<PathBuf>,

    /// 输出文件或目录（省略则输出到源文件同目录）
    #[arg(short, long, value_name = "PATH")]
    output: Option<PathBuf>,

    /// 强制开启/关闭进度条（默认跟随终端）
    #[arg(long, value_name = "BOOL")]
    progress: Option<bool>,
}

fn main() -> ExitCode {
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();

    let cli = Cli::parse();
    let show_progress = cli
        .progress
        .unwrap_or_else(|| std::io::stdout().is_terminal());

    let summary = match run_conversion(ConvertOptions {
        inputs: cli.input,
        output: cli.output,
        show_progress,
    }) {
        Ok(summary) => summary,
        Err(err) => {
            error!("{err:#}");
            return ExitCode::from(1);
        }
    };

    for path in &summary.outputs {
        info!("generated {}", path.display());
    }

    if summary.failed > 0 {
        error!(
            "converted {}/{} input(s), {} failed",
            summary.outputs.len(),
            summary.inputs,
            summary.failed
        );
        ExitCode::from(1)
    } else {
        info!(
            "converted {}/{} input(s)",
            summary.outputs.len(),
            summary.inputs
        );
        ExitCode::SUCCESS
    }
}
