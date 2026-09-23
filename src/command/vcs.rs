use crate::{
    command::{self, header, label, sh_escape, sh_escape_filename},
    process::CommandExt,
    temporary,
};
use anyhow::ensure;
use rayon::prelude::{IntoParallelIterator, ParallelIterator};
use std::{fs, path::PathBuf, process::Command, time::Duration};

/// 从视频生成接触表，编码成 AVIF。
///
/// 抽帧、拼网格，再编码。不写 `--layout` 是等大网格：每一格一样大，贴在一起，没有外缘空白。这时必须给出 `-n`（格数）、`-c`（列数），以及 `-H` 或 `-W`（二者取一）。行数由格数和列数算出来，最后一行可以不满。
///
/// `--layout 1` 改用固定版式。一截 14 格，4 列、5 个行单位，左上和右下各一个 2×2 大格，正中间一行是 4 个小格。格与格之间留 8 像素黑缝，大格盖住内部那条缝；左右外缘各留 8 像素，上下外缘不留。这时 `-n` 是截数，默认 1。从第二截起，两截之间多一行 4 个小格，把上下两个大格隔开，所以 1 截 14 张，2 截 32 张，3 截 50 张。`-c`、`-H`、`-W` 被忽略并提示。小格高度固定 216，宽度按画面比例换算。
///
/// `-f` 默认 30，`-t` 默认 1500ms，`--avif-fps` 默认 20，所以默认大约是 1.5 秒的实时动画。`-f 1` 是静图。输出扩展名必须是 `.avif`。不写 `-o` 时用输入文件名换扩展名。网格上方默认画参数栏。每一格右下角印采样时刻。
#[derive(clap::Parser, Debug, Clone)]
#[group(skip)]
#[command(
    override_usage = "vimg vcs [选项] <视频>",
    after_help = "示例:\n  \
    vimg vcs -c 5 -n 25 -H 288 视频.mkv\n  \
    vimg vcs -c 7 -n 35 -H 288 -f 1 视频.mkv\n  \
    vimg vcs --layout 1 -n 2 视频.mkv"
)]
pub struct Vcs {
    /// 等大网格的列数。
    ///
    /// 必填，除非 `--layout 1`。版式 1 固定 4 列，写了也忽略，并在终端提示。格数不能整除列数时，最后一行不满。
    #[arg(long, short, value_name = "列数")]
    pub columns: Option<u32>,

    /// 输出文件。扩展名必须是 `.avif`。
    ///
    /// 不写时用输入文件名换扩展名，写到当前目录。指定了 `--output-dir` 时写到那个目录。编码先写到临时目录，成功后再挪过来。
    #[arg(long, short, value_name = "文件")]
    pub output: Option<PathBuf>,

    /// 编码质量，传给 ffmpeg 的 `-crf`。越小越清晰，文件越大。
    #[arg(long, value_name = "质量", default_value_t = 30)]
    pub avif_crf: u8,

    /// ffmpeg 用来编码 AVIF 的视频编码器。
    ///
    /// 默认 `libsvtav1`。要接近旧版行为可改成 `libaom-av1`。只有 `libaom-av1` 使用 `--avif-preset` 作为 `-cpu-used`，其余编码器作为 `-preset`。像素格式固定为 yuv420p10le。
    #[arg(long, value_name = "编码器", default_value = "libsvtav1")]
    pub avif_codec: String,

    /// 编码速度预设。数字越小通常越慢、压缩越好。
    ///
    /// 不写时，单帧（`-f 1`）用 1，多帧用 6。编码器是 `libaom-av1` 时传给 `-cpu-used`，其余编码器传给 `-preset`。
    #[arg(long, value_name = "预设")]
    pub avif_preset: Option<u8>,

    /// 多帧 AVIF 的播放帧率。
    ///
    /// 默认 20。配合默认的 `-f 30 -t 1500ms`（1.5 秒里 30 帧）接近实时。改成 10 就是同样素材的半速。静图不受这个值影响。
    #[arg(long, value_name = "帧率", default_value_t = 20.0)]
    pub avif_fps: f32,

    /// 等大网格里每一格的像素宽度。按画面比例缩放，不裁切。
    ///
    /// 与 `-H` 取一，不能同时写。`--layout 1` 时忽略并提示：小格高度固定 216，宽度按比例换算。
    #[arg(
        long,
        short = 'W',
        value_name = "像素",
        conflicts_with = "capture_height"
    )]
    pub capture_width: Option<u32>,

    /// 等大网格里每一格的像素高度。按画面比例缩放，不裁切。
    ///
    /// 与 `-W` 取一，不能同时写。等大网格必须写其中一个。`--layout 1` 时忽略并提示。
    #[arg(
        long,
        short = 'H',
        value_name = "像素",
        conflicts_with = "capture_width"
    )]
    pub capture_height: Option<u32>,

    #[clap(flatten)]
    pub args: command::Extract,

    #[clap(flatten)]
    pub header: header::HeaderArgs,

    /// 退出时保留临时目录。
    ///
    /// 默认在当前目录（或 `--output-dir`）下建 `.vimg-` 加 12 位随机字符的目录，存放抽出的 BMP 和编码前的成片。正常退出和 Ctrl-C 都会删掉。加上这个开关则留下，并在开始时打印路径。
    #[arg(long, default_value_t = false)]
    pub keep: bool,

    /// 接触表版式。不写是等大网格。
    ///
    /// `1` 是固定的一截 14 格：4 列、5 个行单位，左上和右下各一个 2×2 大格，正中间一行 4 个小格。格缝 8 像素，左右外缘同样宽，上下外缘不留。这时 `-n` 改成截数，默认 1；多截之间另有一行 4 个小格隔开大格。不认识的编号会直接失败。目前只有 1。
    #[arg(long, value_name = "编号")]
    pub layout: Option<u32>,
}

impl Vcs {
    pub fn run(mut self) -> anyhow::Result<()> {
        ensure!(
            self.output
                .as_ref()
                .is_none_or(|p| p.extension().and_then(|e| e.to_str()) == Some("avif")),
            "output must be avif"
        );

        let parent_dir = self
            .args
            .output_dir
            .clone()
            .unwrap_or_else(|| PathBuf::from("."));
        let temp_dir = temporary::process_dir(self.args.output_dir.clone(), !self.keep);

        self.args.output_dir = Some(temp_dir.clone());
        self.args.capture_frames = self.args.capture_frames.or(Some(30));

        let spinner = indicatif::ProgressBar::new_spinner().with_style(
            indicatif::ProgressStyle::default_spinner()
                .template("{spinner:.cyan.bold} {elapsed_precise:.bold} {msg}")?,
        );
        spinner.enable_steady_tick(Duration::from_millis(100));

        let layout = command::layout::known(self.layout)?;
        let layout_small_w = self.prepare_layout(layout, &spinner)?;
        let ex_scale = if let Some(small_w) = layout_small_w {
            let (large_w, large_h) = command::layout::large_size(small_w);
            Some(format!("scale={large_w}:{large_h}:flags=bicubic,setsar=1"))
        } else {
            self.extract_scale()?
        };
        self.args.vfilter = match (self.args.vfilter, ex_scale) {
            (Some(vf), Some(scale)) => Some(format!("{vf},{scale}")),
            (vf, scale) => vf.or(scale),
        };

        if self.keep {
            spinner.println(format!(
                "Keeping temporary files in {}",
                sh_escape(&temp_dir)
            ));
        }

        spinner.set_message("Extracting");
        let extract = self.args.run()?;

        for msg in &extract.warnings {
            spinner.println(format!("Warning: {msg}"));
        }

        spinner.set_message("Joining");
        let header_band = self.header_band(&extract, &temp_dir, &spinner, layout_small_w)?;
        let frame_w = self.args.capture_frames().to_string().len();
        let file_prefix = self.args.video.with_extension("");
        let file_prefix = file_prefix
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .replace('%', "");

        (0..self.args.capture_frames())
            .into_par_iter()
            .try_for_each(|f| {
                let capture_images: Vec<_> = extract
                    .out_templates
                    .iter()
                    .map(|tmpl| {
                        let mut o = temp_dir.to_path_buf();
                        o.push(tmpl.with_frame(f + 1));
                        o
                    })
                    .collect();

                let label = extract
                    .out_templates
                    .iter()
                    .map(|tmpl| label::seconds_text(tmpl.seconds))
                    .collect();

                command::Join {
                    columns: self.columns.filter(|_| layout.is_none()),
                    layout: self.layout.filter(|_| layout.is_some()),
                    output: {
                        let mut o = temp_dir.to_path_buf();
                        o.push(format!("{file_prefix}-{f:0frame_w$}.bmp"));
                        o
                    },
                    capture_images,
                    capture_width: None,
                    capture_height: None,
                    label,
                    video: None,
                    header: command::header::HeaderArgs::default(),
                    header_band: header_band.clone(),
                    layout_small_w,
                }
                .run()
            })?;

        // write to temp location until successful
        let temp_out_file = {
            let mut o = temp_dir.clone();
            o.push(format!("{file_prefix}.avif"));
            o
        };
        // output file if successful
        let out_file = self.output.unwrap_or_else(|| {
            let mut o = parent_dir;
            o.push(format!("{file_prefix}.avif"));
            o
        });

        spinner.set_message(format!("Encoding {}", sh_escape_filename(&out_file)));
        let out = Command::new("ffmpeg")
            .arg2("-r", self.avif_fps)
            .arg2("-i", {
                let mut o = temp_dir;
                o.push(format!("{file_prefix}-%0{frame_w}d.bmp"));
                o
            })
            .arg2("-c:v", &self.avif_codec)
            .arg2(
                match self.avif_codec.as_str() {
                    "libaom-av1" => "-cpu-used",
                    _ => "-preset",
                },
                self.avif_preset
                    .unwrap_or(match self.args.capture_frames() {
                        1 => 1,
                        _ => 6,
                    }),
            )
            .arg2("-crf", self.avif_crf)
            .arg2("-pix_fmt", "yuv420p10le")
            .arg("-y")
            .arg(&temp_out_file)
            .output()?;
        ensure!(
            out.status.success(),
            "ffmpeg convert-to-avif failed\n---stderr---\n{}\n------",
            String::from_utf8_lossy(&out.stderr).trim(),
        );

        fs::rename(&temp_out_file, &out_file)
            .or_else(|_| fs::copy(&temp_out_file, &out_file).map(|_| ()))?;

        spinner.finish();
        Ok(())
    }

    /// 版式 1 把 `-n` 改成截数，并忽略列数和格子尺寸。返回小格宽。
    fn prepare_layout(
        &mut self,
        layout: Option<u32>,
        spinner: &indicatif::ProgressBar,
    ) -> anyhow::Result<Option<u32>> {
        let Some(1) = layout else {
            ensure!(self.args.number.is_some(), "需要 -n");
            ensure!(self.columns.is_some(), "需要 -c");
            return Ok(None);
        };

        for warning in [
            self.columns.map(|_| "警告: -c 在版式 1 不起作用。"),
            self.capture_width.map(|_| "警告: -W 在版式 1 不起作用。"),
            self.capture_height.map(|_| "警告: -H 在版式 1 不起作用。"),
        ]
        .into_iter()
        .flatten()
        {
            spinner.println(warning);
        }

        let bands = self.args.number.unwrap_or(1);
        // 截数不合法时先停，不必去读视频。
        let captures = command::layout::captures_for_bands(bands)?;
        let (src_w, src_h) = command::header::frame_size(&self.args.video)?;
        let small_w = command::layout::small_width(src_w, src_h)?;
        self.args.number = Some(captures);
        self.args.strict_frames = true;
        Ok(Some(small_w))
    }

    fn header_band(
        &self,
        extract: &command::ExtractData,
        temp_dir: &std::path::Path,
        spinner: &indicatif::ProgressBar,
        layout_small_w: Option<u32>,
    ) -> anyhow::Result<Option<std::sync::Arc<image::RgbaImage>>> {
        let mode = self.header.mode();
        if mode == header::InfoMode::Off {
            return Ok(None);
        }
        let Some(first) = extract.out_templates.first() else {
            return Ok(None);
        };
        let mut path = temp_dir.to_path_buf();
        path.push(first.with_frame(1));
        let width = if let Some(small_w) = layout_small_w {
            let bands = command::layout::bands_from_images(extract.out_templates.len() as u32)?;
            command::layout::grid_px(bands, small_w).0
        } else {
            let (cap_w, _) = image::image_dimensions(&path)?;
            let (_, cols) = command::grid_shape(
                extract.out_templates.len() as u32,
                self.columns.unwrap_or(1),
            );
            cap_w * cols.max(1)
        };
        let band = header::render(&self.args.video, mode, self.header.font.as_deref(), width)?;
        if let Some(warning) = &band.warning {
            spinner.println(warning.clone());
        }
        Ok(Some(band.image))
    }

    fn extract_scale(&self) -> anyhow::Result<Option<String>> {
        if let Some(h) = self.capture_height {
            return Ok(Some(format!("scale=-1:{h}:flags=bicubic")));
        }
        let Some(w) = self.capture_width else {
            anyhow::bail!("需要 -H 或 -W");
        };
        Ok(Some(format!("scale={w}:-1:flags=bicubic")))
    }
}
