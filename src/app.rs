use crate::scanner::{FileNode, ScanProgress, scan_directory};
use crate::treemap;
use eframe::egui;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::Arc;

// --- Performance constants ---
const MAX_DRAW_RECTS: usize = 6000;
const MAX_DEPTH: usize = 5;
const MIN_RECT_PX: f32 = 3.0;
const SCROLL_THRESHOLD: f32 = 5.0;
const SCROLL_COOLDOWN_SECS: f64 = 0.25;
const ZOOM_ANIM_DURATION: f64 = 0.25; // seconds

// --- Cached draw rect ---
#[derive(Clone)]
struct DrawRect {
    rect: egui::Rect,
    depth: usize,
    name: String,
    size: u64,
    is_dir: bool,
    has_children: bool,
    /// Index path from current root (sorted order at each level)
    sorted_path: Vec<usize>,
    /// Top-level color index (inherited by children for visual grouping)
    color_index: usize,
}

// --- Zoom animation state ---
struct ZoomAnim {
    start_time: f64,
    duration: f64,
    /// The rect in the NEW layout that we're zooming from (zoom-in) or to (zoom-out)
    focus_rect: egui::Rect,
    /// The viewport rect (full treemap area)
    viewport: egui::Rect,
    zooming_in: bool,
}

/// Ease-out cubic: fast start, smooth deceleration
fn ease_out_cubic(t: f64) -> f64 {
    1.0 - (1.0 - t).powi(3)
}

// --- Main app ---
pub struct SpaceViewApp {
    // Scan state
    scan_root: Option<FileNode>,
    scanning: bool,
    scan_progress: Option<Arc<ScanProgress>>,
    scan_receiver: Option<std::sync::mpsc::Receiver<Option<FileNode>>>,

    // Navigation
    nav_stack: Vec<usize>,
    nav_generation: u64,

    // Layout cache — only recomputed on nav change or resize
    cached_rects: Vec<DrawRect>,
    cache_nav_gen: u64,
    cache_rect: egui::Rect,
    // Cached info for status bar (avoid re-traversing tree each frame)
    cached_current_name: String,
    cached_current_size: u64,
    cached_current_count: usize,

    // Interaction
    hovered_idx: Option<usize>,
    last_scroll_time: f64,

    // Zoom animation
    zoom_anim: Option<ZoomAnim>,
    /// After zoom-out nav, the sorted index we need to find in the new parent layout
    pending_zoom_out_index: Option<usize>,
}

impl SpaceViewApp {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        Self {
            scan_root: None,
            scanning: false,
            scan_progress: None,
            scan_receiver: None,
            nav_stack: Vec::new(),
            nav_generation: 0,
            cached_rects: Vec::new(),
            cache_nav_gen: u64::MAX,
            cache_rect: egui::Rect::NOTHING,
            cached_current_name: String::new(),
            cached_current_size: 0,
            cached_current_count: 0,
            hovered_idx: None,
            last_scroll_time: 0.0,
            zoom_anim: None,
            pending_zoom_out_index: None,
        }
    }

    fn start_scan(&mut self, path: PathBuf) {
        if let Some(ref prog) = self.scan_progress {
            prog.cancel.store(true, Ordering::Relaxed);
        }
        self.scan_root = None;
        self.nav_stack.clear();
        self.nav_generation = 0;
        self.invalidate_cache();
        self.scanning = true;

        let progress = Arc::new(ScanProgress::new());
        self.scan_progress = Some(progress.clone());

        let (tx, rx) = std::sync::mpsc::channel();
        self.scan_receiver = Some(rx);

        std::thread::spawn(move || {
            let result = scan_directory(&path, progress);
            let _ = tx.send(result);
        });
    }

    fn invalidate_cache(&mut self) {
        self.cache_nav_gen = u64::MAX;
        self.cached_rects.clear();
        self.zoom_anim = None;
        self.pending_zoom_out_index = None;
    }

    fn navigate_in(&mut self, sorted_index: usize) {
        // Find the focus rect in the current (old) layout: the top-level folder we're zooming into
        let focus = self.cached_rects.iter().find(|dr| {
            dr.sorted_path.len() == 1 && dr.sorted_path[0] == sorted_index
        }).map(|dr| dr.rect);

        self.nav_stack.push(sorted_index);
        self.nav_generation += 1;

        // Start zoom-in animation if we found the rect
        if let Some(focus_rect) = focus {
            if !self.cache_rect.is_negative() {
                self.zoom_anim = Some(ZoomAnim {
                    start_time: -1.0, // will be set on first frame
                    duration: ZOOM_ANIM_DURATION,
                    focus_rect,
                    viewport: self.cache_rect,
                    zooming_in: true,
                });
            }
        }
    }

    fn navigate_in_path(&mut self, path: &[usize]) {
        // For click navigation that pushes multiple levels at once
        // Find the deepest folder rect in current layout matching this path
        let focus = self.cached_rects.iter().find(|dr| {
            dr.sorted_path == path
        }).map(|dr| dr.rect);

        for &si in path {
            self.nav_stack.push(si);
        }
        self.nav_generation += 1;

        if let Some(focus_rect) = focus {
            if !self.cache_rect.is_negative() {
                self.zoom_anim = Some(ZoomAnim {
                    start_time: -1.0,
                    duration: ZOOM_ANIM_DURATION,
                    focus_rect,
                    viewport: self.cache_rect,
                    zooming_in: true,
                });
            }
        }
    }

    fn navigate_out(&mut self) {
        if !self.nav_stack.is_empty() {
            let popped_index = self.nav_stack.pop().unwrap();
            self.nav_generation += 1;

            // We'll determine the focus_rect after the new layout is computed
            // Store the popped index temporarily in the animation
            if !self.cache_rect.is_negative() {
                self.zoom_anim = Some(ZoomAnim {
                    start_time: -1.0,
                    duration: ZOOM_ANIM_DURATION,
                    // Placeholder — will be resolved in ensure_cache after layout recomputes
                    focus_rect: egui::Rect::NOTHING,
                    viewport: self.cache_rect,
                    zooming_in: false,
                });
                // Store the index we need to find in the new layout
                self.pending_zoom_out_index = Some(popped_index);
            }
        }
    }

    fn navigate_to_level(&mut self, level: usize) {
        if level < self.nav_stack.len() {
            // Cancel any animation — multi-level jump is instant
            self.zoom_anim = None;
            self.nav_stack.truncate(level);
            self.nav_generation += 1;
        }
    }

    fn ensure_cache(&mut self, available_rect: egui::Rect) {
        let same_gen = self.cache_nav_gen == self.nav_generation;
        let same_rect = (self.cache_rect.width() - available_rect.width()).abs() < 1.0
            && (self.cache_rect.height() - available_rect.height()).abs() < 1.0;

        if same_gen && same_rect {
            return; // Cache is valid
        }

        // Cancel animation on window resize (size changed but not due to navigation)
        if !same_rect && same_gen {
            self.zoom_anim = None;
            self.pending_zoom_out_index = None;
        }

        // Recompute layout
        let new_rects = {
            if let Some(ref root) = self.scan_root {
                if let Some(node) = navigate_to(root, &self.nav_stack) {
                    self.cached_current_name = node.name.clone();
                    self.cached_current_size = node.size;
                    self.cached_current_count = node.children.len();

                    let mut rects = Vec::with_capacity(2048);
                    compute_recursive_layout(
                        node,
                        available_rect,
                        0,
                        MAX_DEPTH,
                        MIN_RECT_PX,
                        &[],
                        None,
                        &mut rects,
                    );
                    Some(rects)
                } else {
                    None
                }
            } else {
                None
            }
        };

        if let Some(rects) = new_rects {
            self.cached_rects = rects;
        } else {
            self.cached_rects.clear();
        }

        self.cache_nav_gen = self.nav_generation;
        self.cache_rect = available_rect;

        // Resolve pending zoom-out animation: find the child rect in the new parent layout
        if let Some(idx) = self.pending_zoom_out_index.take() {
            if let Some(anim) = self.zoom_anim.as_mut() {
                // Find the top-level rect in the NEW layout that matches the index we zoomed out from
                if let Some(dr) = self.cached_rects.iter().find(|dr| {
                    dr.sorted_path.len() == 1 && dr.sorted_path[0] == idx
                }) {
                    anim.focus_rect = dr.rect;
                    anim.viewport = available_rect;
                } else {
                    // Couldn't find it — cancel animation
                    self.zoom_anim = None;
                }
            }
        }
    }

    fn breadcrumb_path(&self) -> Vec<String> {
        let mut path = Vec::new();
        if let Some(ref root) = self.scan_root {
            path.push(root.name.clone());
            let mut node = root;
            for &idx in &self.nav_stack {
                let sorted = node.sorted_children();
                if let Some(child) = sorted.get(idx) {
                    path.push(child.name.clone());
                    node = child;
                }
            }
        }
        path
    }
}

impl eframe::App for SpaceViewApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Check for scan completion
        if self.scanning {
            if let Some(ref rx) = self.scan_receiver {
                if let Ok(result) = rx.try_recv() {
                    self.scan_root = result;
                    self.scanning = false;
                    self.scan_receiver = None;
                    self.nav_generation += 1;
                }
            }
            ctx.request_repaint();
        }

        // ---- Top panel ----
        egui::TopBottomPanel::top("top_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("SpaceView");
                ui.separator();

                if ui.button("Open Folder...").clicked() {
                    if let Some(path) = rfd::FileDialog::new().pick_folder() {
                        self.start_scan(path);
                    }
                }

                ui.separator();
                for letter in ['C', 'D', 'E', 'F'] {
                    let drive = format!("{}:\\", letter);
                    let drive_path = PathBuf::from(&drive);
                    if drive_path.exists() && ui.button(&drive).clicked() {
                        self.start_scan(drive_path);
                    }
                }

                if self.scanning {
                    ui.separator();
                    ui.spinner();
                    if let Some(ref prog) = self.scan_progress {
                        let files = prog.files_scanned.load(Ordering::Relaxed);
                        let bytes = prog.bytes_scanned.load(Ordering::Relaxed);
                        ui.label(format!(
                            "Scanning... {} files, {}",
                            format_count(files),
                            format_size(bytes)
                        ));
                    }
                    if ui.button("Cancel").clicked() {
                        if let Some(ref prog) = self.scan_progress {
                            prog.cancel.store(true, Ordering::Relaxed);
                        }
                    }
                }
            });

            // Breadcrumb
            if self.scan_root.is_some() && !self.scanning {
                ui.horizontal(|ui| {
                    let crumbs = self.breadcrumb_path();
                    for (i, crumb) in crumbs.iter().enumerate() {
                        if i > 0 {
                            ui.label(">");
                        }
                        if i < crumbs.len() - 1 {
                            if ui.link(crumb).clicked() {
                                self.navigate_to_level(i);
                            }
                        } else {
                            ui.strong(crumb);
                        }
                    }
                    if !self.nav_stack.is_empty() {
                        ui.separator();
                        if ui.button("Back").clicked() {
                            self.navigate_out();
                        }
                    }
                });
            }
        });

        // ---- Status bar ----
        if self.scan_root.is_some() && !self.scanning {
            egui::TopBottomPanel::bottom("status_bar").show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(format!(
                        "Total: {} | {} items",
                        format_size(self.cached_current_size),
                        self.cached_current_count
                    ));

                    if let Some(idx) = self.hovered_idx {
                        if let Some(dr) = self.cached_rects.get(idx) {
                            ui.separator();
                            let pct = if self.cached_current_size > 0 {
                                (dr.size as f64 / self.cached_current_size as f64) * 100.0
                            } else {
                                0.0
                            };
                            let icon = if dr.is_dir { "D" } else { "F" };
                            ui.label(format!(
                                "[{}] {} - {} ({:.1}%)",
                                icon,
                                dr.name,
                                format_size(dr.size),
                                pct
                            ));
                        }
                    }
                });
            });
        }

        // ---- Central panel: treemap ----
        egui::CentralPanel::default().show(ctx, |ui| {
            if self.scan_root.is_none() && !self.scanning {
                ui.vertical_centered(|ui| {
                    ui.add_space(ui.available_height() / 3.0);
                    ui.heading("Welcome to SpaceView");
                    ui.add_space(10.0);
                    ui.label("Pick a drive or folder above to visualize disk usage.");
                    ui.add_space(20.0);
                    if ui.button("Open Folder...").clicked() {
                        if let Some(path) = rfd::FileDialog::new().pick_folder() {
                            self.start_scan(path);
                        }
                    }
                });
                return;
            }

            if self.scanning {
                ui.vertical_centered(|ui| {
                    ui.add_space(ui.available_height() / 3.0);
                    ui.heading("Scanning...");
                    if let Some(ref prog) = self.scan_progress {
                        let files = prog.files_scanned.load(Ordering::Relaxed);
                        let bytes = prog.bytes_scanned.load(Ordering::Relaxed);
                        ui.label(format!("{} files found", format_count(files)));
                        ui.label(format!("{} total", format_size(bytes)));
                    }
                    ui.spinner();
                });
                return;
            }

            let rect = ui.available_rect_before_wrap();

            // Ensure cache is up to date (only recomputes if nav or size changed)
            self.ensure_cache(rect);

            if self.cached_rects.is_empty() {
                ui.vertical_centered(|ui| {
                    ui.add_space(ui.available_height() / 3.0);
                    ui.label("This folder is empty.");
                });
                return;
            }

            let painter = ui.painter_at(rect);

            // --- Compute zoom animation transform ---
            let now = ctx.input(|i| i.time);
            let animating = self.zoom_anim.is_some();
            let mut anim_done = false;

            // Compute scale and offset for the current animation frame
            // (scale=1.0, offset=zero when no animation)
            let (scale, offset) = if let Some(ref mut anim) = self.zoom_anim {
                // Initialize start_time on first frame
                if anim.start_time < 0.0 {
                    anim.start_time = now;
                }
                // Skip animation if focus_rect is invalid
                if anim.focus_rect.is_negative() || anim.focus_rect.width() < 1.0 || anim.focus_rect.height() < 1.0 {
                    anim_done = true;
                    (1.0f32, egui::Vec2::ZERO)
                } else {
                    let elapsed = now - anim.start_time;
                    let raw_t = (elapsed / anim.duration).min(1.0) as f32;
                    let t = ease_out_cubic(raw_t as f64) as f32;

                    if raw_t >= 1.0 {
                        anim_done = true;
                        (1.0f32, egui::Vec2::ZERO)
                    } else {
                        let vp = anim.viewport;
                        let fr = anim.focus_rect;

                        // Transform maps layout coordinates → screen coordinates:
                        //   screen_pos = layout_pos * scale + offset
                        // At t=1 (end): identity → scale=1, offset=0
                        // At t=0 (start): depends on direction

                        if anim.zooming_in {
                            // Zoom IN: new layout fills viewport. We want it to
                            // START appearing at focus_rect's old position (small)
                            // and EXPAND to fill the viewport.
                            //
                            // At t=0: viewport → focus_rect
                            //   fr.min = vp.min * s0 + off0
                            //   s0 = fr.width / vp.width  (< 1, shrink)
                            //   off0 = fr.min - vp.min * s0
                            let s0 = (fr.width() / vp.width())
                                .min(fr.height() / vp.height());
                            let off0 = egui::vec2(
                                fr.min.x - vp.min.x * s0,
                                fr.min.y - vp.min.y * s0,
                            );
                            let s = s0 + (1.0 - s0) * t; // lerp s0 → 1.0
                            let off = egui::vec2(
                                off0.x * (1.0 - t), // lerp off0 → 0
                                off0.y * (1.0 - t),
                            );
                            (s, off)
                        } else {
                            // Zoom OUT: new layout is the parent. We want it to
                            // START zoomed into focus_rect (where the child sits)
                            // and PULL BACK to show the full parent.
                            //
                            // At t=0: focus_rect fills viewport
                            //   vp.min = fr.min * s0 + off0
                            //   s0 = vp.width / fr.width  (> 1, magnify)
                            //   off0 = vp.min - fr.min * s0
                            let s0 = (vp.width() / fr.width())
                                .min(vp.height() / fr.height());
                            let off0 = egui::vec2(
                                vp.min.x - fr.min.x * s0,
                                vp.min.y - fr.min.y * s0,
                            );
                            let s = s0 + (1.0 - s0) * t; // lerp s0 → 1.0
                            let off = egui::vec2(
                                off0.x * (1.0 - t), // lerp off0 → 0
                                off0.y * (1.0 - t),
                            );
                            (s, off)
                        }
                    }
                }
            } else {
                (1.0f32, egui::Vec2::ZERO)
            };

            if anim_done {
                self.zoom_anim = None;
            }

            if animating && !anim_done {
                ctx.request_repaint();
            }

            // Helper: transform a rect by scale+offset, clamped to viewport
            let transform_rect = |r: egui::Rect| -> egui::Rect {
                if scale == 1.0 && offset == egui::Vec2::ZERO {
                    return r;
                }
                let min = egui::pos2(
                    r.min.x * scale + offset.x,
                    r.min.y * scale + offset.y,
                );
                let max = egui::pos2(
                    r.max.x * scale + offset.x,
                    r.max.y * scale + offset.y,
                );
                egui::Rect::from_min_max(min, max)
            };

            // --- Hit test: find deepest rect under mouse ---
            // During animation, disable hover to avoid flickering
            let mouse_pos = ctx.input(|i| i.pointer.hover_pos());
            let mut new_hovered: Option<usize> = None;
            if !animating || anim_done {
                if let Some(pos) = mouse_pos {
                    if rect.contains(pos) {
                        for (i, dr) in self.cached_rects.iter().enumerate() {
                            if dr.rect.contains(pos) {
                                new_hovered = Some(i);
                            }
                        }
                    }
                }
            }
            self.hovered_idx = new_hovered;

            // --- Draw all cached rects (with transform applied) ---
            for (i, dr) in self.cached_rects.iter().enumerate() {
                let draw_rect = transform_rect(dr.rect);

                // Skip rects that are too small or outside viewport
                if draw_rect.width() < 1.0 || draw_rect.height() < 1.0 {
                    continue;
                }
                if !draw_rect.intersects(rect) {
                    continue;
                }

                let is_hovered = self.hovered_idx == Some(i);

                if dr.is_dir && dr.has_children {
                    // FOLDER: draw header bar + dark body
                    let hh = header_height(dr.depth) * scale;
                    let inner = draw_rect.shrink(0.5);

                    // Body (dark background — children will draw on top)
                    let body_col = body_color(dr.color_index, dr.depth);
                    painter.rect_filled(inner, 1.0, body_col);

                    // Header bar
                    if inner.height() > hh + 4.0 {
                        let header = egui::Rect::from_min_size(
                            inner.min,
                            egui::vec2(inner.width(), hh),
                        );
                        let hdr_col = header_color(dr.color_index, dr.depth, is_hovered);
                        painter.rect_filled(header, 1.0, hdr_col);

                        // Label in header
                        if inner.width() > 30.0 && hh > 10.0 {
                            let font_size = (hh - 4.0).clamp(9.0, 13.0);
                            let max_chars = ((inner.width() - 8.0) / (font_size * 0.55)) as usize;
                            let label = truncate_str(&dr.name, max_chars);
                            painter.text(
                                header.min + egui::vec2(3.0, 1.0),
                                egui::Align2::LEFT_TOP,
                                label,
                                egui::FontId::proportional(font_size),
                                egui::Color32::WHITE,
                            );
                            // Size in header if room
                            if inner.width() > 100.0 {
                                painter.text(
                                    egui::pos2(header.max.x - 3.0, header.min.y + 1.0),
                                    egui::Align2::RIGHT_TOP,
                                    format_size(dr.size),
                                    egui::FontId::proportional(font_size - 1.0),
                                    egui::Color32::from_white_alpha(180),
                                );
                            }
                        }
                    } else {
                        // Too small for header, just fill with color
                        let col = dir_color(dr.color_index, dr.depth, is_hovered);
                        painter.rect_filled(inner, 1.0, col);
                    }
                } else {
                    // FILE or leaf folder: solid color fill
                    let inner = draw_rect.shrink(0.5);
                    let col = if dr.is_dir {
                        dir_color(dr.color_index, dr.depth, is_hovered)
                    } else {
                        file_color(dr.color_index, dr.depth, is_hovered)
                    };
                    painter.rect_filled(inner, 1.0, col);

                    // Labels
                    if inner.width() > 35.0 && inner.height() > 14.0 {
                        let text_col = text_color_for(col);
                        let font_size = 11.0f32.min(inner.height() - 3.0);
                        let max_chars =
                            ((inner.width() - 6.0) / (font_size * 0.55)) as usize;
                        let label = truncate_str(&dr.name, max_chars);

                        painter.text(
                            inner.min + egui::vec2(3.0, 2.0),
                            egui::Align2::LEFT_TOP,
                            label,
                            egui::FontId::proportional(font_size),
                            text_col,
                        );

                        if inner.height() > 28.0 {
                            painter.text(
                                inner.min + egui::vec2(3.0, font_size + 3.0),
                                egui::Align2::LEFT_TOP,
                                format_size(dr.size),
                                egui::FontId::proportional(9.0),
                                text_col.gamma_multiply(0.6),
                            );
                        }
                    }
                }
            }

            // --- Input handling ---
            // Block input during animation
            let input_blocked = animating && !anim_done;

            // Mouse wheel zoom (with cooldown to prevent hyper-zooming)
            let scroll_y = ctx.input(|i| i.smooth_scroll_delta.y);
            let scroll_ready = (now - self.last_scroll_time) > SCROLL_COOLDOWN_SECS;

            if !input_blocked && scroll_ready && scroll_y > SCROLL_THRESHOLD {
                // Scroll up → zoom IN to hovered folder
                if let Some(idx) = self.hovered_idx {
                    let dr = &self.cached_rects[idx];
                    if !dr.sorted_path.is_empty() {
                        let top_idx = dr.sorted_path[0];
                        let is_navigable = self.cached_rects.iter().any(|r| {
                            r.sorted_path.len() == 1
                                && r.sorted_path[0] == top_idx
                                && r.has_children
                        });
                        if is_navigable {
                            self.navigate_in(top_idx);
                            self.last_scroll_time = now;
                        }
                    }
                }
            } else if !input_blocked && scroll_ready && scroll_y < -SCROLL_THRESHOLD {
                // Scroll down → zoom OUT
                self.navigate_out();
                self.last_scroll_time = now;
            }

            // Left click → zoom into clicked folder
            if !input_blocked && ctx.input(|i| i.pointer.primary_clicked()) {
                if let Some(idx) = self.hovered_idx {
                    let dr = &self.cached_rects[idx].clone();
                    if dr.has_children {
                        // Navigate directly to this folder
                        let path = dr.sorted_path.clone();
                        self.navigate_in_path(&path);
                    } else if dr.is_dir {
                        // Empty dir, no-op
                    } else if dr.sorted_path.len() > 1 {
                        // File: navigate to its parent folder
                        let parent_path = dr.sorted_path[..dr.sorted_path.len() - 1].to_vec();
                        self.navigate_in_path(&parent_path);
                    }
                }
            }

            // Right click → back
            if !input_blocked && ctx.input(|i| i.pointer.secondary_clicked()) {
                self.navigate_out();
            }

            // Keyboard: Backspace/Escape → back
            if !input_blocked
                && (ctx.input(|i| i.key_pressed(egui::Key::Backspace))
                    || ctx.input(|i| i.key_pressed(egui::Key::Escape)))
            {
                self.navigate_out();
            }

            ui.allocate_rect(rect, egui::Sense::click_and_drag());
        });
    }
}

// ===================== Layout computation =====================

fn navigate_to<'a>(root: &'a FileNode, nav_stack: &[usize]) -> Option<&'a FileNode> {
    let mut node = root;
    for &idx in nav_stack {
        let sorted = node.sorted_children();
        node = sorted.get(idx).copied()?;
    }
    Some(node)
}

fn compute_recursive_layout(
    node: &FileNode,
    bounds: egui::Rect,
    depth: usize,
    max_depth: usize,
    min_size: f32,
    parent_path: &[usize],
    color_override: Option<usize>,
    result: &mut Vec<DrawRect>,
) {
    if result.len() >= MAX_DRAW_RECTS {
        return;
    }

    let sorted = node.sorted_children();
    if sorted.is_empty() {
        return;
    }

    let sizes: Vec<f64> = sorted.iter().map(|c| c.size as f64).collect();
    let layout_rects =
        treemap::layout(bounds.min.x, bounds.min.y, bounds.width(), bounds.height(), &sizes);

    for tr in &layout_rects {
        if result.len() >= MAX_DRAW_RECTS {
            break;
        }

        let child = sorted[tr.index];
        let r = egui::Rect::from_min_size(egui::pos2(tr.x, tr.y), egui::vec2(tr.w, tr.h));

        if r.width() < min_size || r.height() < min_size {
            continue;
        }

        let mut path = Vec::with_capacity(parent_path.len() + 1);
        path.extend_from_slice(parent_path);
        path.push(tr.index);

        let ci = color_override.unwrap_or(tr.index);
        let has_children = child.is_dir && !child.children.is_empty();

        result.push(DrawRect {
            rect: r,
            depth,
            name: child.name.clone(),
            size: child.size,
            is_dir: child.is_dir,
            has_children,
            sorted_path: path.clone(),
            color_index: ci,
        });

        // Recurse into directories
        if has_children && depth < max_depth {
            let hh = header_height(depth);
            let pad = 1.0;
            if r.height() > hh + 16.0 && r.width() > 24.0 {
                let inner = egui::Rect::from_min_max(
                    egui::pos2(r.min.x + pad, r.min.y + hh),
                    egui::pos2(r.max.x - pad, r.max.y - pad),
                );
                compute_recursive_layout(
                    child, inner, depth + 1, max_depth, min_size, &path, Some(ci), result,
                );
            }
        }
    }
}

fn header_height(depth: usize) -> f32 {
    match depth {
        0 => 20.0,
        1 => 16.0,
        2 => 14.0,
        3 => 12.0,
        _ => 2.0,
    }
}

// ===================== Colors =====================

const PALETTE: [(u8, u8, u8); 8] = [
    (66, 133, 244),  // blue
    (52, 168, 83),   // green
    (251, 188, 4),   // yellow
    (234, 67, 53),   // red
    (171, 71, 188),  // purple
    (0, 172, 193),   // teal
    (255, 112, 67),  // orange
    (63, 81, 181),   // indigo
];

fn darken(c: u8, depth: usize) -> u8 {
    let factor = 1.0 - (depth as f32 * 0.12).min(0.45);
    (c as f32 * factor) as u8
}

fn dir_color(ci: usize, depth: usize, hovered: bool) -> egui::Color32 {
    let (r, g, b) = PALETTE[ci % PALETTE.len()];
    let (r, g, b) = (darken(r, depth), darken(g, depth), darken(b, depth));
    if hovered {
        egui::Color32::from_rgb(r.saturating_add(35), g.saturating_add(35), b.saturating_add(35))
    } else {
        egui::Color32::from_rgb(r, g, b)
    }
}

fn file_color(ci: usize, depth: usize, hovered: bool) -> egui::Color32 {
    let (r, g, b) = PALETTE[ci % PALETTE.len()];
    // Files are lighter than folders
    let lighten = |c: u8| c.saturating_add(50).min(230);
    let (r, g, b) = (
        lighten(darken(r, depth)),
        lighten(darken(g, depth)),
        lighten(darken(b, depth)),
    );
    if hovered {
        egui::Color32::from_rgb(r.saturating_add(25), g.saturating_add(25), b.saturating_add(25))
    } else {
        egui::Color32::from_rgb(r, g, b)
    }
}

fn header_color(ci: usize, depth: usize, hovered: bool) -> egui::Color32 {
    let (r, g, b) = PALETTE[ci % PALETTE.len()];
    // Headers are slightly darker/more saturated than body
    let dim = |c: u8| darken(c, depth).saturating_sub(15);
    let (r, g, b) = (dim(r), dim(g), dim(b));
    if hovered {
        egui::Color32::from_rgb(r.saturating_add(40), g.saturating_add(40), b.saturating_add(40))
    } else {
        egui::Color32::from_rgb(r, g, b)
    }
}

fn body_color(ci: usize, depth: usize) -> egui::Color32 {
    let (r, g, b) = PALETTE[ci % PALETTE.len()];
    // Very dark version — children draw on top, gaps reveal this
    let d = |c: u8| (darken(c, depth) as f32 * 0.18) as u8;
    egui::Color32::from_rgb(d(r), d(g), d(b))
}

fn text_color_for(bg: egui::Color32) -> egui::Color32 {
    let lum = 0.299 * bg.r() as f64 + 0.587 * bg.g() as f64 + 0.114 * bg.b() as f64;
    if lum > 150.0 {
        egui::Color32::from_gray(20)
    } else {
        egui::Color32::from_gray(235)
    }
}

// ===================== Helpers =====================

fn truncate_str(s: &str, max_chars: usize) -> String {
    if max_chars < 4 {
        return String::new();
    }
    if s.len() <= max_chars {
        s.to_string()
    } else {
        format!("{}...", &s[..max_chars - 3])
    }
}

pub fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;
    const GB: u64 = 1024 * MB;
    const TB: u64 = 1024 * GB;

    if bytes >= TB {
        format!("{:.2} TB", bytes as f64 / TB as f64)
    } else if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.0} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

fn format_count(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}K", n as f64 / 1_000.0)
    } else {
        format!("{}", n)
    }
}
