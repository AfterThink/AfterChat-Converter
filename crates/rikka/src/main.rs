use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;
use afterchat_rikka::{ConvertOptions, run_conversion};

#[derive(Debug, Parser)]
#[command(
    name = "rikka",
    version,
    about = "把 RikkaHub 备份（SQLite）转换为 AfterChat 对话 Markdown / ZIP"
)]
struct Args {
    /// RikkaHub 备份 zip 或裸 .db（可直接拖拽到 exe 上）
    #[arg(value_name = "INPUT")]
    input: PathBuf,

    /// 输出目录，或显式 .zip 路径；省略则写到源文件同目录
    #[arg(short = 'o', long = "output", value_name = "PATH")]
    output: Option<PathBuf>,
}

fn main() -> Result<()> {
    let args = Args::parse();
    let summary = run_conversion(ConvertOptions {
        input: args.input,
        output: args.output,
    })?;

    println!(
        "已打包 {} 个对话 → {}",
        summary.exported,
        summary.output.display()
    );
    if summary.skipped > 0 {
        println!(
            "跳过 {} 个无消息对话（详见压缩包内 export-failures.md）",
            summary.skipped
        );
    }

    Ok(())
}
