use anyhow::{Context, Result};
use clap::Parser;

use crate::dandan_match::DandanMatch;

use super::input_path_to_list;

#[derive(Parser, Debug)]
pub struct MatchResultArgs {
    #[clap(help = "输入文件路径", default_value = ".")]
    pub input: String,
}

impl MatchResultArgs {
    pub async fn process(&self) -> Result<()> {
        let filepaths = input_path_to_list(&self.input)?;
        for filepath in filepaths {
            let params = DandanMatch::get_match_params(&filepath)?;
            let result = DandanMatch::get_matches_json(&params).await?;
            println!(
                "{}",
                filepath
                    .file_name()
                    .context("文件名错误")?
                    .to_string_lossy()
            );
            println!("{}", serde_json::to_string(&result)?);
        }
        Ok(())
    }
}
