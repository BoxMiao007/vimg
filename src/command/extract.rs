use crate::{
    command::{DurationOrPercent, HumanDuration, sh_escape},
    process::CommandExt,
};
use anyhow::{Context, bail, ensure};
use rayon::prelude::*;
use std::{
    fmt, fs,
    path::{Path, PathBuf},
    process::Command,
};

/// 用 ffmpeg 从视频抽出 BMP 截帧，不拼图、不编码。
///
/// `-n` 必填，`-f` 默认 1。在忽略首尾之后的区间里等距取点，点落在每一段的中点。
///
/// 文件写到当前目录或 `--output-dir`，文件名形如 `视频名-12s-01.bmp`。偶尔抽不够帧时，用前一帧补上，并在终端警告。
#[derive(clap::Parser, Debug, Clone)]
#[group(skip)]
#[command(
    override_usage = "vimg extract [选项] <视频>",
    after_help = "示例:\n  vimg extract -n 12 视频.mkv\n  vimg extract -n 8 -f 30 -t 1500ms 视频.mkv"
)]
pub struct Extract {
    /// 等距采样的点数。
    ///
    /// `extract` 和等大网格的 `vcs` 必填，表示要抽几格。`vcs -l 1` 时改成截数：一截 14 张，之后每多一截加 18 张。不写默认 1 截。
    #[arg(long, short, value_name = "数量")]
    pub number: Option<u32>,

    /// 计算采样点时忽略开头的一段。
    ///
    /// 可写时长，如 `30s`、`1500ms`、`1m30s`；也可写百分比，如 `5%`，相对整段时长。
    #[arg(long = "ignore-start", value_name = "时间", default_value = "0s")]
    pub ignore_start: DurationOrPercent,

    /// 计算采样点时忽略结尾的一段。
    ///
    /// 写法与 `--ignore-start` 相同。去掉首尾之后剩下的时长必须大于 0。
    #[arg(long = "ignore-end", value_name = "时间", default_value = "0s")]
    pub ignore_end: DurationOrPercent,

    /// 每一格输出几帧。大于 1 就是一小段动画。
    ///
    /// `extract` 不写时是 1，只出静帧。`vcs` 不写时是 30。偶尔抽不够时，等大网格用前一帧补上；版式 1 不补，不出这张接触表。
    #[arg(long, short = 'f', value_name = "帧数")]
    pub capture_frames: Option<u32>,

    /// 多帧时，每一格从视频里取多长的素材。
    ///
    /// 可写 `1500ms`、`1.5s`。窗口不会超出片尾。单帧时这个时长仍然要大于 0。
    #[arg(long, short = 't', value_name = "时长", default_value = "1500ms")]
    pub capture_time: HumanDuration,

    /// 追加的 ffmpeg 视频滤镜，接在缩放后面。
    ///
    /// 原样传给 ffmpeg 的 `-vf`。`vcs` 会先放自己的缩放，再接这里的滤镜。
    #[arg(long, value_name = "滤镜")]
    pub vfilter: Option<String>,

    /// 同时跑几路 ffmpeg。
    ///
    /// 默认 3。写 0 则交给 rayon 按逻辑 CPU 数自动决定。
    #[arg(long, short = 'T', value_name = "数量", default_value_t = 3)]
    pub threads: usize,

    /// 截帧写到哪个目录。
    ///
    /// `extract` 不写时是当前目录。`vcs` 会改写到自己的临时目录；这时这个选项变成临时目录的父目录。目录不存在会创建。
    #[arg(long, value_name = "目录")]
    pub output_dir: Option<PathBuf>,

    /// 要抽帧的视频文件。
    #[arg(required = true, value_name = "视频")]
    pub video: PathBuf,

    /// 版式 1 不补缺帧。抽到的张数不够就停。
    #[arg(skip)]
    pub strict_frames: bool,
}

impl Extract {
    pub fn run(&self) -> anyhow::Result<ExtractData> {
        let number = self.number.context("需要 -n")?;
        let Self {
            ignore_start,
            ignore_end,
            threads,
            video,
            output_dir,
            ..
        } = self;

        let video_duration_s = ffprobe::ffprobe(video)?
            .format
            .duration
            .context("invalid video duration")?
            .parse::<f32>()
            .context("invalid video duration")?;

        let duration_s = video_duration_s
            - ignore_start.to_secs(video_duration_s)
            - ignore_end.to_secs(video_duration_s);

        ensure!(
            duration_s > 0.0,
            "invalid negative video duration minus offsets"
        );

        let out_dir = match output_dir {
            Some(dir) => {
                fs::create_dir_all(dir)?;
                dir.clone()
            }
            None => PathBuf::from("."),
        };

        rayon::ThreadPoolBuilder::new()
            .num_threads(*threads)
            .build()?
            .install(|| {
                let out_templates = (0..number)
                    .into_par_iter()
                    .map(|n| {
                        let interval = duration_s / number as f32;
                        let start_s = ignore_start.to_secs(video_duration_s)
                            + interval * 0.5
                            + interval * n as f32;
                        let start_s = start_s.min(video_duration_s - self.capture_time.seconds);
                        let out_template = self.out_template(start_s, duration_s);
                        self.capture(start_s, &out_template)?;
                        Ok(out_template)
                    })
                    .collect::<anyhow::Result<Vec<_>>>()?;

                let warnings = self.fix_missing(&out_templates, &out_dir)?;

                Ok(ExtractData {
                    out_templates,
                    warnings,
                })
            })
    }

    pub fn capture_frames(&self) -> u32 {
        self.capture_frames.unwrap_or(1)
    }

    fn out_template(&self, start_s: f32, duration_s: f32) -> OutTemplate {
        let prefix = self.video.with_extension("");
        let prefix = prefix.file_name().unwrap_or_default().to_string_lossy();

        OutTemplate::new(prefix, start_s as _, duration_s as _, self.capture_frames())
    }

    fn capture(&self, start_s: f32, out_template: &OutTemplate) -> anyhow::Result<()> {
        let Self {
            capture_time,
            vfilter,
            output_dir,
            video,
            ..
        } = self;
        let capture_frames = self.capture_frames();
        ensure!(
            capture_frames > 0,
            "invalid capture-frames must be non-zero"
        );
        ensure!(
            capture_time.seconds > 0.0,
            "invalid capture-time must be non-zero"
        );

        let mut out = match output_dir {
            Some(dir) => dir.clone(),
            None => PathBuf::from("."),
        };
        out.push(out_template.to_string());

        let out = Command::new("ffmpeg")
            .arg2("-ss", start_s)
            .arg2("-t", capture_time.seconds)
            .arg2("-i", video)
            .arg2("-r", format!("{capture_frames}/{}", capture_time.seconds))
            .arg2("-fps_mode", "cfr")
            .arg2_opt("-vf", vfilter.as_ref())
            .arg2("-vframes", capture_frames)
            .arg("-y")
            .arg(&out)
            .output()?;

        ensure!(
            out.status.success(),
            "ffmpeg capture failed\n---stderr---\n{}\n------",
            String::from_utf8_lossy(&out.stderr).trim(),
        );

        Ok(())
    }

    /// Check extractions and fix missing. Returns a list of warnings.
    ///
    /// In fairly rare cases ffmpeg can fail to extract the expected number of frames.
    /// Auto fixing will simply cover these missing frames with duplicates of the previous frame.
    fn fix_missing(
        &self,
        extracts: &[OutTemplate],
        temp_dir: &Path,
    ) -> anyhow::Result<Vec<String>> {
        let mut warnings = Vec::new();

        // ensure all captures exist
        for tmpl in extracts {
            let mut first = temp_dir.to_path_buf();
            first.push(tmpl.with_frame(1));
            ensure!(first.is_file(), "Failed to extract: {}", sh_escape(&first));

            let mut prev = first;
            let mut fixes = 0;
            for f in 2..=self.capture_frames() {
                let mut next = temp_dir.to_path_buf();
                next.push(tmpl.with_frame(f));
                if !next.is_file() {
                    if self.strict_frames {
                        bail!("版式 1 缺帧，不出接触表: {}", sh_escape(&next));
                    }
                    fs::hard_link(&prev, &next).or_else(|_| fs::copy(&prev, &next).map(|_| ()))?;
                    fixes += 1;
                }
                prev = next;
            }
            if fixes != 0 {
                warnings.push(format!(
                    "Duplicated {fixes} captures to cover missing {tmpl} frames"
                ));
            }
        }

        Ok(warnings)
    }
}

pub struct ExtractData {
    /// All ffmpeg capture output templates.
    pub out_templates: Vec<OutTemplate>,
    pub warnings: Vec<String>,
}

/// "prefix-Ss-F.bmp" template.
///
/// S = seconds. Constant for a given template.
/// F = frames using a ffmpeg/printf `%0nd` style.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct OutTemplate {
    pub prefix: String,
    pub seconds: u32,
    second_w: usize,
    frame_w: usize,
}

impl OutTemplate {
    fn new(prefix: impl Into<String>, seconds: u32, max_seconds: u32, max_frames: u32) -> Self {
        let second_w = max_seconds.to_string().len();
        let frame_w = max_frames.to_string().len();
        let mut prefix = prefix.into();
        // try to avoid breaking the ffmpeg output template
        if prefix.contains('%') {
            prefix = prefix.replace('%', "");
        }
        Self {
            prefix,
            seconds,
            second_w,
            frame_w,
        }
    }

    /// Return a string capture file name with the given frame number.
    pub fn with_frame(&self, f: u32) -> String {
        let Self {
            prefix,
            seconds,
            second_w,
            frame_w,
        } = self;
        format!("{prefix}-{seconds:0second_w$}s-{f:0frame_w$}.bmp")
    }
}

impl fmt::Display for OutTemplate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self {
            prefix,
            seconds,
            second_w,
            frame_w,
        } = self;
        write!(f, "{prefix}-{seconds:0second_w$}s-%0{frame_w}d.bmp")
    }
}
