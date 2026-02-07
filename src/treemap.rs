/// A positioned rectangle in the treemap.
#[derive(Clone, Debug)]
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

    // Greedily add items to current row while aspect ratio improves
    let mut row: Vec<usize> = Vec::new();
    let mut row_sum = 0.0;
    let mut best_ratio = f64::MAX;

    for (i, &size) in sizes.iter().enumerate() {
        let new_sum = row_sum + size;
        let new_ratio = worst_ratio(&sizes[..i + 1], new_sum, short_side);

        if !row.is_empty() && new_ratio > best_ratio {
            // Adding this item made it worse; lay out current row
            break;
        }

        row.push(i);
        row_sum = new_sum;
        best_ratio = new_ratio;
    }

    // Lay out the row
    let row_fraction = row_sum / total;
    let row_len = row.len();

    if is_wide {
        let row_w = w * row_fraction;
        let mut cy = y;
        for &idx in &row {
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
        for &idx in &row {
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

fn worst_ratio(sizes: &[f64], row_sum: f64, short_side: f64) -> f64 {
    if row_sum <= 0.0 || short_side <= 0.0 {
        return f64::MAX;
    }
    let row_area = row_sum;
    let row_len = row_area / short_side;

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
