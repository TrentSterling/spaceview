use crate::scanner::FileNode;
use crate::treemap::{self, TreemapRect};
use eframe::egui;

/// Hard ceiling on children materialized per directory expansion; the
/// remainder collapses into a single aggregate "+N more" tail node. The
/// scanner sorts children largest-first, so the tail is the smallest items.
pub const EXPAND_CHILD_CAP: usize = 2048;

/// Global budget for simultaneously materialized LayoutNodes. Expansion is
/// refused while over budget; prune runs every frame until back under it.
pub const NODE_BUDGET: usize = 250_000;

/// Sentinel child_index for aggregate tail nodes (no backing FileNode).
pub const AGGREGATE_INDEX: usize = usize::MAX;

/// A node in the world-space layout tree.
/// Each node corresponds to a FileNode and has a fixed world-space rect.
pub struct LayoutNode {
    pub world_rect: egui::Rect,
    pub depth: usize,
    pub name: String,
    pub size: u64,
    pub file_count: u64,
    pub is_dir: bool,
    pub has_children: bool,
    pub is_aggregate: bool,
    pub color_index: usize,
    pub child_index: usize,
    pub children_expanded: bool,
    pub modified: u64, // seconds since epoch (0 = unknown)
    pub children: Vec<LayoutNode>,
    /// Squarified rects for `children`, normalized to a 1.0 x child_norm_aspect
    /// box, parallel to `children`. Computed ONCE at expansion and reused by
    /// render, hit test, and minimap every frame (the subdivision depends only
    /// on relative child sizes + aspect, never on the camera).
    pub child_norm: Vec<TreemapRect>,
    pub child_norm_aspect: f32,
}

/// The top-level world-space layout.
pub struct WorldLayout {
    pub root_nodes: Vec<LayoutNode>,
    pub world_rect: egui::Rect,
    /// Total materialized LayoutNodes (root + all expanded children).
    pub live_nodes: usize,
    frame_counter: u64,
}

/// Fraction of parent rect height used for directory headers at a given depth.
/// Approximate. World_rects are only used for camera/expand/prune decisions, not rendering.
fn header_fraction(_depth: usize) -> f32 {
    0.01
}

/// Compute the content rect inside a directory rect (below the header).
/// Approximate. World_rects are only used for camera/expand/prune decisions, not rendering.
pub fn content_rect(dir_rect: egui::Rect, depth: usize) -> egui::Rect {
    let hh = dir_rect.height() * header_fraction(depth);
    let pad = 0.002 * dir_rect.width().min(dir_rect.height());
    egui::Rect::from_min_max(
        egui::pos2(dir_rect.min.x + pad, dir_rect.min.y + hh),
        egui::pos2(dir_rect.max.x - pad, dir_rect.max.y - pad),
    )
}

impl WorldLayout {
    /// Create a new world layout from a scanned file tree.
    /// The root fills (0,0) to (1.0, aspect_ratio).
    pub fn new(file_root: &FileNode, aspect_ratio: f32) -> Self {
        let world_rect = egui::Rect::from_min_max(
            egui::pos2(0.0, 0.0),
            egui::pos2(1.0, aspect_ratio),
        );

        let (root_nodes, _norm, _aspect) = layout_children(file_root, world_rect, 0);
        let live_nodes = root_nodes.len();

        WorldLayout {
            root_nodes,
            world_rect,
            live_nodes,
            frame_counter: 0,
        }
    }

    /// Expand directories that are large enough on screen but not yet expanded.
    /// Caps expansions per call to prevent hitches; refuses expansion while the
    /// global node budget is exceeded.
    pub fn expand_visible(&mut self, file_root: &FileNode, camera: &crate::camera::Camera, viewport: egui::Rect, max_expansions: usize) {
        let mut expansions = 0;

        expand_recursive(
            &mut self.root_nodes,
            file_root,
            camera,
            viewport,
            &mut expansions,
            max_expansions,
            &mut self.live_nodes,
        );
    }

    /// Prune children of off-screen or tiny nodes to free memory.
    /// Runs every 15 frames, or every frame while over the node budget.
    pub fn maybe_prune(&mut self, camera: &crate::camera::Camera, viewport: egui::Rect) {
        self.frame_counter += 1;
        if self.live_nodes <= NODE_BUDGET && self.frame_counter % 15 != 0 {
            return;
        }
        let freed = prune_recursive(&mut self.root_nodes, camera, viewport);
        self.live_nodes = self.live_nodes.saturating_sub(freed);
    }

    /// Build an ancestor chain from the root to the deepest node containing world_pos.
    /// Returns Vec of (name, color_index, world_rect).
    pub fn ancestor_chain(&self, world_pos: egui::Pos2) -> Vec<(&str, usize, egui::Rect)> {
        let mut chain = Vec::new();
        ancestor_chain_recursive(&self.root_nodes, world_pos, &mut chain);
        chain
    }

    /// Walk-count all materialized LayoutNodes (diagnostic; the perf harness
    /// cross-checks this against the incremental `live_nodes` accounting).
    pub fn count_nodes(&self) -> usize {
        count_nodes(&self.root_nodes)
    }

}

/// Lay out the children of `file_node` into `parent_rect` using squarified
/// treemap. Returns the nodes, their normalized rects (1.0 x aspect box,
/// parallel to the nodes), and the aspect used.
fn layout_children(
    file_node: &FileNode,
    parent_rect: egui::Rect,
    depth: usize,
) -> (Vec<LayoutNode>, Vec<TreemapRect>, f32) {
    if file_node.children.is_empty() || parent_rect.width() <= 0.0 || parent_rect.height() <= 0.0 {
        return (Vec::new(), Vec::new(), 1.0);
    }

    // Cap materialized children; aggregate the tail. Children are sorted
    // largest-first by the scanner, except "<Free Space>" which build_layout
    // forces to the end at the root; always keep it.
    let n = file_node.children.len();
    let mut kept: Vec<usize>;
    let mut agg_size = 0u64;
    let mut agg_items = 0u64;
    let mut agg_files = 0u64;
    if n > EXPAND_CHILD_CAP {
        kept = (0..EXPAND_CHILD_CAP - 1).collect();
        for i in (EXPAND_CHILD_CAP - 1)..n {
            let c = &file_node.children[i];
            if c.name == "<Free Space>" {
                kept.push(i);
            } else {
                agg_size += c.size;
                agg_items += 1;
                agg_files += if c.is_dir { c.file_count } else { 1 };
            }
        }
    } else {
        kept = (0..n).collect();
    }

    let mut sizes: Vec<f64> = kept
        .iter()
        .map(|&i| file_node.children[i].size as f64)
        .collect();
    // Keep the aggregate even at size 0 (all-zero-byte tail): it gets a
    // zero-area rect like any zero-size child, so the capped items are never
    // silently unrepresented.
    let has_agg = agg_items > 0;
    if has_agg {
        sizes.push(agg_size as f64);
    }

    let aspect = parent_rect.height() / parent_rect.width();
    let norm = treemap::layout_norm(aspect, &sizes);

    // World rects derive from the SAME normalized layout (uniform scale by
    // parent width), so world-space decisions and screen-space rendering
    // always agree.
    let scale = parent_rect.width();
    let mut nodes = Vec::with_capacity(norm.len());
    for tr in &norm {
        let world_rect = egui::Rect::from_min_size(
            egui::pos2(
                parent_rect.min.x + tr.x * scale,
                parent_rect.min.y + tr.y * scale,
            ),
            egui::vec2(tr.w * scale, tr.h * scale),
        );

        if tr.index < kept.len() {
            let orig = kept[tr.index];
            let child = &file_node.children[orig];
            let has_children = child.is_dir && !child.children.is_empty();
            nodes.push(LayoutNode {
                world_rect,
                depth,
                name: child.name.clone(),
                size: child.size,
                file_count: child.file_count,
                is_dir: child.is_dir,
                has_children,
                is_aggregate: false,
                color_index: depth,
                child_index: orig,
                children_expanded: false,
                modified: child.modified,
                children: Vec::new(),
                child_norm: Vec::new(),
                child_norm_aspect: 1.0,
            });
        } else {
            nodes.push(LayoutNode {
                world_rect,
                depth,
                name: format!("+{} more", agg_items),
                size: agg_size,
                file_count: agg_files,
                is_dir: false,
                has_children: false,
                is_aggregate: true,
                color_index: depth,
                child_index: AGGREGATE_INDEX,
                children_expanded: false,
                modified: 0,
                children: Vec::new(),
                child_norm: Vec::new(),
                child_norm_aspect: 1.0,
            });
        }
    }

    (nodes, norm, aspect)
}

/// Recursively expand nodes that are visible and large enough on screen.
fn expand_recursive(
    nodes: &mut [LayoutNode],
    file_node: &FileNode,
    camera: &crate::camera::Camera,
    viewport: egui::Rect,
    expansions: &mut usize,
    max_expansions: usize,
    live_nodes: &mut usize,
) {
    for node in nodes.iter_mut() {
        if *expansions >= max_expansions {
            return;
        }

        let screen_rect = camera.world_to_screen(node.world_rect, viewport);

        // Skip if off-screen
        if !screen_rect.intersects(viewport) {
            continue;
        }

        // Skip tiny rects
        let screen_size = screen_rect.width().min(screen_rect.height());
        if screen_size < 2.0 {
            continue;
        }

        // Expand if it's a non-expanded directory that's big enough on screen
        if node.is_dir && node.has_children && !node.children_expanded && screen_size > 80.0 {
            // Find the corresponding FileNode child (aggregates have no
            // backing FileNode; get(AGGREGATE_INDEX) is None)
            if let Some(child_file) = file_node.children.get(node.child_index) {
                // Worst case materialized: (CAP-1) kept + kept "<Free Space>"
                // + aggregate tail = CAP+1. The estimate must never undercount
                // or live_nodes could exceed NODE_BUDGET.
                let candidate = child_file.children.len().min(EXPAND_CHILD_CAP + 1);
                if *live_nodes + candidate > NODE_BUDGET {
                    continue;
                }
                let cr = content_rect(node.world_rect, node.depth);
                let (children, norm, aspect) = layout_children(child_file, cr, node.depth + 1);
                *live_nodes += children.len();
                node.children = children;
                node.child_norm = norm;
                node.child_norm_aspect = aspect;
                node.children_expanded = true;
                *expansions += 1;
            }
        }

        // Recurse into expanded children
        if node.children_expanded {
            if let Some(child_file) = file_node.children.get(node.child_index) {
                expand_recursive(
                    &mut node.children,
                    child_file,
                    camera,
                    viewport,
                    expansions,
                    max_expansions,
                    live_nodes,
                );
            }
        }
    }
}

/// Prune children of nodes that are off-screen or tiny.
/// Returns the number of LayoutNodes freed. Assigns fresh Vecs (never
/// `clear()`, which retains heap capacity indefinitely).
fn prune_recursive(
    nodes: &mut [LayoutNode],
    camera: &crate::camera::Camera,
    viewport: egui::Rect,
) -> usize {
    let mut freed = 0;
    for node in nodes.iter_mut() {
        if !node.children_expanded {
            continue;
        }

        let screen_rect = camera.world_to_screen(node.world_rect, viewport);

        // If off-screen or very small, prune children
        if !screen_rect.intersects(viewport) || screen_rect.width().min(screen_rect.height()) < 20.0 {
            freed += count_nodes(&node.children);
            node.children = Vec::new();
            node.child_norm = Vec::new();
            node.children_expanded = false;
        } else {
            freed += prune_recursive(&mut node.children, camera, viewport);
        }
    }
    freed
}

/// Count all materialized nodes in a subtree (the slice itself + descendants).
fn count_nodes(nodes: &[LayoutNode]) -> usize {
    let mut c = nodes.len();
    for n in nodes {
        if n.children_expanded {
            c += count_nodes(&n.children);
        }
    }
    c
}

/// Build ancestor chain down to the deepest node at the point.
fn ancestor_chain_recursive<'a>(
    nodes: &'a [LayoutNode],
    pos: egui::Pos2,
    chain: &mut Vec<(&'a str, usize, egui::Rect)>,
) {
    for node in nodes {
        if !node.world_rect.contains(pos) {
            continue;
        }
        chain.push((&node.name, node.color_index, node.world_rect));
        if node.children_expanded {
            ancestor_chain_recursive(&node.children, pos, chain);
        }
        return;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::camera::Camera;
    use std::path::PathBuf;

    fn file(name: &str, size: u64) -> FileNode {
        FileNode {
            name: name.to_string(),
            path: PathBuf::new(),
            size,
            is_dir: false,
            file_count: 0,
            modified: 0,
            children: Vec::new(),
        }
    }

    fn dir(name: &str, mut children: Vec<FileNode>) -> FileNode {
        children.sort_by(|a, b| b.size.cmp(&a.size));
        let size = children.iter().map(|c| c.size).sum();
        let file_count = children
            .iter()
            .map(|c| if c.is_dir { c.file_count } else { 1 })
            .sum();
        FileNode {
            name: name.to_string(),
            path: PathBuf::new(),
            size,
            is_dir: true,
            file_count,
            modified: 0,
            children,
        }
    }

    fn viewport() -> egui::Rect {
        egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(1000.0, 700.0))
    }

    fn camera_at_root(wl: &WorldLayout) -> Camera {
        let mut cam = Camera::new(wl.world_rect.center(), 1.0);
        cam.set_world_rect(wl.world_rect);
        cam
    }

    #[test]
    fn expansion_caches_norm_rects() {
        let root = dir(
            "root",
            vec![
                dir("big", (0..50).map(|i| file(&format!("f{i}"), 100 + i)).collect()),
                file("other", 2000),
            ],
        );
        let mut wl = WorldLayout::new(&root, 0.7);
        let cam = camera_at_root(&wl);
        wl.expand_visible(&root, &cam, viewport(), 32);

        let big = wl
            .root_nodes
            .iter()
            .find(|n| n.name == "big")
            .expect("big dir laid out");
        assert!(big.children_expanded);
        assert_eq!(big.children.len(), big.child_norm.len());
        assert!(!big.children.is_empty());
        for nr in &big.child_norm {
            assert!(nr.x.is_finite() && nr.y.is_finite() && nr.w >= 0.0 && nr.h >= 0.0);
            assert!(nr.x + nr.w <= 1.0 + 1e-3);
            assert!(nr.y + nr.h <= big.child_norm_aspect + 1e-3);
        }
        assert_eq!(wl.live_nodes, wl.root_nodes.len() + big.children.len());
    }

    #[test]
    fn expansion_capped_with_aggregate_tail() {
        let wide = dir(
            "wide",
            (0..5000).map(|i| file(&format!("f{i}"), 10_000 - i)).collect(),
        );
        let root = dir("root", vec![wide, file("other", 40_000_000)]);
        let mut wl = WorldLayout::new(&root, 0.7);
        let cam = camera_at_root(&wl);
        wl.expand_visible(&root, &cam, viewport(), 32);

        let wide = wl.root_nodes.iter().find(|n| n.name == "wide").unwrap();
        assert!(wide.children_expanded);
        assert_eq!(wide.children.len(), EXPAND_CHILD_CAP);
        let agg = wide.children.last().unwrap();
        assert!(agg.is_aggregate);
        assert_eq!(agg.child_index, AGGREGATE_INDEX);
        let materialized: u64 = wide.children[..EXPAND_CHILD_CAP - 1]
            .iter()
            .map(|c| c.size)
            .sum();
        assert_eq!(materialized + agg.size, wide.size);
    }

    #[test]
    fn prune_releases_capacity() {
        let root = dir(
            "root",
            vec![
                dir("big", (0..100).map(|i| file(&format!("f{i}"), 100)).collect()),
                file("other", 10_000),
            ],
        );
        let mut wl = WorldLayout::new(&root, 0.7);
        let cam = camera_at_root(&wl);
        wl.expand_visible(&root, &cam, viewport(), 32);
        let expanded_nodes = wl.live_nodes;
        assert!(expanded_nodes > wl.root_nodes.len());

        // Move the camera deep into "other" so "big" ends up tiny/off-screen.
        let other_rect = wl
            .root_nodes
            .iter()
            .find(|n| n.name == "other")
            .unwrap()
            .world_rect;
        let mut far_cam = Camera::new(other_rect.center(), 4000.0);
        far_cam.set_world_rect(wl.world_rect);
        for _ in 0..20 {
            wl.maybe_prune(&far_cam, viewport());
        }

        let big = wl.root_nodes.iter().find(|n| n.name == "big").unwrap();
        assert!(!big.children_expanded);
        assert_eq!(big.children.capacity(), 0, "prune must release capacity");
        assert_eq!(big.child_norm.capacity(), 0);
        assert!(wl.live_nodes < expanded_nodes);
    }

    #[test]
    fn node_budget_blocks_expansion() {
        let root = dir(
            "root",
            vec![
                dir("big", (0..100).map(|i| file(&format!("f{i}"), 100)).collect()),
                file("other", 10_000),
            ],
        );
        let mut wl = WorldLayout::new(&root, 0.7);
        wl.live_nodes = NODE_BUDGET; // simulate a saturated layout
        let cam = camera_at_root(&wl);
        wl.expand_visible(&root, &cam, viewport(), 32);

        let big = wl.root_nodes.iter().find(|n| n.name == "big").unwrap();
        assert!(!big.children_expanded, "expansion must respect NODE_BUDGET");
    }
}
