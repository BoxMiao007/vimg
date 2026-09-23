use anyhow::{bail, ensure};

/// 版式 1 的一截有这么多格。
pub const BAND_CELLS: u32 = 19;
/// 一截的列数，也是小格宽度的倍数。
pub const COLUMNS: u32 = 5;
/// 一截有这么多个小格高度。
pub const ROW_UNITS: u32 = 5;
/// 小格的固定像素高度。大格是它的两倍。
pub const SMALL_HEIGHT: u32 = 216;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Slot {
    /// 以小格为单位，从左往右数。
    pub col: u32,
    /// 以小格为单位，在这一截里从上往下数。
    pub row: u32,
    /// 1 是小格，2 是大格。
    pub span: u32,
}

/// 不写 `--layout` 是等大网格，只认识 1。
pub fn known(layout: Option<u32>) -> anyhow::Result<Option<u32>> {
    match layout {
        None => Ok(None),
        Some(1) => Ok(Some(1)),
        Some(n) => bail!("不认识的版式: {n}"),
    }
}

/// 版式 1 下 `-n` 是截数。至少一截。
pub fn captures_for_bands(bands: u32) -> anyhow::Result<u32> {
    ensure!(bands >= 1, "版式 1 至少需要一截");
    Ok(bands * BAND_CELLS)
}

/// `join` 按张数推断截数。0 张和凑不满一截的都不出图。
pub fn bands_from_images(count: u32) -> anyhow::Result<u32> {
    ensure!(
        count >= BAND_CELLS && count.is_multiple_of(BAND_CELLS),
        "版式 1 的图片张数必须是 {BAND_CELLS} 的正整数倍，实际 {count} 张"
    );
    Ok(count / BAND_CELLS)
}

/// 小格宽。高度固定，宽按画面比例四舍五入；奇数少 1 像素。
pub fn small_width(src_w: u32, src_h: u32) -> anyhow::Result<u32> {
    ensure!(src_w > 0 && src_h > 0, "版式 1 算不出格子宽度");
    let exact = SMALL_HEIGHT as f32 * src_w as f32 / src_h as f32;
    let mut width = exact.round() as i32;
    if width % 2 == 1 {
        width -= 1;
    }
    ensure!(width > 0, "版式 1 算不出格子宽度");
    Ok(width as u32)
}

/// 大格像素。宽高都是小格的两倍。
pub fn large_size(small_w: u32) -> (u32, u32) {
    (small_w * 2, SMALL_HEIGHT * 2)
}

/// 整张格子区的像素，不含参数栏。多截只往下加高。
pub fn grid_px(bands: u32, small_w: u32) -> (u32, u32) {
    (COLUMNS * small_w, bands * ROW_UNITS * SMALL_HEIGHT)
}

/// 一截里 19 个格子的位置。从左到右、从上到下，扫到大格左上角就占用它。
pub fn slots() -> [Slot; BAND_CELLS as usize] {
    let large_origins = [(0, 0), (3, 3)];
    let mut occupied = [[false; COLUMNS as usize]; ROW_UNITS as usize];
    let mut out = [Slot {
        col: 0,
        row: 0,
        span: 1,
    }; BAND_CELLS as usize];
    let mut n = 0;
    for row in 0..ROW_UNITS {
        for col in 0..COLUMNS {
            if occupied[row as usize][col as usize] {
                continue;
            }
            let span = if large_origins.contains(&(col, row)) {
                2
            } else {
                1
            };
            for dy in 0..span {
                for dx in 0..span {
                    occupied[(row + dy) as usize][(col + dx) as usize] = true;
                }
            }
            out[n] = Slot { col, row, span };
            n += 1;
        }
    }
    out
}

/// 第 `index` 张（从 0 计）落在哪，跨截也算。
pub fn place(index: u32, small_w: u32) -> (u32, u32, u32, u32) {
    let slot = slots()[(index % BAND_CELLS) as usize];
    let band = index / BAND_CELLS;
    let x = slot.col * small_w;
    let y = (band * ROW_UNITS + slot.row) * SMALL_HEIGHT;
    let side_w = slot.span * small_w;
    let side_h = slot.span * SMALL_HEIGHT;
    (x, y, side_w, side_h)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_layout_one_is_known() {
        assert_eq!(known(None).unwrap(), None);
        assert_eq!(known(Some(1)).unwrap(), Some(1));
        assert!(known(Some(0)).is_err());
        assert!(known(Some(2)).is_err());
    }

    #[test]
    fn bands_become_nineteen_captures_each() {
        assert_eq!(captures_for_bands(1).unwrap(), 19);
        assert_eq!(captures_for_bands(3).unwrap(), 57);
        assert!(captures_for_bands(0).is_err());
    }

    #[test]
    fn image_count_must_fill_whole_bands() {
        assert_eq!(bands_from_images(19).unwrap(), 1);
        assert_eq!(bands_from_images(38).unwrap(), 2);
        assert!(bands_from_images(0).is_err());
        assert!(bands_from_images(10).is_err());
    }

    #[test]
    fn sixteen_by_nine_is_384_wide() {
        assert_eq!(small_width(1920, 1080).unwrap(), 384);
        assert_eq!(large_size(384), (768, 432));
        assert_eq!(grid_px(1, 384), (1920, 1080));
        assert_eq!(grid_px(2, 384), (1920, 2160));
    }

    #[test]
    fn width_rounds_then_drops_one_if_odd() {
        // 216 * 1920 / 800 = 518.4，四舍五入后已是偶数。
        assert_eq!(small_width(1920, 800).unwrap(), 518);
        // 216 * 385 / 216 = 385，奇数再少 1。
        assert_eq!(small_width(385, 216).unwrap(), 384);
        assert!(small_width(1, 100_000).is_err());
        assert!(small_width(0, 1080).is_err());
    }

    #[test]
    fn reading_order_occupies_the_large_cells_when_reached() {
        let slots = slots();
        assert_eq!(slots.len(), 19);
        assert_eq!(
            slots[0],
            Slot {
                col: 0,
                row: 0,
                span: 2
            }
        );
        assert_eq!(
            slots[15],
            Slot {
                col: 3,
                row: 3,
                span: 2
            }
        );
        assert_eq!(
            slots[16],
            Slot {
                col: 0,
                row: 4,
                span: 1
            }
        );
        assert_eq!(
            slots[17],
            Slot {
                col: 1,
                row: 4,
                span: 1
            }
        );
        assert_eq!(
            slots[18],
            Slot {
                col: 2,
                row: 4,
                span: 1
            }
        );

        let mut cover = [[0u8; 5]; 5];
        for slot in slots {
            for dy in 0..slot.span {
                for dx in 0..slot.span {
                    cover[(slot.row + dy) as usize][(slot.col + dx) as usize] += 1;
                }
            }
        }
        assert!(cover.iter().all(|row| row.iter().all(|n| *n == 1)));
    }

    #[test]
    fn the_second_band_starts_below_the_first() {
        let (x, y, w, h) = place(19, 384);
        assert_eq!((x, y, w, h), (0, 1080, 768, 432));
        // 第二截的右下大格是这一截的第 16 张，整体下标 34。
        let (_, bottom_y, _, _) = place(19 + 15, 384);
        assert_eq!(bottom_y, 1080 + 3 * SMALL_HEIGHT);
    }
}
