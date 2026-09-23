use crate::{
    command::{self, header, label, sh_escape, sh_escape_filename},
    process::CommandExt,
    temporary,
};
use anyhow::ensure;
use rayon::prelude::{IntoParallelIterator, ParallelIterator};
use std::{fs, path::PathBuf, process::Command, time::Duration};

/// Create a new contact sheet for a video.
///
/// Extracts capture frames and joins into sheet(s) then encodes into
/// an animated, or static, vcs avif.
#[derive(clap::Parser, Debug, Clone)]
#[group(skip)]
pub struct Vcs {
    /// Number of capture columns in output.
    ///
    /// Required unless `--layout 1`, which is always 4 columns.
    #[arg(long, short)]
    pub columns: Option<u32>,

    /// Output file name. Defaults to input with .avif extension.
    #[arg(long, short)]
    pub output: Option<PathBuf>,

    /// Crf quality level for encoding the output avif.
    #[arg(long, default_value_t = 30)]
    pub avif_crf: u8,

    /// Ffmpeg vcodec to use for encoding the output avif.
    #[arg(long, default_value = "libsvtav1")]
    pub avif_codec: String,

    /// Preset (or "cpu-used" for libaom-av1) for encoding the output avif.
    ///
    /// Default 1 for single-frame, 6 for multi-frame.
    #[arg(long)]
    pub avif_preset: Option<u8>,

    /// Output avif framerate for multi-frame outputs.
    ///
    /// Example: The default 20fps will result in real time playback for
    /// the default args: -f30 -t1500ms (30 frames over a 1.5s duration).
    /// So using 10fps will result in half-time playback for: -f30 -t1500ms.
    #[arg(long, default_value_t = 20.0)]
    pub avif_fps: f32,

    /// Pixel width of each capture inside the grid. Will be scaled preserving aspect.
    ///
    /// Use this or -H (not both).
    #[arg(long, short = 'W', conflicts_with = "capture_height")]
    pub capture_width: Option<u32>,

    /// Pixel height of each capture inside the grid. Will be scaled preserving aspect.
    ///
    /// Use this or -W (not both). Required unless `--layout 1`.
    #[arg(long, short = 'H', conflicts_with = "capture_width")]
    pub capture_height: Option<u32>,

    #[clap(flatten)]
    pub args: command::Extract,

    #[clap(flatten)]
    pub header: header::HeaderArgs,

    /// Keep temporary files.
    #[arg(long, default_value_t = false)]
    pub keep: bool,

    /// Contact-sheet layout. Omit for an even grid. `1` is the fixed 14-cell band.
    #[arg(long)]
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
