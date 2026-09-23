use crate::command::label::CANTARELL;
use anyhow::{Context, bail};
use ffprobe::{FfProbe, Stream};
use glyph_brush_layout::ab_glyph::{Font, FontVec, PxScale, PxScaleFont, ScaleFont};
use glyph_brush_layout::{
    GlyphPositioner, HorizontalAlign, SectionGeometry, SectionText, VerticalAlign,
};
use image::RgbaImage;
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

/// How much of the source video to print above the grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InfoMode {
    Off,
    #[default]
    Brief,
    Full,
}

/// Switches shared by `vcs` and `join`.
#[derive(clap::Args, Debug, Clone, Default)]
pub struct HeaderArgs {
    /// Print the short source-video header. This is the default.
    #[arg(long, conflicts_with_all = ["info_all", "no_info"])]
    pub info: bool,

    /// Print the long source-video header.
    #[arg(long, conflicts_with_all = ["info", "no_info"])]
    pub info_all: bool,

    /// Do not print a source-video header.
    #[arg(long, conflicts_with_all = ["info", "info_all"])]
    pub no_info: bool,

    /// Font file for the header. Overrides the VIMG_FONT environment variable.
    #[arg(long)]
    pub font: Option<PathBuf>,
}

impl HeaderArgs {
    pub fn mode(&self) -> InfoMode {
        if self.no_info {
            InfoMode::Off
        } else if self.info_all {
            InfoMode::Full
        } else {
            InfoMode::Brief
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Lang {
    Zh,
    En,
}

/// Facts taken from one file. Built without ffprobe in tests.
struct Facts {
    file_name: String,
    size_bytes: Option<u64>,
    videos: Vec<Track>,
    audios: Vec<Track>,
    /// First real video stream only. Cover art is not a video track.
    picture: Option<Picture>,
    duration_s: Option<f32>,
    total_bitrate: Option<u64>,
    container: Option<String>,
}

struct Track {
    codec: String,
    bitrate: Option<u64>,
    channels: Option<i64>,
    sample_rate: Option<String>,
}

struct Picture {
    width: i64,
    height: i64,
    aspect: String,
    fps: Option<String>,
    pix_fmt: Option<String>,
    frames: Option<String>,
}

pub struct Band {
    pub image: Arc<RgbaImage>,
    pub warning: Option<String>,
}

/// 第一条真正的视频轨的像素宽高。封面图不算。
pub fn frame_size(video: &Path) -> anyhow::Result<(u32, u32)> {
    let facts = probe(video)?;
    let picture = facts
        .picture
        .filter(|pic| pic.width > 0 && pic.height > 0)
        .context("没有视频轨，无法生成接触表")?;
    Ok((picture.width as u32, picture.height as u32))
}

pub fn render(
    video: &Path,
    mode: InfoMode,
    font: Option<&Path>,
    grid_width: u32,
) -> anyhow::Result<Band> {
    if mode == InfoMode::Off {
        bail!("内部错误: 关闭参数栏时不应绘制");
    }
    let facts = probe(video)?;
    if facts.videos.is_empty() {
        bail!("没有视频轨，无法生成接触表");
    }
    let (font_data, face, lang, warning) = load_font(font)?;
    let face = FontVec::try_from_vec_and_index(font_data, face)
        .map_err(|_| anyhow::anyhow!("不是可用的字体文件"))?;
    let lines = lines(&facts, mode, lang);
    let image = draw_band(&face, &lines, grid_width)?;
    Ok(Band {
        image: Arc::new(image),
        warning,
    })
}

fn probe(video: &Path) -> anyhow::Result<Facts> {
    let info = ffprobe::ffprobe(video)
        .with_context(|| format!("无法读取视频参数: {}", video.display()))?;
    Ok(facts_from(video, &info))
}

fn facts_from(video: &Path, info: &FfProbe) -> Facts {
    let videos: Vec<&Stream> = info.streams.iter().filter(|s| is_video(s)).collect();
    let audios: Vec<&Stream> = info
        .streams
        .iter()
        .filter(|s| s.codec_type.as_deref() == Some("audio"))
        .collect();

    let picture = videos.first().map(|s| Picture {
        width: s.width.unwrap_or(0),
        height: s.height.unwrap_or(0),
        aspect: aspect(s),
        fps: s
            .avg_frame_rate
            .parse::<FrameRate>()
            .ok()
            .map(|r| r.format()),
        pix_fmt: s.pix_fmt.clone(),
        frames: s.nb_frames.clone().filter(|f| !f.is_empty() && f != "N/A"),
    });

    let ext = video
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default();

    Facts {
        file_name: video
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default(),
        size_bytes: parse_u64(&info.format.size),
        videos: videos.iter().map(|s| track(s)).collect(),
        audios: audios.iter().map(|s| track(s)).collect(),
        picture,
        duration_s: info.format.duration.as_deref().and_then(|d| d.parse().ok()),
        total_bitrate: info.format.bit_rate.as_deref().and_then(parse_u64),
        container: container_name(&info.format.format_name, ext),
    }
}

fn is_video(stream: &Stream) -> bool {
    stream.codec_type.as_deref() == Some("video") && stream.disposition.attached_pic == 0
}

fn track(stream: &Stream) -> Track {
    Track {
        codec: stream
            .codec_long_name
            .clone()
            .filter(|s| !s.is_empty())
            .or_else(|| stream.codec_name.clone())
            .unwrap_or_else(|| "unknown".to_string()),
        bitrate: stream.bit_rate.as_deref().and_then(parse_u64),
        channels: stream.channels.filter(|n| *n > 0),
        sample_rate: stream
            .sample_rate
            .clone()
            .filter(|s| !s.is_empty() && s != "0"),
    }
}

fn parse_u64(v: &str) -> Option<u64> {
    let v = v.trim();
    if v.is_empty() || v == "N/A" {
        return None;
    }
    v.parse().ok()
}

struct FrameRate {
    fps: f64,
}

impl std::str::FromStr for FrameRate {
    type Err = ();

    fn from_str(v: &str) -> Result<Self, Self::Err> {
        let (n, d) = v.split_once('/').ok_or(())?;
        let n: f64 = n.parse().map_err(|_| ())?;
        let d: f64 = d.parse().map_err(|_| ())?;
        if d == 0.0 || n == 0.0 {
            return Err(());
        }
        Ok(Self { fps: n / d })
    }
}

impl FrameRate {
    fn format(&self) -> String {
        if (self.fps - self.fps.round()).abs() < 0.001 {
            format!("{}", self.fps.round() as u64)
        } else {
            format!("{:.2}", self.fps)
        }
    }
}

fn aspect(stream: &Stream) -> String {
    if let Some(dar) = stream.display_aspect_ratio.as_deref()
        && let Some(ratio) = parse_ratio(dar)
    {
        return ratio;
    }
    let w = stream.width.unwrap_or(0) as u64;
    let h = stream.height.unwrap_or(0) as u64;
    if w == 0 || h == 0 {
        return String::new();
    }
    let g = gcd(w, h);
    format!("{}:{}", w / g, h / g)
}

fn parse_ratio(dar: &str) -> Option<String> {
    let (a, b) = dar.split_once(':')?;
    let a: u64 = a.parse().ok()?;
    let b: u64 = b.parse().ok()?;
    if a == 0 || b == 0 {
        return None;
    }
    Some(format!("{a}:{b}"))
}

fn gcd(mut a: u64, mut b: u64) -> u64 {
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a
}

fn container_name(format_name: &str, extension: &str) -> Option<String> {
    if format_name.is_empty() {
        return None;
    }
    let ext = extension.to_ascii_lowercase();
    let names: Vec<_> = format_name
        .split(',')
        .map(|s| s.trim().to_ascii_lowercase())
        .collect();
    if !ext.is_empty() && names.iter().any(|n| n == &ext) {
        Some(ext)
    } else {
        Some(format_name.to_string())
    }
}

fn lines(facts: &Facts, mode: InfoMode, lang: Lang) -> Vec<String> {
    let zh = lang == Lang::Zh;
    let mut out = Vec::new();

    out.push(format!(
        "{}: {}",
        if zh { "文件名" } else { "File" },
        facts.file_name
    ));

    if let Some(bytes) = facts.size_bytes {
        out.push(format!(
            "{}: {}",
            if zh { "大小" } else { "Size" },
            format_size(bytes)
        ));
    }

    if let Some(pic) = &facts.picture
        && pic.width > 0
        && pic.height > 0
    {
        let mut line = format!(
            "{}: {}×{}({})",
            if zh { "分辨率" } else { "Resolution" },
            pic.width,
            pic.height,
            pic.aspect
        );
        if let Some(fps) = &pic.fps {
            line.push_str(&format!(", fps: {fps}"));
        }
        out.push(line);
    }

    match mode {
        InfoMode::Brief => {
            if let Some(line) = brief_codec_line(facts, zh) {
                out.push(line);
            }
        }
        InfoMode::Full => {
            for video in &facts.videos {
                out.push(format!(
                    "{}: {}",
                    if zh { "视频解码器" } else { "Video" },
                    video.codec
                ));
            }
            for audio in &facts.audios {
                out.push(audio_line(audio, zh));
            }
        }
        InfoMode::Off => {}
    }

    if let Some(secs) = facts.duration_s {
        out.push(format!(
            "{}: {}",
            if zh { "时长" } else { "Duration" },
            format_duration(secs)
        ));
    }

    if mode == InfoMode::Full {
        if let Some(rate) = facts.total_bitrate {
            out.push(format!(
                "{}: {}",
                if zh { "总码率" } else { "Bitrate" },
                format_bitrate(rate)
            ));
        }
        for video in &facts.videos {
            if let Some(rate) = video.bitrate {
                out.push(format!(
                    "{}: {}",
                    if zh { "视频码率" } else { "Video bitrate" },
                    format_bitrate(rate)
                ));
            }
        }
        if let Some(pic) = &facts.picture {
            if let Some(pix) = &pic.pix_fmt {
                out.push(format!(
                    "{}: {pix}",
                    if zh { "像素格式" } else { "Pixel format" }
                ));
            }
            if let Some(frames) = &pic.frames {
                out.push(format!(
                    "{}: {frames}",
                    if zh { "总帧数" } else { "Frames" }
                ));
            }
        }
        if let Some(container) = &facts.container {
            out.push(format!(
                "{}: {container}",
                if zh { "容器" } else { "Container" }
            ));
        }
    }

    out
}

fn brief_codec_line(facts: &Facts, zh: bool) -> Option<String> {
    let video = facts.videos.first()?;
    let video_label = if zh { "视频解码器" } else { "Video" };
    let mut line = format!("{video_label}: {}", video.codec);
    if let Some(audio) = facts.audios.first() {
        let audio_label = if zh { "音频解码器" } else { "Audio" };
        line.push_str(&format!(", {audio_label}: {}", audio.codec));
    }
    Some(line)
}

fn audio_line(audio: &Track, zh: bool) -> String {
    let mut parts = vec![audio.codec.clone()];
    if let Some(rate) = audio.bitrate {
        parts.push(format_bitrate(rate));
    }
    if let Some(ch) = audio.channels {
        parts.push(if zh {
            format!("{ch} 声道")
        } else {
            format!("{ch} ch")
        });
    }
    if let Some(rate) = &audio.sample_rate {
        parts.push(format!("{rate} Hz"));
    }
    format!(
        "{}: {}",
        if zh { "音频解码器" } else { "Audio" },
        parts.join(", ")
    )
}

fn format_size(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = 1024.0 * 1024.0;
    const GB: f64 = 1024.0 * 1024.0 * 1024.0;
    let (value, unit) = if bytes as f64 >= GB {
        (bytes as f64 / GB, "GB")
    } else if bytes as f64 >= MB {
        (bytes as f64 / MB, "MB")
    } else {
        (bytes as f64 / KB, "KB")
    };
    format!("{value:.1}{unit}({})", with_commas(bytes))
}

fn with_commas(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    format!("{out} bytes")
}

fn format_duration(secs: f32) -> String {
    let total = secs.floor().max(0.0) as u64;
    let h = total / 3600;
    let m = (total % 3600) / 60;
    let s = total % 60;
    format!("{h:02}:{m:02}:{s:02}")
}

/// 1000-based, matching ffmpeg. Under 1 Mbps the unit is kbps, and a whole
/// number of kbps drops the decimal.
fn format_bitrate(bits: u64) -> String {
    if bits >= 1_000_000 {
        format_tenths(bits, 1_000_000, "Mbps")
    } else {
        let whole = bits / 1_000;
        let frac = bits % 1_000;
        if frac < 50 {
            format!("{whole} kbps")
        } else {
            format_tenths(bits, 1_000, "kbps")
        }
    }
}

/// One decimal place, rounded with integer arithmetic so 156.25 does not
/// become 156.2 through binary floating point.
fn format_tenths(bits: u64, unit: u64, name: &str) -> String {
    let tenths = (bits * 10 + unit / 2) / unit;
    format!("{}.{} {name}", tenths / 10, tenths % 10)
}

const FONT_MISS: &str =
    "警告: 未找到中文字体，参数栏改用英文。可用 --font 或环境变量 VIMG_FONT 指定字体文件。";

fn load_font(explicit: Option<&Path>) -> anyhow::Result<(Vec<u8>, u32, Lang, Option<String>)> {
    if let Some(path) = explicit {
        return load_required(path, "--font");
    }
    if let Some(path) = env_font() {
        return load_required(&path, "VIMG_FONT");
    }
    if let Some(found) = find_system_font() {
        let data = fs::read(&found.path)?;
        let face = preferred_face(&data).unwrap_or(0);
        if FontVec::try_from_vec_and_index(data.clone(), face).is_err() {
            return Ok((CANTARELL.to_vec(), 0, Lang::En, Some(FONT_MISS.into())));
        }
        return Ok((data, face, Lang::Zh, None));
    }
    Ok((CANTARELL.to_vec(), 0, Lang::En, Some(FONT_MISS.into())))
}

fn load_required(
    path: &Path,
    source: &str,
) -> anyhow::Result<(Vec<u8>, u32, Lang, Option<String>)> {
    let data = fs::read(path)
        .with_context(|| format!("{source} 不是可用的字体文件: {}", path.display()))?;
    let face = preferred_face(&data).unwrap_or(0);
    if FontVec::try_from_vec_and_index(data.clone(), face).is_err() {
        bail!("{source} 不是可用的字体文件: {}", path.display());
    }
    Ok((data, face, Lang::Zh, None))
}

fn env_font() -> Option<PathBuf> {
    let raw = std::env::var_os("VIMG_FONT")?;
    let path = PathBuf::from(raw);
    if path.as_os_str().is_empty() {
        None
    } else {
        Some(path)
    }
}

struct FoundFont {
    path: PathBuf,
}

fn find_system_font() -> Option<FoundFont> {
    find_font_in(&font_roots())
}

fn font_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if cfg!(windows) {
        if let Some(windir) = std::env::var_os("WINDIR") {
            roots.push(PathBuf::from(windir).join("Fonts"));
        }
        roots.push(PathBuf::from(r"C:\Windows\Fonts"));
    } else {
        roots.push(PathBuf::from("/usr/share/fonts"));
        if let Some(home) = std::env::var_os("HOME") {
            roots.push(PathBuf::from(home).join(".local/share/fonts"));
        }
    }
    roots
}

fn find_font_in(roots: &[PathBuf]) -> Option<FoundFont> {
    let mut files = Vec::new();
    for root in roots {
        collect_fonts(root, &mut files);
    }
    files.sort_by(|a, b| a.file_name().cmp(&b.file_name()));

    if let Some(path) = files.iter().find(|p| name_has(p, &["misans"])).cloned() {
        return Some(FoundFont { path });
    }

    let needles: &[&[&str]] = if cfg!(windows) {
        &[&["msyh.ttc"], &["simhei.ttf"], &["simsun.ttc"]]
    } else {
        &[
            &["notosanscjk", "noto sans cjk", "notosans-cjk"],
            &["notosanssc", "noto sans sc", "notosans-sc"],
            &["sourcehansans", "source han sans"],
            &["wqy-microhei", "wenquanyi", "文泉驿"],
        ]
    };

    for group in needles {
        if let Some(path) = files.iter().find(|p| name_has(p, group)).cloned() {
            return Some(FoundFont { path });
        }
    }
    None
}

fn name_has(path: &Path, needles: &[&str]) -> bool {
    let Some(name) = path.file_name() else {
        return false;
    };
    let name = name.to_string_lossy().to_lowercase();
    needles.iter().any(|n| name.contains(&n.to_lowercase()))
}

fn collect_fonts(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(ty) = entry.file_type() else { continue };
        if ty.is_dir() {
            collect_fonts(&path, out);
        } else if is_font_file(&path) {
            out.push(path);
        }
    }
}

fn is_font_file(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
            .as_deref(),
        Some("ttf" | "otf" | "ttc" | "otc")
    )
}

/// Prefer a face whose family name says it is Simplified Chinese.
/// Collections that lead with a Japanese face would otherwise draw the wrong glyphs.
fn preferred_face(data: &[u8]) -> Option<u32> {
    let count = ttf_parser::fonts_in_collection(data).unwrap_or(1);
    let mut fallback = None;
    for index in 0..count {
        let Ok(face) = ttf_parser::Face::parse(data, index) else {
            continue;
        };
        if fallback.is_none() {
            fallback = Some(index);
        }
        if face_is_simplified(&face) {
            return Some(index);
        }
    }
    fallback
}

fn face_is_simplified(face: &ttf_parser::Face<'_>) -> bool {
    for name in face.names() {
        let id = name.name_id;
        if id != ttf_parser::name_id::FAMILY && id != ttf_parser::name_id::TYPOGRAPHIC_FAMILY {
            continue;
        }
        let Some(text) = name.to_string() else {
            continue;
        };
        let lower = text.to_lowercase();
        if lower.contains("simplified")
            || text.contains("简体")
            || lower.contains(" sc")
            || lower.ends_with("sc")
            || lower.contains("cjk sc")
        {
            return true;
        }
    }
    false
}

fn draw_band(font: &FontVec, lines: &[String], grid_width: u32) -> anyhow::Result<RgbaImage> {
    let width = grid_width.max(1);
    // 1.5% of a narrow grid is unreadably small for CJK, so the floor is
    // higher than the Latin case would need.
    let font_px = (width as f32 * 0.022).max(22.0);
    let pad = font_px * 0.4;
    let scale = PxScale::from(font_px);
    // CJK faces often report a line gap of zero, so a single wrapped section
    // stacks every row on the same baseline. Each source line is positioned
    // from the face's own height instead.
    let scaled = font.as_scaled(font_px);
    // CJK em boxes are much taller than the drawn strokes. Measure a typical
    // glyph so rows sit on the ink instead of on the empty em box.
    let ink = ['国', 'M']
        .into_iter()
        .find_map(|ch| {
            font.outline_glyph(glyph_brush_layout::ab_glyph::Glyph {
                id: font.glyph_id(ch),
                scale,
                position: glyph_brush_layout::ab_glyph::point(0.0, font_px),
            })
            .map(|g| g.px_bounds().height())
        })
        .unwrap_or(font_px);
    let line_height = (ink * 1.45).max(font_px);
    let layout = glyph_brush_layout::Layout::default_single_line()
        .h_align(HorizontalAlign::Left)
        .v_align(VerticalAlign::Top);
    let text_width = (width as f32 - pad * 2.0).max(1.0);

    let mut glyphs = Vec::new();
    let mut y = pad;
    for line in lines {
        for row_text in wrap_line(font, &scaled, line, text_width) {
            let geometry = SectionGeometry {
                screen_position: (pad, y),
                bounds: (text_width, line_height),
            };
            glyphs.extend(layout.calculate_glyphs(
                &[font],
                &geometry,
                &[SectionText {
                    text: &row_text,
                    scale,
                    ..<_>::default()
                }],
            ));
            y += line_height;
        }
    }
    let height = (y + pad).ceil().max(1.0) as u32;

    let mut img = RgbaImage::from_pixel(width, height, image::Rgba([0, 0, 0, 255]));
    for glyph in glyphs {
        let Some(outlined) = font.outline_glyph(glyph.glyph) else {
            continue;
        };
        let bounds = outlined.px_bounds();
        outlined.draw(|x, y, c| {
            // BMP 不存 alpha。覆盖率若只写进 alpha，存盘时被丢掉，
            // 包围盒里每个像素都变成纯白，字形就成了实心方块。
            let v = (c.clamp(0.0, 1.0) * 255.0) as u8;
            let px = (bounds.min.x + x as f32).round() as i32;
            let py = (bounds.min.y + y as f32).round() as i32;
            if px < 0 || py < 0 || px as u32 >= width || py as u32 >= height {
                return;
            }
            img.put_pixel(px as u32, py as u32, image::Rgba([v, v, v, 255]));
        });
    }
    Ok(img)
}

fn wrap_line(
    font: &FontVec,
    scaled: &PxScaleFont<&FontVec>,
    line: &str,
    width: f32,
) -> Vec<String> {
    let mut rows = Vec::new();
    let mut start = 0;
    let mut used = 0.0;
    for (i, ch) in line.char_indices() {
        let advance = scaled.h_advance(font.glyph_id(ch));
        if start != i && used + advance > width {
            rows.push(line[start..i].to_string());
            start = i;
            used = 0.0;
        }
        used += advance;
    }
    rows.push(line[start..].to_string());
    if rows.is_empty() {
        rows.push(String::new());
    }
    rows
}

/// Stack a header band on top of an already-built grid. Widths must match.
pub fn stack(band: &RgbaImage, grid: &RgbaImage) -> RgbaImage {
    let width = grid.width().max(band.width());
    let mut out = RgbaImage::from_pixel(
        width,
        band.height() + grid.height(),
        image::Rgba([0, 0, 0, 255]),
    );
    image::imageops::replace(&mut out, band, 0, 0);
    image::imageops::replace(&mut out, grid, 0, band.height() as i64);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Facts {
        Facts {
            file_name: "bbb-test-video.mp4".into(),
            size_bytes: Some(276_134_947),
            videos: vec![Track {
                codec: "H.264 / AVC / MPEG-4 AVC / MPEG-4 part 10".into(),
                bitrate: Some(2_998_715),
                channels: None,
                sample_rate: None,
            }],
            audios: vec![
                Track {
                    codec: "MP3 (MPEG audio layer 3)".into(),
                    bitrate: Some(160_000),
                    channels: Some(2),
                    sample_rate: Some("48000".into()),
                },
                Track {
                    codec: "ATSC A/52A (AC-3)".into(),
                    bitrate: Some(320_000),
                    channels: Some(6),
                    sample_rate: Some("48000".into()),
                },
            ],
            picture: Some(Picture {
                width: 1920,
                height: 1080,
                aspect: "16:9".into(),
                fps: Some("30".into()),
                pix_fmt: Some("yuv420p".into()),
                frames: Some("19036".into()),
            }),
            duration_s: Some(634.6),
            total_bitrate: Some(3_481_058),
            container: Some("mp4".into()),
        }
    }

    #[test]
    fn brief_is_five_lines_and_truncates_duration() {
        let lines = lines(&sample(), InfoMode::Brief, Lang::Zh);
        assert_eq!(
            lines,
            vec![
                "文件名: bbb-test-video.mp4",
                "大小: 263.3MB(276,134,947 bytes)",
                "分辨率: 1920×1080(16:9), fps: 30",
                "视频解码器: H.264 / AVC / MPEG-4 AVC / MPEG-4 part 10, 音频解码器: MP3 (MPEG audio layer 3)",
                "时长: 00:10:34",
            ]
        );
    }

    #[test]
    fn full_lists_every_audio_and_the_closed_tail() {
        let lines = lines(&sample(), InfoMode::Full, Lang::Zh);
        assert_eq!(
            lines,
            vec![
                "文件名: bbb-test-video.mp4",
                "大小: 263.3MB(276,134,947 bytes)",
                "分辨率: 1920×1080(16:9), fps: 30",
                "视频解码器: H.264 / AVC / MPEG-4 AVC / MPEG-4 part 10",
                "音频解码器: MP3 (MPEG audio layer 3), 160 kbps, 2 声道, 48000 Hz",
                "音频解码器: ATSC A/52A (AC-3), 320 kbps, 6 声道, 48000 Hz",
                "时长: 00:10:34",
                "总码率: 3.5 Mbps",
                "视频码率: 3.0 Mbps",
                "像素格式: yuv420p",
                "总帧数: 19036",
                "容器: mp4",
            ]
        );
    }

    #[test]
    fn english_fallback_uses_the_agreed_labels() {
        let lines = lines(&sample(), InfoMode::Brief, Lang::En);
        assert_eq!(lines[0], "File: bbb-test-video.mp4");
        assert_eq!(
            lines[3],
            "Video: H.264 / AVC / MPEG-4 AVC / MPEG-4 part 10, Audio: MP3 (MPEG audio layer 3)"
        );
        assert_eq!(lines[4], "Duration: 00:10:34");
    }

    #[test]
    fn missing_fields_are_omitted() {
        let facts = Facts {
            file_name: "clip.mkv".into(),
            size_bytes: None,
            videos: vec![Track {
                codec: "H.264".into(),
                bitrate: None,
                channels: None,
                sample_rate: None,
            }],
            audios: vec![],
            picture: Some(Picture {
                width: 1920,
                height: 800,
                aspect: "12:5".into(),
                fps: Some("23.98".into()),
                pix_fmt: None,
                frames: None,
            }),
            duration_s: None,
            total_bitrate: None,
            container: None,
        };
        let lines = lines(&facts, InfoMode::Full, Lang::Zh);
        assert_eq!(
            lines,
            vec![
                "文件名: clip.mkv",
                "分辨率: 1920×800(12:5), fps: 23.98",
                "视频解码器: H.264",
            ]
        );
    }

    #[test]
    fn size_steps_through_kb_mb_gb() {
        assert_eq!(format_size(512), "0.5KB(512 bytes)");
        assert_eq!(format_size(276_134_947), "263.3MB(276,134,947 bytes)");
        assert_eq!(
            format_size(1024 * 1024 * 1024),
            "1.0GB(1,073,741,824 bytes)"
        );
    }

    #[test]
    fn bitrate_follows_ffmpeg_decimal_units() {
        assert_eq!(format_bitrate(2_998_715), "3.0 Mbps");
        assert_eq!(format_bitrate(3_481_058), "3.5 Mbps");
        assert_eq!(format_bitrate(160_000), "160 kbps");
        assert_eq!(format_bitrate(156_250), "156.3 kbps");
    }

    #[test]
    fn frame_rate_and_container_and_aspect() {
        assert_eq!("30/1".parse::<FrameRate>().unwrap().format(), "30");
        assert_eq!("30000/1001".parse::<FrameRate>().unwrap().format(), "29.97");
        assert_eq!(
            container_name("mov,mp4,m4a,3gp,3g2,mj2", "mp4").as_deref(),
            Some("mp4")
        );
        assert_eq!(
            container_name("matroska,webm", "mkv").as_deref(),
            Some("matroska,webm")
        );
        assert_eq!(
            container_name("mov,mp4,m4a,3gp,3g2,mj2", "bin").as_deref(),
            Some("mov,mp4,m4a,3gp,3g2,mj2")
        );
    }

    #[test]
    fn simplified_face_is_preferred_inside_a_collection() {
        let path = Path::new("/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc");
        if !path.exists() {
            return;
        }
        let data = fs::read(path).unwrap();
        let index = preferred_face(&data).unwrap();
        let face = ttf_parser::Face::parse(&data, index).unwrap();
        assert!(
            face_is_simplified(&face),
            "index {index} was not Simplified Chinese"
        );
    }

    #[test]
    fn empty_font_dir_finds_nothing() {
        let dir = Path::new("/tmp/opencode/vimg-no-fonts");
        let _ = fs::create_dir_all(dir);
        assert!(find_font_in(&[dir.to_path_buf()]).is_none());
    }

    #[test]
    fn misans_is_preferred_over_a_later_system_font() {
        let dir = Path::new("/tmp/opencode/vimg-font-order");
        let _ = fs::remove_dir_all(dir);
        fs::create_dir_all(dir).unwrap();
        fs::write(dir.join("NotoSansCJK-Regular.ttc"), []).unwrap();
        fs::write(dir.join("MiSans-Regular.ttf"), []).unwrap();
        let found = find_font_in(&[dir.to_path_buf()]).unwrap();
        assert!(found.path.ends_with("MiSans-Regular.ttf"));
    }

    #[test]
    fn glyph_counters_stay_open() {
        let font = FontVec::try_from_vec(CANTARELL.to_vec()).unwrap();
        let img = draw_band(&font, &["0".into()], 240).unwrap();
        let (mut min_x, mut min_y) = (img.width(), img.height());
        let (mut max_x, mut max_y) = (0u32, 0u32);
        let mut ink = 0u32;
        for (x, y, px) in img.enumerate_pixels() {
            if px.0[0] > 200 {
                ink += 1;
                min_x = min_x.min(x);
                min_y = min_y.min(y);
                max_x = max_x.max(x);
                max_y = max_y.max(y);
            }
        }
        assert!(ink > 20, "没有画出字形");
        let mut hole = 0u32;
        for y in min_y..=max_y {
            for x in min_x..=max_x {
                if img.get_pixel(x, y).0[0] < 32 {
                    hole += 1;
                }
            }
        }
        assert!(
            hole > 10,
            "字形包围盒被涂成实心，洞里没有背景像素 (ink={ink}, hole={hole})"
        );
    }
}
