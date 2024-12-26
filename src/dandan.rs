use crate::{
    cli::SimplifiedOrTraditional, dandan_match::DandanMatch, util::display_filename, AssCreator,
    CanvasConfig, Danmu, DanmuType,
};
use anyhow::{anyhow, Context, Result};
use promkit::preset::listbox::Listbox;
use serde::{Deserialize, Serialize};
use std::{
    cmp::Ordering,
    collections::HashSet,
    fs::{self, read_to_string, File},
    io::Write,
    path::PathBuf,
    process::Command,
};

#[derive(Serialize, Deserialize)]
struct CommentsJson {
    count: i64,
    #[serde(rename = "episodeId")]
    pub episode_id: Option<i64>,
    #[serde(rename = "animeId")]
    pub anime_id: Option<i64>,
    #[serde(rename = "animeTitle")]
    pub anime_title: Option<String>,
    #[serde(rename = "episodeTitle")]
    pub episode_title: Option<String>,
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

#[derive(Serialize, Deserialize)]
struct FfprobeSubJson {
    #[serde(rename = "streams")]
    streams: Vec<FfprobeSubStream>,
}

#[derive(Serialize, Deserialize)]
struct FfprobeSubStream {
    index: i64,
    tags: FfprobeSubStreamTag,
}

#[derive(Serialize, Deserialize)]
struct FfprobeSubStreamTag {
    language: String,
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
        let split: Vec<_> = s.split(",").map(|v| v.to_string()).collect();
        let (timestamp_seconds, mode, color, user_id) = (
            split[0].clone(),
            split[1].clone(),
            split[2].clone(),
            split[3].clone(),
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
    async fn fetch_comments_json(
        input_path: &PathBuf,
        force: bool,
        change_match: bool,
        simplified_or_traditional: SimplifiedOrTraditional,
    ) -> Result<CommentsJson> {
        let json_path = input_path.with_extension("dandanplay.json");

        if json_path.is_dir() {
            return Err(anyhow!(
                "弹幕缓存 {} 文件不能是一个目录",
                display_filename(&json_path),
            ));
        }

        if json_path.exists() && !change_match && !force {
            log::warn!(
                "弹幕缓存 {} 已经存在，使用 --force 参数强制更新",
                display_filename(&json_path),
            );
            let json = read_to_string(json_path)?;
            return Ok(serde_json::from_str::<CommentsJson>(&json)?);
        }

        let anime_episode_item =
            DandanMatch::get_anime_episode_item(input_path, change_match).await?;

        let comments_url = format!(
            "https://api.dandanplay.net/api/v2/comment/{}?withRelated=true&chConvert={}",
            anime_episode_item.episode_id,
            match simplified_or_traditional {
                SimplifiedOrTraditional::Original => 0,
                SimplifiedOrTraditional::Simplified => 1,
                SimplifiedOrTraditional::Traditional => 2,
            }
        );
        let mut comments_json = reqwest::Client::new()
            .get(comments_url)
            .header("Accept", "application/json")
            .header("User-Agent", "curl")
            .send()
            .await?
            .json::<CommentsJson>()
            .await?;

        comments_json.episode_id = Some(anime_episode_item.episode_id);
        comments_json.anime_id = Some(anime_episode_item.anime_id);
        comments_json.anime_title = Some(anime_episode_item.anime_title.clone());
        comments_json.episode_title = Some(anime_episode_item.episode_title.clone());

        fs::write(json_path, serde_json::to_string(&comments_json)?)?;

        Ok(comments_json)
    }

    fn built_in_ass_from(input_path_str: String, merge_built_in: String) -> Result<String> {
        Ok(String::from_utf8(
            Command::new("ffmpeg")
                .args([
                    "-i",
                    &input_path_str,
                    "-map",
                    &format!("0:{}", merge_built_in),
                    "-f",
                    "ass",
                    "pipe:1",
                ])
                .output()?
                .stdout,
        )?)
    }

    fn built_in_ass_by(
        input_path_str: String,
        merge_built_in: String,
        merge_built_in_interactive: bool,
    ) -> Result<Option<String>> {
        if !merge_built_in.is_empty() {
            Ok(Some(Self::built_in_ass_from(
                input_path_str,
                merge_built_in,
            )?))
        } else if merge_built_in_interactive {
            let sub_json = String::from_utf8(
                Command::new("ffprobe")
                    .args([
                        "-v",
                        "error",
                        "-of",
                        "json",
                        "-show_entries",
                        "stream=index:stream_tags=language",
                        "-select_streams",
                        "s",
                        &input_path_str,
                    ])
                    .output()?
                    .stdout,
            )?;
            let sub_json: FfprobeSubJson = serde_json::from_str(&sub_json)?;
            let options: Vec<_> = sub_json
                .streams
                .iter()
                .map(|s| format!("{} {}", s.index, s.tags.language))
                .collect();
            let mut select_prompt = Listbox::new(options.clone()).title("请选择合并的字幕").prompt()?;
            let ans = select_prompt.run()?;
            let idx = options
                .iter()
                .position(|o| o == &ans)
                .context("Select matches not found")?;
            let sub_index = sub_json.streams[idx].index;
            Ok(Some(Self::built_in_ass_from(
                input_path_str,
                sub_index.to_string(),
            )?))
        } else {
            Ok(None)
        }
    }

    pub async fn process_by_path(
        input_path: &PathBuf,
        force: bool,
        change_match: bool,
        simplified_or_traditional: SimplifiedOrTraditional,
        merge_built_in_interactive: bool,
        merge_built_in: String,
        canvas_config: CanvasConfig,
        denylist: &Option<HashSet<String>>,
    ) -> Result<u64> {
        if !input_path.exists() {
            return Err(anyhow!("视频文件 {} 不存在", display_filename(&input_path)));
        }

        let input_path_str = input_path.to_str().context("视频路径无法解析")?;

        let built_in_ass = Self::built_in_ass_by(
            input_path_str.to_string(),
            merge_built_in,
            merge_built_in_interactive,
        )?;

        let output_path = input_path.with_extension("ass");

        if output_path.is_dir() {
            return Err(anyhow!(
                "输出文件 {} 不能是一个目录",
                display_filename(&output_path)
            ));
        }

        let comments_json =
            Self::fetch_comments_json(&input_path, force, change_match, simplified_or_traditional)
                .await?;

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
        let mut danmus: Vec<Danmu> = Vec::new();

        for c in input_json.comments {
            let pos = Position::parse(c.p)?;
            let danmu = Danmu {
                content: c.m,
                timeline_s: pos.timestamp_s,
                fontsize: 0,
                r#type: pos.mode,
                rgb: pos.color,
            };
            danmus.push(danmu);
        }

        danmus.sort_by(|a, b| {
            a.timeline_s
                .partial_cmp(&b.timeline_s)
                .unwrap_or(Ordering::Equal)
        });

        for danmu in danmus {
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
