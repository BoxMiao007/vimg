pub mod label;

use crate::command::header::{self, HeaderArgs, InfoMode};
use anyhow::{anyhow, bail};
use image::{GenericImage, RgbaImage};
use rayon::prelude::*;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

/// Join same-sized capture images into a single grid image.
#[derive(clap::Parser, Debug, Clone)]
#[group(skip)]
pub struct Join {
    /// Number of capture columns in output.
    #[arg(long, short)]
    pub columns: u32,

    /// Pixel width of each capture inside the grid. Will be scaled preserving aspect.
    #[arg(long, short = 'W')]
    pub capture_width: Option<u32>,

    /// Pixel height of each capture inside the grid. Will be scaled preserving aspect.
    #[arg(long, short = 'H')]
    pub capture_height: Option<u32>,

    /// Output file name.
    #[arg(long, short)]
    pub output: PathBuf,

    #[arg(long)]
    pub label: Vec<String>,

    /// Video file to read the header from. Without it, no header is drawn.
    #[arg(long)]
    pub video: Option<PathBuf>,

    #[clap(flatten)]
    pub header: HeaderArgs,

    /// Images to join.
    #[arg(required = true)]
    pub capture_images: Vec<PathBuf>,

    /// Already-rendered header. Set by `vcs` so each frame does not probe again.
    #[arg(skip)]
    pub header_band: Option<Arc<RgbaImage>>,
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
        let Self {
            columns,
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
        let (rows, cols) = grid_shape(n_captures, *columns);

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
