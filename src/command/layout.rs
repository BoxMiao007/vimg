use anyhow::{bail, ensure};

/// 版式 1 的一截有这么多格。4 列、5 行单位，两个 2×2 大格各占 4 个单位。
pub const BAND_CELLS: u32 = 14;
/// 截与截之间用来隔开大格的那一行有这么多个小格。
pub const SEPARATOR_CELLS: u32 = 4;
/// 一截的列数，也是小格宽度的倍数。
pub const COLUMNS: u32 = 4;
/// 一截有这么多个小格高度。中间那一行四个都是小格。
pub const ROW_UNITS: u32 = 5;
/// 小格的固定像素高度。大格是它的两倍，再盖住内部那条缝。
pub const SMALL_HEIGHT: u32 = 216;
/// 格与格之间的黑缝。左右外缘留同样宽的一条，上下外缘不留。大格内部不裂开。
pub const GAP: u32 = 8;

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
/// 截与截之间各多一行 4 个小格，所以两截是 32 张，不是 28 张。
pub fn captures_for_bands(bands: u32) -> anyhow::Result<u32> {
    ensure!(bands >= 1, "版式 1 至少需要一截");
    Ok(bands * BAND_CELLS + bands.saturating_sub(1) * SEPARATOR_CELLS)
}

/// `join` 按张数推断截数。0 张和凑不齐截与分隔行的都不出图。
pub fn bands_from_images(count: u32) -> anyhow::Result<u32> {
    ensure!(count >= BAND_CELLS, "版式 1 至少需要一截");
    let extra = count - BAND_CELLS;
    ensure!(
        extra.is_multiple_of(BAND_CELLS + SEPARATOR_CELLS),
        "版式 1 的图片张数必须是一截 {BAND_CELLS} 张，之后每多一截加 {BAND_CELLS} 加 {SEPARATOR_CELLS} 张，实际 {count} 张"
    );
    Ok(1 + extra / (BAND_CELLS + SEPARATOR_CELLS))
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

/// 大格像素。跨两个小格，并把它们之间的那条缝盖进去。
pub fn large_size(small_w: u32) -> (u32, u32) {
    (small_w * 2 + GAP, SMALL_HEIGHT * 2 + GAP)
}

/// 整张格子区的像素，不含参数栏。左右各留一条与格缝同宽的空白，上下外缘不加。
/// 从第二截起，截与截之间多一行 4 个小格，把上下两个大格隔开。
pub fn grid_px(bands: u32, small_w: u32) -> (u32, u32) {
    let width = COLUMNS * small_w + (COLUMNS + 1) * GAP;
    let rows = bands * ROW_UNITS + bands.saturating_sub(1);
    let height = rows * SMALL_HEIGHT + (rows - 1) * GAP;
    (width, height)
}

/// 一截里 14 个格子的位置。从左到右、从上到下，扫到大格左上角就占用它。
pub fn slots() -> [Slot; BAND_CELLS as usize] {
    let large_origins = [(0, 0), (2, 3)];
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
    debug_assert_eq!(n, BAND_CELLS as usize);
    out
}

/// 第 `index` 张（从 0 计）落在哪，跨截也算。
/// 每截 14 张之后、下一截之前，是一行 4 个小格。
pub fn place(index: u32, small_w: u32) -> (u32, u32, u32, u32) {
    let stride = BAND_CELLS + SEPARATOR_CELLS;
    let band = index / stride;
    let within = index % stride;
    let slot = if within < BAND_CELLS {
        slots()[within as usize]
    } else {
        Slot {
            col: within - BAND_CELLS,
            row: ROW_UNITS,
            span: 1,
        }
    };
    let row = band * (ROW_UNITS + 1) + slot.row;
    let x = GAP + slot.col * (small_w + GAP);
    let y = row * (SMALL_HEIGHT + GAP);
    // 大格多出来的 GAP 盖住它内部那条缝，外缘仍与旁边的小格对齐。
    let side_w = slot.span * small_w + (slot.span - 1) * GAP;
    let side_h = slot.span * SMALL_HEIGHT + (slot.span - 1) * GAP;
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
    fn bands_become_fourteen_captures_each() {
        assert_eq!(captures_for_bands(1).unwrap(), 14);
        assert_eq!(captures_for_bands(2).unwrap(), 32);
        assert_eq!(captures_for_bands(3).unwrap(), 50);
        assert!(captures_for_bands(0).is_err());
    }

    #[test]
    fn image_count_must_fill_whole_bands() {
        assert_eq!(bands_from_images(14).unwrap(), 1);
        assert_eq!(bands_from_images(32).unwrap(), 2);
        assert_eq!(bands_from_images(50).unwrap(), 3);
        assert!(bands_from_images(0).is_err());
        assert!(bands_from_images(28).is_err());
    }

    #[test]
    fn sixteen_by_nine_is_384_wide() {
        assert_eq!(small_width(1920, 1080).unwrap(), 384);
        assert_eq!(large_size(384), (776, 440));
        // 左右各留 8 像素，比紧贴外缘的宽度多一条缝。
        assert_eq!(grid_px(1, 384), (1576, 1112));
        // 第二截上面多一行 4 小格，所以比两截紧贴多一个小格高度和一条缝。
        assert_eq!(grid_px(2, 384), (1576, 2456));
        assert_eq!(grid_px(3, 384), (1576, 3800));
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
        assert_eq!(slots.len(), 14);
        assert_eq!(
            slots[0],
            Slot {
                col: 0,
                row: 0,
                span: 2
            }
        );
        // 左上大格右边只有两格，所以中间那一行四个小格从第 6 张开始。
        assert_eq!(
            slots[5],
            Slot {
                col: 0,
                row: 2,
                span: 1
            }
        );
        assert_eq!(
            slots[8],
            Slot {
                col: 3,
                row: 2,
                span: 1
            }
        );
        assert_eq!(
            slots[11],
            Slot {
                col: 2,
                row: 3,
                span: 2
            }
        );
        assert_eq!(
            slots[12],
            Slot {
                col: 0,
                row: 4,
                span: 1
            }
        );
        assert_eq!(
            slots[13],
            Slot {
                col: 1,
                row: 4,
                span: 1
            }
        );

        let mut cover = [[0u8; COLUMNS as usize]; ROW_UNITS as usize];
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
        let step = SMALL_HEIGHT + GAP;
        // 第一截的最后四张是隔开大格的那一行小格。
        let (x, y, w, h) = place(14, 384);
        assert_eq!((x, y, w, h), (GAP, 5 * step, 384, SMALL_HEIGHT));
        assert_eq!(place(17, 384).0, GAP + 3 * (384 + GAP));
        // 第二截左上大格在这一行下面。
        assert_eq!(place(18, 384).1, 6 * step);
        // 第二截的右下大格是这一截的第 12 张，整体下标 29。
        assert_eq!(place(18 + 11, 384).1, 6 * step + 3 * step);
    }
}
