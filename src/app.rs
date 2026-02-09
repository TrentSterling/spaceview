use crate::camera::Camera;
use crate::scanner::{FileNode, ScanProgress, get_free_space, scan_directory};
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

struct Prefs {
    hide_about: bool,
    dark_mode: bool,
}

fn prefs_path() -> Option<PathBuf> {
    std::env::var("APPDATA").ok().map(|appdata| {
        PathBuf::from(appdata).join("SpaceView").join("prefs.txt")
    })
}

fn load_prefs() -> Prefs {
    let mut prefs = Prefs { hide_about: false, dark_mode: true };
    if let Some(content) = prefs_path().and_then(|p| std::fs::read_to_string(p).ok()) {
        // Backwards-compatible: old single-line format "hide_about=true" still works
        for line in content.lines() {
            let line = line.trim();
            if let Some((key, val)) = line.split_once('=') {
                match key.trim() {
                    "hide_about" => prefs.hide_about = val.trim() == "true",
                    "dark_mode" => prefs.dark_mode = val.trim() == "true",
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
        let content = format!(
            "hide_about={}\ndark_mode={}",
            prefs.hide_about, prefs.dark_mode,
        );
        let _ = std::fs::write(p, content);
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
        self.scan_path = Some(path.clone());

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
        if let Some(ref mut root) = self.scan_root {
            // Inject free space as a child if enabled
            if self.show_free_space {
                if let Some(ref path) = self.scan_path {
                    if let Some(free) = get_free_space(path) {
                        if free > 0 {
                            // Remove any previous free space node
                            root.children.retain(|c| c.name != "<Free Space>");
                            root.children.push(FileNode {
                                name: "<Free Space>".to_string(),
                                path: PathBuf::new(),
                                size: free,
                                is_dir: false,
                                file_count: 0,
                                children: Vec::new(),
                            });
                            root.size += free;
                            root.children.sort_by(|a, b| b.size.cmp(&a.size));
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

        // Handle drag-and-drop folders
        let dropped: Vec<_> = ctx.input(|i| {
            i.raw.dropped_files.iter()
                .filter_map(|f| f.path.clone())
                .collect()
        });
        if let Some(path) = dropped.into_iter().find(|p| p.is_dir()) {
            self.start_scan(path);
        }

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
                        save_prefs(&Prefs { hide_about: hide, dark_mode: self.dark_mode });
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

                // Theme selector + dark/light toggle
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
                    let mode_label = if self.dark_mode { "Light" } else { "Dark" };
                    if ui.button(mode_label).clicked() {
                        self.dark_mode = !self.dark_mode;
                        save_prefs(&Prefs { hide_about: self.hide_about_on_start, dark_mode: self.dark_mode });
                    }
                }

                // Right-aligned About button + Free Space toggle
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("About").clicked() {
                        self.show_about = !self.show_about;
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

        // Phase 1: body fill + border stroke
        let col = body_color(node.color_index, theme);
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
        } else if node.is_dir {
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
        file_count: node.file_count,
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
