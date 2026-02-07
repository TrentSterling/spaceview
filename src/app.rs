use crate::camera::Camera;
use crate::scanner::{FileNode, ScanProgress, scan_directory};
use crate::treemap;
use crate::world_layout::{LayoutNode, WorldLayout};
use eframe::egui;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::Arc;

const ZOOM_FRAME_WIDTH: f32 = 4.0;
const MIN_SCREEN_PX: f32 = 2.0;
const HEADER_PX: f32 = 16.0;
const PAD_PX: f32 = 2.0;
const BORDER_PX: f32 = 1.0;
const VERSION: &str = "0.5.0";

// ===================== Color Theme =====================

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ColorTheme {
    Rainbow,
    Heatmap,
    Pastel,
}

impl ColorTheme {
    fn base_rgb(self, depth: usize) -> (u8, u8, u8) {
        match self {
            ColorTheme::Rainbow => {
                let hue = (depth as f32 * 45.0) % 360.0;
                hsl_to_rgb(hue, 0.65, 0.55)
            }
            ColorTheme::Heatmap => {
                let hue = (depth as f32 * 270.0 / 7.0) % 270.0;
                hsl_to_rgb(hue, 0.70, 0.50)
            }
            ColorTheme::Pastel => {
                let hue = (depth as f32 * 45.0) % 360.0;
                hsl_to_rgb(hue, 0.40, 0.72)
            }
        }
    }

    fn label(self) -> &'static str {
        match self {
            ColorTheme::Rainbow => "Rainbow",
            ColorTheme::Heatmap => "Heatmap",
            ColorTheme::Pastel => "Pastel",
        }
    }
}

const THEMES: [ColorTheme; 3] = [ColorTheme::Rainbow, ColorTheme::Heatmap, ColorTheme::Pastel];

fn hsl_to_rgb(h: f32, s: f32, l: f32) -> (u8, u8, u8) {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let h2 = h / 60.0;
    let x = c * (1.0 - (h2 % 2.0 - 1.0).abs());
    let (r1, g1, b1) = if h2 < 1.0 {
        (c, x, 0.0)
    } else if h2 < 2.0 {
        (x, c, 0.0)
    } else if h2 < 3.0 {
        (0.0, c, x)
    } else if h2 < 4.0 {
        (0.0, x, c)
    } else if h2 < 5.0 {
        (x, 0.0, c)
    } else {
        (c, 0.0, x)
    };
    let m = l - c / 2.0;
    (
        ((r1 + m) * 255.0) as u8,
        ((g1 + m) * 255.0) as u8,
        ((b1 + m) * 255.0) as u8,
    )
}

// ===================== Preferences =====================

fn prefs_path() -> Option<PathBuf> {
    std::env::var("APPDATA").ok().map(|appdata| {
        PathBuf::from(appdata).join("SpaceView").join("prefs.txt")
    })
}

fn load_hide_welcome() -> bool {
    prefs_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .map(|s| s.trim() == "hide_welcome=true")
        .unwrap_or(false)
}

fn save_hide_welcome(hide: bool) {
    if let Some(p) = prefs_path() {
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(p, if hide { "hide_welcome=true" } else { "hide_welcome=false" });
    }
}

// ===================== Main App =====================

pub struct SpaceViewApp {
    // Scan state
    scan_root: Option<FileNode>,
    scanning: bool,
    scan_progress: Option<Arc<ScanProgress>>,
    scan_receiver: Option<std::sync::mpsc::Receiver<Option<FileNode>>>,

    // Camera + layout
    camera: Camera,
    world_layout: Option<WorldLayout>,
    last_viewport: egui::Rect,

    // Interaction
    hovered_node_info: Option<HoveredInfo>,
    is_dragging: bool,
    /// Current depth context from camera center (for breadcrumbs/zoom frame)
    depth_context: Vec<BreadcrumbEntry>,

    // Cached status bar info
    root_name: String,
    root_size: u64,

    // Last frame time for dt calculation
    last_time: f64,

    // Theme
    theme: ColorTheme,

    // Welcome / About
    hide_welcome: bool,
    show_about: bool,
}

#[derive(Clone)]
struct HoveredInfo {
    name: String,
    size: u64,
    is_dir: bool,
    world_rect: egui::Rect,
    has_children: bool,
    screen_rect: egui::Rect,
}

#[derive(Clone)]
struct BreadcrumbEntry {
    name: String,
    color_index: usize,
    world_rect: egui::Rect,
}

impl SpaceViewApp {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        Self {
            scan_root: None,
            scanning: false,
            scan_progress: None,
            scan_receiver: None,
            camera: Camera::new(egui::pos2(0.5, 0.5), 1.0),
            world_layout: None,
            last_viewport: egui::Rect::NOTHING,
            hovered_node_info: None,
            is_dragging: false,
            depth_context: Vec::new(),
            root_name: String::new(),
            root_size: 0,
            last_time: 0.0,
            theme: ColorTheme::Rainbow,
            hide_welcome: load_hide_welcome(),
            show_about: false,
        }
    }

    fn start_scan(&mut self, path: PathBuf) {
        if let Some(ref prog) = self.scan_progress {
            prog.cancel.store(true, Ordering::Relaxed);
        }
        self.scan_root = None;
        self.world_layout = None;
        self.camera = Camera::new(egui::pos2(0.5, 0.5), 1.0);
        self.scanning = true;
        self.depth_context.clear();
        self.hovered_node_info = None;

        let progress = Arc::new(ScanProgress::new());
        self.scan_progress = Some(progress.clone());

        let (tx, rx) = std::sync::mpsc::channel();
        self.scan_receiver = Some(rx);

        std::thread::spawn(move || {
            let result = scan_directory(&path, progress);
            let _ = tx.send(result);
        });
    }

    fn build_layout(&mut self, viewport: egui::Rect) {
        if let Some(ref root) = self.scan_root {
            let aspect = viewport.height() / viewport.width();
            let layout = WorldLayout::new(root, aspect);
            self.camera.reset(layout.world_rect);
            self.world_layout = Some(layout);
            self.root_name = root.name.clone();
            self.root_size = root.size;
        }
    }

    fn rebuild_layout_preserving_camera(&mut self, viewport: egui::Rect) {
        if let Some(ref root) = self.scan_root {
            let old_aspect = self.world_layout.as_ref()
                .map(|l| l.world_rect.height() / l.world_rect.width())
                .unwrap_or(1.0);
            let new_aspect = viewport.height() / viewport.width();

            // Remap camera center.y proportionally
            let y_ratio = if old_aspect > 0.0 {
                new_aspect / old_aspect
            } else {
                1.0
            };

            let layout = WorldLayout::new(root, new_aspect);
            self.world_layout = Some(layout);

            // Scale the camera center Y proportionally
            self.camera.center.y *= y_ratio;
            self.camera.target_center.y *= y_ratio;
        }
    }

    fn update_breadcrumbs(&mut self) {
        self.depth_context.clear();
        if let Some(ref layout) = self.world_layout {
            let chain = layout.ancestor_chain(self.camera.center);
            for (name, ci, wr) in chain {
                self.depth_context.push(BreadcrumbEntry {
                    name: name.to_string(),
                    color_index: ci,
                    world_rect: wr,
                });
            }
        }
    }
}

impl eframe::App for SpaceViewApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let now = ctx.input(|i| i.time);
        let dt = if self.last_time > 0.0 {
            (now - self.last_time) as f32
        } else {
            1.0 / 60.0
        };
        self.last_time = now;

        // Check for scan completion
        if self.scanning {
            if let Some(ref rx) = self.scan_receiver {
                if let Ok(result) = rx.try_recv() {
                    self.scan_root = result;
                    self.scanning = false;
                    self.scan_receiver = None;
                }
            }
            ctx.request_repaint();
        }

        // ---- About popup ----
        if self.show_about {
            let mut open = true;
            egui::Window::new("About SpaceView")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .open(&mut open)
                .show(ctx, |ui| {
                    ui.vertical_centered(|ui| {
                        ui.heading(format!("SpaceView v{}", VERSION));
                        ui.add_space(4.0);
                        ui.label("Disk space visualizer");
                        ui.label("By tront");
                        ui.add_space(4.0);
                        ui.label("Built with Rust + egui");
                        ui.add_space(12.0);
                    });

                    ui.separator();
                    ui.add_space(4.0);
                    ui.strong("Keyboard Shortcuts");
                    ui.add_space(4.0);
                    egui::Grid::new("about_shortcuts")
                        .num_columns(2)
                        .spacing([20.0, 4.0])
                        .show(ui, |ui| {
                            ui.label("Scroll");
                            ui.label("Zoom in/out");
                            ui.end_row();
                            ui.label("Double-click");
                            ui.label("Zoom into folder");
                            ui.end_row();
                            ui.label("Right-click");
                            ui.label("Zoom out");
                            ui.end_row();
                            ui.label("Drag");
                            ui.label("Pan view");
                            ui.end_row();
                            ui.label("Backspace / Esc");
                            ui.label("Zoom out");
                            ui.end_row();
                        });

                    ui.add_space(8.0);
                    ui.vertical_centered(|ui| {
                        if ui.button("Close").clicked() {
                            self.show_about = false;
                        }
                    });
                });
            if !open {
                self.show_about = false;
            }
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
                        let elapsed = prog.scan_start.elapsed().as_secs_f64();
                        let rate = if elapsed > 0.5 {
                            files as f64 / elapsed
                        } else {
                            0.0
                        };
                        let mut text = format!(
                            "Scanning... {} files, {}",
                            format_count(files),
                            format_size(bytes),
                        );
                        if elapsed >= 1.0 {
                            text += &format!(
                                " — {} ({}/sec)",
                                format_duration(elapsed),
                                format_count(rate as u64),
                            );
                        }
                        ui.label(text);
                    }
                    if ui.button("Cancel").clicked() {
                        if let Some(ref prog) = self.scan_progress {
                            prog.cancel.store(true, Ordering::Relaxed);
                        }
                    }
                }

                // Theme selector
                if !self.scanning {
                    ui.separator();
                    let current_label = self.theme.label();
                    egui::ComboBox::from_id_salt("theme_selector")
                        .selected_text(current_label)
                        .show_ui(ui, |ui| {
                            for &t in &THEMES {
                                ui.selectable_value(&mut self.theme, t, t.label());
                            }
                        });
                }

                // Right-aligned About button
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("About").clicked() {
                        self.show_about = !self.show_about;
                    }
                });
            });

            // Breadcrumb bar
            if self.scan_root.is_some() && !self.scanning {
                ui.horizontal(|ui| {
                    // Root crumb
                    if self.depth_context.is_empty() {
                        ui.strong(&self.root_name);
                    } else {
                        let root_name = self.root_name.clone();
                        if ui.link(&root_name).clicked() {
                            if let Some(ref layout) = self.world_layout {
                                let viewport = self.last_viewport;
                                if !viewport.is_negative() {
                                    self.camera.snap_to(layout.world_rect, viewport);
                                }
                            }
                        }
                    }

                    // Depth context crumbs
                    let crumbs = self.depth_context.clone();
                    let last_idx = crumbs.len().saturating_sub(1);
                    for (i, crumb) in crumbs.iter().enumerate() {
                        ui.label(">");
                        if i < last_idx {
                            if ui.link(&crumb.name).clicked() {
                                let viewport = self.last_viewport;
                                if !viewport.is_negative() {
                                    self.camera.snap_to(crumb.world_rect, viewport);
                                }
                            }
                        } else {
                            ui.strong(&crumb.name);
                        }
                    }

                    // Zoom level indicator
                    if self.camera.zoom > 1.5 {
                        ui.separator();
                        ui.label(format!("{:.0}x", self.camera.zoom));
                    }
                });
            }
        });

        // ---- Status bar ----
        if self.scan_root.is_some() && !self.scanning {
            egui::TopBottomPanel::bottom("status_bar").show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(format!(
                        "{} — {}",
                        self.root_name,
                        format_size(self.root_size),
                    ));

                    if let Some(ref info) = self.hovered_node_info {
                        ui.separator();
                        let pct = if self.root_size > 0 {
                            (info.size as f64 / self.root_size as f64) * 100.0
                        } else {
                            0.0
                        };
                        let icon = if info.is_dir { "D" } else { "F" };
                        ui.label(format!(
                            "[{}] {} — {} ({:.1}%)",
                            icon,
                            info.name,
                            format_size(info.size),
                            pct
                        ));
                    }
                });
            });
        }

        // ---- Central panel: treemap ----
        egui::CentralPanel::default().show(ctx, |ui| {
            if self.scan_root.is_none() && !self.scanning {
                // Welcome screen
                if self.hide_welcome {
                    ui.vertical_centered(|ui| {
                        ui.add_space(ui.available_height() / 3.0);
                        ui.label("Pick a drive or folder above to visualize disk usage.");
                    });
                } else {
                    ui.vertical_centered(|ui| {
                        ui.add_space(ui.available_height() / 5.0);
                        ui.heading(format!("SpaceView v{}", VERSION));
                        ui.add_space(6.0);
                        ui.label("A disk space visualizer inspired by SpaceMonger.");
                        ui.label("Select a drive or folder to see where your space goes.");
                        ui.add_space(16.0);

                        if ui.button("Open Folder...").clicked() {
                            if let Some(path) = rfd::FileDialog::new().pick_folder() {
                                self.start_scan(path);
                            }
                        }

                        ui.add_space(20.0);
                        ui.strong("Keyboard Shortcuts");
                        ui.add_space(6.0);

                        egui::Grid::new("welcome_shortcuts")
                            .num_columns(2)
                            .spacing([20.0, 4.0])
                            .show(ui, |ui| {
                                ui.label("Scroll");
                                ui.label("Zoom in/out");
                                ui.end_row();
                                ui.label("Double-click");
                                ui.label("Zoom into folder");
                                ui.end_row();
                                ui.label("Right-click");
                                ui.label("Zoom out");
                                ui.end_row();
                                ui.label("Drag");
                                ui.label("Pan view");
                                ui.end_row();
                                ui.label("Backspace / Esc");
                                ui.label("Zoom out");
                                ui.end_row();
                            });

                        ui.add_space(16.0);
                        let mut hide = self.hide_welcome;
                        if ui.checkbox(&mut hide, "Don't show this again").changed() {
                            self.hide_welcome = hide;
                            save_hide_welcome(hide);
                        }
                    });
                }
                return;
            }

            if self.scanning {
                ui.vertical_centered(|ui| {
                    ui.add_space(ui.available_height() / 3.0);
                    ui.heading("Scanning...");
                    if let Some(ref prog) = self.scan_progress {
                        let files = prog.files_scanned.load(Ordering::Relaxed);
                        let bytes = prog.bytes_scanned.load(Ordering::Relaxed);
                        let elapsed = prog.scan_start.elapsed().as_secs_f64();
                        ui.label(format!("{} files found", format_count(files)));
                        ui.label(format!("{} total", format_size(bytes)));
                        if elapsed >= 1.0 {
                            let rate = files as f64 / elapsed;
                            ui.label(format!(
                                "{} elapsed ({}/sec)",
                                format_duration(elapsed),
                                format_count(rate as u64),
                            ));
                        }
                    }
                    ui.spinner();
                });
                return;
            }

            let viewport = ui.available_rect_before_wrap();
            self.last_viewport = viewport;

            // Build layout on first frame after scan (or on resize)
            if self.world_layout.is_none() {
                self.build_layout(viewport);
            }

            // Handle viewport resize: rebuild layout with new aspect, preserving camera
            if let Some(ref layout) = self.world_layout {
                let current_aspect = viewport.height() / viewport.width();
                let layout_aspect = layout.world_rect.height() / layout.world_rect.width();
                if (current_aspect - layout_aspect).abs() > 0.01 {
                    self.rebuild_layout_preserving_camera(viewport);
                }
            }

            let has_layout = self.world_layout.is_some();
            if !has_layout {
                return;
            }

            // 1. Advance camera animation
            let camera_moving = self.camera.tick(dt);

            // 2. Handle input
            let response = ui.allocate_rect(viewport, egui::Sense::click_and_drag());

            // Mouse position
            let mouse_pos = ctx.input(|i| i.pointer.hover_pos());
            let mouse_in_viewport = mouse_pos.map(|p| viewport.contains(p)).unwrap_or(false);

            // Scroll zoom
            let scroll_y = ctx.input(|i| i.raw_scroll_delta.y);
            if mouse_in_viewport && scroll_y.abs() > 0.1 {
                if let Some(pos) = mouse_pos {
                    let world_focus = self.camera.screen_to_world(pos, viewport);
                    self.camera.scroll_zoom(scroll_y / 120.0, world_focus);
                }
            }

            // Drag pan
            if response.dragged_by(egui::PointerButton::Primary) {
                self.is_dragging = true;
                let delta = response.drag_delta();
                // Convert screen delta to world delta
                let scale = self.camera.zoom * viewport.width();
                let world_delta = egui::vec2(delta.x / scale, delta.y / scale);
                self.camera.drag_pan(world_delta);
            }

            if response.drag_stopped_by(egui::PointerButton::Primary) {
                self.is_dragging = false;
            }

            // Double-click: snap zoom into hovered directory
            if response.double_clicked() && !self.is_dragging {
                if let Some(ref info) = self.hovered_node_info {
                    if info.is_dir && info.has_children {
                        self.camera.snap_to(info.world_rect, viewport);
                    }
                }
            }

            // Right-click or Backspace/Escape: zoom out
            let zoom_out = ctx.input(|i| i.pointer.secondary_clicked())
                || ctx.input(|i| i.key_pressed(egui::Key::Backspace))
                || ctx.input(|i| i.key_pressed(egui::Key::Escape));

            if zoom_out {
                // Zoom out: snap to parent of current center, or to root
                if !self.depth_context.is_empty() {
                    // If we have 2+ breadcrumbs, go to second-to-last; otherwise root
                    if self.depth_context.len() >= 2 {
                        let parent = &self.depth_context[self.depth_context.len() - 2];
                        self.camera.snap_to(parent.world_rect, viewport);
                    } else if let Some(ref layout) = self.world_layout {
                        self.camera.snap_to(layout.world_rect, viewport);
                    }
                } else if let Some(ref layout) = self.world_layout {
                    self.camera.snap_to(layout.world_rect, viewport);
                }
            }

            // 3. Lazy expand visible detail
            if let (Some(ref mut layout), Some(ref root)) =
                (&mut self.world_layout, &self.scan_root)
            {
                layout.expand_visible(root, &self.camera, viewport);
                layout.maybe_prune(&self.camera, viewport);
            }

            // 4. Render
            let painter = ui.painter_at(viewport);
            let theme = self.theme;

            // Walk the layout tree and draw visible nodes
            if let Some(ref layout) = self.world_layout {
                render_nodes(&painter, &layout.root_nodes, &self.camera, viewport, theme);
            }

            // 5. Hit test for hover (screen-space, skip while dragging)
            if !self.is_dragging {
                if let Some(pos) = mouse_pos {
                    if mouse_in_viewport {
                        if let Some(ref layout) = self.world_layout {
                            if let Some(hit) = screen_hit_test(&layout.root_nodes, &self.camera, viewport, pos) {
                                // Draw hover highlight using the screen_rect from hit test
                                if hit.screen_rect.intersects(viewport) {
                                    painter.rect_stroke(
                                        hit.screen_rect.shrink(0.5),
                                        1.0,
                                        egui::Stroke::new(2.0, egui::Color32::WHITE),
                                        egui::StrokeKind::Outside,
                                    );
                                }
                                self.hovered_node_info = Some(hit);
                            } else {
                                self.hovered_node_info = None;
                            }
                        }
                    } else {
                        self.hovered_node_info = None;
                    }
                } else {
                    self.hovered_node_info = None;
                }
            }

            // 6. Update breadcrumbs from camera center
            self.update_breadcrumbs();

            // 7. Draw zoom frame borders (when zoomed in)
            if !self.depth_context.is_empty() && self.camera.zoom > 1.2 {
                // Use the color of the deepest breadcrumb
                let last = &self.depth_context[self.depth_context.len() - 1];
                let ci = last.color_index;
                let (r, g, b) = theme.base_rgb(ci);
                let frame_col = egui::Color32::from_rgb(
                    (r as f32 * 0.7) as u8,
                    (g as f32 * 0.7) as u8,
                    (b as f32 * 0.7) as u8,
                );
                let w = ZOOM_FRAME_WIDTH;
                let fr = viewport;
                // Top
                painter.rect_filled(
                    egui::Rect::from_min_max(fr.min, egui::pos2(fr.max.x, fr.min.y + w)),
                    0.0, frame_col,
                );
                // Bottom
                painter.rect_filled(
                    egui::Rect::from_min_max(egui::pos2(fr.min.x, fr.max.y - w), fr.max),
                    0.0, frame_col,
                );
                // Left
                painter.rect_filled(
                    egui::Rect::from_min_max(
                        egui::pos2(fr.min.x, fr.min.y + w),
                        egui::pos2(fr.min.x + w, fr.max.y - w),
                    ),
                    0.0, frame_col,
                );
                // Right
                painter.rect_filled(
                    egui::Rect::from_min_max(
                        egui::pos2(fr.max.x - w, fr.min.y + w),
                        egui::pos2(fr.max.x, fr.max.y - w),
                    ),
                    0.0, frame_col,
                );
            }

            // 8. Request repaint if camera is moving
            if camera_moving {
                ctx.request_repaint();
            }
        });
    }
}

// ===================== Rendering =====================
//
// Screen-space rendering pipeline (v0.5.0):
//   Children are positioned at render time via treemap::layout in screen pixels.
//   Fixed 16px headers, 2px padding, 1px border — no proportional world-space mismatch.
//   For directories with children: body fill → recurse children → header on top
//   For files/empty dirs: single-pass fill + clipped text
//
// Headers are drawn AFTER children so they're never obscured.
// All text is clipped to its containing rect via painter.with_clip_rect().

/// Top-level entry: transform root nodes from world to screen, then recurse.
fn render_nodes(
    painter: &egui::Painter,
    nodes: &[LayoutNode],
    camera: &Camera,
    viewport: egui::Rect,
    theme: ColorTheme,
) {
    for node in nodes {
        let screen_rect = camera.world_to_screen(node.world_rect, viewport);
        render_node(painter, node, screen_rect, viewport, theme);
    }
}

/// Core recursive render. `screen_rect` is the allocated screen area for this node
/// (computed by the parent via treemap::layout, NOT from world_rect for children).
fn render_node(
    painter: &egui::Painter,
    node: &LayoutNode,
    screen_rect: egui::Rect,
    viewport: egui::Rect,
    theme: ColorTheme,
) {
    // Viewport culling
    if !screen_rect.intersects(viewport) {
        return;
    }
    // Size culling
    if screen_rect.width() < MIN_SCREEN_PX || screen_rect.height() < MIN_SCREEN_PX {
        return;
    }

    if node.is_dir && node.has_children {
        let inner = screen_rect.shrink(BORDER_PX);
        let hh = HEADER_PX.min(inner.height());

        // Phase 1: body fill
        let col = body_color(node.color_index, theme);
        painter.rect_filled(inner, 1.0, col);

        // Phase 2: children in screen-space content area
        if node.children_expanded && !node.children.is_empty() {
            let content = egui::Rect::from_min_max(
                egui::pos2(inner.min.x + PAD_PX, inner.min.y + hh),
                egui::pos2(inner.max.x - PAD_PX, inner.max.y - PAD_PX),
            );
            if content.width() > MIN_SCREEN_PX && content.height() > MIN_SCREEN_PX {
                let sizes: Vec<f64> = node.children.iter().map(|c| c.size as f64).collect();
                let rects = treemap::layout(
                    content.min.x,
                    content.min.y,
                    content.width(),
                    content.height(),
                    &sizes,
                );
                for tr in &rects {
                    let child_rect = egui::Rect::from_min_size(
                        egui::pos2(tr.x, tr.y),
                        egui::vec2(tr.w, tr.h),
                    );
                    render_node(painter, &node.children[tr.index], child_rect, viewport, theme);
                }
            }
        }

        // Phase 3: header ON TOP of children
        if inner.height() >= 12.0 && inner.width() >= 8.0 {
            let header = egui::Rect::from_min_size(inner.min, egui::vec2(inner.width(), hh));
            let clipped = header.intersect(viewport);
            if clipped.width() > 0.0 && clipped.height() > 0.0 {
                let hdr_col = header_color(node.color_index, theme);
                painter.rect_filled(clipped, 1.0, hdr_col);

                if hh >= 14.0 && inner.width() > 30.0 {
                    let text_painter = painter.with_clip_rect(clipped);
                    let font_size = (hh - 4.0).clamp(9.0, 13.0);
                    let size_text = format_size(node.size);
                    let show_size = inner.width() > 100.0;
                    let size_reserve = if show_size {
                        size_text.len() as f32 * (font_size - 1.0) * 0.55 + 12.0
                    } else {
                        0.0
                    };
                    let name_width = inner.width() - 8.0 - size_reserve;
                    let max_chars = (name_width / (font_size * 0.55)).max(0.0) as usize;
                    let label = truncate_str(&node.name, max_chars);
                    text_painter.text(
                        clipped.min + egui::vec2(3.0, 1.0),
                        egui::Align2::LEFT_TOP,
                        label,
                        egui::FontId::proportional(font_size),
                        egui::Color32::WHITE,
                    );
                    if show_size {
                        text_painter.text(
                            egui::pos2(clipped.max.x - 3.0, clipped.min.y + 1.0),
                            egui::Align2::RIGHT_TOP,
                            size_text,
                            egui::FontId::proportional(font_size - 1.0),
                            egui::Color32::from_white_alpha(180),
                        );
                    }
                }
            }
        }
    } else {
        // Files / empty dirs: single pass
        let inner = screen_rect.shrink(0.5);
        let col = if node.is_dir {
            dir_color(node.color_index, theme)
        } else {
            file_color(node.color_index, theme)
        };
        painter.rect_filled(inner, 1.0, col);

        if inner.width() > 35.0 && inner.height() > 14.0 {
            let text_clip = inner.intersect(viewport);
            if text_clip.width() > 0.0 && text_clip.height() > 0.0 {
                let text_painter = painter.with_clip_rect(text_clip);
                let text_col = text_color_for(col);
                let font_size = 11.0f32.min(inner.height() - 3.0);
                let max_chars = ((inner.width() - 6.0) / (font_size * 0.55)) as usize;
                let label = truncate_str(&node.name, max_chars);

                text_painter.text(
                    inner.min + egui::vec2(3.0, 2.0),
                    egui::Align2::LEFT_TOP,
                    label,
                    egui::FontId::proportional(font_size),
                    text_col,
                );

                if inner.height() > 28.0 {
                    text_painter.text(
                        inner.min + egui::vec2(3.0, font_size + 3.0),
                        egui::Align2::LEFT_TOP,
                        format_size(node.size),
                        egui::FontId::proportional(9.0),
                        text_col.gamma_multiply(0.6),
                    );
                }
            }
        }
    }
}

// ===================== Screen-Space Hit Testing =====================

/// Hit test by traversing the layout tree and computing screen rects
/// the same way rendering does (via treemap::layout at each level).
fn screen_hit_test(
    nodes: &[LayoutNode],
    camera: &Camera,
    viewport: egui::Rect,
    screen_pos: egui::Pos2,
) -> Option<HoveredInfo> {
    for node in nodes {
        let screen_rect = camera.world_to_screen(node.world_rect, viewport);
        if let Some(hit) = hit_test_node(node, screen_rect, viewport, screen_pos) {
            return Some(hit);
        }
    }
    None
}

/// Recursive screen-space hit test for a single node.
fn hit_test_node(
    node: &LayoutNode,
    screen_rect: egui::Rect,
    viewport: egui::Rect,
    pos: egui::Pos2,
) -> Option<HoveredInfo> {
    if !screen_rect.contains(pos) {
        return None;
    }
    if screen_rect.width() < MIN_SCREEN_PX || screen_rect.height() < MIN_SCREEN_PX {
        return None;
    }

    // Check children first (deeper = more specific)
    if node.is_dir && node.has_children && node.children_expanded && !node.children.is_empty() {
        let inner = screen_rect.shrink(BORDER_PX);
        let hh = HEADER_PX.min(inner.height());
        let content = egui::Rect::from_min_max(
            egui::pos2(inner.min.x + PAD_PX, inner.min.y + hh),
            egui::pos2(inner.max.x - PAD_PX, inner.max.y - PAD_PX),
        );
        if content.width() > MIN_SCREEN_PX && content.height() > MIN_SCREEN_PX && content.contains(pos) {
            let sizes: Vec<f64> = node.children.iter().map(|c| c.size as f64).collect();
            let rects = treemap::layout(
                content.min.x,
                content.min.y,
                content.width(),
                content.height(),
                &sizes,
            );
            for tr in &rects {
                let child_rect = egui::Rect::from_min_size(
                    egui::pos2(tr.x, tr.y),
                    egui::vec2(tr.w, tr.h),
                );
                if let Some(deeper) = hit_test_node(&node.children[tr.index], child_rect, viewport, pos) {
                    return Some(deeper);
                }
            }
        }
    }

    Some(HoveredInfo {
        name: node.name.clone(),
        size: node.size,
        is_dir: node.is_dir,
        world_rect: node.world_rect,
        has_children: node.has_children,
        screen_rect,
    })
}

// ===================== Colors =====================

fn dir_color(ci: usize, theme: ColorTheme) -> egui::Color32 {
    let (r, g, b) = theme.base_rgb(ci);
    egui::Color32::from_rgb(r, g, b)
}

fn file_color(ci: usize, theme: ColorTheme) -> egui::Color32 {
    let (r, g, b) = theme.base_rgb(ci);
    let lighten = |c: u8| c.saturating_add(50).min(230);
    egui::Color32::from_rgb(lighten(r), lighten(g), lighten(b))
}

fn header_color(ci: usize, theme: ColorTheme) -> egui::Color32 {
    let (r, g, b) = theme.base_rgb(ci);
    egui::Color32::from_rgb(r.saturating_sub(15), g.saturating_sub(15), b.saturating_sub(15))
}

fn body_color(ci: usize, theme: ColorTheme) -> egui::Color32 {
    let (r, g, b) = theme.base_rgb(ci);
    egui::Color32::from_rgb(
        (r as f32 * 0.18) as u8,
        (g as f32 * 0.18) as u8,
        (b as f32 * 0.18) as u8,
    )
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

fn format_duration(secs: f64) -> String {
    let s = secs as u64;
    if s >= 3600 {
        format!("{}h {}m", s / 3600, (s % 3600) / 60)
    } else if s >= 60 {
        format!("{}m {}s", s / 60, s % 60)
    } else {
        format!("{}s", s)
    }
}
