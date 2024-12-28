use anyhow::{Context, Result};
use md5;
use promkit::{
    crossterm::style::Stylize,
    preset::{query_selector::QuerySelector, readline::Readline},
    suggest::Suggest,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    fmt,
    fs::File,
    io::{BufReader, Read},
    path::PathBuf,
};

use crate::InputFile;

pub struct MatchParams {
    pub match_name: String,
    pub json: MatchParamsJson,
}

#[derive(Serialize, Deserialize, Clone)]
pub enum MatchMode {
    #[serde(rename = "hashAndFileName")]
    HashAndFileName,
    #[serde(rename = "fileNameOnly")]
    FileNameOnly,
    #[serde(rename = "hashOnly")]
    HashOnly,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct MatchParamsJson {
    #[serde(rename = "fileName")]
    pub file_name: String,
    #[serde(rename = "fileHash")]
    pub file_hash: String,
    #[serde(rename = "fileSize")]
    pub file_size: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "matchMode")]
    pub match_mode: Option<MatchMode>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct MatchAcceptParamsJson {
    pub hash: Option<String>,
    #[serde(rename = "fileName")]
    pub file_name: Option<String>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct MatchItem {
    #[serde(rename = "episodeId")]
    pub episode_id: i64,
    #[serde(rename = "animeId")]
    pub anime_id: i64,
    #[serde(rename = "animeTitle")]
    pub anime_title: String,
    #[serde(rename = "episodeTitle")]
    pub episode_title: String,
    #[serde(rename = "type")]
    pub match_type: String,
    #[serde(rename = "typeDescription")]
    pub type_description: String,
    pub shift: f64,
}

#[derive(Serialize, Deserialize)]
pub struct MatchesJson {
    #[serde(rename = "isMatched")]
    pub is_matched: bool,
    pub matches: Vec<MatchItem>,
    #[serde(rename = "errorCode")]
    pub error_code: i64,
    pub success: bool,
    #[serde(rename = "errorMessage")]
    pub error_message: String,
}

#[derive(Serialize, Deserialize)]
pub struct SearchJson {
    #[serde(rename = "hasMore")]
    pub has_more: bool,
    #[serde(rename = "errorCode")]
    pub error_code: i64,
    pub success: bool,
    #[serde(rename = "errorMessage")]
    pub error_message: String,
    pub animes: Vec<SearchAnimeJson>,
}

#[derive(Serialize, Deserialize)]
pub struct SearchAnimeJson {
    #[serde(rename = "animeId")]
    pub anime_id: i64,
    #[serde(rename = "animeTitle")]
    pub anime_title: String,
    #[serde(rename = "type")]
    pub type_: String,
    #[serde(rename = "typeDescription")]
    pub type_description: String,
    pub episodes: Vec<SearchAnimeEpisodeJson>,
}

#[derive(Serialize, Deserialize)]
pub struct SearchAnimeEpisodeJson {
    #[serde(rename = "episodeId")]
    episode_id: i64,
    #[serde(rename = "episodeTitle")]
    episode_title: String,
}

#[derive(Clone)]
pub enum SearchOption {
    SearchEditInput(SearchEditInput),
    SearchAnimeOption(AnimeEpisodeItem),
}

#[derive(Clone)]
pub struct AnimeEpisodeItem {
    pub anime_id: i64,
    pub anime_title: String,
    pub episode_id: i64,
    pub episode_title: String,
}

#[derive(Clone)]
pub struct SearchEditInput {}

impl fmt::Display for SearchOption {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            SearchOption::SearchEditInput(_) => write!(f, "Edit Search name"),
            SearchOption::SearchAnimeOption(o) => {
                write!(f, "{} {}", o.anime_title, o.episode_title)
            }
        }
    }
}

pub struct DandanMatch {}

impl DandanMatch {
    pub fn get_file_hash(path: &PathBuf) -> Result<String> {
        let file = File::open(path)?;
        let mut reader = BufReader::new(file);
        let bytes_to_read: usize = 16 * 1024 * 1024;
        let mut buf = vec![0u8; bytes_to_read];
        reader.read(&mut buf)?;
        let hash = format!("{:x}", md5::compute(buf));
        Ok(hash)
    }

    pub fn get_match_params(input_file: &InputFile) -> Result<MatchParams> {
        let hash = Self::get_file_hash(&input_file.path)?;
        let folder_name = match input_file.path.parent() {
            Some(p) => match p.file_name() {
                Some(p) => p.to_string_lossy().to_string(),
                None => "".to_string(),
            },
            None => "".to_string(),
        };
        let filename = match input_file.path.with_extension("").file_name() {
            Some(p) => p.to_string_lossy().to_string(),
            None => "".to_string(),
        };
        let match_name = format!("{} {}", folder_name, filename);
        let file_size = input_file.path.metadata()?.len();

        Ok(MatchParams {
            match_name,
            json: MatchParamsJson {
                file_name: filename,
                file_hash: hash,
                file_size: file_size.to_string(),
                match_mode: None,
            },
        })
    }

    pub async fn get_matches_json(match_params: &MatchParams) -> Result<MatchesJson> {
        let match_json = json!(match_params.json);
        let matches_json = reqwest::Client::new()
            .post("https://api.dandanplay.net/api/v2/match")
            .json(&match_json)
            .header("Accept", "application/json")
            .header("User-Agent", "curl")
            .send()
            .await?
            .json::<MatchesJson>()
            .await?;
        Ok(matches_json)
    }

    pub async fn search_anime(
        match_params: &MatchParams,
        anime_name: &str,
        episode: Option<&str>,
    ) -> Result<AnimeEpisodeItem> {
        let mut query = vec![("anime", anime_name)];
        if let Some(episode) = episode {
            query.push(("episode", episode));
        }
        let search_json = reqwest::Client::new()
            .get("https://api.dandanplay.net/api/v2/search/episodes")
            .query(&query)
            .header("Accept", "application/json")
            .header("User-Agent", "curl")
            .send()
            .await?
            .json::<SearchJson>()
            .await?;
        if search_json.animes.is_empty() {
            println!(
                "搜索 {} 结果为空",
                match_params.match_name.clone().underlined()
            );
            let new_anime_name = Self::input_search_params(anime_name)?;
            return Box::pin(Self::search_anime(match_params, &new_anime_name, None)).await;
        }
        let mut options: Vec<SearchOption> = vec![];
        for anime in search_json.animes {
            for episode in anime.episodes {
                options.push(SearchOption::SearchAnimeOption(AnimeEpisodeItem {
                    anime_id: anime.anime_id,
                    anime_title: anime.anime_title.clone(),
                    episode_id: episode.episode_id,
                    episode_title: episode.episode_title.clone(),
                }));
            }
        }
        options.push(SearchOption::SearchEditInput(SearchEditInput {}));
        let mut select_prompt = QuerySelector::new(&options, |text, items| {
            items.iter().filter(|i| i.contains(text)).cloned().collect()
        })
        .title("请选择匹配的动画:")
        .prompt()?;
        let selected = select_prompt.run()?;
        let option = options
            .iter()
            .find(|o| o.to_string() == selected)
            .context("Select anime not found")?;
        match option {
            SearchOption::SearchAnimeOption(o) => {
                Self::accept_match(
                    o.episode_id,
                    &MatchAcceptParamsJson {
                        hash: None,
                        file_name: None,
                    },
                )
                .await?;
                Ok(o.to_owned())
            }
            _ => {
                let new_anime_name = Self::input_search_params(anime_name)?;
                Box::pin(Self::search_anime(match_params, &new_anime_name, None)).await
            }
        }
    }

    pub async fn accept_match(
        episode_id: i64,
        match_accept_params: &MatchAcceptParamsJson,
    ) -> Result<()> {
        let match_json = json!(match_accept_params);
        reqwest::Client::new()
            .post(format!(
                "https://api.dandanplay.net/api/v2/match/{}",
                episode_id
            ))
            .json(&match_json)
            .header("Accept", "application/json")
            .header("User-Agent", "curl")
            .send()
            .await?;
        Ok(())
    }

    pub fn input_search_params(match_name: &str) -> Result<String> {
        let mut match_name_prompt = Readline::default()
            .title("输入要搜索的名字(用 tab 补全):")
            .enable_suggest(Suggest::from_iter([&match_name]))
            .enable_history()
            .prompt()?;
        match_name_prompt.run()
    }

    pub async fn get_anime_episode_item(
        input_file: &InputFile,
        change_match: bool,
    ) -> Result<AnimeEpisodeItem> {
        let match_params = Self::get_match_params(input_file)?;
        let matches_json = Self::get_matches_json(&match_params).await?;

        Ok(if change_match {
            Self::search_anime(&match_params, &match_params.match_name, None).await?
        } else if matches_json.is_matched {
            let match_item = &matches_json.matches[0];
            info!(
                "{}, {}, 话数 {}",
                input_file.log("匹配弹幕"),
                match_item.anime_title.clone().underlined().to_string(),
                match_item.episode_title.clone().underlined().to_string()
            );
            AnimeEpisodeItem {
                anime_id: match_item.anime_id,
                anime_title: match_item.anime_title.clone(),
                episode_id: match_item.episode_id,
                episode_title: match_item.episode_title.clone(),
            }
        } else {
            println!("无法精确匹配 {}", match_params.match_name);
            Self::search_anime(&match_params, &match_params.match_name, None).await?
        })
    }
}
