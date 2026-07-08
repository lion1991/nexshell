//! 下划线新样式（虚线/点线/波浪线）几何生成：纯函数，输入一段连续同色同样式
//! 的 cell run，产出渲染层可直接绘制的段/四边形。坐标系与 `DecorationRect`
//! 一致：x 已含列偏移（col * cell_w），y 是 cell 内局部坐标，row 偏移由调用方叠加。

use pathfinder_geometry::vector::Vector2F;

/// 虚线/点线的单段矩形。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DecorSegment {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// 波浪线每 cell 宽（=一个周期）拆的折线段数，越大越平滑。
const CURL_SEGMENTS_PER_CELL: usize = 12;

/// 虚线：每 cell 两段实心矩形（段长 cell_w*0.35），间隙均分贴底，天然按列对齐。
pub fn dashed_rects(
    start_col: usize,
    cols: usize,
    cell_w: f32,
    cell_h: f32,
    thickness: f32,
) -> Vec<DecorSegment> {
    if cols == 0 {
        return Vec::new();
    }
    let dash_len = cell_w * 0.35;
    let gap = ((cell_w - dash_len * 2.0) / 2.0).max(0.0);
    let y = cell_h - thickness;
    let mut segs = Vec::with_capacity(cols * 2);
    for col in start_col..start_col + cols {
        let base = col as f32 * cell_w;
        segs.push(DecorSegment {
            x: base,
            y,
            width: dash_len,
            height: thickness,
        });
        segs.push(DecorSegment {
            x: base + dash_len + gap,
            y,
            width: dash_len,
            height: thickness,
        });
    }
    segs
}

/// 点线：方点边长=thickness，点距=thickness（周期 2*thickness），相位以绝对 x
/// （非 run 局部）为基准，保证跨 run/跨 cell 拼接不断点。
pub fn dotted_rects(
    start_col: usize,
    cols: usize,
    cell_w: f32,
    cell_h: f32,
    thickness: f32,
) -> Vec<DecorSegment> {
    if cols == 0 || thickness <= 0.0 {
        return Vec::new();
    }
    let period = thickness * 2.0;
    let y = cell_h - thickness;
    let run_left = start_col as f32 * cell_w;
    let run_right = (start_col + cols) as f32 * cell_w;

    let mut n = (run_left / period).floor() as i64;
    let mut segs = Vec::new();
    loop {
        let x = n as f32 * period;
        if x >= run_right {
            break;
        }
        if x >= run_left && x + thickness <= run_right {
            segs.push(DecorSegment {
                x,
                y,
                width: thickness,
                height: thickness,
            });
        }
        n += 1;
    }
    segs
}

/// 波浪线：正弦波，周期=cell_w，相位以绝对列号（col * cell_w）为基准，
/// 波带贴 cell 底边不越界。返回每段折线的四角点（局部坐标，row 偏移由调用方
/// 叠加），供 `scene.draw_quad` 绘制。
pub fn curl_quads(
    start_col: usize,
    cols: usize,
    cell_w: f32,
    cell_h: f32,
    thickness: f32,
) -> Vec<[Vector2F; 4]> {
    if cols == 0 || cell_w <= 0.0 {
        return Vec::new();
    }
    let amplitude = thickness * 0.9;
    let half_thickness = thickness / 2.0;
    // centerline 上摆振幅 + 半线宽后仍不超过 cell_h，贴底。
    let y0 = cell_h - half_thickness - amplitude;
    let segment_width = cell_w / CURL_SEGMENTS_PER_CELL as f32;
    let total_segments = cols * CURL_SEGMENTS_PER_CELL;
    let run_left = start_col as f32 * cell_w;

    let wave_y = |x: f32| y0 + amplitude * (2.0 * std::f32::consts::PI * x / cell_w).sin();

    let mut quads = Vec::with_capacity(total_segments);
    for i in 0..total_segments {
        let x0 = run_left + i as f32 * segment_width;
        let x1 = x0 + segment_width;
        let y_left = wave_y(x0);
        let y_right = wave_y(x1);

        let dx = x1 - x0;
        let dy = y_right - y_left;
        let len = (dx * dx + dy * dy).sqrt().max(f32::EPSILON);
        let nx = -dy / len * half_thickness;
        let ny = dx / len * half_thickness;

        quads.push([
            Vector2F::new(x0 + nx, y_left + ny),
            Vector2F::new(x1 + nx, y_right + ny),
            Vector2F::new(x1 - nx, y_right - ny),
            Vector2F::new(x0 - nx, y_left - ny),
        ]);
    }
    quads
}

#[cfg(test)]
mod tests {
    use super::*;

    const CELL_W: f32 = 10.0;
    const CELL_H: f32 = 20.0;
    const THICKNESS: f32 = 1.5;

    #[test]
    fn dashed_two_segments_per_cell_within_bounds() {
        let segs = dashed_rects(2, 3, CELL_W, CELL_H, THICKNESS);
        assert_eq!(segs.len(), 6);
        let run_right = 5.0 * CELL_W;
        for seg in &segs {
            assert!(seg.x >= 2.0 * CELL_W - 1e-4);
            assert!(seg.x + seg.width <= run_right + 1e-4);
        }
    }

    #[test]
    fn dotted_dots_do_not_cross_run_right_edge() {
        let segs = dotted_rects(0, 4, CELL_W, CELL_H, THICKNESS);
        assert!(!segs.is_empty());
        let run_right = 4.0 * CELL_W;
        for seg in &segs {
            assert!(seg.x >= -1e-4);
            assert!(seg.x + seg.width <= run_right + 1e-4);
        }
    }

    #[test]
    fn dotted_phase_is_continuous_across_split_runs() {
        let whole = dotted_rects(0, 4, CELL_W, CELL_H, THICKNESS);
        let mut split = dotted_rects(0, 2, CELL_W, CELL_H, THICKNESS);
        split.extend(dotted_rects(2, 2, CELL_W, CELL_H, THICKNESS));
        // 分段调用可能在边界丢一颗跨界点，但不应产生位置不一致的点。
        for seg in &split {
            assert!(whole
                .iter()
                .any(|w| (w.x - seg.x).abs() < 1e-4 && w.y == seg.y));
        }
    }

    #[test]
    fn curl_quads_count_matches_segments_per_cell() {
        let quads = curl_quads(0, 2, CELL_W, CELL_H, THICKNESS);
        assert_eq!(quads.len(), 2 * CURL_SEGMENTS_PER_CELL);
    }

    #[test]
    fn curl_amplitude_never_exceeds_cell_bottom() {
        let quads = curl_quads(0, 4, CELL_W, CELL_H, THICKNESS);
        for quad in &quads {
            for corner in quad {
                assert!(
                    corner.y() <= CELL_H + 1e-3,
                    "corner {:?} exceeds cell bottom",
                    corner
                );
            }
        }
    }

    #[test]
    fn curl_phase_continuous_when_run_is_split() {
        let whole = curl_quads(0, 4, CELL_W, CELL_H, THICKNESS);
        let mut split = curl_quads(0, 2, CELL_W, CELL_H, THICKNESS);
        split.extend(curl_quads(2, 2, CELL_W, CELL_H, THICKNESS));

        assert_eq!(whole.len(), split.len());
        for (a, b) in whole.iter().zip(split.iter()) {
            for (ca, cb) in a.iter().zip(b.iter()) {
                assert!((*ca - *cb).length() < 1e-3);
            }
        }
    }

    #[test]
    fn curl_phase_keyed_by_absolute_column_not_run_start() {
        // 同一绝对列区间，起点不同但覆盖同一段落时波形应完全重合。
        let a = curl_quads(1, 2, CELL_W, CELL_H, THICKNESS);
        let mut b = curl_quads(1, 1, CELL_W, CELL_H, THICKNESS);
        b.extend(curl_quads(2, 1, CELL_W, CELL_H, THICKNESS));
        assert_eq!(a.len(), b.len());
        for (ca, cb) in a.iter().zip(b.iter()) {
            for (p, q) in ca.iter().zip(cb.iter()) {
                assert!((*p - *q).length() < 1e-3);
            }
        }
    }
}
