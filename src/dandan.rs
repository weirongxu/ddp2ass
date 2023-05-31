use crate::{
    cli::SimplifiedOrTraditional, util::display_path, AssCreator, CanvasConfig, Danmu, DanmuType,
};
use anyhow::{anyhow, Context, Result};
use inquire::Select;
use md5;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    collections::HashSet,
    fs::{self, read_to_string, File},
    io::{BufReader, Read, Write},
    path::PathBuf,
    process::Command,
};

#[derive(Serialize, Deserialize)]
struct MatchesJson {
    #[serde(rename = "isMatched")]
    is_matched: bool,
    matches: Vec<MatchItem>,
    #[serde(rename = "errorCode")]
    error_code: i64,
    success: bool,
    #[serde(rename = "errorMessage")]
    error_message: String,
}

#[derive(Serialize, Deserialize, Clone)]
struct MatchItem {
    #[serde(rename = "episodeId")]
    episode_id: i64,
    #[serde(rename = "animeId")]
    anime_id: i64,
    #[serde(rename = "animeTitle")]
    anime_title: String,
    #[serde(rename = "episodeTitle")]
    episode_title: String,
    #[serde(rename = "type")]
    match_type: String,
    #[serde(rename = "typeDescription")]
    type_description: String,
    shift: f64,
}

#[derive(Serialize, Deserialize)]
struct CommentsJson {
    count: i64,
    comments: Vec<CommentItem>,
}

#[derive(Serialize, Deserialize)]
struct CommentItem {
    /// comment id
    cid: u64,
    /// position
    p: String,
    /// comment
    m: String,
}

struct Position {
    timestamp_s: f64,
    mode: DanmuType,
    color: (u8, u8, u8),
    #[allow(dead_code)]
    user_id: String,
}

impl Position {
    fn parse_mode(mode: String) -> Result<DanmuType> {
        let v = mode.parse::<u8>()?;
        match v {
            1 => Ok(DanmuType::Float),
            4 => Ok(DanmuType::Bottom),
            5 => Ok(DanmuType::Top),
            v => Err(anyhow!("不支持的弹幕类型 {}", v)),
        }
    }

    fn parse_color(s: String) -> Result<(u8, u8, u8)> {
        let value = s.parse::<i32>()?;
        let b = (value % 256) as u8;
        let g = ((value / 256) % 256) as u8;
        let r = (value / (256 * 256)) as u8;

        Ok((r, g, b))
    }

    fn parse(s: String) -> Result<Position> {
        let splited: Vec<_> = s.split(",").map(|v| v.to_string()).collect();
        let (timestamp_seconds, mode, color, user_id) = (
            splited[0].clone(),
            splited[1].clone(),
            splited[2].clone(),
            splited[3].clone(),
        );
        let color = Position::parse_color(color)?;
        Ok(Position {
            timestamp_s: timestamp_seconds.parse::<f64>()?,
            mode: Position::parse_mode(mode)?,
            color,
            user_id,
        })
    }
}

pub struct Dandan {}

impl Dandan {
    fn get_file_hash(path: &PathBuf) -> Result<String> {
        let file = File::open(path)?;
        let mut reader = BufReader::new(file);
        let bytes_to_read: usize = 16 * 1024 * 1024;
        let mut buf = vec![0u8; bytes_to_read];
        reader.read(&mut buf)?;
        let hash = format!("{:x}", md5::compute(buf));
        Ok(hash)
    }

    async fn fetch_comments_json(
        input_path: &PathBuf,
        force: bool,
        simplified_or_traditional: SimplifiedOrTraditional,
    ) -> Result<CommentsJson> {
        let json_path = input_path.with_extension("dandanplay.json");

        if json_path.is_dir() {
            return Err(anyhow!(
                "弹幕缓存 {} 文件不能是一个目录",
                display_path(&json_path),
            ));
        }

        if json_path.exists() && !force {
            log::warn!(
                "弹幕缓存 {} 已经存在，使用 --force 参数强制更新",
                display_path(&json_path),
            );
            let json = read_to_string(json_path)?;
            return Ok(serde_json::from_str(&json)?);
        }

        let hash = Self::get_file_hash(input_path)?;
        let folder_name = match input_path.parent() {
            Some(p) => match p.file_name() {
                Some(p) => p.to_string_lossy().to_string(),
                None => "".to_string(),
            },
            None => "".to_string(),
        };
        let filename = match input_path.with_extension("").file_name() {
            Some(p) => p.to_string_lossy().to_string(),
            None => "".to_string(),
        };
        let match_filename = format!("{} {}", folder_name, filename);
        let file_size = input_path.metadata()?.len();

        let match_json = json!({
            "fileName": match_filename,
            "fileHash": hash,
            "fileSize": file_size,
        });

        debug!("match_json: {}", match_json.to_string());

        let matches_json = reqwest::Client::new()
            .post("https://api.dandanplay.net/api/v2/match")
            .json(&match_json)
            .header("Accept", "application/json")
            .header("User-Agent", "curl")
            .send()
            .await?
            .json::<MatchesJson>()
            .await?;

        let episode_id = if matches_json.is_matched {
            matches_json.matches[0].episode_id
        } else {
            Self::select_matches(&match_filename, &matches_json)?.episode_id
        };

        let comments_url = format!(
            "https://api.dandanplay.net/api/v2/comment/{}?withRelated=true&chConvert={}",
            episode_id,
            match simplified_or_traditional {
                SimplifiedOrTraditional::Original => 0,
                SimplifiedOrTraditional::Simplified => 1,
                SimplifiedOrTraditional::Traditional => 2,
            }
        );
        let comments_json = reqwest::Client::new()
            .get(comments_url)
            .json(&matches_json)
            .header("Accept", "application/json")
            .header("User-Agent", "curl")
            .send()
            .await?
            .json::<CommentsJson>()
            .await?;

        fs::write(json_path, serde_json::to_string(&comments_json)?)?;

        Ok(comments_json)
    }

    pub async fn process_by_path(
        input_path: &PathBuf,
        force: bool,
        simplified_or_traditional: SimplifiedOrTraditional,
        merge_built_in: String,
        canvas_config: CanvasConfig,
        denylist: &Option<HashSet<String>>,
    ) -> Result<u64> {
        if !input_path.exists() {
            return Err(anyhow!("视频文件 {} 不存在", display_path(&input_path)));
        }

        let input_path_str = input_path.to_str().context("视频路径无法解析")?;

        let built_in_ass = if merge_built_in.is_empty() {
            None
        } else {
            let mut cmd = Command::new("ffmpeg");
            cmd.args([
                "-i",
                input_path_str,
                "-map",
                &format!("0:s:{}", merge_built_in),
                "-f",
                "ass",
                "pipe:1",
            ]);
            let ass = cmd.output()?;
            Some(String::from_utf8(ass.stdout)?)
        };

        let output_path = input_path.with_extension("ass");

        if output_path.is_dir() {
            return Err(anyhow!(
                "输出文件 {} 不能是一个目录",
                display_path(&output_path)
            ));
        }

        let comments_json =
            Self::fetch_comments_json(&input_path, force, simplified_or_traditional).await?;

        let count = Self::process_by_json(
            &input_path,
            &output_path,
            comments_json,
            built_in_ass,
            &denylist,
            canvas_config,
        )?;

        Ok(count)
    }

    fn select_matches(filename: &String, matches_json: &MatchesJson) -> Result<MatchItem> {
        let options: Vec<_> = matches_json
            .matches
            .iter()
            .map(|m| format!("{} {} {}", m.episode_id, m.anime_title, m.episode_title))
            .collect();
        println!("无法精确匹配 {}", filename);
        let ans = Select::new("请选择匹配的动画:", options.clone()).prompt()?;
        let idx = options
            .iter()
            .position(|o| o == &ans)
            .context("Select matches not found")?;
        Ok(matches_json.matches[idx].clone())
    }

    fn process_by_json(
        input_path: &PathBuf,
        output_path: &PathBuf,
        input_json: CommentsJson,
        built_in_ass: Option<String>,
        denylist: &Option<HashSet<String>>,
        canvas_config: CanvasConfig,
    ) -> Result<u64> {
        let title = input_path
            .file_name()
            .context("Filename not found")?
            .to_string_lossy()
            .to_string();

        let mut file = File::create(output_path)?;

        let (count, s) =
            Self::json_to_ass(input_json, built_in_ass, title, denylist, canvas_config)?;

        file.write(s.as_bytes())?;

        Ok(count)
    }

    fn json_to_ass(
        input_json: CommentsJson,
        built_in_ass: Option<String>,
        title: String,
        denylist: &Option<HashSet<String>>,
        canvas_config: CanvasConfig,
    ) -> Result<(u64, String)> {
        let mut ass = AssCreator::new(title.clone(), canvas_config.clone())?;

        let mut count = 0;
        let mut canvas = canvas_config.canvas();
        let t = std::time::Instant::now();

        for c in input_json.comments {
            let pos = Position::parse(c.p)?;
            let danmu = Danmu {
                content: c.m,
                timeline_s: pos.timestamp_s,
                fontsize: 0,
                r#type: pos.mode,
                rgb: pos.color,
            };
            if let Some(denylist) = denylist.as_ref() {
                if denylist.iter().any(|s| danmu.content.contains(s)) {
                    continue;
                }
            }
            if let Some(drawable) = canvas.draw(danmu)? {
                count += 1;
                ass.write(drawable)?;
            }
        }

        if let Some(built_in_ass) = built_in_ass {
            ass.merge(built_in_ass)?;
        }

        log::info!("弹幕数量：{}, 耗时 {:?}（{}）", count, t.elapsed(), title);

        Ok((count, String::from_utf8(ass.buf)?))
    }
}

#[cfg(test)]
mod tests {

    use crate::{Args, Dandan};
    use anyhow::Result;
    use clap::Parser;

    #[test]
    fn test_convert() -> Result<()> {
        let json = serde_json::from_str(
            r#"
            {
                "count": 1008,
                "comments": [
                    {
                        "cid": 1684989836,
                        "p": "0.00,1,16777215,[Gamer]bill88919",
                        "m": "頭香"
                    },
                    {
                        "cid": 1684989837,
                        "p": "0.00,1,16777215,[Gamer]mia105067",
                        "m": ":)"
                    },
                    {
                        "cid": 1684989838,
                        "p": "0.00,1,16777215,[Gamer]renwendy",
                        "m": "簽"
                    },
                    {
                        "cid": 1684989839,
                        "p": "0.00,1,16777215,[Gamer]s39101149",
                        "m": "簽"
                    },
                    {
                        "cid": 1684989840,
                        "p": "0.00,1,16777215,[Gamer]huryan951006",
                        "m": "我已經等三年了！"
                    },
                    {
                        "cid": 1684989841,
                        "p": "0.50,1,16777215,[Gamer]Vigar09995",
                        "m": "22:00馬上簽到 2023/4/3"
                    },
                    {
                        "cid": 1684989842,
                        "p": "0.50,1,16777215,[Gamer]thomas627",
                        "m": "Kuma~~~"
                    },
                    {
                        "cid": 1684989843,
                        "p": "0.60,1,16777215,[Gamer]ppleo8888",
                        "m": "2023/04/16直接看完第一季過來 真的太爽啦"
                    }
                ]
            }
        "#,
        )?;

        let args = Args::parse_from(["test"]);
        let (_count, ass) =
            Dandan::json_to_ass(json, None, "test".to_string(), &None, args.canvas_config())?;

        assert_eq!(
            ass,
            "[Script Info]\n; Script generated by danmu2ass\nTitle: test\nScript Updated By: danmu2ass (https://github.com/gwy15/danmu2ass)\nScriptType: v4.00+\nPlayResX: 1280\nPlayResY: 720\nAspect Ratio: 1280:720\nCollisions: Normal\nWrapStyle: 2\nScaledBorderAndShadow: yes\nYCbCr Matrix: TV.601\n\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Float,黑体,25,&H4cFFFFFF,&H00FFFFFF,&H4c000000,&H00000000,0, 0, 0, 0, 100, 100, 0.00, 0.00, 1, 0.8, 0, 7, 0, 0, 0, 1\nStyle: Bottom,黑体,25,&H4cFFFFFF,&H00FFFFFF,&H4c000000,&H00000000,0, 0, 0, 0, 100, 100, 0.00, 0.00, 1, 0.8, 0, 7, 0, 0, 0, 1\nStyle: Top,黑体,25,&H4cFFFFFF,&H00FFFFFF,&H4c000000,&H00000000,0, 0, 0, 0, 100, 100, 0.00, 0.00, 1, 0.8, 0, 7, 0, 0, 0, 1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 2,0:00:00.00,0:00:15.00,Float,,0,0,0,,{\\move(1280, 0, -60, 0)\\c&Hffffff&}頭香\nDialogue: 2,0:00:00.00,0:00:15.00,Float,,0,0,0,,{\\move(1280, 32, -39, 32)\\c&Hffffff&}:)\nDialogue: 2,0:00:00.00,0:00:15.00,Float,,0,0,0,,{\\move(1280, 64, -30, 64)\\c&Hffffff&}簽\nDialogue: 2,0:00:00.00,0:00:15.00,Float,,0,0,0,,{\\move(1280, 96, -30, 96)\\c&Hffffff&}簽\nDialogue: 2,0:00:00.00,0:00:15.00,Float,,0,0,0,,{\\move(1280, 128, -240, 128)\\c&Hffffff&}我已經等三年了！\nDialogue: 2,0:00:00.50,0:00:15.50,Float,,0,0,0,,{\\move(1280, 160, -399, 160)\\c&Hffffff&}22:00馬上簽到 2023/4/3\nDialogue: 2,0:00:00.50,0:00:15.50,Float,,0,0,0,,{\\move(1280, 192, -139, 192)\\c&Hffffff&}Kuma~~~\nDialogue: 2,0:00:00.60,0:00:15.60,Float,,0,0,0,,{\\move(1280, 224, -639, 224)\\c&Hffffff&}2023/04/16直接看完第一季過來 真的太爽啦\n"
        );

        Ok(())
    }
}
