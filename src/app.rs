use crate::camera::Camera;
use crate::scanner::{FileNode, ScanProgress, get_free_space, scan_directory_live};
use crate::treemap;
use crate::world_layout::{LayoutNode, WorldLayout};
use eframe::egui;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::Arc;

const ZOOM_FRAME_WIDTH: f32 = 4.0;
const MIN_SCREEN_PX: f32 = 2.0;
const HEADER_PX: f32 = 16.0;
const PAD_PX: f32 = 3.0;
const BORDER_PX: f32 = 1.5;
const VERSION: &str = env!("CARGO_PKG_VERSION");

// ===================== Color Theme =====================

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ColorTheme {
    Rainbow,
    Neon,
    Ocean,
}

impl ColorTheme {
    fn base_rgb(self, depth: usize) -> (u8, u8, u8) {
        let golden = (depth as f32 * 137.508) % 360.0;
        match self {
            ColorTheme::Rainbow => hsl_to_rgb(golden, 0.75, 0.65),
            ColorTheme::Neon => hsl_to_rgb(golden, 0.95, 0.65),
            ColorTheme::Ocean => hsl_to_rgb((golden + 180.0) % 360.0, 0.60, 0.60),
        }
    }

    fn label(self) -> &'static str {
        match self {
            ColorTheme::Rainbow => "Rainbow",
            ColorTheme::Neon => "Neon",
            ColorTheme::Ocean => "Ocean",
        }
    }
}

const THEMES: [ColorTheme; 3] = [ColorTheme::Rainbow, ColorTheme::Neon, ColorTheme::Ocean];

#[derive(Clone, Copy, Debug, PartialEq)]
enum ViewMode {
    Treemap,
    List,
    LargestFiles,
    Extensions,
    Duplicates,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum ColorMode {
    Depth,
    Age,
    Extension,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum SortColumn {
    Name,
    Size,
    FileCount,
}

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

pub struct Prefs {
    pub hide_about: bool,
    pub dark_mode: bool,
    pub window_x: Option<f32>,
    pub window_y: Option<f32>,
    pub window_w: Option<f32>,
    pub window_h: Option<f32>,
}

pub fn prefs_path() -> Option<PathBuf> {
    std::env::var("APPDATA").ok().map(|appdata| {
        PathBuf::from(appdata).join("SpaceView").join("prefs.txt")
    })
}

pub fn load_prefs() -> Prefs {
    let mut prefs = Prefs {
        hide_about: false,
        dark_mode: true,
        window_x: None,
        window_y: None,
        window_w: None,
        window_h: None,
    };
    if let Some(content) = prefs_path().and_then(|p| std::fs::read_to_string(p).ok()) {
        for line in content.lines() {
            let line = line.trim();
            if let Some((key, val)) = line.split_once('=') {
                match key.trim() {
                    "hide_about" => prefs.hide_about = val.trim() == "true",
                    "dark_mode" => prefs.dark_mode = val.trim() == "true",
                    "window_x" => prefs.window_x = val.trim().parse().ok(),
                    "window_y" => prefs.window_y = val.trim().parse().ok(),
                    "window_w" => prefs.window_w = val.trim().parse().ok(),
                    "window_h" => prefs.window_h = val.trim().parse().ok(),
                    _ => {}
                }
            }
        }
    }
    prefs
}

fn save_prefs(prefs: &Prefs) {
    if let Some(p) = prefs_path() {
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let mut content = format!(
            "hide_about={}\ndark_mode={}",
            prefs.hide_about, prefs.dark_mode,
        );
        if let (Some(x), Some(y), Some(w), Some(h)) =
            (prefs.window_x, prefs.window_y, prefs.window_w, prefs.window_h)
        {
            content += &format!("\nwindow_x={}\nwindow_y={}\nwindow_w={}\nwindow_h={}", x, y, w, h);
        }
        let _ = std::fs::write(p, content);
    }
}

// ===================== Main App =====================

pub struct SpaceViewApp {
    // Scan state
    scan_root: Option<FileNode>,
    scanning: bool,
    scan_progress: Option<Arc<ScanProgress>>,
    scan_receiver: Option<std::sync::mpsc::Receiver<(Option<FileNode>, Option<Vec<(String, u64, String)>>, Option<Vec<(String, u64, u64)>>, (u64, u64))>>,
    snapshot_receiver: Option<std::sync::mpsc::Receiver<FileNode>>,

    // Camera + layout
    camera: Camera,
    world_layout: Option<WorldLayout>,
    last_viewport: egui::Rect,

    // Interaction
    hovered_node_info: Option<HoveredInfo>,
    context_menu_info: Option<HoveredInfo>,
    is_dragging: bool,
    /// Current depth context from camera center (for breadcrumbs/zoom frame)
    depth_context: Vec<BreadcrumbEntry>,

    // Cached status bar info
    root_name: String,
    root_size: u64,
    root_file_count: u64,
    scan_path: Option<PathBuf>,
    show_free_space: bool,

    // Last frame time for dt calculation
    last_time: f64,

    // Theme
    theme: ColorTheme,
    dark_mode: bool,

    // About dialog
    hide_about_on_start: bool,
    show_about: bool,

    // About dialog textures
    icon_texture: Option<egui::TextureHandle>,
    face_texture: Option<egui::TextureHandle>,

    // Version check
    update_check_receiver: Option<std::sync::mpsc::Receiver<Option<String>>>,
    latest_version: Option<String>,

    // Pending delete confirmation
    pending_delete: Option<PathBuf>,

    // View mode
    view_mode: ViewMode,
    search_text: String,
    list_sort: SortColumn,
    list_sort_asc: bool,
    list_path: Vec<String>,
    cached_largest: Option<Vec<(String, u64, String)>>,
    cached_extensions: Option<Vec<(String, u64, u64)>>, // (extension, total_size, file_count)
    cached_duplicates: Option<Vec<DuplicateGroup>>,
    dup_receiver: Option<std::sync::mpsc::Receiver<Vec<DuplicateGroup>>>,

    // Color mode
    color_mode: ColorMode,
    time_range: (u64, u64), // (oldest, newest) modified timestamps across all files
    ext_color_map: std::collections::HashMap<String, usize>, // extension -> color index

    // Window position tracking (saved on exit)
    last_window_outer_pos: Option<egui::Pos2>,
    last_window_inner_size: Option<egui::Vec2>,
}

#[derive(Clone)]
struct HoveredInfo {
    name: String,
    size: u64,
    file_count: u64,
    is_dir: bool,
    world_rect: egui::Rect,
    has_children: bool,
    screen_rect: egui::Rect,
}

#[derive(Clone)]
struct DuplicateGroup {
    size: u64,
    paths: Vec<String>, // full paths of duplicate files
}

#[derive(Clone)]
struct BreadcrumbEntry {
    name: String,
    color_index: usize,
    world_rect: egui::Rect,
}

/// Compare two version strings (e.g. "0.5.3" vs "0.5.4").
/// Returns true if `remote` is strictly newer than `local`.
fn is_newer_version(local: &str, remote: &str) -> bool {
    let parse = |s: &str| -> Vec<u32> {
        s.split('.').filter_map(|p| p.parse().ok()).collect()
    };
    let l = parse(local);
    let r = parse(remote);
    let len = l.len().max(r.len());
    for i in 0..len {
        let lv = l.get(i).copied().unwrap_or(0);
        let rv = r.get(i).copied().unwrap_or(0);
        if rv > lv {
            return true;
        }
        if rv < lv {
            return false;
        }
    }
    false
}

impl SpaceViewApp {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        let prefs = load_prefs();

        // Spawn background version check
        let (update_tx, update_rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let result = (|| -> Option<String> {
                let resp = ureq::get("https://api.github.com/repos/TrentSterling/SpaceView/releases/latest")
                    .set("User-Agent", &format!("SpaceView/{}", env!("CARGO_PKG_VERSION")))
                    .call()
                    .ok()?;
                let body = resp.into_string().ok()?;
                // Minimal JSON parsing: find "tag_name":"..."
                let marker = "\"tag_name\":";
                let idx = body.find(marker)?;
                let rest = &body[idx + marker.len()..];
                let rest = rest.trim_start();
                if !rest.starts_with('"') {
                    return None;
                }
                let rest = &rest[1..];
                let end = rest.find('"')?;
                let tag = &rest[..end];
                let version = tag.strip_prefix('v').unwrap_or(tag);
                if is_newer_version(env!("CARGO_PKG_VERSION"), version) {
                    Some(version.to_string())
                } else {
                    None
                }
            })();
            let _ = update_tx.send(result);
        });

        Self {
            scan_root: None,
            scanning: false,
            scan_progress: None,
            scan_receiver: None,
            snapshot_receiver: None,
            camera: Camera::new(egui::pos2(0.5, 0.5), 1.0),
            world_layout: None,
            last_viewport: egui::Rect::NOTHING,
            hovered_node_info: None,
            context_menu_info: None,
            is_dragging: false,
            depth_context: Vec::new(),
            root_name: String::new(),
            root_size: 0,
            root_file_count: 0,
            scan_path: None,
            show_free_space: true,
            last_time: 0.0,
            theme: ColorTheme::Rainbow,
            dark_mode: prefs.dark_mode,
            hide_about_on_start: prefs.hide_about,
            show_about: !prefs.hide_about,
            icon_texture: None,
            face_texture: None,
            update_check_receiver: Some(update_rx),
            latest_version: None,
            pending_delete: None,
            view_mode: ViewMode::Treemap,
            search_text: String::new(),
            list_sort: SortColumn::Size,
            list_sort_asc: false,
            list_path: Vec::new(),
            cached_largest: None,
            cached_extensions: None,
            cached_duplicates: None,
            dup_receiver: None,
            color_mode: ColorMode::Depth,
            time_range: (0, 0),
            ext_color_map: std::collections::HashMap::new(),
            last_window_outer_pos: None,
            last_window_inner_size: None,
        }
    }

    fn start_scan(&mut self, path: PathBuf) {
        if let Some(ref prog) = self.scan_progress {
            prog.cancel.store(true, Ordering::Relaxed);
        }

        // Deferred drops: move old data to background thread for deallocation
        let old_root = self.scan_root.take();
        let old_layout = self.world_layout.take();
        let old_largest = self.cached_largest.take();
        let old_extensions = self.cached_extensions.take();
        if old_root.is_some() || old_layout.is_some() {
            std::thread::spawn(move || {
                drop(old_root);
                drop(old_layout);
                drop(old_largest);
                drop(old_extensions);
            });
        }

        self.camera = Camera::new(egui::pos2(0.5, 0.5), 1.0);
        self.scanning = true;
        self.view_mode = ViewMode::Treemap;
        self.depth_context.clear();
        self.hovered_node_info = None;
        self.scan_path = Some(path.clone());
        self.list_path.clear();
        self.cached_duplicates = None;
        self.dup_receiver = None;

        let progress = Arc::new(ScanProgress::new());
        self.scan_progress = Some(progress.clone());

        let (tx, rx) = std::sync::mpsc::channel();
        self.scan_receiver = Some(rx);

        let (snapshot_tx, snapshot_rx) = std::sync::mpsc::channel();
        self.snapshot_receiver = Some(snapshot_rx);

        std::thread::spawn(move || {
            let result = scan_directory_live(&path, progress, snapshot_tx);
            let (largest, extensions, time_range) = if let Some(ref root) = result {
                // Compute time range on scan thread (not UI thread)
                let time_range = compute_time_range(root);

                // Collect all files once, derive both largest and extension stats
                let mut all_files: Vec<(String, u64, String)> = Vec::new();
                collect_all_files(root, &mut all_files);

                // Extension stats from all files
                let mut ext_map: std::collections::HashMap<String, (u64, u64)> = std::collections::HashMap::new();
                for (name, size, _) in &all_files {
                    let ext = name.rsplit('.').next()
                        .filter(|e| e.len() < 10 && *e != name.as_str())
                        .map(|e| format!(".{}", e.to_lowercase()))
                        .unwrap_or_else(|| "(no ext)".to_string());
                    let entry = ext_map.entry(ext).or_insert((0, 0));
                    entry.0 += size;
                    entry.1 += 1;
                }
                let mut ext_list: Vec<(String, u64, u64)> = ext_map.into_iter()
                    .map(|(ext, (size, count))| (ext, size, count))
                    .collect();
                ext_list.sort_by(|a, b| b.1.cmp(&a.1));

                // Largest 1000 files
                all_files.sort_by(|a, b| b.1.cmp(&a.1));
                all_files.truncate(1000);

                (Some(all_files), Some(ext_list), time_range)
            } else {
                (None, None, (0, 0))
            };
            let _ = tx.send((result, largest, extensions, time_range));
        });
    }

    fn build_layout(&mut self, viewport: egui::Rect) {
        if let Some(ref mut root) = self.scan_root {
            // Skip free space injection during live scanning (changes every frame)
            if !self.scanning && self.show_free_space {
                if let Some(ref path) = self.scan_path {
                    if let Some(free) = get_free_space(path) {
                        if free > 0 {
                            // Remove any previous free space node and its size
                            if let Some(old) = root.children.iter().find(|c| c.name == "<Free Space>") {
                                root.size -= old.size;
                            }
                            root.children.retain(|c| c.name != "<Free Space>");
                            root.children.push(FileNode {
                                name: "<Free Space>".to_string(),
                                path: PathBuf::new(),
                                size: free,
                                is_dir: false,
                                file_count: 0,
                                modified: 0,
                                children: Vec::new(),
                            });
                            root.size += free;
                            // Sort by size descending, but force free space to the end
                            // so the treemap places it in the bottom-right corner
                            root.children.sort_by(|a, b| {
                                let a_free = a.name == "<Free Space>";
                                let b_free = b.name == "<Free Space>";
                                if a_free && !b_free { return std::cmp::Ordering::Greater; }
                                if !a_free && b_free { return std::cmp::Ordering::Less; }
                                b.size.cmp(&a.size)
                            });
                        }
                    }
                }
            }

            let aspect = viewport.height() / viewport.width();
            let layout = WorldLayout::new(root, aspect);
            self.camera.reset(layout.world_rect);
            self.camera.set_world_rect(layout.world_rect);
            self.world_layout = Some(layout);
            self.root_name = root.name.clone();
            self.root_size = root.size;
            self.root_file_count = root.file_count;
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
            self.camera.set_world_rect(layout.world_rect);
            self.world_layout = Some(layout);

            // Scale the camera center Y proportionally
            self.camera.center.y *= y_ratio;
            self.camera.target_center.y *= y_ratio;
        }
    }

    fn current_prefs(&self) -> Prefs {
        Prefs {
            hide_about: self.hide_about_on_start,
            dark_mode: self.dark_mode,
            window_x: self.last_window_outer_pos.map(|p| p.x),
            window_y: self.last_window_outer_pos.map(|p| p.y),
            window_w: self.last_window_inner_size.map(|s| s.x),
            window_h: self.last_window_inner_size.map(|s| s.y),
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

fn load_image_from_png(ctx: &egui::Context, name: &str, png_data: &[u8]) -> egui::TextureHandle {
    let img = image::load_from_memory(png_data).expect("Failed to decode PNG");
    let rgba = img.to_rgba8();
    let (w, h) = rgba.dimensions();
    let color_image = egui::ColorImage::from_rgba_unmultiplied(
        [w as usize, h as usize],
        rgba.as_raw(),
    );
    ctx.load_texture(name, color_image, egui::TextureOptions::LINEAR)
}

impl eframe::App for SpaceViewApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Apply dark/light mode
        if self.dark_mode {
            ctx.set_visuals(egui::Visuals::dark());
        } else {
            ctx.set_visuals(egui::Visuals::light());
        }

        let now = ctx.input(|i| i.time);
        let dt = if self.last_time > 0.0 {
            (now - self.last_time) as f32
        } else {
            1.0 / 60.0
        };
        self.last_time = now;

        // Track window position for save-on-exit
        let vp_info = ctx.input(|i| i.viewport().clone());
        if let Some(outer) = vp_info.outer_rect {
            self.last_window_outer_pos = Some(outer.min);
        }
        if let Some(inner) = vp_info.inner_rect {
            self.last_window_inner_size = Some(inner.size());
        }

        // Handle drag-and-drop folders
        let dropped: Vec<_> = ctx.input(|i| {
            i.raw.dropped_files.iter()
                .filter_map(|f| f.path.clone())
                .collect()
        });
        if let Some(path) = dropped.into_iter().find(|p| p.is_dir()) {
            self.start_scan(path);
        }

        // Check for scan completion and live snapshots
        if self.scanning {
            // Drain live tree snapshots (keep only the newest)
            if let Some(ref rx) = self.snapshot_receiver {
                let mut latest = None;
                while let Ok(snapshot) = rx.try_recv() {
                    latest = Some(snapshot);
                }
                if let Some(tree) = latest {
                    self.scan_root = Some(tree);
                    self.world_layout = None; // Force layout rebuild
                }
            }

            // Check for final scan completion
            if let Some(ref rx) = self.scan_receiver {
                if let Ok((result, largest, extensions, time_range)) = rx.try_recv() {
                    self.time_range = time_range;
                    self.scan_root = result;
                    self.cached_largest = largest;
                    // Build extension color map (sorted by size, largest first)
                    self.ext_color_map.clear();
                    if let Some(ref exts) = extensions {
                        for (i, (ext, _, _)) in exts.iter().enumerate() {
                            self.ext_color_map.insert(ext.clone(), i);
                        }
                    }
                    self.cached_extensions = extensions;
                    self.scanning = false;
                    self.scan_receiver = None;
                    self.snapshot_receiver = None;
                    self.world_layout = None; // Force final layout rebuild

                    // Start background duplicate detection
                    self.cached_duplicates = None;
                    if let Some(ref root) = self.scan_root {
                        let root_clone = root.clone();
                        let (dup_tx, dup_rx) = std::sync::mpsc::channel();
                        self.dup_receiver = Some(dup_rx);
                        std::thread::spawn(move || {
                            let dups = find_duplicates(&root_clone);
                            let _ = dup_tx.send(dups);
                        });
                    }
                }
            }
            ctx.request_repaint();
        }

        // Check for duplicate detection result
        if let Some(ref rx) = self.dup_receiver {
            if let Ok(dups) = rx.try_recv() {
                self.cached_duplicates = Some(dups);
                self.dup_receiver = None;
            }
        }

        // Check for version update result
        if let Some(ref rx) = self.update_check_receiver {
            if let Ok(result) = rx.try_recv() {
                self.latest_version = result;
                self.update_check_receiver = None;
            }
        }

        // ---- About popup ----
        let mut escape_consumed = false;
        if self.show_about && ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.show_about = false;
            escape_consumed = true;
        }
        if self.show_about {
            // Lazy-load textures on first open
            if self.icon_texture.is_none() {
                self.icon_texture = Some(load_image_from_png(
                    ctx, "app_icon", include_bytes!("../assets/icon.png"),
                ));
            }
            if self.face_texture.is_none() {
                self.face_texture = Some(load_image_from_png(
                    ctx, "tront_face", include_bytes!("../assets/tront.png"),
                ));
            }

            let mut open = true;
            let icon_tex = self.icon_texture.clone();
            let face_tex = self.face_texture.clone();
            egui::Window::new("About SpaceView")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .open(&mut open)
                .show(ctx, |ui| {
                    ui.vertical_centered(|ui| {
                        // Icon at top
                        if let Some(ref tex) = icon_tex {
                            ui.image(egui::load::SizedTexture::new(tex.id(), egui::vec2(64.0, 64.0)));
                            ui.add_space(8.0);
                        }
                        ui.heading(format!("SpaceView v{}", VERSION));
                        ui.add_space(4.0);
                        ui.label("Disk space visualizer");
                        ui.add_space(4.0);

                        // Face next to author name
                        ui.horizontal(|ui| {
                            ui.add_space(ui.available_width() / 2.0 - 50.0);
                            if let Some(ref tex) = face_tex {
                                ui.image(egui::load::SizedTexture::new(tex.id(), egui::vec2(24.0, 24.0)));
                            }
                            ui.label("By tront");
                        });

                        ui.add_space(4.0);
                        ui.label("Built with Rust + egui");
                        ui.add_space(12.0);
                    });

                    // Update notification
                    if let Some(ref ver) = self.latest_version {
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            ui.label(format!("Update available: v{}", ver));
                            ui.hyperlink_to(
                                "Download",
                                "https://github.com/TrentSterling/SpaceView/releases/latest",
                            );
                        });
                        ui.add_space(4.0);
                    }

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
                    ui.separator();
                    ui.add_space(4.0);
                    let mut hide = self.hide_about_on_start;
                    if ui.checkbox(&mut hide, "Don't show on startup").changed() {
                        self.hide_about_on_start = hide;
                        save_prefs(&self.current_prefs());
                    }
                    ui.add_space(4.0);
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

        // ---- Delete confirmation dialog ----
        if self.pending_delete.is_some() {
            let path = self.pending_delete.clone().unwrap();
            let mut keep_open = true;
            egui::Window::new("Confirm Delete")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.label("Send to Recycle Bin?");
                    ui.add_space(4.0);
                    ui.label(egui::RichText::new(path.to_string_lossy().to_string()).monospace());
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.button("Delete").clicked() {
                            #[cfg(target_os = "windows")]
                            {
                                // Use PowerShell to send to recycle bin
                                let path_str = path.to_string_lossy().to_string();
                                let script = format!(
                                    "Add-Type -AssemblyName Microsoft.VisualBasic; [Microsoft.VisualBasic.FileIO.FileSystem]::DeleteFile('{}', 'OnlyErrorDialogs', 'SendToRecycleBin')",
                                    path_str.replace('\'', "''")
                                );
                                let _ = std::process::Command::new("powershell")
                                    .args(["-NoProfile", "-Command", &script])
                                    .spawn();
                            }
                            // Rescan after delete
                            if let Some(ref scan_path) = self.scan_path {
                                self.start_scan(scan_path.clone());
                            }
                            keep_open = false;
                        }
                        if ui.button("Cancel").clicked() {
                            keep_open = false;
                        }
                    });
                });
            if !keep_open {
                self.pending_delete = None;
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
                                " - {} ({}/sec)",
                                format_duration(elapsed),
                                format_count(rate as u64),
                            );
                        }
                        ui.label(text);
                    }
                    if let Some(ref prog) = self.scan_progress {
                        let is_paused = prog.paused.load(Ordering::Relaxed);
                        let pause_label = if is_paused { "Resume" } else { "Pause" };
                        if ui.button(pause_label).clicked() {
                            prog.paused.store(!is_paused, Ordering::Relaxed);
                        }
                    }
                    if ui.button("Cancel").clicked() {
                        if let Some(ref prog) = self.scan_progress {
                            prog.cancel.store(true, Ordering::Relaxed);
                        }
                    }
                }

                // Theme selector + dark/light toggle (show when not scanning or when we have live data)
                if !self.scanning || self.scan_root.is_some() {
                    ui.separator();
                    let current_label = self.theme.label();
                    egui::ComboBox::from_id_salt("theme_selector")
                        .selected_text(current_label)
                        .show_ui(ui, |ui| {
                            for &t in &THEMES {
                                ui.selectable_value(&mut self.theme, t, t.label());
                            }
                        });
                    let mode_label = if self.dark_mode { "Light" } else { "Dark" };
                    if ui.button(mode_label).clicked() {
                        self.dark_mode = !self.dark_mode;
                        save_prefs(&self.current_prefs());
                    }
                    // Color mode toggle (cycles Depth -> Age -> Extension -> Depth)
                    if self.scan_root.is_some() {
                        let color_label = match self.color_mode {
                            ColorMode::Depth => "Age Map",
                            ColorMode::Age => "By Type",
                            ColorMode::Extension => "Depth",
                        };
                        if ui.button(color_label).clicked() {
                            self.color_mode = match self.color_mode {
                                ColorMode::Depth => ColorMode::Age,
                                ColorMode::Age => ColorMode::Extension,
                                ColorMode::Extension => ColorMode::Depth,
                            };
                        }
                    }
                }

                // View mode tabs (only when scan is complete, since List/TopFiles need final data)
                if self.scan_root.is_some() && !self.scanning {
                    ui.separator();
                    ui.selectable_value(&mut self.view_mode, ViewMode::Treemap, "Map");
                    ui.selectable_value(&mut self.view_mode, ViewMode::List, "List");
                    ui.selectable_value(&mut self.view_mode, ViewMode::LargestFiles, "Top Files");
                    ui.selectable_value(&mut self.view_mode, ViewMode::Extensions, "Types");
                    let dup_label = if self.cached_duplicates.is_some() {
                        "Dupes"
                    } else if self.dup_receiver.is_some() {
                        "Dupes..."
                    } else {
                        "Dupes"
                    };
                    ui.selectable_value(&mut self.view_mode, ViewMode::Duplicates, dup_label);
                }

                // Right-aligned About button + Free Space toggle
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("About").clicked() {
                        self.show_about = !self.show_about;
                    }
                    if self.scan_root.is_some() && !self.scanning {
                        ui.add(egui::TextEdit::singleline(&mut self.search_text)
                            .hint_text("Search...")
                            .desired_width(120.0));
                    }
                    if self.scan_root.is_some() && !self.scanning {
                        let fs_label = if self.show_free_space { "Hide Free" } else { "Show Free" };
                        if ui.button(fs_label).clicked() {
                            self.show_free_space = !self.show_free_space;
                            // Remove free space node if hiding
                            if !self.show_free_space {
                                if let Some(ref mut root) = self.scan_root {
                                    if let Some(pos) = root.children.iter().position(|c| c.name == "<Free Space>") {
                                        let free_size = root.children[pos].size;
                                        root.children.remove(pos);
                                        root.size -= free_size;
                                    }
                                }
                            }
                            self.world_layout = None;
                        }
                    }
                });
            });

            // Breadcrumb bar
            if self.scan_root.is_some() {
                ui.horizontal(|ui| {
                    match self.view_mode {
                        ViewMode::Treemap => {
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
                            if self.camera.zoom > 1.5 {
                                ui.separator();
                                ui.label(format!("{:.0}x", self.camera.zoom));
                            }
                        }
                        ViewMode::List => {
                            let root_name = self.root_name.clone();
                            if self.list_path.is_empty() {
                                ui.strong(&root_name);
                            } else {
                                if ui.link(&root_name).clicked() {
                                    self.list_path.clear();
                                }
                            }
                            let path = self.list_path.clone();
                            let last_idx = path.len().saturating_sub(1);
                            for (i, segment) in path.iter().enumerate() {
                                ui.label(">");
                                if i < last_idx {
                                    if ui.link(segment).clicked() {
                                        self.list_path.truncate(i + 1);
                                    }
                                } else {
                                    ui.strong(segment);
                                }
                            }
                        }
                        ViewMode::LargestFiles => {
                            ui.strong(&self.root_name);
                            ui.label("> Largest Files");
                        }
                        ViewMode::Extensions => {
                            ui.strong(&self.root_name);
                            ui.label("> File Types");
                        }
                        ViewMode::Duplicates => {
                            ui.strong(&self.root_name);
                            ui.label("> Duplicate Files");
                        }
                    }
                });
            }
        });

        // ---- Status bar ----
        if self.scan_root.is_some() {
            egui::TopBottomPanel::bottom("status_bar").show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(format!(
                        "{}: {} ({} files)",
                        self.root_name,
                        format_size(self.root_size),
                        format_count(self.root_file_count),
                    ));

                    if let Some(ref info) = self.hovered_node_info {
                        ui.separator();
                        let pct = if self.root_size > 0 {
                            (info.size as f64 / self.root_size as f64) * 100.0
                        } else {
                            0.0
                        };
                        let icon = if info.is_dir { "D" } else { "F" };
                        if info.is_dir {
                            ui.label(format!(
                                "[{}] {} - {} ({:.1}%, {} files)",
                                icon,
                                info.name,
                                format_size(info.size),
                                pct,
                                format_count(info.file_count),
                            ));
                        } else {
                            ui.label(format!(
                                "[{}] {} - {} ({:.1}%)",
                                icon,
                                info.name,
                                format_size(info.size),
                                pct
                            ));
                        }
                    }

                    if self.color_mode == ColorMode::Age {
                        ui.separator();
                        ui.colored_label(egui::Color32::from_rgb(220, 60, 50), "Old");
                        ui.label("-");
                        ui.colored_label(egui::Color32::from_rgb(220, 220, 50), "Mid");
                        ui.label("-");
                        ui.colored_label(egui::Color32::from_rgb(60, 220, 80), "New");
                    }
                    if self.color_mode == ColorMode::Extension {
                        ui.separator();
                        ui.label("Color: by file type");
                    }
                });
            });
        }

        // ---- Central panel: treemap ----
        egui::CentralPanel::default().show(ctx, |ui| {
            if self.scan_root.is_none() && !self.scanning {
                // Welcome screen (always shows quickhelp)
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
                });
                return;
            }

            // If scanning but no data yet, show spinner
            if self.scanning && self.scan_root.is_none() {
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
            // If scanning with data, fall through to render the treemap live

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

            match self.view_mode {
            ViewMode::Treemap => {

            let has_layout = self.world_layout.is_some();
            if !has_layout {
                return;
            }

            // 1. Advance camera animation
            let camera_moving = self.camera.tick(dt, viewport);

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
                    self.camera.scroll_zoom(scroll_y / 120.0, world_focus, viewport);
                }
            }

            // Drag pan
            if response.dragged_by(egui::PointerButton::Primary) {
                self.is_dragging = true;
                let delta = response.drag_delta();
                // Convert screen delta to world delta
                let scale = self.camera.zoom * viewport.width();
                let world_delta = egui::vec2(delta.x / scale, delta.y / scale);
                self.camera.drag_pan(world_delta, viewport);
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

            // Right-click context menu or zoom out
            let right_clicked = ctx.input(|i| i.pointer.secondary_clicked());
            let key_zoom_out = ctx.input(|i| i.key_pressed(egui::Key::Backspace))
                || (!escape_consumed && ctx.input(|i| i.key_pressed(egui::Key::Escape)));

            // Show context menu on right-click over a hovered node
            let mut context_zoom_out = false;
            if right_clicked && self.hovered_node_info.is_some() {
                self.context_menu_info = self.hovered_node_info.clone();
            }

            if self.context_menu_info.is_some() {
                let info = self.context_menu_info.clone().unwrap();
                let menu_id = egui::Id::new("node_context_menu");
                if right_clicked && self.hovered_node_info.is_some() {
                    ui.memory_mut(|mem| mem.open_popup(menu_id));
                }
                egui::popup::popup_above_or_below_widget(
                    ui,
                    menu_id,
                    &response,
                    egui::AboveOrBelow::Below,
                    egui::PopupCloseBehavior::CloseOnClick,
                    |ui| {
                        ui.set_min_width(160.0);
                        ui.label(egui::RichText::new(&info.name).strong());
                        ui.label(format!("{} ({:.1}%)", format_size(info.size),
                            if self.root_size > 0 { info.size as f64 / self.root_size as f64 * 100.0 } else { 0.0 }));
                        ui.separator();
                        if info.is_dir && info.has_children {
                            if ui.button("Zoom In").clicked() {
                                self.camera.snap_to(info.world_rect, viewport);
                            }
                        }
                        if ui.button("Zoom Out").clicked() {
                            context_zoom_out = true;
                        }
                        ui.separator();
                        if ui.button("Open in Explorer").clicked() {
                            if let Some(ref root) = self.scan_root {
                                let path = find_path_for_node(root, &info.name, info.size);
                                if let Some(p) = path {
                                    let _ = std::process::Command::new("explorer")
                                        .arg("/select,")
                                        .arg(&p)
                                        .spawn();
                                }
                            }
                        }
                        if ui.button("Copy Path").clicked() {
                            if let Some(ref root) = self.scan_root {
                                let path = find_path_for_node(root, &info.name, info.size);
                                if let Some(p) = path {
                                    ctx.copy_text(p.to_string_lossy().to_string());
                                }
                            }
                        }
                        if info.name != "<Free Space>" {
                            ui.separator();
                            if ui.button("Delete to Recycle Bin").clicked() {
                                if let Some(ref root) = self.scan_root {
                                    let path = find_path_for_node(root, &info.name, info.size);
                                    if let Some(p) = path {
                                        self.pending_delete = Some(p);
                                    }
                                }
                            }
                        }
                    },
                );
                if !ui.memory(|mem| mem.is_popup_open(menu_id)) {
                    self.context_menu_info = None;
                }
            }

            let zoom_out = (right_clicked && self.hovered_node_info.is_none())
                || key_zoom_out || context_zoom_out;

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
                let budget = if self.camera.is_animating() { 32 } else { 8 };
                layout.expand_visible(root, &self.camera, viewport, budget);
                layout.maybe_prune(&self.camera, viewport);
            }

            // 4. Render
            let painter = ui.painter_at(viewport);
            let theme = self.theme;

            // Walk the layout tree and draw visible nodes
            if let Some(ref layout) = self.world_layout {
                render_nodes(&painter, &layout.root_nodes, &self.camera, viewport, theme, self.color_mode, self.time_range, &self.ext_color_map);
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

            // Rich tooltip on hover
            if let Some(ref info) = self.hovered_node_info {
                if response.hovered() {
                    let pct = if self.root_size > 0 {
                        (info.size as f64 / self.root_size as f64) * 100.0
                    } else { 0.0 };
                    let mut tip = format!("{}\n{} ({:.2}%)", info.name, format_size(info.size), pct);
                    if info.is_dir {
                        tip += &format!("\n{} files", format_count(info.file_count));
                    }
                    if let Some(ref root) = self.scan_root {
                        if let Some(p) = find_path_for_node(root, &info.name, info.size) {
                            tip += &format!("\n{}", p.to_string_lossy());
                        }
                    }
                    response.clone().on_hover_text(tip);
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

            // 8. Zoom minimap (bottom-right corner when zoomed in)
            if self.camera.zoom > 1.5 {
                if let Some(ref layout) = self.world_layout {
                    let mini_w = 180.0f32;
                    let world_aspect = layout.world_rect.height() / layout.world_rect.width();
                    let mini_h = mini_w * world_aspect;
                    let margin = 8.0;
                    let mini_rect = egui::Rect::from_min_size(
                        egui::pos2(viewport.max.x - mini_w - margin, viewport.max.y - mini_h - margin),
                        egui::vec2(mini_w, mini_h),
                    );

                    // Background
                    painter.rect_filled(mini_rect, 4.0, egui::Color32::from_rgba_premultiplied(20, 20, 20, 200));

                    // Render simplified treemap into minimap
                    let mini_camera = Camera::new(
                        egui::pos2(
                            layout.world_rect.center().x,
                            layout.world_rect.center().y,
                        ),
                        1.0,
                    );
                    render_minimap_nodes(&painter, &layout.root_nodes, &mini_camera, mini_rect, theme);

                    // Draw viewport indicator
                    let vp_world_min = self.camera.screen_to_world(viewport.min, viewport);
                    let vp_world_max = self.camera.screen_to_world(viewport.max, viewport);
                    let to_mini = |world_pos: egui::Pos2| -> egui::Pos2 {
                        let nx = (world_pos.x - layout.world_rect.min.x) / layout.world_rect.width();
                        let ny = (world_pos.y - layout.world_rect.min.y) / layout.world_rect.height();
                        egui::pos2(
                            mini_rect.min.x + nx * mini_rect.width(),
                            mini_rect.min.y + ny * mini_rect.height(),
                        )
                    };
                    let vp_mini = egui::Rect::from_min_max(
                        to_mini(vp_world_min),
                        to_mini(vp_world_max),
                    ).intersect(mini_rect);
                    painter.rect_stroke(
                        vp_mini, 0.0,
                        egui::Stroke::new(1.5, egui::Color32::WHITE),
                        egui::StrokeKind::Outside,
                    );

                    // Border
                    painter.rect_stroke(
                        mini_rect, 4.0,
                        egui::Stroke::new(1.0, egui::Color32::from_gray(80)),
                        egui::StrokeKind::Outside,
                    );
                }
            }

            // 9. Request repaint if camera is moving
            if camera_moving {
                ctx.request_repaint();
            }

            } // ViewMode::Treemap

            ViewMode::List => {
                if let Some(ref root) = self.scan_root {
                    let current_dir = if self.list_path.is_empty() {
                        root
                    } else {
                        find_dir_by_path(root, &self.list_path).unwrap_or(root)
                    };
                    let parent_size = current_dir.size.max(1);
                    let depth = self.list_path.len() + 1;
                    let theme = self.theme;

                    // Collect entries as owned data (avoids borrow issues)
                    let mut entries: Vec<(String, u64, u64, bool, bool, PathBuf)> = current_dir.children.iter()
                        .map(|c| (c.name.clone(), c.size, c.file_count, c.is_dir, !c.children.is_empty(), c.path.clone()))
                        .collect();

                    // Search filter
                    if !self.search_text.is_empty() {
                        let q = self.search_text.to_lowercase();
                        entries.retain(|e| e.0.to_lowercase().contains(&q));
                    }

                    // Sort
                    match self.list_sort {
                        SortColumn::Name => {
                            entries.sort_by(|a, b| {
                                let dir_order = b.3.cmp(&a.3); // dirs first
                                if dir_order != std::cmp::Ordering::Equal { return dir_order; }
                                let cmp = a.0.to_lowercase().cmp(&b.0.to_lowercase());
                                if self.list_sort_asc { cmp } else { cmp.reverse() }
                            });
                        }
                        SortColumn::Size => {
                            entries.sort_by(|a, b| {
                                let cmp = b.1.cmp(&a.1);
                                if self.list_sort_asc { cmp.reverse() } else { cmp }
                            });
                        }
                        SortColumn::FileCount => {
                            entries.sort_by(|a, b| {
                                let cmp = b.2.cmp(&a.2);
                                if self.list_sort_asc { cmp.reverse() } else { cmp }
                            });
                        }
                    }

                    // Column headers (pre-compute arrows to avoid borrow conflict)
                    let arrow = |col: SortColumn| -> &str {
                        if self.list_sort == col {
                            if self.list_sort_asc { " ^" } else { " v" }
                        } else { "" }
                    };
                    let name_arrow = arrow(SortColumn::Name).to_string();
                    let size_arrow = arrow(SortColumn::Size).to_string();
                    let fc_arrow = arrow(SortColumn::FileCount).to_string();
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        let w = ui.available_width();
                        if ui.add_sized([w * 0.50, 18.0], egui::SelectableLabel::new(false,
                            format!("Name{}", name_arrow))).clicked() {
                            if self.list_sort == SortColumn::Name { self.list_sort_asc = !self.list_sort_asc; }
                            else { self.list_sort = SortColumn::Name; self.list_sort_asc = true; }
                        }
                        if ui.add_sized([w * 0.20, 18.0], egui::SelectableLabel::new(false,
                            format!("Size{}", size_arrow))).clicked() {
                            if self.list_sort == SortColumn::Size { self.list_sort_asc = !self.list_sort_asc; }
                            else { self.list_sort = SortColumn::Size; self.list_sort_asc = false; }
                        }
                        ui.add_sized([w * 0.10, 18.0], egui::Label::new("%"));
                        if ui.add_sized([w * 0.15, 18.0], egui::SelectableLabel::new(false,
                            format!("Files{}", fc_arrow))).clicked() {
                            if self.list_sort == SortColumn::FileCount { self.list_sort_asc = !self.list_sort_asc; }
                            else { self.list_sort = SortColumn::FileCount; self.list_sort_asc = false; }
                        }
                    });
                    ui.separator();

                    let mut nav_target: Option<String> = None;
                    let list_action: std::cell::Cell<Option<(usize, u8)>> = std::cell::Cell::new(None);

                    // ".." entry (outside virtual scroll)
                    if !self.list_path.is_empty() {
                        if ui.selectable_label(false, "  ..").double_clicked() {
                            nav_target = Some("..".to_string());
                        }
                    }

                    if entries.is_empty() && !self.search_text.is_empty() {
                        ui.label("No matching items.");
                    } else {
                        let row_h = 22.0;
                        egui::ScrollArea::vertical().auto_shrink(false).show_rows(
                            ui, row_h, entries.len(), |ui, row_range| {
                            for i in row_range {
                                let (name, size, file_count, is_dir, has_children, _path) = &entries[i];
                                let pct = (*size as f64 / parent_size as f64) * 100.0;
                                let (r, g, b) = if *name == "<Free Space>" {
                                    (60u8, 140u8, 60u8)
                                } else {
                                    theme.base_rgb(depth)
                                };
                                let icon_col = egui::Color32::from_rgb(r, g, b);
                                let icon = if *is_dir { "D" } else { "F" };

                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = 4.0;
                                    let w = ui.available_width();

                                    let name_text = format!("[{}] {}", icon, name);
                                    let label = if *is_dir {
                                        egui::RichText::new(&name_text).strong().color(icon_col)
                                    } else {
                                        egui::RichText::new(&name_text)
                                    };
                                    let resp = ui.add_sized([w * 0.50, 18.0],
                                        egui::SelectableLabel::new(false, label));
                                    if resp.double_clicked() && *is_dir && *has_children {
                                        nav_target = Some(name.clone());
                                    }
                                    resp.context_menu(|ui| {
                                        ui.label(egui::RichText::new(name).strong());
                                        ui.label(format!("{} ({:.1}%)", format_size(*size), pct));
                                        ui.separator();
                                        if ui.button("Open in Explorer").clicked() {
                                            list_action.set(Some((i, 0)));
                                            ui.close_menu();
                                        }
                                        if ui.button("Copy Path").clicked() {
                                            list_action.set(Some((i, 1)));
                                            ui.close_menu();
                                        }
                                        if *name != "<Free Space>" {
                                            ui.separator();
                                            if ui.button("Delete to Recycle Bin").clicked() {
                                                list_action.set(Some((i, 2)));
                                                ui.close_menu();
                                            }
                                        }
                                    });

                                    ui.add_sized([w * 0.20, 18.0], egui::Label::new(format_size(*size)));
                                    ui.add_sized([w * 0.10, 18.0], egui::Label::new(format!("{:.1}%", pct)));
                                    let fc = if *is_dir { format_count(*file_count) } else { String::new() };
                                    ui.add_sized([w * 0.15, 18.0], egui::Label::new(fc));
                                });
                            }
                        });
                    }

                    // Handle navigation
                    if let Some(ref target) = nav_target {
                        if target == ".." {
                            self.list_path.pop();
                        } else {
                            self.list_path.push(target.clone());
                        }
                    }
                    // Handle context menu actions
                    if let Some((idx, action)) = list_action.get() {
                        let path = &entries[idx].5;
                        match action {
                            0 => { // Open in Explorer
                                let _ = std::process::Command::new("explorer")
                                    .arg("/select,")
                                    .arg(path)
                                    .spawn();
                            }
                            1 => { // Copy Path
                                ctx.copy_text(path.to_string_lossy().to_string());
                            }
                            2 => { // Delete to Recycle Bin
                                self.pending_delete = Some(path.clone());
                            }
                            _ => {}
                        }
                    }
                }
            }

            ViewMode::LargestFiles => {
                // Data is pre-collected during scan (no freeze on tab click)
                if let Some(ref files) = self.cached_largest {
                    let total_size = self.root_size.max(1);
                    let theme = self.theme;
                    {
                    let mut filtered: Vec<(usize, &(String, u64, String))> = files.iter().enumerate().collect();
                    if !self.search_text.is_empty() {
                        let q = self.search_text.to_lowercase();
                        filtered.retain(|(_, f)| f.0.to_lowercase().contains(&q) || f.2.to_lowercase().contains(&q));
                    }

                    // Column headers
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        let w = ui.available_width();
                        ui.add_sized([w * 0.04, 18.0], egui::Label::new("#"));
                        ui.add_sized([w * 0.28, 18.0], egui::Label::new("Name"));
                        ui.add_sized([w * 0.38, 18.0], egui::Label::new("Path"));
                        ui.add_sized([w * 0.15, 18.0], egui::Label::new("Size"));
                        ui.add_sized([w * 0.10, 18.0], egui::Label::new("%"));
                    });
                    ui.separator();

                    if filtered.is_empty() && !self.search_text.is_empty() {
                        ui.label("No matching files.");
                    } else {
                        let row_h = 22.0;
                        egui::ScrollArea::vertical().auto_shrink(false).show_rows(
                            ui, row_h, filtered.len(), |ui, row_range| {
                            for rank in row_range {
                                let (_, entry) = &filtered[rank];
                                let pct = (entry.1 as f64 / total_size as f64) * 100.0;
                                let ci = rank % 20;
                                let (r, g, b) = theme.base_rgb(ci);

                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = 4.0;
                                    let w = ui.available_width();
                                    ui.add_sized([w * 0.04, 18.0], egui::Label::new(
                                        egui::RichText::new(format!("{}", rank + 1)).weak()));
                                    ui.add_sized([w * 0.28, 18.0], egui::Label::new(
                                        egui::RichText::new(&entry.0).color(egui::Color32::from_rgb(r, g, b))));
                                    ui.add_sized([w * 0.38, 18.0], egui::Label::new(
                                        egui::RichText::new(&entry.2).weak()));
                                    ui.add_sized([w * 0.15, 18.0], egui::Label::new(format_size(entry.1)));
                                    ui.add_sized([w * 0.10, 18.0], egui::Label::new(format!("{:.1}%", pct)));
                                });
                            }
                        });
                    }
                }
                } // else if cached_largest
            }

            ViewMode::Extensions => {
                if let Some(ref ext_data) = self.cached_extensions {
                    let total_size = self.root_size.max(1);
                    let theme = self.theme;

                    let mut filtered: Vec<&(String, u64, u64)> = ext_data.iter().collect();
                    if !self.search_text.is_empty() {
                        let q = self.search_text.to_lowercase();
                        filtered.retain(|e| e.0.to_lowercase().contains(&q));
                    }

                    if filtered.is_empty() {
                        ui.label("No matching file types.");
                    } else {
                        // Render as a treemap of extensions
                        let ext_rect = ui.available_rect_before_wrap();
                        let painter = ui.painter_at(ext_rect);
                        let _response = ui.allocate_rect(ext_rect, egui::Sense::hover());

                        let sizes: Vec<f64> = filtered.iter().map(|e| e.1 as f64).collect();
                        let rects = treemap::layout(
                            ext_rect.min.x, ext_rect.min.y,
                            ext_rect.width(), ext_rect.height(),
                            &sizes,
                        );

                        for tr in &rects {
                            let ext = &filtered[tr.index];
                            let rect = egui::Rect::from_min_size(
                                egui::pos2(tr.x, tr.y),
                                egui::vec2(tr.w, tr.h),
                            );
                            let inner = rect.shrink(1.0);
                            let ci = tr.index;
                            let (r, g, b) = theme.base_rgb(ci);
                            let col = egui::Color32::from_rgb(r, g, b);
                            painter.rect_filled(inner, 2.0, col);

                            // Draw text if block is big enough
                            if inner.width() > 40.0 && inner.height() > 18.0 {
                                let text_clip = inner.intersect(ext_rect);
                                let text_painter = painter.with_clip_rect(text_clip);
                                let text_col = text_color_for(col);
                                let pct = (ext.1 as f64 / total_size as f64) * 100.0;

                                // Extension name
                                let font_size = (inner.height() * 0.3).clamp(11.0, 24.0);
                                let max_chars = ((inner.width() - 6.0) / (font_size * 0.55)) as usize;
                                let label = truncate_str(&ext.0, max_chars);
                                text_painter.text(
                                    inner.min + egui::vec2(4.0, 4.0),
                                    egui::Align2::LEFT_TOP,
                                    label,
                                    egui::FontId::proportional(font_size),
                                    text_col,
                                );

                                // Size and count
                                if inner.height() > 36.0 {
                                    let info = format!("{} ({:.1}%, {} files)",
                                        format_size(ext.1), pct, format_count(ext.2));
                                    let info_size = (font_size * 0.7).clamp(9.0, 14.0);
                                    text_painter.text(
                                        inner.min + egui::vec2(4.0, font_size + 6.0),
                                        egui::Align2::LEFT_TOP,
                                        info,
                                        egui::FontId::proportional(info_size),
                                        text_col.gamma_multiply(0.7),
                                    );
                                }
                            }
                        }
                    }
                }
            }

            ViewMode::Duplicates => {
                if self.dup_receiver.is_some() && self.cached_duplicates.is_none() {
                    ui.vertical_centered(|ui| {
                        ui.add_space(ui.available_height() / 3.0);
                        ui.heading("Analyzing duplicates...");
                        ui.spinner();
                    });
                } else if let Some(ref dups) = self.cached_duplicates {
                    let total_waste: u64 = dups.iter()
                        .map(|g| g.size * (g.paths.len() as u64 - 1))
                        .sum();
                    let total_groups = dups.len();

                    // Summary header
                    ui.horizontal(|ui| {
                        ui.label(format!(
                            "{} duplicate groups. {} wasted.",
                            format_count(total_groups as u64),
                            format_size(total_waste),
                        ));
                    });
                    ui.separator();

                    let mut filtered: Vec<&DuplicateGroup> = dups.iter().collect();
                    if !self.search_text.is_empty() {
                        let q = self.search_text.to_lowercase();
                        filtered.retain(|g| g.paths.iter().any(|p| p.to_lowercase().contains(&q)));
                    }

                    if filtered.is_empty() && !self.search_text.is_empty() {
                        ui.label("No matching duplicates.");
                    } else {
                        egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                            for (gi, group) in filtered.iter().enumerate() {
                                let waste = group.size * (group.paths.len() as u64 - 1);
                                let ci = gi % 20;
                                let (r, g, b) = self.theme.base_rgb(ci);
                                let col = egui::Color32::from_rgb(r, g, b);

                                ui.horizontal(|ui| {
                                    ui.colored_label(col, format!(
                                        "{} x {} (wastes {})",
                                        group.paths.len(),
                                        format_size(group.size),
                                        format_size(waste),
                                    ));
                                });

                                for path in &group.paths {
                                    ui.horizontal(|ui| {
                                        ui.add_space(16.0);
                                        let resp = ui.add(egui::Label::new(
                                            egui::RichText::new(path).weak()
                                        ).sense(egui::Sense::click()));
                                        resp.context_menu(|ui| {
                                            if ui.button("Open in Explorer").clicked() {
                                                let _ = std::process::Command::new("explorer")
                                                    .arg("/select,")
                                                    .arg(path)
                                                    .spawn();
                                                ui.close_menu();
                                            }
                                            if ui.button("Copy Path").clicked() {
                                                ctx.copy_text(path.clone());
                                                ui.close_menu();
                                            }
                                            if ui.button("Delete to Recycle Bin").clicked() {
                                                self.pending_delete = Some(PathBuf::from(path));
                                                ui.close_menu();
                                            }
                                        });
                                    });
                                }
                                ui.add_space(4.0);
                                ui.separator();
                            }
                        });
                    }
                } else {
                    ui.label("No duplicate data available. Scan a drive first.");
                }
            }

            } // match self.view_mode
        });
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        save_prefs(&self.current_prefs());
    }
}

// ===================== Rendering =====================
//
// Screen-space rendering pipeline (v0.5.0):
//   Children are positioned at render time via treemap::layout in screen pixels.
//   Fixed 16px headers, 3px padding, 1.5px border. No proportional world-space mismatch.
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
    color_mode: ColorMode,
    time_range: (u64, u64),
    ext_colors: &std::collections::HashMap<String, usize>,
) {
    for node in nodes {
        let screen_rect = camera.world_to_screen(node.world_rect, viewport);
        render_node(painter, node, screen_rect, viewport, theme, color_mode, time_range, ext_colors);
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
    color_mode: ColorMode,
    time_range: (u64, u64),
    ext_colors: &std::collections::HashMap<String, usize>,
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

        // Phase 1: body fill + border stroke
        let col = match color_mode {
            ColorMode::Depth | ColorMode::Extension => body_color(node.color_index, theme),
            ColorMode::Age => age_body_color(node.modified, time_range),
        };
        painter.rect_filled(inner, 1.0, col);
        painter.rect_stroke(inner, 1.0, egui::Stroke::new(1.0, egui::Color32::from_gray(30)), egui::StrokeKind::Outside);

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
                    render_node(painter, &node.children[tr.index], child_rect, viewport, theme, color_mode, time_range, ext_colors);
                }
            }
        }

        // Phase 3: header ON TOP of children
        if inner.height() >= 12.0 && inner.width() >= 8.0 {
            let header = egui::Rect::from_min_size(inner.min, egui::vec2(inner.width(), hh));
            let clipped = header.intersect(viewport);
            if clipped.width() > 0.0 && clipped.height() > 0.0 {
                let hdr_col = match color_mode {
                    ColorMode::Depth | ColorMode::Extension => header_color(node.color_index, theme),
                    ColorMode::Age => age_header_color(node.modified, time_range),
                };
                painter.rect_filled(clipped, 1.0, hdr_col);

                if hh >= 14.0 && inner.width() > 30.0 {
                    let text_painter = painter.with_clip_rect(clipped);
                    let font_size = (hh - 4.0).clamp(9.0, 13.0);
                    let size_text = if node.file_count > 0 && inner.width() > 180.0 {
                        format!("{} ({})", format_size(node.size), format_count(node.file_count))
                    } else {
                        format_size(node.size)
                    };
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
                        text_color_for(hdr_col),
                    );
                    if show_size {
                        text_painter.text(
                            egui::pos2(clipped.max.x - 3.0, clipped.min.y + 1.0),
                            egui::Align2::RIGHT_TOP,
                            size_text,
                            egui::FontId::proportional(font_size - 1.0),
                            text_color_for(hdr_col).gamma_multiply(0.6),
                        );
                    }
                }
            }
        }
    } else {
        // Files / empty dirs: single pass
        let inner = screen_rect.shrink(1.0);
        let is_free_space = node.name == "<Free Space>";
        let col = if is_free_space {
            egui::Color32::from_rgb(60, 140, 60)
        } else {
            match color_mode {
                ColorMode::Depth => {
                    if node.is_dir { dir_color(node.color_index, theme) }
                    else { file_color(node.color_index, theme) }
                }
                ColorMode::Age => age_color(node.modified, time_range),
                ColorMode::Extension => {
                    if node.is_dir { dir_color(node.color_index, theme) }
                    else { ext_file_color(&node.name, ext_colors, theme) }
                }
            }
        };
        painter.rect_filled(inner, 1.0, col);

        // Cushion shading: darken edges for 3D effect
        if inner.width() > 6.0 && inner.height() > 6.0 {
            draw_cushion(painter, inner);
        }

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

// ===================== Minimap Rendering =====================

/// Simplified treemap render for the minimap. Just colored blocks, no text.
fn render_minimap_nodes(
    painter: &egui::Painter,
    nodes: &[LayoutNode],
    camera: &Camera,
    viewport: egui::Rect,
    theme: ColorTheme,
) {
    for node in nodes {
        let screen_rect = camera.world_to_screen(node.world_rect, viewport);
        render_minimap_node(painter, node, screen_rect, viewport, theme);
    }
}

fn render_minimap_node(
    painter: &egui::Painter,
    node: &LayoutNode,
    screen_rect: egui::Rect,
    viewport: egui::Rect,
    theme: ColorTheme,
) {
    if !screen_rect.intersects(viewport) { return; }
    if screen_rect.width() < 1.0 || screen_rect.height() < 1.0 { return; }

    if node.is_dir && node.has_children && node.children_expanded && !node.children.is_empty() {
        // Just recurse into children
        let inner = screen_rect.shrink(0.5);
        let sizes: Vec<f64> = node.children.iter().map(|c| c.size as f64).collect();
        let rects = treemap::layout(inner.min.x, inner.min.y, inner.width(), inner.height(), &sizes);
        for tr in &rects {
            let child_rect = egui::Rect::from_min_size(
                egui::pos2(tr.x, tr.y), egui::vec2(tr.w, tr.h),
            );
            render_minimap_node(painter, &node.children[tr.index], child_rect, viewport, theme);
        }
    } else {
        // Leaf or unexpanded: solid color block
        let col = if node.name == "<Free Space>" {
            egui::Color32::from_rgb(60, 140, 60)
        } else {
            let (r, g, b) = theme.base_rgb(node.color_index);
            egui::Color32::from_rgb(r, g, b)
        };
        painter.rect_filled(screen_rect, 0.0, col);
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
        file_count: node.file_count,
        is_dir: node.is_dir,
        world_rect: node.world_rect,
        has_children: node.has_children,
        screen_rect,
    })
}

// ===================== Tree Helpers =====================

fn find_dir_by_path<'a>(root: &'a FileNode, path: &[String]) -> Option<&'a FileNode> {
    let mut current = root;
    for segment in path {
        current = current.children.iter().find(|c| c.name == *segment && c.is_dir)?;
    }
    Some(current)
}

/// Compute (min, max) modified timestamps across all files in the tree.
fn compute_time_range(node: &FileNode) -> (u64, u64) {
    let mut min_t = u64::MAX;
    let mut max_t = 0u64;
    compute_time_range_recursive(node, &mut min_t, &mut max_t);
    if min_t == u64::MAX { min_t = 0; }
    (min_t, max_t)
}

fn compute_time_range_recursive(node: &FileNode, min_t: &mut u64, max_t: &mut u64) {
    if !node.is_dir && node.modified > 0 && node.name != "<Free Space>" {
        if node.modified < *min_t { *min_t = node.modified; }
        if node.modified > *max_t { *max_t = node.modified; }
    }
    for child in &node.children {
        compute_time_range_recursive(child, min_t, max_t);
    }
}

/// Tiered duplicate detection: group by size, then partial hash (first 4KB), then full hash.
fn find_duplicates(root: &FileNode) -> Vec<DuplicateGroup> {
    use std::collections::HashMap;

    // Step 1: Collect all files with paths, grouped by size
    let mut by_size: HashMap<u64, Vec<String>> = HashMap::new();
    collect_file_paths(root, &mut by_size);

    // Filter to sizes with 2+ files (potential duplicates). Skip tiny files.
    let candidates: Vec<(u64, Vec<String>)> = by_size.into_iter()
        .filter(|(size, paths)| paths.len() >= 2 && *size >= 1024)
        .collect();

    // Step 2: For each size group, hash first 4KB
    let mut results: Vec<DuplicateGroup> = Vec::new();

    for (size, paths) in candidates {
        let mut by_partial: HashMap<u64, Vec<String>> = HashMap::new();
        for path in &paths {
            if let Ok(hash) = hash_file_partial(path) {
                by_partial.entry(hash).or_default().push(path.clone());
            }
        }

        // Step 3: For partial-hash matches with 2+ files, do full hash
        for (_phash, partial_group) in by_partial {
            if partial_group.len() < 2 {
                continue;
            }
            // For small files (<=4KB), partial hash IS the full hash
            if size <= 4096 {
                results.push(DuplicateGroup { size, paths: partial_group });
                continue;
            }

            let mut by_full: HashMap<u64, Vec<String>> = HashMap::new();
            for path in &partial_group {
                if let Ok(hash) = hash_file_full(path) {
                    by_full.entry(hash).or_default().push(path.clone());
                }
            }
            for (_fhash, full_group) in by_full {
                if full_group.len() >= 2 {
                    results.push(DuplicateGroup { size, paths: full_group });
                }
            }
        }
    }

    // Sort by wasted space (size * (count-1)) descending
    results.sort_by(|a, b| {
        let waste_a = a.size * (a.paths.len() as u64 - 1);
        let waste_b = b.size * (b.paths.len() as u64 - 1);
        waste_b.cmp(&waste_a)
    });

    results
}

fn collect_file_paths(node: &FileNode, by_size: &mut std::collections::HashMap<u64, Vec<String>>) {
    for child in &node.children {
        if child.is_dir {
            collect_file_paths(child, by_size);
        } else if child.name != "<Free Space>" && child.size > 0 {
            by_size.entry(child.size).or_default()
                .push(child.path.to_string_lossy().to_string());
        }
    }
}

fn hash_file_partial(path: &str) -> std::io::Result<u64> {
    use std::hash::{Hash, Hasher};
    let mut file = std::fs::File::open(path)?;
    let mut buf = [0u8; 4096];
    let n = std::io::Read::read(&mut file, &mut buf)?;
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    buf[..n].hash(&mut hasher);
    Ok(hasher.finish())
}

fn hash_file_full(path: &str) -> std::io::Result<u64> {
    use std::hash::{Hash, Hasher};
    let mut file = std::fs::File::open(path)?;
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    let mut buf = [0u8; 8192];
    loop {
        let n = std::io::Read::read(&mut file, &mut buf)?;
        if n == 0 { break; }
        buf[..n].hash(&mut hasher);
    }
    Ok(hasher.finish())
}

fn collect_all_files(node: &FileNode, files: &mut Vec<(String, u64, String)>) {
    for child in &node.children {
        if child.is_dir {
            collect_all_files(child, files);
        } else if child.name != "<Free Space>" {
            files.push((child.name.clone(), child.size, child.path.to_string_lossy().to_string()));
        }
    }
}

// ===================== Colors =====================

fn dir_color(ci: usize, theme: ColorTheme) -> egui::Color32 {
    let (r, g, b) = theme.base_rgb(ci);
    egui::Color32::from_rgb(r, g, b)
}

fn file_color(ci: usize, theme: ColorTheme) -> egui::Color32 {
    let (r, g, b) = theme.base_rgb(ci);
    egui::Color32::from_rgb(r, g, b)
}

fn header_color(ci: usize, theme: ColorTheme) -> egui::Color32 {
    let (r, g, b) = theme.base_rgb(ci);
    let darken = |c: u8| (c as f32 * 0.80) as u8;
    egui::Color32::from_rgb(darken(r), darken(g), darken(b))
}

fn body_color(ci: usize, theme: ColorTheme) -> egui::Color32 {
    let (r, g, b) = theme.base_rgb(ci);
    let dim = |c: u8| (c as f32 * 0.35) as u8;
    egui::Color32::from_rgb(dim(r), dim(g), dim(b))
}

/// Get the color index for a file based on its extension.
fn ext_color_index(name: &str, ext_colors: &std::collections::HashMap<String, usize>) -> Option<usize> {
    let ext = name.rsplit('.').next()
        .filter(|e| e.len() < 10 && *e != name)
        .map(|e| format!(".{}", e.to_lowercase()))
        .unwrap_or_else(|| "(no ext)".to_string());
    ext_colors.get(&ext).copied()
}

/// File color for extension mode. Uses theme colors indexed by extension rank.
fn ext_file_color(name: &str, ext_colors: &std::collections::HashMap<String, usize>, theme: ColorTheme) -> egui::Color32 {
    if let Some(ci) = ext_color_index(name, ext_colors) {
        let (r, g, b) = theme.base_rgb(ci);
        egui::Color32::from_rgb(r, g, b)
    } else {
        egui::Color32::from_rgb(128, 128, 128)
    }
}

/// Map a file's modified timestamp to a red-to-green gradient.
/// Old files = red/warm. Recent files = green/cool.
fn age_color(modified: u64, time_range: (u64, u64)) -> egui::Color32 {
    if modified == 0 || time_range.0 >= time_range.1 {
        return egui::Color32::from_rgb(128, 128, 128); // unknown = gray
    }
    // Log scale: spreads out recent files instead of clustering at green.
    // age_secs = how old this file is (0 = newest). Log compresses the old end
    // and expands the new end, so "1 week ago" vs "1 month ago" is visible
    // even when the oldest file is 15 years old.
    let age_secs = (time_range.1 - modified) as f64;
    let max_age = (time_range.1 - time_range.0) as f64;
    let t = 1.0 - (age_secs + 1.0).ln() / (max_age + 1.0).ln();
    let t = t.clamp(0.0, 1.0) as f32;
    // Red (old) -> Yellow (mid) -> Green (new)
    let (r, g, b) = if t < 0.5 {
        // Red to Yellow
        let s = t * 2.0;
        (220.0, 60.0 + 160.0 * s, 50.0)
    } else {
        // Yellow to Green
        let s = (t - 0.5) * 2.0;
        (220.0 - 160.0 * s, 220.0, 50.0 + 30.0 * s)
    };
    egui::Color32::from_rgb(r as u8, g as u8, b as u8)
}

/// Darker version of age color for directory bodies.
fn age_body_color(modified: u64, time_range: (u64, u64)) -> egui::Color32 {
    let col = age_color(modified, time_range);
    let dim = |c: u8| (c as f32 * 0.35) as u8;
    egui::Color32::from_rgb(dim(col.r()), dim(col.g()), dim(col.b()))
}

/// Header version of age color.
fn age_header_color(modified: u64, time_range: (u64, u64)) -> egui::Color32 {
    let col = age_color(modified, time_range);
    let darken = |c: u8| (c as f32 * 0.80) as u8;
    egui::Color32::from_rgb(darken(col.r()), darken(col.g()), darken(col.b()))
}

/// Draw cushion shading: darken edges to create a 3D raised effect.
fn draw_cushion(painter: &egui::Painter, rect: egui::Rect) {
    let w = (rect.width() * 0.15).min(6.0).max(1.0);
    let h = (rect.height() * 0.15).min(6.0).max(1.0);
    let dark = egui::Color32::from_rgba_premultiplied(0, 0, 0, 30);
    let light = egui::Color32::from_rgba_premultiplied(255, 255, 255, 18);

    // Top highlight
    painter.rect_filled(
        egui::Rect::from_min_max(rect.min, egui::pos2(rect.max.x, rect.min.y + h)),
        0.0, light,
    );
    // Left highlight
    painter.rect_filled(
        egui::Rect::from_min_max(egui::pos2(rect.min.x, rect.min.y + h), egui::pos2(rect.min.x + w, rect.max.y)),
        0.0, light,
    );
    // Bottom shadow
    painter.rect_filled(
        egui::Rect::from_min_max(egui::pos2(rect.min.x, rect.max.y - h), rect.max),
        0.0, dark,
    );
    // Right shadow
    painter.rect_filled(
        egui::Rect::from_min_max(egui::pos2(rect.max.x - w, rect.min.y), egui::pos2(rect.max.x, rect.max.y - h)),
        0.0, dark,
    );
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

/// Find the path of a node by name and size in the file tree.
fn find_path_for_node(root: &FileNode, name: &str, size: u64) -> Option<PathBuf> {
    if root.name == name && root.size == size {
        return Some(root.path.clone());
    }
    for child in &root.children {
        if let Some(p) = find_path_for_node(child, name, size) {
            return Some(p);
        }
    }
    None
}
