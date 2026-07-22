/// A positioned rectangle in the treemap.
#[derive(Clone, Debug, PartialEq)]
pub struct TreemapRect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub index: usize,
}

/// Squarified treemap layout (Bruls, Huizing, van Wijk).
/// Takes a bounding rectangle and a slice of sizes (must be sorted descending),
/// returns positioned rectangles for each item.
pub fn layout(x: f32, y: f32, w: f32, h: f32, sizes: &[f64]) -> Vec<TreemapRect> {
    if sizes.is_empty() || w <= 0.0 || h <= 0.0 {
        return Vec::new();
    }

    let total: f64 = sizes.iter().sum();
    if total <= 0.0 {
        return Vec::new();
    }

    // Normalize sizes to fill the area
    let area = (w as f64) * (h as f64);
    let normalized: Vec<f64> = sizes.iter().map(|s| s / total * area).collect();

    let mut result = Vec::with_capacity(sizes.len());
    squarify(
        &normalized,
        0,
        x as f64,
        y as f64,
        w as f64,
        h as f64,
        &mut result,
    );
    result
}

/// Layout into a normalized box of width 1.0 and height `aspect`.
/// Callers scale the resulting rects into any target rect with the same
/// aspect ratio; the subdivision only depends on relative sizes + aspect,
/// so this is computed once per node and cached.
pub fn layout_norm(aspect: f32, sizes: &[f64]) -> Vec<TreemapRect> {
    layout(0.0, 0.0, 1.0, aspect.max(1e-6), sizes)
}

/// Worst aspect ratio of a row with sum `s`, min element `mn`, max element
/// `mx`, laid against short side `w`. O(1): the worst ratio in a row is
/// always attained at its min or max element.
fn worst_for(s: f64, mn: f64, mx: f64, short_side: f64) -> f64 {
    if s <= 0.0 || short_side <= 0.0 || mn <= 0.0 {
        return f64::MAX;
    }
    let w2 = short_side * short_side;
    (w2 * mx / (s * s)).max((s * s) / (w2 * mn))
}

fn squarify(
    sizes: &[f64],
    start_index: usize,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    result: &mut Vec<TreemapRect>,
) {
    if sizes.is_empty() {
        return;
    }
    if sizes.len() == 1 {
        result.push(TreemapRect {
            x: x as f32,
            y: y as f32,
            w: w as f32,
            h: h as f32,
            index: start_index,
        });
        return;
    }

    let total: f64 = sizes.iter().sum();
    if total <= 0.0 {
        return;
    }

    // Determine the shorter side
    let short_side = w.min(h);
    let is_wide = w >= h;

    // Greedily add items to the current row while the worst aspect ratio
    // improves. Running sum/min/max makes each candidate O(1) (the old
    // implementation rescanned the whole row prefix per candidate).
    let mut row_len = 0usize;
    let mut row_sum = 0.0f64;
    let mut row_min = f64::MAX;
    let mut row_max = 0.0f64;
    let mut best_ratio = f64::MAX;
    let mut row_has_positive = false;

    for (i, &size) in sizes.iter().enumerate() {
        if size <= 0.0 {
            // Zero-area item: absorb into the current row (input is sorted
            // descending, so these only appear in the tail). Contributes
            // nothing to the ratio; gets a zero-area rect below.
            row_len = i + 1;
            continue;
        }
        let new_sum = row_sum + size;
        let new_min = row_min.min(size);
        let new_max = row_max.max(size);
        let new_ratio = worst_for(new_sum, new_min, new_max, short_side);

        if row_has_positive && new_ratio > best_ratio {
            // Adding this item made it worse; lay out current row
            break;
        }

        row_len = i + 1;
        row_sum = new_sum;
        row_min = new_min;
        row_max = new_max;
        best_ratio = new_ratio;
        row_has_positive = true;
    }

    // Lay out the row
    let row_fraction = row_sum / total;

    if is_wide {
        let row_w = w * row_fraction;
        let mut cy = y;
        for idx in 0..row_len {
            let item_h = if row_sum > 0.0 {
                h * (sizes[idx] / row_sum)
            } else {
                0.0
            };
            result.push(TreemapRect {
                x: x as f32,
                y: cy as f32,
                w: row_w as f32,
                h: item_h as f32,
                index: start_index + idx,
            });
            cy += item_h;
        }
        // Recurse on remaining
        squarify(
            &sizes[row_len..],
            start_index + row_len,
            x + row_w,
            y,
            w - row_w,
            h,
            result,
        );
    } else {
        let row_h = h * row_fraction;
        let mut cx = x;
        for idx in 0..row_len {
            let item_w = if row_sum > 0.0 {
                w * (sizes[idx] / row_sum)
            } else {
                0.0
            };
            result.push(TreemapRect {
                x: cx as f32,
                y: y as f32,
                w: item_w as f32,
                h: row_h as f32,
                index: start_index + idx,
            });
            cx += item_w;
        }
        squarify(
            &sizes[row_len..],
            start_index + row_len,
            x,
            y + row_h,
            w,
            h - row_h,
            result,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- Reference implementation: the original O(row^2) greedy scan, ----
    // ---- kept verbatim to pin the new code to identical behavior.     ----

    fn worst_ratio_ref(sizes: &[f64], row_sum: f64, short_side: f64) -> f64 {
        if row_sum <= 0.0 || short_side <= 0.0 {
            return f64::MAX;
        }
        let row_len = row_sum / short_side;
        let mut worst = 0.0f64;
        for &s in sizes {
            if s <= 0.0 {
                continue;
            }
            let item_short = s / row_len;
            let ratio = if row_len > item_short {
                row_len / item_short
            } else {
                item_short / row_len
            };
            worst = worst.max(ratio);
        }
        worst
    }

    fn squarify_ref(
        sizes: &[f64],
        start_index: usize,
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        result: &mut Vec<TreemapRect>,
    ) {
        if sizes.is_empty() {
            return;
        }
        if sizes.len() == 1 {
            result.push(TreemapRect {
                x: x as f32,
                y: y as f32,
                w: w as f32,
                h: h as f32,
                index: start_index,
            });
            return;
        }
        let total: f64 = sizes.iter().sum();
        if total <= 0.0 {
            return;
        }
        let short_side = w.min(h);
        let is_wide = w >= h;

        let mut row: Vec<usize> = Vec::new();
        let mut row_sum = 0.0;
        let mut best_ratio = f64::MAX;
        for (i, &size) in sizes.iter().enumerate() {
            let new_sum = row_sum + size;
            let new_ratio = worst_ratio_ref(&sizes[..i + 1], new_sum, short_side);
            if !row.is_empty() && new_ratio > best_ratio {
                break;
            }
            row.push(i);
            row_sum = new_sum;
            best_ratio = new_ratio;
        }

        let row_fraction = row_sum / total;
        let row_len = row.len();
        if is_wide {
            let row_w = w * row_fraction;
            let mut cy = y;
            for &idx in &row {
                let item_h = if row_sum > 0.0 { h * (sizes[idx] / row_sum) } else { 0.0 };
                result.push(TreemapRect {
                    x: x as f32,
                    y: cy as f32,
                    w: row_w as f32,
                    h: item_h as f32,
                    index: start_index + idx,
                });
                cy += item_h;
            }
            squarify_ref(&sizes[row_len..], start_index + row_len, x + row_w, y, w - row_w, h, result);
        } else {
            let row_h = h * row_fraction;
            let mut cx = x;
            for &idx in &row {
                let item_w = if row_sum > 0.0 { w * (sizes[idx] / row_sum) } else { 0.0 };
                result.push(TreemapRect {
                    x: cx as f32,
                    y: y as f32,
                    w: item_w as f32,
                    h: row_h as f32,
                    index: start_index + idx,
                });
                cx += item_w;
            }
            squarify_ref(&sizes[row_len..], start_index + row_len, x, y + row_h, w, h - row_h, result);
        }
    }

    fn layout_ref(x: f32, y: f32, w: f32, h: f32, sizes: &[f64]) -> Vec<TreemapRect> {
        if sizes.is_empty() || w <= 0.0 || h <= 0.0 {
            return Vec::new();
        }
        let total: f64 = sizes.iter().sum();
        if total <= 0.0 {
            return Vec::new();
        }
        let area = (w as f64) * (h as f64);
        let normalized: Vec<f64> = sizes.iter().map(|s| s / total * area).collect();
        let mut result = Vec::with_capacity(sizes.len());
        squarify_ref(&normalized, 0, x as f64, y as f64, w as f64, h as f64, &mut result);
        result
    }

    // ---- Tests ----

    #[test]
    fn areas_sum_to_container() {
        let sizes = vec![50.0, 30.0, 15.0, 5.0];
        let rects = layout(0.0, 0.0, 100.0, 100.0, &sizes);
        let sum: f32 = rects.iter().map(|r| r.w * r.h).sum();
        assert!((sum - 10_000.0).abs() < 1.0, "sum={sum}");
        assert_eq!(rects.len(), 4);
    }

    #[test]
    fn handles_zero_and_many() {
        // 10k sorted-descending sizes incl. zero tail must not hang, NaN,
        // or drop items.
        let mut sizes: Vec<f64> = (0..10_000).map(|i| (10_000 - i) as f64).collect();
        sizes.extend([0.0, 0.0]);
        let rects = layout(0.0, 0.0, 1000.0, 600.0, &sizes);
        assert_eq!(rects.len(), sizes.len());
        for r in &rects {
            assert!(r.w.is_finite() && r.h.is_finite() && r.w >= 0.0 && r.h >= 0.0);
        }
    }

    #[test]
    fn norm_layout_scales() {
        let sizes = vec![60.0, 40.0];
        let rects = layout_norm(0.5, &sizes);
        for r in &rects {
            assert!(r.x >= 0.0 && r.x + r.w <= 1.0 + 1e-4);
            assert!(r.y >= 0.0 && r.y + r.h <= 0.5 + 1e-4);
        }
    }

    #[test]
    fn matches_reference_implementation() {
        let cases: Vec<Vec<f64>> = vec![
            vec![6.0, 6.0, 4.0, 3.0, 2.0, 2.0, 1.0],
            vec![100.0],
            vec![5.0, 5.0],
            (0..500).map(|i| ((500 - i) as f64).powi(2)).collect(),
            {
                let mut v: Vec<f64> = (0..200).map(|i| (200 - i) as f64 * 7.3).collect();
                v.extend([0.0, 0.0, 0.0]);
                v
            },
        ];
        for sizes in &cases {
            for (w, h) in [(100.0, 100.0), (640.0, 480.0), (30.0, 900.0)] {
                let new = layout(0.0, 0.0, w, h, sizes);
                let old = layout_ref(0.0, 0.0, w, h, sizes);
                assert_eq!(new, old, "diverged for {} sizes in {w}x{h}", sizes.len());
            }
        }
    }
}
