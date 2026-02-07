use crate::scanner::FileNode;
use crate::treemap;
use eframe::egui;

/// A node in the world-space layout tree.
/// Each node corresponds to a FileNode and has a fixed world-space rect.
pub struct LayoutNode {
    pub world_rect: egui::Rect,
    pub depth: usize,
    pub name: String,
    pub size: u64,
    pub is_dir: bool,
    pub has_children: bool,
    pub color_index: usize,
    pub child_index: usize,
    pub children_expanded: bool,
    pub children: Vec<LayoutNode>,
}

/// The top-level world-space layout.
pub struct WorldLayout {
    pub root_nodes: Vec<LayoutNode>,
    pub world_rect: egui::Rect,
    frame_counter: u64,
}

/// Fraction of parent rect height used for directory headers at a given depth.
/// Approximate — world_rects are only used for camera/expand/prune decisions, not rendering.
fn header_fraction(_depth: usize) -> f32 {
    0.01
}

/// Compute the content rect inside a directory rect (below the header).
/// Approximate — world_rects are only used for camera/expand/prune decisions, not rendering.
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

        let root_nodes = layout_children(file_root, world_rect, 0);

        WorldLayout {
            root_nodes,
            world_rect,
            frame_counter: 0,
        }
    }

    /// Expand directories that are large enough on screen but not yet expanded.
    /// Caps expansions per call to prevent hitches.
    pub fn expand_visible(&mut self, file_root: &FileNode, camera: &crate::camera::Camera, viewport: egui::Rect, max_expansions: usize) {
        let mut expansions = 0;

        expand_recursive(
            &mut self.root_nodes,
            file_root,
            camera,
            viewport,
            &mut expansions,
            max_expansions,
        );
    }

    /// Prune children of off-screen or tiny nodes to free memory.
    /// Called every N frames.
    pub fn maybe_prune(&mut self, camera: &crate::camera::Camera, viewport: egui::Rect) {
        self.frame_counter += 1;
        if self.frame_counter % 60 != 0 {
            return;
        }
        prune_recursive(&mut self.root_nodes, camera, viewport);
    }

    /// Build an ancestor chain from the root to the deepest node containing world_pos.
    /// Returns Vec of (name, color_index, world_rect).
    pub fn ancestor_chain(&self, world_pos: egui::Pos2) -> Vec<(&str, usize, egui::Rect)> {
        let mut chain = Vec::new();
        ancestor_chain_recursive(&self.root_nodes, world_pos, &mut chain);
        chain
    }

}

/// Lay out the children of `file_node` into `parent_rect` using squarified treemap.
fn layout_children(file_node: &FileNode, parent_rect: egui::Rect, depth: usize) -> Vec<LayoutNode> {
    if file_node.children.is_empty() {
        return Vec::new();
    }

    let sizes: Vec<f64> = file_node.children.iter().map(|c| c.size as f64).collect();
    let rects = treemap::layout(
        parent_rect.min.x,
        parent_rect.min.y,
        parent_rect.width(),
        parent_rect.height(),
        &sizes,
    );

    let mut nodes = Vec::with_capacity(rects.len());
    for tr in &rects {
        let child = &file_node.children[tr.index];
        let world_rect = egui::Rect::from_min_size(
            egui::pos2(tr.x, tr.y),
            egui::vec2(tr.w, tr.h),
        );
        let has_children = child.is_dir && !child.children.is_empty();

        // Color by depth: each nesting level gets its own palette color (SpaceMonger style)
        let color_index = depth % 8;

        nodes.push(LayoutNode {
            world_rect,
            depth,
            name: child.name.clone(),
            size: child.size,
            is_dir: child.is_dir,
            has_children,
            color_index,
            child_index: tr.index,
            children_expanded: false,
            children: Vec::new(),
        });
    }

    nodes
}

/// Lay out children (color is depth-based, no inheritance needed).
fn layout_children_at_depth(
    file_node: &FileNode,
    parent_rect: egui::Rect,
    depth: usize,
) -> Vec<LayoutNode> {
    layout_children(file_node, parent_rect, depth)
}

/// Recursively expand nodes that are visible and large enough on screen.
fn expand_recursive(
    nodes: &mut [LayoutNode],
    file_node: &FileNode,
    camera: &crate::camera::Camera,
    viewport: egui::Rect,
    expansions: &mut usize,
    max_expansions: usize,
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
            // Find the corresponding FileNode child
            if let Some(child_file) = file_node.children.get(node.child_index) {
                let cr = content_rect(node.world_rect, node.depth);
                node.children = layout_children_at_depth(child_file, cr, node.depth + 1);
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
                );
            }
        }
    }
}

/// Prune children of nodes that are off-screen or tiny.
fn prune_recursive(
    nodes: &mut [LayoutNode],
    camera: &crate::camera::Camera,
    viewport: egui::Rect,
) {
    for node in nodes.iter_mut() {
        if !node.children_expanded {
            continue;
        }

        let screen_rect = camera.world_to_screen(node.world_rect, viewport);

        // If off-screen or very small, prune children
        if !screen_rect.intersects(viewport) || screen_rect.width().min(screen_rect.height()) < 20.0 {
            node.children.clear();
            node.children_expanded = false;
        } else {
            prune_recursive(&mut node.children, camera, viewport);
        }
    }
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

