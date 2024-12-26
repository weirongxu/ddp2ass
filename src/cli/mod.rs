mod args;
mod match_params;
mod match_result;

use std::path::{absolute, PathBuf};

use anyhow::Result;
pub use args::*;
use clap::{command, Parser, Subcommand};
pub use match_params::*;
pub use match_result::*;

#[derive(Parser, Debug)]
#[clap(
    author = "weirongxu",
    version,
    about = "将 dandanplay 弹幕转换为 ASS 文件"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    #[clap(about = "下载弹幕 (默认命令)")]
    Download(Args),

    #[clap(about = "匹配弹幕参数")]
    MatchParams(MatchParamsArgs),

    #[clap(about = "匹配弹幕结果")]
    MatchResult(MatchResultArgs),
}

pub fn input_path_to_list(input: &str) -> Result<Vec<PathBuf>> {
    let input_path = absolute(PathBuf::from(&input))?;
    let match_exts = vec![
        ".mp4", ".mov", ".wmv", ".avi", ".flv", ".f4v", ".swf", ".mkv", ".webm",
    ];
    Ok(if input_path.is_dir() {
        input_path
            .read_dir()?
            .filter_map(|f| f.ok())
            .map(|f| f.path())
            .filter(|f| match_exts.iter().any(|m| f.to_string_lossy().ends_with(m)))
            .collect()
    } else {
        [input_path].to_vec()
    })
}
