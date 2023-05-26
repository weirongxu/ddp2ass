use crate::{CanvasConfig, Dandan};
use anyhow::Result;
use clap::Parser;
use std::{collections::HashSet, path::PathBuf};

#[derive(Parser, Debug, serde::Deserialize)]
#[clap(
    author = "weirongxu",
    version,
    about = "将 dandanplay 弹幕转换为 ASS 文件"
)]
pub struct Args {
    #[clap(help = "需要转换的输入，可以是视频、文件夹", default_value = ".")]
    pub input: String,

    #[clap(long = "width", help = "屏幕宽度", default_value = "1280")]
    width: u32,

    #[clap(long = "height", help = "屏幕高度", default_value = "720")]
    height: u32,

    #[clap(
        long = "font",
        short = 'f',
        help = "弹幕使用字体。单位：像素",
        default_value = "黑体"
    )]
    font: String,

    #[clap(long = "font-size", help = "弹幕字体大小", default_value = "25")]
    font_size: u32,

    #[clap(
        long = "width-ratio",
        help = "计算弹幕宽度的比例，为避免重叠可以调大这个数值",
        default_value = "1.2"
    )]
    width_ratio: f64,

    #[clap(
        long = "horizontal-gap",
        help = "每条弹幕之间的最小水平间距，为避免重叠可以调大这个数值。单位：像素",
        default_value = "20.0"
    )]
    #[serde(default)]
    horizontal_gap: f64,

    #[clap(
        long = "duration",
        short = 'd',
        help = "弹幕在屏幕上的持续时间，单位为秒，可以有小数",
        default_value = "15"
    )]
    duration: f64,

    #[clap(
        long = "lane-size",
        short = 'l',
        help = "弹幕所占据的高度，即“行高度/行间距”",
        default_value = "32"
    )]
    lane_size: u32,

    #[clap(
        long = "float-percentage",
        short = 'p',
        help = "屏幕上滚动弹幕最多高度百分比",
        default_value = "0.4"
    )]
    float_percentage: f64,

    #[clap(
        long = "alpha",
        short = 'a',
        help = "弹幕不透明度",
        default_value = "0.7"
    )]
    alpha: f64,

    #[clap(long = "force", help = "默认会跳过已经存在的文件，此参数会强制转换")]
    pub force: bool,

    #[clap(
        long = "denylist",
        help = "黑名单，需要过滤的关键词列表文件，每行一个关键词"
    )]
    denylist: Option<PathBuf>,

    #[clap(long = "pause", help = "在处理完后暂停等待输入")]
    pub pause: bool,

    #[clap(long = "outline", help = "描边宽度", default_value = "0.8")]
    pub outline: f64,

    #[clap(long = "bold", help = "加粗")]
    #[serde(default)]
    pub bold: bool,

    #[clap(
        long = "time-offset",
        help = "时间轴偏移，>0 会让弹幕延后，<0 会让弹幕提前，单位为秒",
        default_value = "0.0"
    )]
    #[serde(default)]
    pub time_offset: f64,
}

impl Args {
    pub fn check(&mut self) -> Result<()> {
        if let Some(f) = self.denylist.as_ref() {
            if !f.exists() {
                anyhow::bail!("黑名单文件不存在");
            }
            if f.is_dir() {
                anyhow::bail!("黑名单文件不能是目录");
            }
        }
        if self.float_percentage < 0.0 {
            anyhow::bail!("滚动弹幕最大高度百分比不能小于 0");
        }
        if self.float_percentage > 1.0 {
            anyhow::bail!("滚动弹幕最大高度百分比不能大于 1");
        }

        Ok(())
    }

    pub fn canvas_config(&self) -> CanvasConfig {
        CanvasConfig {
            width: self.width,
            height: self.height,
            font: self.font.clone(),
            font_size: self.font_size,
            width_ratio: self.width_ratio,
            horizontal_gap: self.horizontal_gap,
            duration: self.duration,
            lane_size: self.lane_size,
            float_percentage: self.float_percentage,
            opacity: ((1.0 - self.alpha) * 255.0) as u8,
            bottom_percentage: 0.3,
            outline: self.outline,
            bold: u8::from(self.bold),
            time_offset: self.time_offset,
        }
    }

    fn denylist(&self) -> Result<Option<HashSet<String>>> {
        match self.denylist.as_ref() {
            None => Ok(None),
            Some(path) => {
                let denylist = std::fs::read_to_string(path)?;
                let list: HashSet<String> = denylist
                    .split('\n')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
                log::info!("黑名单载入 {} 个", list.len());
                log::debug!("黑名单：{:?}", list);
                Ok(Some(list))
            }
        }
    }

    pub async fn process(&self) -> Result<()> {
        let path = PathBuf::from(self.input.clone());
        if path.is_dir() {
            self.process_folder(path).await?;
        } else {
            self.process_file(path).await?;
        }

        Ok(())
    }

    async fn process_file(&self, file: PathBuf) -> Result<()> {
        let canvas_config = self.canvas_config();
        let denylist = self.denylist()?;

        let t = std::time::Instant::now();
        let danmu_count =
            Dandan::process_by_path(&file, self.force, canvas_config, &denylist).await?;

        log::info!(
            "共转换 {} 个文件，共转换 {} 条弹幕，耗时 {:?}",
            1,
            danmu_count,
            t.elapsed()
        );

        Ok(())
    }

    async fn process_folder(&self, folder: PathBuf) -> Result<()> {
        let canvas_config = self.canvas_config();
        let denylist = self.denylist()?;

        // Windows 下 canonicalize 会莫名其妙，见 https://stackoverflow.com/questions/1816691/how-do-i-resolve-a-canonical-filename-in-windows
        #[cfg(not(windows))]
        let folder = folder.canonicalize()?;

        let files: Vec<_> = folder
            .read_dir()?
            .filter_map(|f| f.ok())
            .map(|f| f.path())
            .filter(|f| !f.ends_with(".ass"))
            .collect();

        if files.is_empty() {
            anyhow::bail!("没有找到任何文件");
        }

        log::info!("共找到 {} 个文件", files.len());
        let t = std::time::Instant::now();
        let mut process_file_total = 0;
        let mut process_danmu_total = 0;

        for file in files {
            let (file_count, danmu_count) =
                match Dandan::process_by_path(&file, self.force, canvas_config.clone(), &denylist)
                    .await
                {
                    Ok(danmu_count) => (1, danmu_count),
                    Err(e) => {
                        log::error!("文件 {} 转换错误：{:?}", file.display(), e);
                        (0, 0)
                    }
                };
            process_file_total += file_count;
            process_danmu_total += danmu_count;
        }

        log::info!(
            "共转换 {} 个文件，共转换 {} 条弹幕，耗时 {:?}",
            process_file_total,
            process_danmu_total,
            t.elapsed()
        );

        Ok(())
    }
}
