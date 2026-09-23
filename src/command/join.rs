pub mod label;

use crate::command::{
    header::{self, HeaderArgs, InfoMode},
    layout,
};
use anyhow::{Context, anyhow, bail, ensure};
use image::{GenericImage, RgbaImage};
use rayon::prelude::*;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

/// 把尺寸相同的截帧拼成一张网格图。
///
/// 不调用 ffmpeg，除非为了画参数栏而去探测 `--video`。等大网格必须给 `-c`，格子贴在一起。`-H`、`-W` 可选，给出就按比例缩放。
///
/// `--layout 1` 的张数必须正好是一截或连续的多截：14、32、50……对不上就不出图。输出格式看 `-o` 的扩展名，常见的有 png、jpg、bmp。
#[derive(clap::Parser, Debug, Clone)]
#[group(skip)]
#[command(
    override_usage = "vimg join [选项] -o <输出> <图片>...",
    after_help = "示例:\n  \
    vimg join -c 4 -H 216 -o sheet.png 帧1.bmp 帧2.bmp\n  \
    vimg join --layout 1 -o sheet.png --video 视频.mkv 帧01.bmp ..."
)]
pub struct Join {
    /// 等大网格的列数。
    ///
    /// 必填，除非 `--layout 1`。版式 1 固定 4 列，写了也忽略，并在终端提示。
    #[arg(long, short, value_name = "列数")]
    pub columns: Option<u32>,

    /// 每一格的像素宽度。按画面比例缩放，不裁切。
    ///
    /// 与 `-H` 可以同时写，也可以都不写。都不写就用原图尺寸。`--layout 1` 时忽略并提示。
    #[arg(long, short = 'W', value_name = "像素")]
    pub capture_width: Option<u32>,

    /// 每一格的像素高度。按画面比例缩放，不裁切。
    ///
    /// 与 `-W` 可以同时写，也可以都不写。都不写就用原图尺寸。`--layout 1` 时忽略并提示：比例跟第一张图走，小格高度固定 216。
    #[arg(long, short = 'H', value_name = "像素")]
    pub capture_height: Option<u32>,

    /// 输出图片路径。扩展名决定格式。
    #[arg(long, short, value_name = "文件")]
    pub output: PathBuf,

    /// 印在格子右下角的文字。可重复，按顺序对应每一张图。
    ///
    /// 少给的格子留空。字号约为该格短边的 16%，最大 40 像素，没有底色。
    #[arg(long, value_name = "文字")]
    pub label: Vec<String>,

    /// 用来画参数栏的源视频。
    ///
    /// 不给就不画参数栏。`--info-all` 必须和它一起用，否则失败。没有真正的画面流时不出图；封面图不算视频轨。
    #[arg(long, value_name = "视频")]
    pub video: Option<PathBuf>,

    /// 接触表版式。不写是等大网格。
    ///
    /// `1` 是固定的一截 14 格。图片张数必须是 14，或之后每多一截加 18：32、50……对不上就不出图。不认识的编号会直接失败。
    #[arg(long, value_name = "编号")]
    pub layout: Option<u32>,

    #[clap(flatten)]
    pub header: HeaderArgs,

    /// 要拼进去的图片，至少一张。等大网格要求尺寸相同。
    #[arg(required = true, value_name = "图片")]
    pub capture_images: Vec<PathBuf>,

    /// Already-rendered header. Set by `vcs` so each frame does not probe again.
    #[arg(skip)]
    pub header_band: Option<Arc<RgbaImage>>,

    /// 版式 1 的小格宽。`vcs` 按源视频算好传入；抽帧已经缩成大格，不能再拿来重算。
    #[arg(skip)]
    pub layout_small_w: Option<u32>,
}

/// Rows, then the number of columns actually used.
pub fn grid_shape(n_captures: u32, columns: u32) -> (u32, u32) {
    if columns == 0 || n_captures <= columns {
        (1, n_captures.max(1))
    } else {
        let mut rows = n_captures / columns;
        if !n_captures.is_multiple_of(columns) {
            rows += 1;
        }
        (rows, columns)
    }
}

impl Join {
    pub fn run(&self) -> anyhow::Result<()> {
        match layout::known(self.layout)? {
            Some(1) => self.run_layout1(),
            None => self.run_uniform(),
            Some(_) => unreachable!("known() 只放行版式 1"),
        }
    }

    fn run_uniform(&self) -> anyhow::Result<()> {
        let columns = self.columns.context("需要 -c")?;
        let Self {
            output,
            capture_images,
            ..
        } = self;

        let n_captures = capture_images.len() as u32;

        // load images concurrently
        let images: Vec<_> = capture_images
            .par_iter()
            .map(|i| self.load_image(i).map_err(|e| anyhow!("{i:?}: {e}")))
            .collect::<Result<Vec<_>, _>>()?;

        let (cap_w, cap_h) = (images[0].width(), images[0].height());
        let (rows, cols) = grid_shape(n_captures, columns);

        let mut labels = self.label.clone();
        labels.resize_with(images.len(), String::new);

        let mut all = image::RgbaImage::new(cap_w * cols, cap_h * rows);
        for (idx, (img, label)) in images.into_iter().zip(labels).enumerate() {
            let idx = idx as u32;
            let x = (idx % cols) * cap_w;
            let y = (idx / cols) * cap_h;
            let img = label::draw(img, &label, &label::Config::default())?;
            all.copy_from(&img, x as _, y as _)?;
        }

        let all = self.with_header(all)?;
        image::DynamicImage::from(all).into_rgb8().save(output)?;

        Ok(())
    }

    fn run_layout1(&self) -> anyhow::Result<()> {
        for warning in layout1_warnings(self.columns, self.capture_width, self.capture_height) {
            eprintln!("{warning}");
        }

        let bands = layout::bands_from_images(self.capture_images.len() as u32)?;
        let images: Vec<_> = self
            .capture_images
            .par_iter()
            .map(|i| self.load_unscaled(i).map_err(|e| anyhow!("{i:?}: {e}")))
            .collect::<Result<Vec<_>, _>>()?;

        let mut labels = self.label.clone();
        labels.resize_with(images.len(), String::new);

        let small_w = match self.layout_small_w {
            Some(width) => width,
            None => layout::small_width(images[0].width(), images[0].height())?,
        };
        let grid = compose_layout1(images, &labels, small_w)?;
        let (_, expected_h) = layout::grid_px(bands, small_w);
        ensure!(
            grid.height() == expected_h,
            "内部错误: 版式 1 的高度和截数不一致"
        );
        let grid = self.with_header(grid)?;
        image::DynamicImage::from(grid)
            .into_rgb8()
            .save(&self.output)?;
        Ok(())
    }

    /// 版式 1 的格子尺寸是固定的，调用方传来的缩放不在这里用。
    fn load_unscaled(&self, path: impl AsRef<Path>) -> anyhow::Result<image::DynamicImage> {
        let path = path.as_ref();
        Ok(image::ImageReader::open(path)?.decode()?)
    }

    fn with_header(&self, grid: RgbaImage) -> anyhow::Result<RgbaImage> {
        if let Some(band) = &self.header_band {
            return Ok(header::stack(band, &grid));
        }
        let mode = self.header.mode();
        let Some(video) = &self.video else {
            if mode == InfoMode::Full {
                bail!("完整参数需要 --video");
            }
            return Ok(grid);
        };
        if mode == InfoMode::Off {
            return Ok(grid);
        }
        let band = header::render(video, mode, self.header.font.as_deref(), grid.width())?;
        if let Some(warning) = &band.warning {
            eprintln!("{warning}");
        }
        Ok(header::stack(&band.image, &grid))
    }

    fn load_image(&self, path: impl AsRef<Path>) -> anyhow::Result<image::DynamicImage> {
        let path = path.as_ref();
        let mut img = image::ImageReader::open(path)?.decode()?;

        if self.capture_width.is_some() || self.capture_height.is_some() {
            img = img.resize(
                self.capture_width.unwrap_or(u32::MAX),
                self.capture_height.unwrap_or(u32::MAX),
                image::imageops::FilterType::CatmullRom,
            );
        }

        Ok(img)
    }
}

fn layout1_warnings(
    columns: Option<u32>,
    capture_width: Option<u32>,
    capture_height: Option<u32>,
) -> Vec<String> {
    let mut out = Vec::new();
    if columns.is_some() {
        out.push("警告: -c 在版式 1 不起作用。".into());
    }
    if capture_width.is_some() {
        out.push("警告: -W 在版式 1 不起作用。".into());
    }
    if capture_height.is_some() {
        out.push("警告: -H 在版式 1 不起作用。".into());
    }
    out
}

/// 先缩成固定大格，小格再缩小一半，然后按格子大小印时间戳。
fn compose_layout1(
    images: Vec<image::DynamicImage>,
    labels: &[String],
    small_w: u32,
) -> anyhow::Result<RgbaImage> {
    let small_w = if small_w > 0 {
        small_w
    } else {
        let (src_w, src_h) = (images[0].width(), images[0].height());
        layout::small_width(src_w, src_h)?
    };
    let (large_w, large_h) = layout::large_size(small_w);
    let bands = layout::bands_from_images(images.len() as u32)?;
    let (grid_w, grid_h) = layout::grid_px(bands, small_w);
    let mut canvas = RgbaImage::new(grid_w, grid_h);

    for (index, img) in images.into_iter().enumerate() {
        let (x, y, side_w, side_h) = layout::place(index as u32, small_w);
        let mut sized = img.resize_exact(large_w, large_h, image::imageops::FilterType::CatmullRom);
        if side_w != large_w {
            sized = sized.resize_exact(side_w, side_h, image::imageops::FilterType::CatmullRom);
        }
        let label = labels.get(index).map(String::as_str).unwrap_or("");
        let sized = label::draw(sized, label, &label::Config::default())?;
        canvas.copy_from(&sized, x, y)?;
    }
    Ok(canvas)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgba, RgbaImage};

    #[test]
    fn layout1_paints_each_capture_in_reading_order() {
        let images: Vec<_> = (0..14)
            .map(|n| {
                let px = Rgba([n, 0, 255 - n, 255]);
                image::DynamicImage::ImageRgba8(RgbaImage::from_pixel(100, 80, px))
            })
            .collect();
        let grid = compose_layout1(images, &[], 0).unwrap();
        // 100×80 的比例：216 * 100 / 80 = 270，已经是偶数。
        let small_w = 270;
        assert_eq!(grid.dimensions(), layout::grid_px(1, small_w));

        for index in 0..14u32 {
            let (x, y, side_w, side_h) = layout::place(index, small_w);
            let px = grid.get_pixel(x + side_w / 2, y + side_h / 2);
            assert_eq!(
                px.0[0],
                index as u8,
                "第 {} 张没有落在自己的格子中间",
                index + 1
            );
            assert_eq!(px.0[2], 255 - index as u8);
        }
        // 中间行右边两格之间的竖缝不在任何大格里面，应仍是画布原色。
        let (x, y, side_w, _) = layout::place(7, small_w);
        assert_eq!(
            grid.get_pixel(x + side_w + layout::GAP / 2, y + layout::SMALL_HEIGHT / 2)
                .0,
            [0, 0, 0, 0]
        );
        // 左右外缘是一条与格缝同宽的空白。
        assert_eq!(grid.get_pixel(layout::GAP / 2, y).0, [0, 0, 0, 0]);
        assert_eq!(
            grid.get_pixel(grid.width() - layout::GAP / 2, y).0,
            [0, 0, 0, 0]
        );
    }

    #[test]
    fn two_bands_keep_a_row_of_small_cells_between_the_large_ones() {
        let images: Vec<_> = (0..32)
            .map(|n| {
                let px = Rgba([n as u8, 0, 0, 255]);
                image::DynamicImage::ImageRgba8(RgbaImage::from_pixel(16, 9, px))
            })
            .collect();
        let small_w = layout::small_width(16, 9).unwrap();
        let grid = compose_layout1(images, &[], small_w).unwrap();
        assert_eq!(grid.dimensions(), layout::grid_px(2, small_w));

        // 第 15 到 18 张是隔开两截大格的那一行。
        for index in 14..18u32 {
            let (x, y, side_w, side_h) = layout::place(index, small_w);
            assert_eq!((side_w, side_h), (small_w, layout::SMALL_HEIGHT));
            assert_eq!(
                grid.get_pixel(x + side_w / 2, y + side_h / 2).0[0],
                index as u8
            );
        }
        let (_, separator_y, _, _) = layout::place(14, small_w);
        let (_, next_large_y, _, _) = layout::place(18, small_w);
        assert!(next_large_y > separator_y + layout::SMALL_HEIGHT);
    }
}
