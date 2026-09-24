use glyph_brush_layout::{
    GlyphPositioner, HorizontalAlign, SectionGeometry, SectionText, VerticalAlign,
    ab_glyph::{Font, FontRef, PxScale, Rect, point},
};
use image::Pixel;

pub(crate) const CANTARELL: &[u8] = include_bytes!("Cantarell-Regular.ttf");

#[derive(Debug, Clone)]
pub struct Config {
    pub scale_percent: f32,
    /// 字号封顶。版式 1 小格按比例算出的字号与此相同，大小格一致。
    pub max_px: f32,
    pub margin_percent: f32,
    pub padding_percent: f32,
    pub background_opacity: f32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            scale_percent: 0.14,
            max_px: 30.0,
            margin_percent: 0.03,
            padding_percent: 0.01,
            background_opacity: 0.0,
        }
    }
}

pub fn draw(
    img: image::DynamicImage,
    label: &str,
    conf: &Config,
) -> anyhow::Result<image::DynamicImage> {
    if label.is_empty() {
        return Ok(img);
    }

    let (imgw, imgh) = (img.width() as f32, img.height() as f32);
    let min_dim = imgw.min(imgh);
    let font = FontRef::try_from_slice(CANTARELL)?;
    let scale = PxScale::from((min_dim * conf.scale_percent).min(conf.max_px));
    let margin = min_dim * conf.margin_percent;
    let pad = min_dim * conf.padding_percent;

    let layout = glyph_brush_layout::Layout::default_single_line()
        .v_align(VerticalAlign::Bottom)
        .h_align(HorizontalAlign::Right);
    let geometry = SectionGeometry {
        screen_position: (imgw - margin * 2.0, imgh - margin),
        bounds: (imgw, imgh),
    };

    let glyphs = layout.calculate_glyphs(
        &[&font],
        &geometry,
        &[SectionText {
            text: label,
            scale,
            ..<_>::default()
        }],
    );

    let mut rgba = img.into_rgba8();

    let outline_glyphs: Vec<_> = glyphs
        .into_iter()
        .filter_map(|g| font.outline_glyph(g.glyph))
        .collect();

    // 时间戳不铺底。先画阴影，再画描边，最后盖上白色字形。
    if conf.background_opacity > 0.0
        && let Some(b) = outline_glyphs
            .iter()
            .map(|g| g.px_bounds())
            .fold(None, |b: Option<Rect>, next| {
                b.map(|b| {
                    let min_x = b.min.x.min(next.min.x);
                    let max_x = b.max.x.max(next.max.x);
                    let min_y = b.min.y.min(next.min.y);
                    let max_y = b.max.y.max(next.max.y);
                    Rect {
                        min: point(min_x, min_y),
                        max: point(max_x, max_y),
                    }
                })
                .or(Some(next))
            })
            .map(|mut b| {
                // cap the glyph bounds to the layout specified max bounds
                let Rect { min, max } = layout.bounds_rect(&geometry);
                b.min.x = b.min.x.max(min.x) - pad;
                b.min.y = b.min.y.max(min.y) - pad;
                b.max.x = b.max.x.min(max.x) + pad;
                b.max.y = b.max.y.min(max.y) + pad;
                b
            })
    {
        let max_x = (b.max.x.ceil() as u32).min(rgba.width() - 1);
        let min_x = b.min.x as u32;
        let max_y = (b.max.y.ceil() as u32).min(rgba.height() - 1);
        let min_y = b.min.y as u32;

        for x in min_x..=max_x {
            for y in min_y..=max_y {
                if (x == max_x || x == min_x) && (y == max_y || y == min_y) {
                    // skip corners
                    continue;
                }
                rgba.get_pixel_mut(x, y).blend(&image::Rgba([
                    0,
                    0,
                    0,
                    (conf.background_opacity * 255.0) as u8,
                ]));
            }
        }
    }

    // 描边固定 1.5 像素。按字号比例平移会把 0、4、8 的孔填死。
    let shadow = (scale.y * 0.04).clamp(1.5, 3.0);
    let stroke = 1.5;
    stamp(&mut rgba, &outline_glyphs, shadow, shadow, [0, 0, 0]);
    for dy in [-stroke, 0.0, stroke] {
        for dx in [-stroke, 0.0, stroke] {
            if dx == 0.0 && dy == 0.0 {
                continue;
            }
            stamp(&mut rgba, &outline_glyphs, dx, dy, [0, 0, 0]);
        }
    }
    stamp(&mut rgba, &outline_glyphs, 0.0, 0.0, [255, 255, 255]);

    Ok(rgba.into())
}

/// 按字形覆盖率把一种颜色印上去。超出画面的部分丢掉。
fn stamp(
    rgba: &mut image::RgbaImage,
    glyphs: &[glyph_brush_layout::ab_glyph::OutlinedGlyph],
    dx: f32,
    dy: f32,
    color: [u8; 3],
) {
    let (width, height) = rgba.dimensions();
    for glyph in glyphs {
        let bounds = glyph.px_bounds();
        glyph.draw(|x, y, c| {
            if c <= 0.0 {
                return;
            }
            let px = bounds.min.x + dx + x as f32;
            let py = bounds.min.y + dy + y as f32;
            if px < 0.0 || py < 0.0 {
                return;
            }
            let (px, py) = (px as u32, py as u32);
            if px >= width || py >= height {
                return;
            }
            rgba.get_pixel_mut(px, py).blend(&image::Rgba([
                color[0],
                color[1],
                color[2],
                (c * 255.0) as u8,
            ]));
        });
    }
}

pub fn seconds_text(seconds: u32) -> String {
    let hours = seconds / 3600;
    let mins = (seconds / 60) % 60;
    let secs = seconds % 60;
    match (hours, mins, secs) {
        (0, m, s) => format!("{m:02}:{s:02}"),
        (h, m, s) => format!("{h}:{m:02}:{s:02}"),
    }
}
